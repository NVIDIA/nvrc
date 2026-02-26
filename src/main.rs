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
mod mode;
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

use daemon::FABRIC_MODE_FULL;
use daemon::FABRIC_MODE_SHARED;
use kata_agent::SYSLOG_POLL_FOREVER as POLL_FOREVER;
use nvrc::NVRC;
use toolkit::nvidia_ctk_cdi;

/// VMs with GPU passthrough need driver setup, clock tuning,
/// and monitoring daemons before workloads can use the GPU.
/// On bare metal HGX systems (GPUs + NVSwitches), also starts
/// the fabric manager via the appropriate NVSwitch mode.
fn mode_gpu(init: &mut NVRC, nvswitch: Option<&str>) {
    modprobe::load("nvidia");
    modprobe::load("nvidia-uvm");

    init.nvidia_smi_lmc();
    init.nvidia_smi_lgc();
    init.nvidia_smi_pl();

    init.nvidia_persistenced();

    init.nv_hostengine();
    init.dcgm_exporter();
    nvidia_ctk_cdi();
    init.nvidia_smi_srs();

    nvswitch.inspect(|&nv| {
        let policy = match nv {
            "nvl5" => "symmetric",
            _ => "greedy",
        };
        init.nv_fabricmanager(FABRIC_MODE_FULL, policy);
    });

    init.health_checks();
}

/// NVSwitch NVL4 mode for HGX H100/H200/H800 systems (third-gen NVSwitch).
/// Service VM mode for NVLink 4.0 topologies in shared virtualization.
/// Loads NVIDIA driver and starts fabric manager. GPUs are assigned to service VM.
fn mode_servicevm_nvl4(init: &mut NVRC) {
    modprobe::load("nvidia");
    init.nv_fabricmanager(FABRIC_MODE_SHARED, "greedy");
    init.health_checks();
}

/// HGX Bx00 systems use CX7 bridges for NVLink management instead of direct GPU access.
/// GPUs are passed to tenant VMs; only the CX7 IB devices are visible here.
fn mode_servicevm_nvl5(init: &mut NVRC) {
    // ib_umad exposes /dev/umad* for InfiniBand MAD protocol access;
    // mlx5_ib creates /sys/class/infiniband/mlx5_* entries for the CX7 bridges.
    modprobe::load("ib_umad");
    modprobe::load("mlx5_ib");

    // CX7 port GUID identifies which bridge to use for fabric management
    init.port_guid = Some(
        infiniband::detect_port_guid()
            .expect("servicevm-nvl5 requires SW_MNG IB device with valid port GUID"),
    );

    // NVLSM must initialize the NVLink subnet before FM can manage the fabric
    init.nv_nvlsm();
    init.health_checks();
    init.nv_fabricmanager(FABRIC_MODE_SHARED, "symmetric");
    init.health_checks();
}

fn main() {
    lockdown::set_panic_hook();
    let mut init = NVRC::default();
    mount::setup();
    kmsg::kernlog_setup();
    syslog::poll();
    init.process_kernel_params(None);

    let detected = mode::detect();
    match detected.mode {
        "cpu" => info!("executing cpu mode"),
        "gpu" => mode_gpu(&mut init, detected.nvswitch),
        "servicevm-nvl4" => mode_servicevm_nvl4(&mut init),
        "servicevm-nvl5" => mode_servicevm_nvl5(&mut init),
        unknown => panic!("unknown mode: {unknown}"),
    }

    lockdown::disable_modules_loading();
    kata_agent::fork_agent(POLL_FOREVER);
}
