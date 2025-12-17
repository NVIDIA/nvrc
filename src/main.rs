// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

mod attach;
mod coreutils;
mod cpu;
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

use kata_agent::kata_agent;
use nvrc::NVRC;
use toolkit::{nvidia_ctk_cdi, nvidia_ctk_system};

fn main() {
    lockdown::set_panic_hook();
    let mut init = NVRC::default();
    must!(mount::setup());
    must!(kmsg::kernlog_setup());
    must!(init.setup_syslog());
    must!(init.set_random_identity());
    must!(mount::readonly("/"));
    must!(init.process_kernel_params(None));
    must!(init.query_cpu_vendor());
    must!(init.cold_plug());
}

impl NVRC {
    fn setup_gpu(&mut self) {
        must!(nvidia_ctk_system());
        must!(self.nvidia_persistenced());
        must!(lockdown::disable_modules_loading());
        must!(self.nv_hostengine());
        must!(self.dcgm_exporter());
        must!(self.nv_fabricmanager());
        must!(nvidia_ctk_cdi());
        must!(self.nvidia_smi_srs());
    }
}
