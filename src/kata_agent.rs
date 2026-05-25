// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use log::debug;
use nix::errno::Errno;
use nix::sched::{unshare, CloneFlags};
use nix::sys::reboot::{reboot, RebootMode};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{fork, sync, ForkResult, Pid};
use rlimit::{setrlimit, Resource};
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

const KATA_AGENT_PATH: &str = "/usr/bin/kata-agent";

/// OOM score adjustment for kata-agent. Value of -997 makes it nearly unkillable,
/// ensuring VM stability even under memory pressure. Range is -1000 (never kill)
/// to 1000 (always kill first).
const KATA_AGENT_OOM_SCORE_ADJ: &str = "-997";

/// kata-agent needs high file descriptor limits for container workloads and
/// must survive OOM conditions to maintain VM stability.
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

/// exec() replaces this process with kata-agent. Only returns on failure.
fn exec_agent(cmd: &str) {
    let err = Command::new(cmd).exec();
    panic!("exec {cmd} failed: {err}");
}

/// Path parameter enables testing with a non-existent binary.
fn kata_agent(path: &str) {
    agent_setup();
    exec_agent(path);
}

/// Run kata-agent in a child PID namespace and own the VM power-off.
///
/// NVRC remains PID 1 in the initial PID namespace. The forked child enters a
/// fresh PID namespace where it is PID 1, so kata-agent runs with
/// `init_mode = true` and performs its usual init_agent_as_init setup
/// (cgroups mount, /dev/ptmx symlink, setsid, sethostname).
///
/// When kata-agent later calls `reboot(RB_POWER_OFF)`, the kernel reinterprets
/// reboot(2) issued from a non-initial PID namespace as `SIGINT` to that
/// namespace's init process — kata-agent itself. kata-agent terminates instead
/// of halting the VM. NVRC observes the child exit, drains `/dev/log` one last
/// time, syncs, and powers the VM off here in the initial namespace, where
/// `reboot()` actually halts the guest.
///
/// This hands the reboot policy to NVRC without requiring kata-agent changes.
/// kata-agent still believes it owns shutdown; NVRC owns the hardware power-off.
pub fn run_supervised_agent() -> ! {
    debug!(
        "supervise: about to unshare CLONE_NEWPID (pid={})",
        nix::unistd::getpid()
    );

    // Future fork()s land the child in a new PID namespace as pid 1.
    // The calling process (NVRC) stays in the initial namespace.
    unshare(CloneFlags::CLONE_NEWPID).expect("unshare CLONE_NEWPID");

    debug!("supervise: unshare ok, forking");

    // SAFETY: NVRC is PID 1 and single-threaded at this point. The child only
    // calls exec() (async-signal-safe). The parent runs a non-blocking
    // waitpid + syslog drain loop with no shared mutable state.
    match unsafe { fork() }.expect("fork agent") {
        ForkResult::Child => {
            // Inside the child PID namespace `getpid()` returns 1. Logging it
            // here gives us proof in dmesg that the namespace handoff worked.
            debug!(
                "supervise(child): in-ns pid={} — exec kata-agent",
                nix::unistd::getpid()
            );
            kata_agent(KATA_AGENT_PATH);
            // exec failed; surface a non-zero exit so NVRC still powers off.
            unsafe { libc::_exit(1) };
        }
        ForkResult::Parent { child } => {
            debug!(
                "supervise(parent): pid={} child={} — waiting",
                nix::unistd::getpid(),
                child
            );
            wait_for_agent(child);
            power_off();
        }
    }
}

/// Block until kata-agent exits, draining `/dev/log` opportunistically.
/// Polls with `WNOHANG` so the syslog drain keeps running at the same
/// 500ms cadence as the previous `syslog_loop`.
fn wait_for_agent(agent: Pid) {
    loop {
        // Best-effort drain so /run/syslog.log captures post-fork messages.
        // try_poll silently absorbs transient I/O errors.
        crate::syslog::try_poll();

        match waitpid(agent, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => sleep(Duration::from_millis(500)),
            Ok(WaitStatus::Exited(_, _)) | Ok(WaitStatus::Signaled(_, _, _)) => return,
            Ok(_) => continue,
            Err(Errno::EINTR) => continue,
            Err(e) => panic!("waitpid kata-agent: {e}"),
        }
    }
}

/// Sync filesystem caches and power off the VM. Must run from the initial
/// PID namespace so reboot(2) actually halts the guest; in a child PID ns the
/// kernel reinterprets it into a signal.
fn power_off() -> ! {
    debug!(
        "supervise(parent): kata-agent exited; powering off VM (pid={})",
        nix::unistd::getpid()
    );
    sync();
    let _ = reboot(RebootMode::RB_POWER_OFF);
    unreachable!("reboot returned");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use serial_test::serial;
    use std::panic;

    /// Install a panic hook that exits with code 1. Required in forked children
    /// because Rust's test harness catches panics and exits with 0, which would
    /// break our "panic = abnormal exit" assertions.
    fn set_test_panic_hook() {
        panic::set_hook(Box::new(|info| {
            eprintln!("panic: {info}");
            std::process::exit(1);
        }));
    }

    #[test]
    fn test_agent_setup() {
        require_root();

        agent_setup();

        let (soft, hard) = rlimit::getrlimit(Resource::NOFILE).unwrap();
        assert_eq!(soft, 1024 * 1024);
        assert_eq!(hard, 1024 * 1024);

        let oom = fs::read_to_string("/proc/self/oom_score_adj").unwrap();
        assert_eq!(oom.trim(), KATA_AGENT_OOM_SCORE_ADJ);
    }

    #[test]
    fn test_exec_agent_not_found() {
        let result = panic::catch_unwind(|| {
            exec_agent("/nonexistent/command");
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_kata_agent_not_found() {
        require_root();

        // SAFETY: single-threaded test process, child isolates the panic.
        match unsafe { fork() }.expect("fork") {
            ForkResult::Parent { child } => {
                let status = waitpid(child, None).expect("waitpid");
                assert!(!matches!(status, WaitStatus::Exited(_, 0)));
            }
            ForkResult::Child => {
                set_test_panic_hook();
                kata_agent("/nonexistent/agent");
                std::process::exit(0); // unreachable
            }
        }
    }

    /// `wait_for_agent` returns when its tracked child exits and tolerates an
    /// already-reaped child without spinning. The reboot in `power_off()` is
    /// only exercised in production; tests stop one step before it.
    #[test]
    #[serial]
    fn test_wait_for_agent_returns_on_child_exit() {
        // SAFETY: single-threaded test process; child only calls _exit.
        match unsafe { fork() }.expect("fork") {
            ForkResult::Parent { child } => {
                let start = std::time::Instant::now();
                wait_for_agent(child);
                // WNOHANG + 500ms sleep ≤ ~1s in practice.
                assert!(start.elapsed() < Duration::from_secs(5));
            }
            ForkResult::Child => unsafe {
                libc::_exit(0);
            },
        }
    }
}
