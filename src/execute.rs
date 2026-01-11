// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{anyhow, Context, Result};
use std::os::fd::FromRawFd;
use std::process::{Child, Command, Stdio};

use crate::kmsg::kmsg;

/// Convert hardened_std::fs::File to std::process::Stdio
///
/// # Safety Guarantees
/// This function safely transfers fd ownership between type systems:
/// 1. hardened_std::fs::File -> raw fd (via into_raw_fd)
/// 2. raw fd -> std::fs::File (via from_raw_fd)
/// 3. std::fs::File -> Stdio (via From trait)
///
/// The fd lifecycle is:
/// - Created by hardened_std::fs::File::open() (validated, whitelisted path)
/// - Transferred here (ownership moved, Drop prevented by into_raw_fd)
/// - Adopted by std::fs::File (takes ownership, will close on drop)
/// - Moved into Stdio (takes ownership from std::fs::File)
/// - Eventually closed when Stdio/Command is dropped
///
/// **Why this is safe:**
/// - No double-free: Each fd has exactly one owner at any time
/// - No leaks: std::fs::File/Stdio will close the fd when dropped
/// - No invalid fds: Only opened fds from hardened_std reach here
/// - No use-after-free: Ownership transfer prevents dangling references
fn file_to_stdio(file: hardened_std::fs::File) -> Stdio {
    // Transfer ownership from hardened_std to raw fd
    let fd = file.into_raw_fd();

    // SAFETY: Safe because:
    // 1. `fd` is valid - it came from a successful File::open()
    // 2. We have unique ownership - into_raw_fd() consumed the original File
    // 3. from_raw_fd takes ownership - std::fs::File will close it on drop
    // 4. No double-close possible - original File's Drop was prevented
    let std_file = unsafe { std::fs::File::from_raw_fd(fd) };

    // Transfer to Stdio (which will take ownership and close on drop)
    Stdio::from(std_file)
}

/// Run a command and block until completion. Output goes to kmsg so it appears
/// in dmesg/kernel log - the only reliable log destination in minimal VMs.
/// Used for setup commands that must succeed before continuing (nvidia-smi, modprobe).
pub fn foreground(command: &str, args: &[&str]) -> Result<()> {
    debug!("{} {}", command, args.join(" "));

    let kmsg_file = kmsg().context("Failed to open kmsg device")?;
    let status = Command::new(command)
        .args(args)
        .stdout(file_to_stdio(
            kmsg_file
                .try_clone()
                .map_err(|e| anyhow!("Failed to clone kmsg file: {}", e))?,
        ))
        .stderr(file_to_stdio(kmsg_file))
        .status()
        .context(format!("failed to execute {command}"))?;

    if !status.success() {
        return Err(anyhow!("{} failed with status: {}", command, status));
    }
    Ok(())
}

/// Spawn a daemon without waiting. Returns Child so caller can track it later.
/// Used for long-running services (nvidia-persistenced, fabricmanager) that run
/// alongside kata-agent. Output to kmsg for visibility in kernel log.
pub fn background(command: &str, args: &[&str]) -> Result<Child> {
    debug!("{} {}", command, args.join(" "));
    let kmsg_file = kmsg().context("Failed to open kmsg device")?;
    Command::new(command)
        .args(args)
        .stdout(file_to_stdio(
            kmsg_file
                .try_clone()
                .map_err(|e| anyhow!("Failed to clone kmsg file: {}", e))?,
        ))
        .stderr(file_to_stdio(kmsg_file))
        .spawn()
        .with_context(|| format!("Failed to start {}", command))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== foreground tests ====================

    #[test]
    fn test_foreground_success() {
        let result = foreground("/bin/true", &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_foreground_failure_exit_code() {
        // Command runs but exits non-zero
        let result = foreground("/bin/false", &[]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("failed"));
    }

    #[test]
    fn test_foreground_not_found() {
        // Command doesn't exist - triggers .context() error path
        let result = foreground("/nonexistent/command", &[]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("execute"));
    }

    #[test]
    fn test_foreground_with_args() {
        let result = foreground("/bin/sh", &["-c", "exit 0"]);
        assert!(result.is_ok());

        let result = foreground("/bin/sh", &["-c", "exit 42"]);
        assert!(result.is_err());
    }

    // ==================== background tests ====================

    #[test]
    fn test_background_spawns() {
        let result = background("/bin/sleep", &["0.01"]);
        assert!(result.is_ok());
        let mut child = result.unwrap();
        let status = child.wait().unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_background_not_found() {
        // Command doesn't exist - triggers .with_context() error path
        let result = background("/nonexistent/command", &[]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("start"), "error should mention start: {}", err);
    }

    #[test]
    fn test_background_check_later() {
        let result = background("/bin/sh", &["-c", "exit 7"]);
        assert!(result.is_ok());
        let mut child = result.unwrap();
        let status = child.wait().unwrap();
        assert!(!status.success());
        assert_eq!(status.code(), Some(7));
    }
}
