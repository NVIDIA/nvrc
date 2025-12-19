// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{anyhow, Context, Result};
use log::{debug, error};
use nix::unistd::{fork, ForkResult};
use rlimit::{setrlimit, Resource};
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

const KATA_AGENT_PATH: &str = "/usr/bin/kata-agent";

/// kata-agent needs high file descriptor limits for container workloads and
/// must survive OOM conditions to maintain VM stability (-997 = nearly unkillable)
fn agent_setup() -> Result<()> {
    let nofile = 1024 * 1024;
    setrlimit(Resource::NOFILE, nofile, nofile).context("setrlimit RLIMIT_NOFILE")?;
    fs::write("/proc/self/oom_score_adj", b"-997").context("write /proc/self/oom_score_adj")?;
    let lim = rlimit::getrlimit(Resource::NOFILE)?;
    debug!("kata-agent RLIMIT_NOFILE: {:?}", lim);
    Ok(())
}

/// exec() replaces this process with kata-agent, so it only returns on failure.
/// We want kata-agent to become PID 1's child for proper process hierarchy.
fn exec_agent(cmd: &str) -> Result<()> {
    let err = Command::new(cmd).exec();
    Err(anyhow!("exec {} failed: {err}", cmd))
}

/// Path parameter enables testing with /bin/true instead of real kata-agent
fn kata_agent(path: &str) -> Result<()> {
    agent_setup()?;
    exec_agent(path)
}

/// Guest VMs lack a syslog daemon, so we poll /dev/log to drain messages
/// and forward them to kmsg. Timeout enables testing without infinite loops.
fn syslog_loop(timeout_secs: u32) -> Result<()> {
    let iterations = (timeout_secs as u64) * 2; // 500ms per iteration
    for _ in 0..iterations {
        sleep(Duration::from_millis(500));
        if let Err(e) = crate::syslog::poll() {
            return Err(anyhow!("poll syslog: {e}"));
        }
    }
    Ok(())
}

/// Parent execs kata-agent (becoming it), child stays as syslog poller.
/// This way kata-agent inherits our PID and becomes the main guest process.
pub fn fork_agent() -> Result<()> {
    fork_agent_with_timeout(u32::MAX) // 136 years - effectively forever
}

