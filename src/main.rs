// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

mod daemon;
mod execute;
mod kata_agent;
mod kernel_params;
mod kmsg;
mod lockdown;
mod macros;
mod modprobe;
mod mount;
mod nvrc;
mod smi;
mod syslog;
mod toolkit;

pub use macros::ResultExt;

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
    modprobe::load("nvidia");
    modprobe::load("nvidia-uvm");

    init.nvidia_smi_lmc();
    init.nvidia_smi_lgc();
    init.nvidia_smi_pl();

    init.nvidia_persistenced();

    init.nv_hostengine();
    init.dcgm_exporter();
    init.nv_fabricmanager();
    nvidia_ctk_cdi();
    init.nvidia_smi_srs();
    init.check_daemons();
}

/// NVSwitch NVL4 mode for HGX H100/H200/H800 systems (third-gen NVSwitch).
/// Service VM mode for NVLink 4.0 topologies in shared virtualization.
/// Loads NVIDIA driver and starts fabric manager. GPUs are assigned to service VM.
/// Automatically enables fabricmanager regardless of kernel parameters.
fn mode_nvswitch_nvl4(init: &mut NVRC) {
    // Override kernel parameter: always enable fabricmanager for nvswitch mode
    init.fabricmanager_enabled = Some(true);

    modprobe::load("nvidia");
    init.nv_fabricmanager();
    init.check_daemons();
}

/// NVSwitch NVL5 mode for HGX B200/B300/B100 systems (fourth-gen NVSwitch).
/// Service VM mode for NVLink 5.0 topologies with CX7 bridge devices.
/// Does NOT load nvidia driver (GPUs not attached to service VM).
/// Loads ib_umad for InfiniBand MAD access to CX7 bridges.
/// FM automatically starts NVLSM (NVLink Subnet Manager) internally.
/// Requires kernel 5.17+ and /dev/infiniband/umadX devices.
fn mode_nvswitch_nvl5(init: &mut NVRC) {
    // Override kernel parameter: always enable fabricmanager for nvswitch mode
    init.fabricmanager_enabled = Some(true);

    // Load InfiniBand user MAD module for CX7 bridge device access
    modprobe::load("ib_umad");
    init.nv_fabricmanager();
    init.check_daemons();
}

fn main() {
    // Dispatch table allows adding new modes without touching control flow.
    let modes: HashMap<&str, ModeFn> = HashMap::from([
        ("gpu", mode_gpu as ModeFn),
        ("cpu", (|_| {}) as ModeFn),
        ("nvswitch-nvl4", mode_nvswitch_nvl4 as ModeFn),
        ("nvswitch-nvl5", mode_nvswitch_nvl5 as ModeFn),
    ]);

    lockdown::set_panic_hook();
    let mut init = NVRC::default();
    mount::setup();
    kmsg::kernlog_setup();
    syslog::poll();
    mount::readonly("/");
    init.process_kernel_params(None);

    // Kernel param nvrc.mode selects runtime behavior; GPU is the safe default
    // since most users expect full GPU functionality.
    let mode = init.mode.as_deref().unwrap_or("gpu");
    let setup = modes.get(mode).copied().unwrap_or(mode_gpu);
    setup(&mut init);

    lockdown::disable_modules_loading();
    kata_agent::fork_agent(POLL_FOREVER);
}
