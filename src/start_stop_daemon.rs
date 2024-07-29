use anyhow::{anyhow, Context, Result};
use std::process::{Command, Stdio};

pub fn foreground(command: &str, args: &[&str]) -> Result<()> {
    debug!("{} {}", command, args.join(" "));

    let output = Command::new(command)
        .args(args)
        .output()
        .context(format!("failed to execute {}", command))?;

    if !output.status.success() {
        return Err(anyhow!(
            "{} failed with status: {}\n error:{}\n{}",
            command,
            output.status,
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        ));
    }
    if !output.stdout.is_empty() {
        debug!("{}", String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        debug!("{}", String::from_utf8_lossy(&output.stderr));
    }

    Ok(())
}

pub fn background(command: &str, args: &[&str]) -> Result<()> {
    debug!("{} {}", command, args.join(" "));

    let mut child = Command::new(command)
        .args(args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|_| panic!("failed to start {}", command));

    match child.try_wait() {
        Ok(Some(status)) => Err(anyhow!("{} exited with status: {}", command, status)),
        Ok(None) => Ok(()),
        Err(e) => Err(anyhow!("error attempting to wait: {}", e)),
    }
}
