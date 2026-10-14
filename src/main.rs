// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

mod config;
mod daemon;
mod device;
mod execute;
mod gpu_extension;
mod guest_extension_image;
mod hash;
mod init;
mod kata_agent;
mod kernel_params;
mod kmsg;
mod lockdown;
mod macros;
mod mode;
mod modprobe;
mod mount;
mod net;
mod nvrc;
mod smi;
mod syslog;
mod toolkit;
mod uvm;

pub use macros::ResultExt;

#[cfg(test)]
mod test_utils;

#[macro_use]
extern crate log;
extern crate kernlog;

use daemon::FABRIC_MODE_FULL;
use daemon::FABRIC_MODE_SHARED;
use kata_agent::SYSLOG_POLL_FOREVER as POLL_FOREVER;
use mode::{Fabric, Mode};
use nvrc::NVRC;
use toolkit::nvidia_ctk_cdi;

/// VMs with GPU passthrough need driver setup, clock tuning,
/// and monitoring daemons before workloads can use the GPU.
/// On bare metal HGX systems (GPUs + NVSwitches), also starts
/// the fabric manager via the appropriate NVSwitch mode.
fn mode_gpu(init: &mut NVRC, fabric: Option<Fabric>, isolated: bool) {
    modprobe::load_nvidia(isolated);
    modprobe::load("nvidia-uvm");
    init.setup_uvm_tools();

    if let Some(fabric) = fabric {
        start_fabric(init, fabric, FABRIC_MODE_FULL);
    }

    init.nvidia_persistenced();

    init.nvidia_smi_lmc();
    init.nvidia_smi_lgc();
    init.nvidia_smi_pl();

    init.nv_hostengine();
    init.dcgm_exporter();
    nvidia_ctk_cdi();
    init.nvidia_smi_srs();
    init.health_checks();
}

fn start_fabric(init: &mut NVRC, fabric: Fabric, fabric_mode: u8) {
    match fabric {
        Fabric::DirectNvSwitch => mode_direct_nvswitch(init, fabric_mode),
        Fabric::ConnectX => mode_connectx(init, fabric_mode),
    }
}

fn mode_direct_nvswitch(init: &mut NVRC, fabric_mode: u8) {
    modprobe::load("nvidia");
    init.nv_fabricmanager(fabric_mode, "greedy");
    init.health_checks();
}

fn mode_connectx(init: &mut NVRC, fabric_mode: u8) {
    // Management GUIDs appear only after the RDMA drivers register their ports.
    modprobe::load("ib_umad");
    modprobe::load("mlx5_ib");

    let ports = pcilibs_rs::nvlink::discover_management_ports(&pcilibs_rs::Sysfs::default())
        .or_panic("discover NVLink management ports");
    let port = ports
        .first()
        .expect("ConnectX fabric management requires an SM-enabled management port");
    init.port_guid = Some(format!("0x{:016x}", port.guid));
    debug!(
        "{} {} port {}: GUID {:#018x}",
        port.pci_bdf, port.ib_device, port.port, port.guid
    );

    // NVLSM must initialize the NVLink subnet before FM can manage the fabric
    init.nv_nvlsm();
    init.health_checks();
    init.nv_fabricmanager(fabric_mode, "symmetric");
    init.health_checks();
}

fn main() {
    init::as_pid1();

    lockdown::set_panic_hook();
    let mut init = NVRC::default();
    mount::setup();
    net::loopback_up();
    kmsg::kernlog_setup();
    syslog::poll();
    init.process_kernel_params(None);
    hash::self_exe();

    // Before disable_modules_loading() so dm-verity/erofs modules can still load.
    guest_extension_image::mount_all();

    // Expose gpu-extension libs/firmware before any driver load. No-op if absent.
    gpu_extension::setup();

    match mode::detect() {
        Mode::Cpu => info!("executing cpu mode"),
        Mode::Gpu { fabric, isolated } => mode_gpu(&mut init, fabric, isolated),
        Mode::ServiceVm(fabric) => start_fabric(&mut init, fabric, FABRIC_MODE_SHARED),
    }

    lockdown::disable_modules_loading();
    kata_agent::fork_agent(POLL_FOREVER);
}
