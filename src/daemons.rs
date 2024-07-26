use anyhow::Result;

use crate::proc_cmdline::NVRC;

use super::start_stop_daemon::{foreground, background};

impl NVRC {
    pub fn nvidia_persistenced(&self) -> Result<()> {
        let mut uvm_persistence_mode = "";

        match self.uvm_persistence_mode {
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

        let command = "/bin/nvidia-persistenced";
        let args = [uvm_persistence_mode];

        foreground(command, &args)
    }

    pub fn nv_hostengine(&self) -> Result<()> {
        if !self.dcgm_enabled.unwrap_or(false) {
            return Ok(());
        }
        let command = "/bin/nv-hostengine";
        let args = ["--service-account", "nvidia-dcgm", "--home-dir", "/tmp"];
        foreground(command, &args)
    }

    pub fn dcgm_exporter(&self) -> Result<()> {
        if !self.dcgm_enabled.unwrap_or(false) {
            return Ok(());
        }
        let command = "/bin/dcgm-exporter";
        let args = [
            "-k",
        ];
        background(command, &args)
    }
}
