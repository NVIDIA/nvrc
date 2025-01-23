use anyhow::{anyhow, Context, Result};
use std::fs::OpenOptions;
use std::process::{Command, Stdio};

pub fn kmsg() -> std::fs::File {
    let log_path = if log_enabled!(log::Level::Debug) {
        "/dev/kmsg"
    } else {
        "/dev/null"
    };
    OpenOptions::new()
        .write(true)
        .open(log_path)
        .expect("failed to open /dev/kmsg")
}

pub fn foreground(command: &str, args: &[&str]) -> Result<()> {
    debug!("{} {}", command, args.join(" "));

    let output = Command::new(command)
        .stdout(Stdio::from(kmsg().try_clone().unwrap()))
        .stderr(Stdio::from(kmsg()))
        .args(args)
        .output()
        .context(format!("failed to execute {}", command))?;

    if !output.status.success() {
        return Err(anyhow!("{} failed with status: {}", command, output.status,));
    }
    Ok(())
}

pub fn background(command: &str, args: &[&str]) -> Result<()> {
    debug!("{} {}", command, args.join(" "));

    let mut child = Command::new(command)
        .args(args)
        .stdout(Stdio::from(kmsg().try_clone().unwrap()))
        .stderr(Stdio::from(kmsg()))
        .spawn()
        .unwrap_or_else(|_| panic!("failed to start {}", command));

    match child.try_wait() {
        Ok(Some(status)) => Err(anyhow!("{} exited with status: {}", command, status)),
        Ok(None) => Ok(()),
        Err(e) => Err(anyhow!("error attempting to wait: {}", e)),
    }
}
