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
    if output.stdout.len() > 0 {
        debug!("{}", String::from_utf8_lossy(&output.stdout));
    }
    if output.stderr.len() > 0 {
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
        .expect(format!("failed to start {}", command).as_str());

    match child.try_wait() {
        Ok(Some(status)) => return Err(anyhow!("{} exited with status: {}", command, status)),
        Ok(None) => return Ok(()),
        Err(e) => return Err(anyhow!("error attempting to wait: {}", e)),
    }
}
