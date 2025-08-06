use super::daemon::foreground;
use anyhow::{Context, Result};
use std::process::Command;

const NVIDIA_SMI_PATH: &str = "/bin/nvidia-smi";
const NVIDIA_CTK_PATH: &str = "/bin/nvidia-ctk";

#[allow(dead_code)]
pub fn nvidia_smi() -> Result<()> {
    debug!("nvidia-smi");

    let output = Command::new(NVIDIA_SMI_PATH)
        .output()
        .context("Failed to execute nvidia-smi")?;

    // Use write! macro for better formatting and combine stdout/stderr more elegantly
    let combined_output = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    println!("nvidia-smi output:\n{}", combined_output.trim());
    Ok(())
}

pub fn nvidia_ctk_system() -> Result<()> {
    let args = [
        "-d",
        "system",
        "create-device-nodes",
        "--control-devices",
        "--load-kernel-modules",
    ];
    foreground(NVIDIA_CTK_PATH, &args)
}

pub fn nvidia_ctk_cdi() -> Result<()> {
    let args = ["-d", "cdi", "generate", "--output=/var/run/cdi/nvidia.yaml"];
    foreground(NVIDIA_CTK_PATH, &args)
}
