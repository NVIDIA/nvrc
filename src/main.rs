// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

mod config;
mod daemon;
mod execute;
mod infiniband;
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
fn mode_nvswitch_nvl4(init: &mut NVRC) {
    // Service VM mode requires FABRIC_MODE=1 (shared nvswitch)
    init.fabric_mode = Some(1);

    modprobe::load("nvidia");
    init.nv_fabricmanager();
    init.check_daemons();
}

/// HGX Bx00 systems use CX7 bridges for NVLink management instead of direct GPU access.
/// GPUs are passed to tenant VMs; only the CX7 IB devices are visible here.
fn mode_nvswitch_nvl5(init: &mut NVRC) {
    init.fabric_mode = Some(1);

    // CX7 bridges expose management interface via InfiniBand MAD protocol
    modprobe::load("ib_umad");

    // CX7 port GUID identifies which bridge to use for fabric management
    init.port_guid = Some(
        infiniband::detect_port_guid()
            .expect("nvswitch-nvl5 requires SW_MNG IB device with valid port GUID"),
    );

    // NVLSM must initialize the NVLink subnet before FM can manage the fabric
    init.nv_nvlsm();
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
    init.process_kernel_params(None);

    // Kernel param nvrc.mode selects runtime behavior; GPU is the safe default
    // since most users expect full GPU functionality.
    let mode = init.mode.as_deref().unwrap_or("gpu");
    let setup = modes.get(mode).copied().unwrap_or(mode_gpu);
    setup(&mut init);

    mount::readonly("/");
    lockdown::disable_modules_loading();
    kata_agent::fork_agent(POLL_FOREVER);
}
