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

    println!(
        "nvidia-smi output:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

pub fn nvidia_ctk_system() -> Result<()> {
    foreground(
        NVIDIA_CTK_PATH,
        &[
            "-d",
            "system",
            "create-device-nodes",
            "--control-devices",
            "--load-kernel-modules",
        ],
    )
}

pub fn nvidia_ctk_cdi() -> Result<()> {
    foreground(
        NVIDIA_CTK_PATH,
        &["-d", "cdi", "generate", "--output=/var/run/cdi/nvidia.yaml"],
    )
}
