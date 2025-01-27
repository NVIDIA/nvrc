use anyhow::Result;

use super::start_stop_daemon::{background, foreground};
use crate::proc_cmdline::NVRC;

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
        let args = [
            "--verbose",
            uvm_persistence_mode,
            //           "-u",
            //          &self.user_group.user_name,
            //        "-g",
            //      &self.user_group.group_name,
        ];

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
        let args = ["-k"];
        background(command, &args)
    }

    pub fn nvidia_smi_srs(&self) -> Result<()> {
        if self.gpu_cc_mode != Some("on".to_string()) {
            debug!("CC mode is off, skipping nvidia-smi conf-compute -srs");
            return Ok(());
        }
        let command = "/bin/nvidia-smi";
        let args = [
            "conf-compute",
            "-srs",
            self.nvidia_smi_srs.as_deref().unwrap_or("0"),
        ];
        foreground(command, &args)
    }
}
