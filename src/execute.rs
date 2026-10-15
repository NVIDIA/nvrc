// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use std::process::{Child, Command, Stdio};

use crate::kmsg::kmsg;
use crate::macros::ResultExt;

/// Command with output sent to kmsg.
fn command<S: AsRef<str>>(program: &str, args: &[S]) -> Command {
    let args = args.iter().map(AsRef::as_ref);
    debug!("{} {}", program, args.clone().collect::<Vec<_>>().join(" "));

    let kmsg_file = kmsg();
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdout(Stdio::from(kmsg_file.try_clone().unwrap()))
        .stderr(Stdio::from(kmsg_file));
    cmd
}

/// Run a command and block until completion.
/// Used for setup commands that must succeed before continuing (nvidia-smi, modprobe).
pub fn foreground<S: AsRef<str>>(program: &str, args: &[S]) {
    let status = command(program, args)
        .status()
        .or_panic(format_args!("execute {program}"));

    if !status.success() {
        panic!("{program} failed with status: {status}");
    }
}

/// Spawn a daemon without waiting. Returns Child so caller can track it later.
/// Used for long-running services (nvidia-persistenced, fabricmanager) that run
/// alongside kata-agent.
pub fn background<S: AsRef<str>>(program: &str, args: &[S]) -> Child {
    command(program, args)
        .spawn()
        .or_panic(format_args!("start {program}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic;

    // ==================== foreground tests ====================

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    fn test_foreground_success() {
        foreground::<&str>("/bin/true", &[]);
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    fn test_foreground_failure_exit_code() {
        // Command runs but exits non-zero - should panic
        let result = panic::catch_unwind(|| {
            foreground::<&str>("/bin/false", &[]);
        });
        assert!(result.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    fn test_foreground_not_found() {
        // Command doesn't exist - should panic
        let result = panic::catch_unwind(|| {
            foreground::<&str>("/nonexistent/command", &[]);
        });
        assert!(result.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    fn test_foreground_with_args() {
        foreground("/bin/sh", &["-c", "exit 0"]);

        let result = panic::catch_unwind(|| {
            foreground("/bin/sh", &["-c", "exit 42"]);
        });
        assert!(result.is_err());
    }

    // ==================== background tests ====================

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    fn test_background_spawns() {
        let mut child = background("/bin/sleep", &["0.01"]);
        let status = child.wait().unwrap();
        assert!(status.success());
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    // spawn fails and or_panic diverges, so no child is ever returned to wait on.
    #[allow(clippy::zombie_processes)]
    fn test_background_not_found() {
        // Command doesn't exist - should panic
        let result = panic::catch_unwind(|| {
            background::<&str>("/nonexistent/command", &[]);
        });
        assert!(result.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    fn test_background_check_later() {
        let mut child = background("/bin/sh", &["-c", "exit 7"]);
        let status = child.wait().unwrap();
        assert!(!status.success());
        assert_eq!(status.code(), Some(7));
    }
}
