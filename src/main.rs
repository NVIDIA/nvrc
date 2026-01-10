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

use std::collections::HashMap;

use kata_agent::SYSLOG_POLL_FOREVER as POLL_FOREVER;
use nvrc::NVRC;
use toolkit::nvidia_ctk_cdi;

type ModeFn = fn(&mut NVRC);

/// VMs with GPU passthrough need driver setup, clock tuning,
/// and monitoring daemons before workloads can use the GPU.
fn mode_gpu(init: &mut NVRC) {
    must!(modprobe::load("nvidia"));
    must!(modprobe::load("nvidia-uvm"));

    must!(init.nvidia_smi_lmc());
    must!(init.nvidia_smi_lgc());
    must!(init.nvidia_smi_pl());

    must!(init.nvidia_persistenced());

    must!(init.nv_hostengine());
    must!(init.dcgm_exporter());
    must!(init.nv_fabricmanager());
    must!(nvidia_ctk_cdi());
    must!(init.nvidia_smi_srs());
    must!(init.check_daemons());
}

/// NVSwitch NVL4 mode for HGX H100/H200/H800 systems.
/// Loads NVIDIA driver and starts fabric manager for NVLink 4.0 topologies.
/// Used for NVSwitch configurations without GPU compute workloads.
/// Automatically enables fabricmanager regardless of kernel parameters.
fn mode_nvswitch_nvl4(init: &mut NVRC) {
    // Override kernel parameter: always enable fabricmanager for nvswitch mode
    init.fabricmanager_enabled = Some(true);

    must!(modprobe::load("nvidia"));
    must!(init.nv_fabricmanager());
    must!(init.check_daemons());
}

fn main() {
    // Dispatch table allows adding new modes without touching control flow.
    let modes: HashMap<&str, ModeFn> = HashMap::from([
        ("gpu", mode_gpu as ModeFn),
        ("cpu", (|_| {}) as ModeFn),
        ("nvswitch-nvl4", mode_nvswitch_nvl4 as ModeFn),
    ]);

    must!(lockdown::set_panic_hook());
    let mut init = NVRC::default();
    must!(mount::setup());
    must!(kmsg::kernlog_setup());
    must!(syslog::poll());
    must!(mount::readonly("/"));
    must!(init.process_kernel_params(None));

    // Kernel param nvrc.mode selects runtime behavior; GPU is the safe default
    // since most users expect full GPU functionality.
    let mode = init.mode.as_deref().unwrap_or("gpu");
    let setup = modes.get(mode).copied().unwrap_or(mode_gpu);
    setup(&mut init);

    must!(lockdown::disable_modules_loading());
    must!(kata_agent::fork_agent(POLL_FOREVER));
}
