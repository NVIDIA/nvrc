use anyhow::{anyhow, Context, Result};
use std::process::Command;

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
    let output = Command::new("/bin/nvidia-ctk")
        .args([
            "-d",
            "system",
            "create-device-nodes",
            "--control-devices",
            "--load-kernel-modules",
        ])
        .output()
        .context("failed to execute /bin/nvidia-ctk system create-device-nodes")?;

    if !output.status.success() {
        return Err(anyhow!(
            "nvidia-ctk system create-device-nodes failed with status: {}\n error:{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        ));
    }

    trace!(
        "nvidia-ctk system {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}

pub fn nvidia_ctk_cdi() -> Result<()> {
    debug!("nvidia-ctk cdi generate --output=/var/run/cdi/nvidia.yaml");
    // Run the second nvidia-ctk command
    let output = Command::new("/bin/nvidia-ctk")
        .args(["-d", "cdi", "generate", "--output=/var/run/cdi/nvidia.yaml"])
        .output()
        .context("failed to execute /bin/nvidia-ctk cdi generate")?;

    if !output.status.success() {
        return Err(anyhow!(
            "nvidia-ctk cdi generate --output=/var/run/cdi/nvidia.yaml status: {}\n error:{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        ));
    }
    trace!(
        "nvidia-ctk cdi {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}
