// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use ::NVRC::daemon::{FABRIC_MODE_FULL, FABRIC_MODE_SHARED};
use ::NVRC::kata_agent::{self, SYSLOG_POLL_FOREVER as POLL_FOREVER};
use ::NVRC::mode::{Fabric, Mode};
use ::NVRC::nvrc::NVRC;
use ::NVRC::toolkit::nvidia_ctk_cdi;
use ::NVRC::{
    gpu_extension, guest_extension_image, hash, init, kmsg, lockdown, mode, modprobe, mount, net,
    syslog,
};
use log::info;

/// VMs with GPU passthrough need driver setup, clock tuning, and monitoring
/// daemons before workloads can use the GPU. A fabric assigned alongside the
/// GPUs starts its services first, in full-passthrough mode.
fn mode_gpu(init: &mut NVRC, fabric: Option<Fabric>) {
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

/// Starts the services for an assigned NVLink fabric. Direct NVSwitch devices
/// are PCI functions of this VM and need only Fabric Manager on the already
/// loaded NVIDIA driver. ConnectX management PFs carry NVLink management for
/// GPUs that may belong to other VMs, so NVLSM brings up the subnet before FM,
/// both on the selected management port's GUID.
fn start_fabric(init: &mut NVRC, fabric: Fabric, fabric_mode: u8) {
    match fabric {
        Fabric::DirectNvSwitch => init.nv_fabricmanager(fabric_mode, "greedy", None),
        Fabric::ConnectX => {
            // Port GUIDs exist only once the RDMA drivers have registered the
            // management PFs. Both drivers are in-tree and may be built in, leaving
            // no module tree for modprobe; their sysfs nodes show they are present.
            modprobe::load_unless_present("ib_umad", "/sys/class/infiniband_mad");
            modprobe::load_unless_present("mlx5_ib", "/sys/bus/auxiliary/drivers/mlx5_ib.rdma");

            let guid = mode::management_port_guid();
            init.nv_nvlsm(&guid);
            init.health_checks();
            init.nv_fabricmanager(fabric_mode, "symmetric", Some(&guid));
        }
    }
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

    let mode = mode::detect();
    if mode.needs_nvidia_driver() {
        modprobe::load_nvidia(mode.nvlink_disabled());
    }
    match mode {
        Mode::Cpu => info!("executing cpu mode"),
        Mode::Gpu { fabric, .. } => mode_gpu(&mut init, fabric),
        // A Service VM has no GPU: only the fabric services run.
        Mode::ServiceVm(fabric) => start_fabric(&mut init, fabric, FABRIC_MODE_SHARED),
    }

    lockdown::disable_modules_loading();
    kata_agent::fork_agent(POLL_FOREVER);
}
