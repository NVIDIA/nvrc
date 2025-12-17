// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

mod coreutils;
mod daemon;
mod kata_agent;
mod kmsg;
mod lockdown;
mod mount;
mod nvrc;
mod syslog;
mod toolkit;
mod user_group;

#[macro_use]
extern crate log;
extern crate kernlog;

use anyhow::Result;

macro_rules! must {
    ($expr:expr) => {
        if let Err(e) = $expr {
            panic!("init failure: {} => {e}", stringify!($expr));
        }
    };
    ($expr:expr, $msg:literal) => {
        if let Err(e) = $expr {
            panic!("init failure: {}: {e}", $msg);
        }
    };
}

use nvrc::NVRC;
use toolkit::{nvidia_ctk_cdi, nvidia_ctk_system};

fn main() {
    lockdown::set_panic_hook();
    let mut init = NVRC::default();
    must!(mount::setup());
    must!(syslog::init());
    must!(kmsg::kernlog_setup());
    must!(init.set_random_identity());
    must!(mount::readonly("/"));
    must!(init.process_kernel_params(None));
    must!(init.setup_gpu());
    must!(kata_agent::fork_agent());
}

impl NVRC {
    fn setup_gpu(&mut self) -> Result<()> {
        nvidia_ctk_system()?;
        self.nvidia_persistenced()?;
        lockdown::disable_modules_loading()?;
        self.nv_hostengine()?;
        self.dcgm_exporter()?;
        self.nv_fabricmanager()?;
        nvidia_ctk_cdi()?;
        self.nvidia_smi_srs()?;
        Ok(())
    }
}
