// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{anyhow, Context, Result};
use std::process::{Command, Stdio};

use crate::kmsg::kmsg;
use crate::nvrc::NVRC;
use std::fs;

pub fn foreground(command: &str, args: &[&str]) -> Result<()> {
    debug!("{} {}", command, args.join(" "));

    let kmsg_file = kmsg().context("Failed to open kmsg device")?;
    let output = Command::new(command)
        .stdout(Stdio::from(kmsg_file.try_clone().unwrap()))
        .stderr(Stdio::from(kmsg_file))
        .args(args)
        .output()
        .context(format!("failed to execute {command}"))?;

    if !output.status.success() {
        return Err(anyhow!("{} failed with status: {}", command, output.status,));
    }
    Ok(())
}

fn background(command: &str, args: &[&str]) -> Result<()> {
    debug!("{} {}", command, args.join(" "));
    let kmsg_file = kmsg().context("Failed to open kmsg device")?;
    let mut child = Command::new(command)
        .args(args)
        .stdout(Stdio::from(kmsg_file.try_clone().unwrap()))
        .stderr(Stdio::from(kmsg_file))
        .spawn()
        .with_context(|| format!("Failed to start {}", command))?;

    match child.try_wait() {
        Ok(Some(status)) => Err(anyhow!("{} exited with status: {}", command, status)),
        Ok(None) => Ok(()),
        Err(e) => Err(anyhow!("Error attempting to wait: {}", e)),
    }
}

impl NVRC {
    pub fn nvidia_persistenced(&self) -> Result<()> {
        const DIR: &str = "/var/run/nvidia-persistenced";
        fs::create_dir_all(DIR).with_context(|| format!("create_dir_all {}", DIR))?;

        // UVM persistence mode: enabled by default, only "off" disables it
        let uvm_enabled = self.uvm_persistence_mode.as_deref() != Some("off");

        let args: &[&str] = if uvm_enabled {
            &["--verbose", "--uvm-persistence-mode"]
        } else {
            &["--verbose"]
        };

        background("/bin/nvidia-persistenced", args)
    }

    pub fn nv_hostengine(&mut self) -> Result<()> {
        if !self.dcgm_enabled.unwrap_or(false) {
            return Ok(());
        }
        background(
            "/bin/nv-hostengine",
            &["--service-account", "nvidia-dcgm", "--home-dir", "/tmp"],
        )
    }

    pub fn dcgm_exporter(&mut self) -> Result<()> {
        if !self.dcgm_enabled.unwrap_or(false) {
            return Ok(());
        }
        background(
            "/bin/dcgm-exporter",
            &["-k", "-f", "/etc/dcgm-exporter/default-counters.csv"],
        )
    }

    pub fn nv_fabricmanager(&mut self) -> Result<()> {
        if !self.fabricmanager_enabled.unwrap_or(false) {
            return Ok(());
        }
        background(
            "/bin/nv-fabricmanager",
            &["-c", "/usr/share/nvidia/nvswitch/fabricmanager.cfg"],
        )
    }
}
