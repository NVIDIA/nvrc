// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use log::debug;
use nix::unistd::{fork, ForkResult};
use rlimit::{setrlimit, Resource};
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

const KATA_AGENT_PATH: &str = "/usr/bin/kata-agent";

/// OOM score adjustment for kata-agent. Value of -997 makes it nearly unkillable,
/// ensuring VM stability even under memory pressure. Range is -1000 (never kill) to 1000 (always kill first).
const KATA_AGENT_OOM_SCORE_ADJ: &str = "-997";

/// kata-agent needs high file descriptor limits for container workloads and
/// must survive OOM conditions to maintain VM stability
fn agent_setup() {
    let nofile = 1024 * 1024;
    setrlimit(Resource::NOFILE, nofile, nofile).expect("setrlimit RLIMIT_NOFILE");
    fs::write(
        "/proc/self/oom_score_adj",
        KATA_AGENT_OOM_SCORE_ADJ.as_bytes(),
    )
    .expect("write /proc/self/oom_score_adj");
    let lim = rlimit::getrlimit(Resource::NOFILE).expect("getrlimit RLIMIT_NOFILE");
    debug!("kata-agent RLIMIT_NOFILE: {:?}", lim);
}

/// exec() replaces this process with kata-agent, so it only returns on failure.
/// We want kata-agent to become PID 1's child for proper process hierarchy.
fn exec_agent(cmd: &str) {
    let err = Command::new(cmd).exec();
    panic!("exec {cmd} failed: {err}");
}

/// Path parameter enables testing with /bin/true instead of real kata-agent
fn kata_agent(path: &str) {
    agent_setup();
    exec_agent(path);
}

/// Drains `/dev/log` (bound in `main()` before fork) into `/run/syslog.log`.
///
/// Uses `try_poll()` not `poll()`: keeps NVRC's power-off panic hook for real
/// errors, but a transient drain I/O error must not reboot the VM while
/// kata-agent is still running (kata exit 255). Used in unit tests only;
/// production exits the fork child immediately (see `fork_agent`).
fn syslog_loop(timeout_secs: u32) {
    let iterations = (timeout_secs as u64) * 2; // 500ms per iteration
    for _ in 0..iterations {
        sleep(Duration::from_millis(500));
        crate::syslog::try_poll();
    }
}

/// Fork child exits after fork: kata-agent keeps `/dev/log` across exec.
fn fork_agent_child_exit() {
    crate::syslog::try_poll();
    unsafe { libc::_exit(0) };
}

