use anyhow::{anyhow, Context, Result};
use std::process::Command;

use super::start_stop_daemon::foreground;

#[allow(dead_code)]
pub fn nvidia_smi() -> Result<()> {
    debug!("nvidia-smi");

    let output = Command::new("/bin/nvidia-smi")
        .output()
        .context("failed to execute nvidia-smi")?;

    println!(
        "nvidia-smi {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}

pub fn nvidia_ctk_system() -> Result<()> {
    debug!("nvidia-ctk system create-device-nodes --control-devices --load-kernel-modules");
    let command = "/bin/nvidia-ctk";
    let args = [
        "-d",
        "system",
        "create-device-nodes",
        "--control-devices",
        "--load-kernel-modules",
    ];
    foreground(command, &args)
}

pub fn nvidia_ctk_cdi() -> Result<()> {
    debug!("nvidia-ctk cdi generate --output=/var/run/cdi/nvidia.yaml");
    let command = "/bin/nvidia-ctk";
    let args = [
        "-d",
        "cdi",
        "generate",
        "--output=/var/run/cdi/nvidia.yaml"
    ];
    foreground(command, &args)
}
