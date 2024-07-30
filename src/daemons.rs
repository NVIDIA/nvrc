use anyhow::Result;

use crate::proc_cmdline::NVRC;
use crate::user_group::random_user_group;

use super::start_stop_daemon::{background, foreground};

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

        let _user_group = random_user_group();

        let command = "/bin/nvidia-persistenced";
        let args = [
            "--verbose",
            uvm_persistence_mode,
            //            "-u",
            //            &user_group.user_name,
            //            "-g",
            //            &user_group.group_name,
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
}
