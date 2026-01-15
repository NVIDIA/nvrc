// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use std::process::{Child, Command, Stdio};

use crate::kmsg::kmsg;
use crate::macros::ResultExt;

/// Run a command and block until completion. Output goes to kmsg so it appears
/// in dmesg/kernel log - the only reliable log destination in minimal VMs.
/// Used for setup commands that must succeed before continuing (nvidia-smi, modprobe).
pub fn foreground(command: &str, args: &[&str]) {
    debug!("{} {}", command, args.join(" "));

    let kmsg_file = kmsg();
    let status = Command::new(command)
        .args(args)
        .stdout(Stdio::from(kmsg_file.try_clone().unwrap()))
        .stderr(Stdio::from(kmsg_file))
        .status()
        .or_panic(format_args!("execute {command}"));

    if !status.success() {
        panic!("{command} failed with status: {status}");
    }
}

/// Spawn a daemon without waiting. Returns Child so caller can track it later.
/// Used for long-running services (nvidia-persistenced, fabricmanager) that run
/// alongside kata-agent. Output to kmsg for visibility in kernel log.
pub fn background(command: &str, args: &[&str]) -> Child {
    debug!("{} {}", command, args.join(" "));
    let kmsg_file = kmsg();
    Command::new(command)
        .args(args)
        .stdout(Stdio::from(kmsg_file.try_clone().unwrap()))
        .stderr(Stdio::from(kmsg_file))
        .spawn()
        .or_panic(format_args!("start {command}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic;

    // ==================== foreground tests ====================

    #[test]
    fn test_foreground_success() {
        foreground("/bin/true", &[]);
    }

    #[test]
    fn test_foreground_failure_exit_code() {
        // Command runs but exits non-zero - should panic
        let result = panic::catch_unwind(|| {
            foreground("/bin/false", &[]);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_foreground_not_found() {
        // Command doesn't exist - should panic
        let result = panic::catch_unwind(|| {
            foreground("/nonexistent/command", &[]);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_foreground_with_args() {
        foreground("/bin/sh", &["-c", "exit 0"]);

        let result = panic::catch_unwind(|| {
            foreground("/bin/sh", &["-c", "exit 42"]);
        });
        assert!(result.is_err());
    }

    // ==================== background tests ====================

    #[test]
    fn test_background_spawns() {
        let mut child = background("/bin/sleep", &["0.01"]);
        let status = child.wait().unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_background_not_found() {
        // Command doesn't exist - should panic
        let result = panic::catch_unwind(|| {
            background("/nonexistent/command", &[]);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_background_check_later() {
        let mut child = background("/bin/sh", &["-c", "exit 7"]);
        let status = child.wait().unwrap();
        assert!(!status.success());
        assert_eq!(status.code(), Some(7));
    }
}
