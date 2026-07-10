// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use crate::gpu_extension;
use log::debug;
use nix::unistd::{fork, ForkResult};
use rlimit::{setrlimit, Resource};
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

const KATA_AGENT_PATH: &str = "/usr/bin/kata-agent";

/// Env var kata-agent reads to pick its attestation-agent. Set to
/// [`gpu_extension::ATTESTER_VARIANT_NVIDIA`] when the GPU extension is present, unset
/// otherwise. Contract shared with kata-agent (`src/agent/src/main.rs`).
const ATTESTER_VARIANT_ENV: &str = "KATA_ATTESTER_VARIANT";

/// Syslog polling runs indefinitely in production—VM lifetime measured in hours/days,
/// not the 136 years this represents. Using u32::MAX avoids overflow concerns.
pub const SYSLOG_POLL_FOREVER: u32 = u32::MAX;

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

/// Build the kata-agent command, injecting the attester-variant env var when
/// set. Split from [`exec_agent`] so the env wiring is unit-testable. GPU libs
/// resolve via the loader cache ([`gpu_extension::setup`]), not `LD_LIBRARY_PATH`.
fn agent_command(cmd: &str, attester_variant: Option<&str>) -> Command {
    let mut command = Command::new(cmd);
    if let Some(variant) = attester_variant {
        command.env(ATTESTER_VARIANT_ENV, variant);
    }
    command
}

/// exec() replaces this process with kata-agent, so it only returns on failure.
/// We want kata-agent to become PID 1's child for proper process hierarchy.
fn exec_agent(cmd: &str, attester_variant: Option<&str>) {
    let err = agent_command(cmd, attester_variant).exec();
    panic!("exec {cmd} failed: {err}");
}

/// Path parameter enables testing with /bin/true instead of real kata-agent
fn kata_agent(path: &str, attester_variant: Option<&str>) {
    agent_setup();
    exec_agent(path, attester_variant);
}

/// Drains `/dev/log` (bound in `main()` before fork) into `/run/syslog.log`.
///
/// Uses `try_poll()` not `poll()`: this child inherits NVRC's power-off panic
/// hook, and a transient drain I/O error must not reboot the VM while
/// kata-agent is still running (kata exit 255).
fn syslog_loop(timeout_secs: u32) {
    let iterations = (timeout_secs as u64) * 2; // 500ms per iteration
    for _ in 0..iterations {
        sleep(Duration::from_millis(500));
        crate::syslog::try_poll();
    }
}

/// Parent execs kata-agent (becoming it), child stays as syslog poller.
/// This way kata-agent inherits our PID and becomes the main guest process.
/// Timeout parameter allows tests to verify the fork/syslog logic exits cleanly
pub fn fork_agent(timeout_secs: u32) {
    // SAFETY: fork() is safe here because:
    // 1. We are PID 1 with no other threads (single-threaded process)
    // 2. Parent immediately execs kata-agent (no shared state issues)
    // 3. Child only calls async-signal-safe functions (syslog::try_poll, sleep)
    // 4. No locks or mutexes exist that could deadlock in child
    match unsafe { fork() }.expect("fork agent") {
        ForkResult::Parent { .. } => {
            kata_agent(KATA_AGENT_PATH, gpu_extension::attester_variant());
        }
        ForkResult::Child => {
            syslog_loop(timeout_secs);
        }
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
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
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
    #[cfg_attr(miri, ignore = "exec_agent calls execvp, which miri cannot emulate")]
    fn test_exec_agent_not_found() {
        // exec_agent with nonexistent command panics (doesn't exec)
        let result = panic::catch_unwind(|| {
            exec_agent("/nonexistent/command", None);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_agent_command_injects_attester_variant() {
        use std::ffi::OsStr;
        let cmd = agent_command("/bin/true", Some(gpu_extension::ATTESTER_VARIANT_NVIDIA));
        let found = cmd.get_envs().any(|(k, v)| {
            k == OsStr::new(ATTESTER_VARIANT_ENV)
                && v == Some(OsStr::new(gpu_extension::ATTESTER_VARIANT_NVIDIA))
        });
        assert!(
            found,
            "expected {ATTESTER_VARIANT_ENV} to be set on the agent command"
        );
    }

    #[test]
    fn test_agent_command_no_variant_leaves_env_unset() {
        use std::ffi::OsStr;
        let cmd = agent_command("/bin/true", None);
        let found = cmd
            .get_envs()
            .any(|(k, _)| k == OsStr::new(ATTESTER_VARIANT_ENV));
        assert!(
            !found,
            "did not expect {ATTESTER_VARIANT_ENV} without a GPU extension"
        );
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
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
                kata_agent("/nonexistent/agent", None);
                std::process::exit(0); // Won't reach here
            }
        }
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
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
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    #[cfg_attr(not(miri), serial)]
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

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
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
                // - Inner child: runs syslog_loop(1), exits after ~1 second
                fork_agent(1);
                std::process::exit(0); // Won't reach here due to panic
            }
        }
    }
}
