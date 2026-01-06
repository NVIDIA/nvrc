// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

mod coreutils;
mod daemon;
mod execute;
mod kata_agent;
mod kernel_params;
mod kmsg;
mod lockdown;
#[macro_use]
mod macros;
mod modprobe;
mod mount;
mod nvrc;
mod smi;
mod syslog;
mod toolkit;

#[cfg(test)]
mod test_utils;

#[macro_use]
extern crate log;
extern crate kernlog;

use kata_agent::SYSLOG_POLL_FOREVER as POLL_FOREVER;
use nvrc::NVRC;
use toolkit::nvidia_ctk_cdi;

/// Main entry point - orchestrates the init sequence.
/// Each step is tested individually; this is integration glue.
fn main() {
    lockdown::set_panic_hook();
    let mut init = NVRC::default();
    must!(mount::setup());
    must!(syslog::poll());
    must!(kmsg::kernlog_setup());
    must!(mount::readonly("/"));
    must!(init.process_kernel_params(None));

    must!(modprobe::load("nvidia-uvm"));

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
    must!(kata_agent::fork_agent(POLL_FOREVER));
}