/// Parent execs kata-agent (becoming it); child exits so PID 1 has no stray
/// poller at shutdown (nerdctl exit 255 / `ttrpc: closed` otherwise).
pub fn fork_agent() {
    // SAFETY: fork() is safe here because:
    // 1. We are PID 1 with no other threads (single-threaded process)
    // 2. Parent immediately execs kata-agent (no shared state issues)
    // 3. Child calls try_poll() then _exit(0) — no locks held across fork
    match unsafe { fork() }.expect("fork agent") {
        ForkResult::Parent { .. } => {
            kata_agent(KATA_AGENT_PATH);
        }
        ForkResult::Child => fork_agent_child_exit(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use nix::sys::wait::{waitpid, WaitStatus};
    use serial_test::serial;
    use std::panic;

    /// Install a panic hook that exits with code 1.
    /// Required in forked children because Rust's test harness catches panics
    /// and exits with 0, which breaks our "panic = failure" assertions.
    fn set_test_panic_hook() {
        panic::set_hook(Box::new(|info| {
            eprintln!("panic: {info}");
            std::process::exit(1);
        }));
    }

    #[test]
    fn test_agent_setup() {
        require_root();

        // agent_setup sets rlimit and writes oom_score_adj
        agent_setup();

        // Verify rlimit was set
        let (soft, hard) = rlimit::getrlimit(Resource::NOFILE).unwrap();
        assert_eq!(soft, 1024 * 1024);
        assert_eq!(hard, 1024 * 1024);

        // Verify oom_score_adj was written
        let oom = fs::read_to_string("/proc/self/oom_score_adj").unwrap();
        assert_eq!(oom.trim(), KATA_AGENT_OOM_SCORE_ADJ);
    }

    #[test]
    fn test_exec_agent_not_found() {
        // exec_agent with nonexistent command panics (doesn't exec)
        let result = panic::catch_unwind(|| {
            exec_agent("/nonexistent/command");
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_kata_agent_not_found() {
        require_root();

        // kata_agent with nonexistent path - setup succeeds, exec panics
        // SAFETY: Test forks to isolate agent_setup() and exec failure.
        // Single-threaded test process with no shared state.
        match unsafe { fork() }.expect("fork") {
            ForkResult::Parent { child } => {
                // Child exits abnormally due to panic
                let status = waitpid(child, None).expect("waitpid");
                assert!(!matches!(status, WaitStatus::Exited(_, 0)));
            }
            ForkResult::Child => {
                set_test_panic_hook();
                // Setup succeeds, exec panics
                kata_agent("/nonexistent/agent");
                std::process::exit(0); // Won't reach here
            }
        }
    }

    #[test]
    fn test_syslog_loop_timeout() {
        // ~1s: two 500ms iterations; try_poll() is best-effort on /dev/log.
        let start = std::time::Instant::now();
        syslog_loop(1);
        let elapsed = start.elapsed();

        // Lower bound: at least 1 sleep cycle (500ms) runs before poll
        // Upper bound: 2 iterations + scheduling overhead = ~1200ms max
        assert!(elapsed.as_millis() >= 400);
        assert!(elapsed.as_millis() < 1500);
    }

    /// Regression: with the power-off hook installed, syslog_loop must not panic
    /// on drain I/O (fork isolates from parallel tests). In-VM, `/dev/log` is
    /// already bound by main(); this child does a fresh bind — on dev hosts
    /// with a host syslog daemon, reverting to `poll()` reproduces via EADDRINUSE.
    #[test]
    #[serial]
    fn test_syslog_loop_does_not_trigger_power_off_hook() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let saved_hook = panic::take_hook();

        let triggered = Arc::new(AtomicBool::new(false));
        let triggered_for_hook = triggered.clone();

        crate::lockdown::set_panic_hook_with(move || {
            triggered_for_hook.store(true, Ordering::SeqCst);
        });

        // SAFETY: child runs syslog_loop and exits; no shared state.
        let fork_result = unsafe { fork() }.expect("fork");

        match fork_result {
            ForkResult::Parent { child } => {
                let status = waitpid(child, None).expect("waitpid");
                panic::set_hook(saved_hook);

                assert_eq!(
                    status,
                    WaitStatus::Exited(child, 0),
                    "syslog_loop must not fire the power-off hook on drain I/O \
                     (kata exit 255 regression)"
                );
            }
            ForkResult::Child => {
                let loop_result = panic::catch_unwind(|| syslog_loop(1));

                if triggered.load(Ordering::SeqCst) || loop_result.is_err() {
                    std::process::exit(42);
                } else {
                    std::process::exit(0);
                }
            }
        }
    }

    /// Regression: fork child must not outlive kata-agent (nerdctl `ttrpc: closed`).
    #[test]
    #[serial]
    fn test_fork_agent_child_exits_immediately() {
        match unsafe { fork() }.expect("fork") {
            ForkResult::Parent { child } => {
                assert_eq!(
                    waitpid(child, None).expect("waitpid"),
                    WaitStatus::Exited(child, 0)
                );
            }
            ForkResult::Child => fork_agent_child_exit(),
        }
    }

    #[test]
    fn test_fork_agent_with_timeout() {
        require_root();

        // Double fork: outer fork isolates the test, inner fork (inside fork_agent)
        // does the real work. This lets us actually call fork_agent() directly.
        // SAFETY: Outer fork isolates the test in a child process.
        // Single-threaded test with no shared state.
        match unsafe { fork() }.expect("outer fork") {
            ForkResult::Parent { child } => {
                // Wrapper exits abnormally because kata_agent() panics (no binary)
                let status = waitpid(child, None).expect("waitpid");
                assert!(!matches!(status, WaitStatus::Exited(_, 0)));
            }
            ForkResult::Child => {
                set_test_panic_hook();
                // This child calls fork_agent, which forks again internally.
                // - Inner parent (us): kata_agent() panics
                // - Inner child: exits immediately via fork_agent_child_exit()
                fork_agent();
                std::process::exit(0); // Won't reach here due to panic
            }
        }
    }
}
