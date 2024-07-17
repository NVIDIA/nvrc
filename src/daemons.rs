use anyhow::{anyhow, Context, Result};
use std::process::Command;

use crate::proc_cmdline::NVRC;

pub fn nvidia_persistenced(context: &NVRC) -> Result<()> {
    let mut uvm_persistence_mode = "";

    match context.uvm_persistence_mode {
        Some(ref mode) => {
            if mode == "1" {
                uvm_persistence_mode = "--uvm-persistence-mode";
            } else if mode == "0" {
                uvm_persistence_mode = "";
            }
        }
        None => {
            uvm_persistence_mode = "--uvm-persistence-mode";
        }
    }

    info!("nvidia-persistenced {}", uvm_persistence_mode);

    let output = Command::new("/bin/nvidia-persistenced")
        .args([uvm_persistence_mode])
        .output()
        .context("failed to execute /bin/nvidia-persistenced")?;

    if !output.status.success() {
        return Err(anyhow!(
            "nvidia-persistenced failed with status: {}\n error:{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        ));
    }

    Ok(())
}
