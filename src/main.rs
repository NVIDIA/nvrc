// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use ::NVRC::daemon::{FABRIC_MODE_FULL, FABRIC_MODE_SHARED};
use ::NVRC::kata_agent::{self, SYSLOG_POLL_FOREVER as POLL_FOREVER};
use ::NVRC::macros::ResultExt;
use ::NVRC::mode::{Fabric, Mode};
use ::NVRC::nvrc::NVRC;
use ::NVRC::toolkit::nvidia_ctk_cdi;
use ::NVRC::{
    gpu_extension, guest_extension_image, hash, init, kmsg, lockdown, mode, modprobe, mount, net,
    syslog,
};
use log::{debug, info};

/// VMs with GPU passthrough need driver setup, clock tuning,
/// and monitoring daemons before workloads can use the GPU.
/// On bare metal HGX systems (GPUs + NVSwitches), also starts
/// the fabric manager via the appropriate NVSwitch mode.
fn mode_gpu(init: &mut NVRC, fabric: Option<Fabric>, isolated: bool) {
    modprobe::load_nvidia(isolated);
    modprobe::load("nvidia-uvm");
    init.setup_uvm_tools();

    match fabric {
        Some(Fabric::DirectNvSwitch) => mode_direct_nvswitch(init, FABRIC_MODE_FULL),
        Some(Fabric::ConnectX) => mode_connectx(init, FABRIC_MODE_FULL),
        None => {}
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

/// Direct NVSwitch fabric: the switches are PCI functions of this VM. Starts
/// fabric manager; the caller has already loaded the NVIDIA driver.
fn mode_direct_nvswitch(init: &mut NVRC, fabric_mode: u8) {
    init.nv_fabricmanager(fabric_mode, "greedy");
    init.health_checks();
}

/// ConnectX-managed fabric: management PFs, not the GPUs, carry NVLink
/// management, so the GPUs may belong to other VMs.
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
        // A service VM has no GPU, so nothing else has loaded the driver.
        Mode::ServiceVm(Fabric::DirectNvSwitch) => {
            modprobe::load("nvidia");
            mode_direct_nvswitch(&mut init, FABRIC_MODE_SHARED);
        }
        Mode::ServiceVm(Fabric::ConnectX) => mode_connectx(&mut init, FABRIC_MODE_SHARED),
    }

    lockdown::disable_modules_loading();
    kata_agent::fork_agent(POLL_FOREVER);
}
