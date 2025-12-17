// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{anyhow, Context, Result};
use nix::sys::stat::Mode;
use nix::unistd::{chown, mkdir};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::kmsg::kmsg;
use crate::nvrc::NVRC;

#[cfg(feature = "confidential")]
use crate::gpu::confidential::CC;

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
    pub fn nvidia_persistenced(&mut self) -> Result<()> {
        let uvm_flag = match self.uvm_persistence_mode.as_deref() {
            Some("off") => None,
            Some("on") | None => Some("--uvm-persistence-mode"),
            Some(other) => {
                warn!(
                    "Unknown UVM persistence mode '{}', defaulting to 'on'",
                    other
                );
                Some("--uvm-persistence-mode")
            }
        };

        const DIR: &str = "/var/run/nvidia-persistenced"; // scoped constant for readability
        if !Path::new(DIR).exists() {
            mkdir(DIR, Mode::S_IRWXU).with_context(|| format!("Failed to create dir {}", DIR))?;
        }
        chown(
            DIR,
            Some(self.identity.user_id),
            Some(self.identity.group_id),
        )
        .with_context(|| format!("Failed to chown {}", DIR))?;

        let mut args: Vec<&str> = vec!["--verbose"];
        if let Some(f) = uvm_flag {
            args.push(f);
        }

        #[cfg(feature = "confidential")]
        warn!("GPU CC mode build: not setting user/group for nvidia-persistenced");

        // TODO: nvidia-persistenced will not start with -u or -g flag in both modes
        #[cfg(not(feature = "confidential"))]
        {
            let user = self.identity.user_name.clone();
            let group = self.identity.group_name.clone();
            let _owned = [user, group];
            //args.extend_from_slice(&["-u", owned[0].as_str(), "-g", owned[1].as_str()]);
            background("/bin/nvidia-persistenced", &args)
        }
        #[cfg(feature = "confidential")]
        {
            background("/bin/nvidia-persistenced", &args)
        }
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

    #[cfg(feature = "confidential")]
    pub fn nvidia_smi_srs(&self) -> Result<()> {
        if self.gpu_cc_mode != Some(CC::On) {
            debug!("CC mode off; skip nvidia-smi conf-compute -srs");
            return Ok(());
        }
        foreground(
            "/bin/nvidia-smi",
            &[
                "conf-compute",
                "-srs",
                self.nvidia_smi_srs.as_deref().unwrap_or("0"),
            ],
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
