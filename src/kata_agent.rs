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

/// Syslog polling runs indefinitely in production—VM lifetime measured in hours/days,
/// not the 136 years this represents. Using u32::MAX avoids overflow concerns.
pub const SYSLOG_POLL_FOREVER: u32 = u32::MAX;

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
/// Timeout parameter allows tests to verify the fork/syslog logic exits cleanly
pub fn fork_agent(timeout_secs: u32) -> Result<()> {
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
    fn test_kata_agent_not_found() {
        require_root();

        // kata_agent with nonexistent path - setup succeeds, exec fails
        match unsafe { fork() }.expect("fork") {
            ForkResult::Parent { child } => {
                assert!(matches!(
                    waitpid(child, None).expect("waitpid"),
                    WaitStatus::Exited(_, 1)
                ));
            }
            ForkResult::Child => {
                // Setup succeeds, exec fails - verify and exit with expected code
                assert!(kata_agent("/nonexistent/agent").is_err());
                std::process::exit(1);
            }
        }
    }

    #[test]
    fn test_syslog_loop_timeout() {
        // syslog_loop with 1 second timeout runs up to 2 iterations (500ms each).
        // Two possible outcomes:
        // 1. poll() works: runs full 2 iterations (~1000ms)
        // 2. poll() fails: exits early after 1st iteration (~500ms) due to missing /dev/log
        // Either way, verifies the loop terminates properly.
        let start = std::time::Instant::now();
        let _ = syslog_loop(1); // May error if /dev/log not bound, that's fine
        let elapsed = start.elapsed();

        // Lower bound: at least 1 sleep cycle (500ms) runs before poll
        // Upper bound: 2 iterations + scheduling overhead = ~1200ms max
        assert!(elapsed.as_millis() >= 400);
        assert!(elapsed.as_millis() < 1500);
    }

    #[test]
    fn test_fork_agent_with_timeout() {
        // Double fork: outer fork isolates the test, inner fork (inside fork_agent_with_timeout)
        // does the real work. This lets us actually call fork_agent_with_timeout() directly.
        match unsafe { fork() }.expect("outer fork") {
            ForkResult::Parent { child } => {
                // Wrapper exits 1 because kata_agent() fails (no binary)
                assert!(matches!(
                    waitpid(child, None).expect("waitpid"),
                    WaitStatus::Exited(_, 1)
                ));
            }
            ForkResult::Child => {
                // This child calls fork_agent_with_timeout, which forks again internally.
                // - Inner parent (us): kata_agent() fails, returns Err
                // - Inner child: runs syslog_loop(1), exits after ~1 second
                let result = fork_agent(1);
                // We're the inner parent, so we get the error from kata_agent()
                std::process::exit(if result.is_err() { 1 } else { 0 });
            }
        }
    }
}