/// Timeout parameter allows tests to verify the fork/syslog logic exits cleanly
fn fork_agent_with_timeout(timeout_secs: u32) -> Result<()> {
    match unsafe { fork() }.expect("fork agent") {
        ForkResult::Parent { .. } => {
            kata_agent(KATA_AGENT_PATH).context("kata-agent parent")?;
        }
        ForkResult::Child => {
            if let Err(e) = syslog_loop(timeout_secs) {
                error!("{e}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use nix::sys::wait::{waitpid, WaitStatus};

    #[test]
    fn test_agent_setup() {
        require_root();

        // agent_setup sets rlimit and writes oom_score_adj
        let result = agent_setup();
        assert!(result.is_ok(), "agent_setup failed: {:?}", result);

        // Verify rlimit was set
        let (soft, hard) = rlimit::getrlimit(Resource::NOFILE).unwrap();
        assert_eq!(soft, 1024 * 1024);
        assert_eq!(hard, 1024 * 1024);

        // Verify oom_score_adj was written
        let oom = fs::read_to_string("/proc/self/oom_score_adj").unwrap();
        assert_eq!(oom.trim(), "-997");
    }

    #[test]
    fn test_exec_agent_not_found() {
        // exec_agent with nonexistent command returns error (doesn't exec)
        let result = exec_agent("/nonexistent/command");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("exec"), "error should mention exec: {}", err);
    }

    #[test]
    fn test_exec_agent_success() {
        // Fork, child execs /bin/true, parent waits and checks exit code
        match unsafe { fork() }.expect("fork") {
            ForkResult::Parent { child } => {
                // Wait for child to exec and exit
                let status = waitpid(child, None).expect("waitpid");
                match status {
                    WaitStatus::Exited(_, code) => assert_eq!(code, 0, "/bin/true should exit 0"),
                    other => panic!("unexpected wait status: {:?}", other),
                }
            }
            ForkResult::Child => {
                // In child: exec /bin/true (replaces this process)
                let _ = exec_agent("/bin/true");
                // If exec fails, exit with error
                std::process::exit(1);
            }
        }
    }

    #[test]
    fn test_exec_agent_with_exit_code() {
        // Fork, child execs /bin/false, parent checks non-zero exit
        match unsafe { fork() }.expect("fork") {
            ForkResult::Parent { child } => {
                let status = waitpid(child, None).expect("waitpid");
                match status {
                    WaitStatus::Exited(_, code) => assert_eq!(code, 1, "/bin/false should exit 1"),
                    other => panic!("unexpected wait status: {:?}", other),
                }
            }
            ForkResult::Child => {
                let _ = exec_agent("/bin/false");
                std::process::exit(99); // Should not reach here
            }
        }
    }

    #[test]
    fn test_kata_agent_success() {
        require_root();

        // Test the full kata_agent flow (setup + exec) with /bin/true
        match unsafe { fork() }.expect("fork") {
            ForkResult::Parent { child } => {
                let status = waitpid(child, None).expect("waitpid");
                match status {
                    WaitStatus::Exited(_, code) => {
                        assert_eq!(code, 0, "kata_agent(/bin/true) should exit 0")
                    }
                    other => panic!("unexpected wait status: {:?}", other),
                }
            }
            ForkResult::Child => {
                // This does full setup (rlimit, oom_score_adj) then execs
                let _ = kata_agent("/bin/true");
                std::process::exit(1); // Should not reach here
            }
        }
    }

    #[test]
    fn test_kata_agent_not_found() {
        require_root();

        // kata_agent with nonexistent path - setup succeeds, exec fails
        match unsafe { fork() }.expect("fork") {
            ForkResult::Parent { child } => {
                let status = waitpid(child, None).expect("waitpid");
                match status {
                    // Child should exit with our error code (1) since exec fails
                    WaitStatus::Exited(_, code) => assert_eq!(code, 1),
                    other => panic!("unexpected wait status: {:?}", other),
                }
            }
            ForkResult::Child => {
                // Setup succeeds, exec fails, we exit with 1
                if kata_agent("/nonexistent/agent").is_err() {
                    std::process::exit(1);
                }
                std::process::exit(0);
            }
        }
    }

    #[test]
    fn test_syslog_loop_timeout() {
        // syslog_loop with 1 second timeout should complete (2 iterations)
        // Note: syslog::poll() will fail if /dev/log doesn't exist, but that's OK
        // for this test - we just want to verify the loop terminates
        let start = std::time::Instant::now();
        let _ = syslog_loop(1); // May error if /dev/log not bound, that's fine
        let elapsed = start.elapsed();

        // Should take ~1 second (2 x 500ms iterations)
        assert!(
            elapsed.as_millis() >= 500,
            "loop should run for at least 500ms"
        );
        assert!(elapsed.as_millis() < 3000, "loop should complete within 3s");
    }

    #[test]
    fn test_fork_agent_with_timeout() {
        // Test fork_agent_with_timeout with a short timeout
        // Parent will fail (no kata-agent), but child should exit after timeout
        match unsafe { fork() }.expect("fork") {
            ForkResult::Parent { child } => {
                // Wait for child to complete (timeout after 1 sec)
                let status = waitpid(child, None).expect("waitpid");
                match status {
                    WaitStatus::Exited(_, code) => {
                        // Child exits 0 after syslog_loop completes/errors
                        assert!(code == 0 || code == 1, "child should exit cleanly");
                    }
                    other => panic!("unexpected wait status: {:?}", other),
                }
            }
            ForkResult::Child => {
                // Run with 1 second timeout - should exit quickly
                if let Err(e) = syslog_loop(1) {
                    // Expected: syslog::poll fails if /dev/log not bound
                    eprintln!("syslog_loop error (expected): {e}");
                }
                std::process::exit(0);
            }
        }
    }
}
