// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use super::daemon::foreground;
use anyhow::{Context, Result};
use std::process::Command;

#[allow(dead_code)]
pub fn nvidia_smi() -> Result<()> {
    debug!("nvidia-smi");

    let output = Command::new("/bin/nvidia-smi")
        .output()
        .context("Failed to execute nvidia-smi")?;

    println!(
        "nvidia-smi output:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn ctk(args: &[&str]) -> Result<()> {
    foreground("/bin/nvidia-ctk", args)
}

pub fn nvidia_ctk_cdi() -> Result<()> {
    ctk(&["-d", "cdi", "generate", "--output=/var/run/cdi/nvidia.yaml"])
}
