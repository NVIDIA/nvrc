// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{anyhow, Result};
use std::process::Child;

#[derive(Default)]
#[allow(clippy::upper_case_acronyms)]
pub struct NVRC {
    pub nvidia_smi_srs: Option<String>,
    pub nvidia_smi_lgc: Option<u32>,
    pub nvidia_smi_lmcd: Option<u32>,
    pub nvidia_smi_pl: Option<u32>,
    pub uvm_persistence_mode: Option<bool>,
    pub dcgm_enabled: Option<bool>,
    pub fabricmanager_enabled: Option<bool>,
    children: Vec<(String, Child)>,
}

impl NVRC {
    /// Track a background daemon for later health check
    pub fn track_daemon(&mut self, name: &str, child: Child) {
        self.children.push((name.into(), child));
    }

    /// Check all background daemons haven't failed
    /// Exit status 0 is OK (daemon may fork and parent exits successfully)
    pub fn check_daemons(&mut self) -> Result<()> {
        for (name, child) in &mut self.children {
            if let Ok(Some(status)) = child.try_wait() {
                if !status.success() {
                    return Err(anyhow!("{} exited with status: {}", name, status));
                }
            }
        }
        Ok(())
    }
}
