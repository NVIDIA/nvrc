// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{anyhow, Context, Result};
use std::process::{Child, Command, Stdio};

use crate::kmsg::kmsg;

pub fn foreground(command: &str, args: &[&str]) -> Result<()> {
    debug!("{} {}", command, args.join(" "));

    let kmsg_file = kmsg().context("Failed to open kmsg device")?;
    let status = Command::new(command)
        .args(args)
        .stdout(Stdio::from(kmsg_file.try_clone().unwrap()))
        .stderr(Stdio::from(kmsg_file))
        .status()
        .context(format!("failed to execute {command}"))?;

    if !status.success() {
        return Err(anyhow!("{} failed with status: {}", command, status));
    }
    Ok(())
}

pub fn background(command: &str, args: &[&str]) -> Result<Child> {
    debug!("{} {}", command, args.join(" "));
    let kmsg_file = kmsg().context("Failed to open kmsg device")?;
    Command::new(command)
        .args(args)
        .stdout(Stdio::from(kmsg_file.try_clone().unwrap()))
        .stderr(Stdio::from(kmsg_file))
        .spawn()
        .with_context(|| format!("Failed to start {}", command))
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    fn run_foreground(command: &str, args: &[&str]) -> std::io::Result<bool> {
        let status = Command::new(command)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        Ok(status.success())
    }

    fn run_background(command: &str, args: &[&str]) -> std::io::Result<std::process::Child> {
        Command::new(command)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    }

    #[test]
    fn test_foreground_success() {
        let result = run_foreground("/bin/true", &[]);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_foreground_failure() {
        let result = run_foreground("/bin/false", &[]);
        assert!(result.is_ok());
        assert!(!result.unwrap()); // exit code 1
    }

    #[test]
    fn test_foreground_not_found() {
        let result = run_foreground("/nonexistent/command", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_foreground_with_args() {
        let result = run_foreground("/bin/sh", &["-c", "exit 0"]);
        assert!(result.is_ok());
        assert!(result.unwrap());

        let result = run_foreground("/bin/sh", &["-c", "exit 42"]);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_background_spawns() {
        let result = run_background("/bin/sleep", &["0.1"]);
        assert!(result.is_ok());
        let mut child = result.unwrap();
        let status = child.wait().unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_background_not_found() {
        let result = run_background("/nonexistent/command", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_background_check_later() {
        let result = run_background("/bin/sh", &["-c", "exit 7"]);
        assert!(result.is_ok());
        let mut child = result.unwrap();
        let status = child.wait().unwrap();
        assert!(!status.success());
        assert_eq!(status.code(), Some(7));
    }
}
