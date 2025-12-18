// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{Context, Result};

use crate::execute::background;
use crate::nvrc::NVRC;
use std::fs;

impl NVRC {
    pub fn nvidia_persistenced(&mut self) -> Result<()> {
        const DIR: &str = "/var/run/nvidia-persistenced";
        fs::create_dir_all(DIR).with_context(|| format!("create_dir_all {}", DIR))?;

        // UVM persistence mode: enabled by default
        let uvm_enabled = self.uvm_persistence_mode.unwrap_or(true);

        let args: &[&str] = if uvm_enabled {
            &["--verbose", "--uvm-persistence-mode"]
        } else {
            &["--verbose"]
        };

        let child = background("/bin/nvidia-persistenced", args)?;
        self.track_daemon("nvidia-persistenced", child);
        Ok(())
    }

    pub fn nv_hostengine(&mut self) -> Result<()> {
        if !self.dcgm_enabled.unwrap_or(false) {
            return Ok(());
        }
        let child = background(
            "/bin/nv-hostengine",
            &["--service-account", "nvidia-dcgm", "--home-dir", "/tmp"],
        )?;
        self.track_daemon("nv-hostengine", child);
        Ok(())
    }

    pub fn dcgm_exporter(&mut self) -> Result<()> {
        if !self.dcgm_enabled.unwrap_or(false) {
            return Ok(());
        }
        let child = background(
            "/bin/dcgm-exporter",
            &["-k", "-f", "/etc/dcgm-exporter/default-counters.csv"],
        )?;
        self.track_daemon("dcgm-exporter", child);
        Ok(())
    }

    pub fn nv_fabricmanager(&mut self) -> Result<()> {
        if !self.fabricmanager_enabled.unwrap_or(false) {
            return Ok(());
        }
        let child = background(
            "/bin/nv-fabricmanager",
            &["-c", "/usr/share/nvidia/nvswitch/fabricmanager.cfg"],
        )?;
        self.track_daemon("nv-fabricmanager", child);
        Ok(())
    }
}
