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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Once;

    /// `_exit` a forked child. That skips atexit, so coverage is flushed
    /// explicitly (%p keeps profraw files distinct).
    fn exit_child(code: i32) -> ! {
        #[cfg(coverage)]
        {
            extern "C" {
                fn __llvm_profile_write_file() -> libc::c_int;
            }
            // SAFETY: only reached in a single-threaded forked child.
            unsafe { __llvm_profile_write_file() };
        }
        // SAFETY: _exit is async-signal-safe.
        unsafe { libc::_exit(code) }
    }

    /// True only in the forked child of test_fork_agent_with_timeout.
    static IN_FORKED_CHILD: AtomicBool = AtomicBool::new(false);

    /// Make a panic in a forked child `_exit(1)` at once: the std hook takes
    /// locks a sibling test thread may hold at fork time, hanging the child.
    /// Installed once; every other thread gets the previous hook.
    fn install_child_exit_hook() {
        static INSTALLED: Once = Once::new();
        INSTALLED.call_once(|| {
            let previous = panic::take_hook();
            panic::set_hook(Box::new(move |info| {
                if IN_FORKED_CHILD.load(Ordering::Relaxed) {
                    exit_child(1);
                }
                previous(info);
            }));
        });
    }

    /// Run `f` in a forked child and return its wait status: exit 0 if it
    /// returns, exit 1 if it panics.
    fn in_forked_child(f: impl FnOnce()) -> WaitStatus {
        install_child_exit_hook();

        // SAFETY: the child only runs `f` and `_exit`s.
        match unsafe { fork() }.expect("fork") {
            ForkResult::Parent { child } => waitpid(child, None).expect("waitpid"),
            ForkResult::Child => {
                IN_FORKED_CHILD.store(true, Ordering::Relaxed);
                // A sibling may hold kernlog's mutex at fork time, and the
                // copy never unlocks, so keep debug! away from it.
                log::set_max_level(log::LevelFilter::Off);
                f();
                exit_child(0)
            }
        }
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
    #[cfg_attr(not(miri), serial)]
    fn test_kata_agent_not_found() {
        require_root();

        // Forked: setup alters the process. Setup succeeds, then exec panics.
        let status = in_forked_child(|| kata_agent("/nonexistent/agent", None));
        assert!(matches!(status, WaitStatus::Exited(_, 1)), "{status:?}");
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    fn test_syslog_loop_timeout_and_kata_exit_255_regression() {
        // Two 500ms iterations. A drain error (here EADDRINUSE on /dev/log)
        // must not panic and power off the VM (kata exit 255).
        let start = std::time::Instant::now();
        syslog_loop(1);
        let elapsed = start.elapsed();

        // Sleeps never return early; the generous upper bound only catches a hang.
        assert!(elapsed.as_millis() >= 1000);
        assert!(elapsed.as_millis() < 5000);
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    #[cfg_attr(not(miri), serial)]
    fn test_fork_agent_with_timeout() {
        require_root();

        // The outer child isolates the test; fork_agent forks again. Its parent
        // panics on the missing agent (exit 1), its child (timeout 0) exits at once.
        let status = in_forked_child(|| fork_agent(0));
        assert!(matches!(status, WaitStatus::Exited(_, 1)), "{status:?}");
    }
}
