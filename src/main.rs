// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

mod coreutils;
mod daemon;
mod execute;
mod kata_agent;
mod kernel_params;
mod kmsg;
mod lockdown;
mod modprobe;
mod mount;
mod nvrc;
mod smi;
mod syslog;
mod toolkit;

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

use nvrc::NVRC;
use toolkit::nvidia_ctk_cdi;

fn main() {
    lockdown::set_panic_hook();
    let mut init = NVRC::default();
    must!(mount::setup());
    must!(syslog::poll());
    must!(kmsg::kernlog_setup());
    must!(mount::readonly("/"));
    must!(init.process_kernel_params(None));

    must!(modprobe::nvidia());
    must!(modprobe::nvidia_uvm());

    must!(init.nvidia_smi_lmcd());
    must!(init.nvidia_smi_lgc());
    must!(init.nvidia_smi_pl());

    must!(init.nvidia_persistenced());

    must!(lockdown::disable_modules_loading());
    must!(init.nv_hostengine());
    must!(init.dcgm_exporter());
    must!(init.nv_fabricmanager());
    must!(nvidia_ctk_cdi());
    must!(init.nvidia_smi_srs());
    must!(init.check_daemons());
    must!(kata_agent::fork_agent());
}
