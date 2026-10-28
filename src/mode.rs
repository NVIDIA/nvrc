// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Startup depends on the assigned management interface, not a board's PF count.

use crate::macros::ResultExt;
use log::{debug, info};
use pcilibs_rs::{
    nvlink::NVIDIA,
    platform::{self, FabricInterface, Topology},
    Sysfs,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fabric {
    DirectNvSwitch,
    ConnectX,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Cpu,
    Gpu {
        fabric: Option<Fabric>,
        /// NVIDIA GPUs assigned to this VM; at least one.
        gpus: usize,
    },
    ServiceVm(Fabric),
}

impl Mode {
    /// nvidia.ko drives GPUs and direct NVSwitches; ConnectX management PFs
    /// use the RDMA drivers instead.
    pub fn needs_nvidia_driver(&self) -> bool {
        matches!(
            self,
            Mode::Gpu { .. } | Mode::ServiceVm(Fabric::DirectNvSwitch)
        )
    }

    /// One GPU and no fabric: nothing sits at the other end of its NVLinks.
    pub fn nvlink_disabled(&self) -> bool {
        matches!(
            self,
            Mode::Gpu {
                fabric: None,
                gpus: 1
            }
        )
    }
}

pub fn detect() -> Mode {
    detect_from(&Sysfs::default())
}

fn detect_from(sysfs: &Sysfs) -> Mode {
    let topology = platform::discover_topology(sysfs).or_panic("discover PCI topology");
    select(&topology)
}

fn select(topology: &Topology) -> Mode {
    debug!("topology: {topology:?}");
    let fabric = match topology.fabric_interface() {
        FabricInterface::None => None,
        FabricInterface::DirectNvSwitch => Some(Fabric::DirectNvSwitch),
        FabricInterface::ConnectX => Some(Fabric::ConnectX),
        FabricInterface::Mixed => panic!("mixed direct NVSwitch and ConnectX management topology"),
    };

    let mode = match (topology.gpus_by_vendor(Some(NVIDIA)).count(), fabric) {
        (0, None) => Mode::Cpu,
        (0, Some(fabric)) => Mode::ServiceVm(fabric),
        (gpus, fabric) => Mode::Gpu { fabric, gpus },
    };
    info!("mode: {mode:?}");
    mode
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::add_management_pf;
    use pcilibs_rs::testfs::fake;
    use rstest::rstest;
    use std::fs;

    fn gpu(fabric: Option<Fabric>, gpus: usize) -> Mode {
        Mode::Gpu { fabric, gpus }
    }

    #[rstest]
    #[case(0, 0, 0, Mode::Cpu)]
    #[case(1, 0, 0, gpu(None, 1))]
    #[case(2, 0, 0, gpu(None, 2))]
    #[case(8, 0, 0, gpu(None, 8))]
    #[case(0, 4, 0, Mode::ServiceVm(Fabric::DirectNvSwitch))]
    #[case(8, 4, 0, gpu(Some(Fabric::DirectNvSwitch), 8))]
    #[case(0, 1, 0, Mode::ServiceVm(Fabric::DirectNvSwitch))]
    #[case(1, 1, 0, gpu(Some(Fabric::DirectNvSwitch), 1))]
    #[case(2, 2, 0, gpu(Some(Fabric::DirectNvSwitch), 2))]
    #[case(0, 0, 1, Mode::ServiceVm(Fabric::ConnectX))]
    #[case(0, 0, 2, Mode::ServiceVm(Fabric::ConnectX))]
    #[case(0, 0, 4, Mode::ServiceVm(Fabric::ConnectX))]
    #[case(0, 0, 6, Mode::ServiceVm(Fabric::ConnectX))]
    #[case(8, 0, 2, gpu(Some(Fabric::ConnectX), 8))]
    #[case(8, 0, 4, gpu(Some(Fabric::ConnectX), 8))]
    #[case(1, 0, 1, gpu(Some(Fabric::ConnectX), 1))]
    fn assigned_devices_choose_startup(
        #[case] gpus: usize,
        #[case] switches: usize,
        #[case] management_pfs: usize,
        #[case] expected: Mode,
    ) {
        let f = fake();
        for i in 0..gpus {
            f.add_pci_device(
                &format!("0000:01:{i:02x}.0"),
                0x10de,
                0x2330,
                0x030200,
                None,
            );
        }
        for i in 0..switches {
            f.add_pci_device(
                &format!("0000:02:{i:02x}.0"),
                0x10de,
                0x22a3,
                0x068000,
                None,
            );
        }
        for i in 0..management_pfs {
            add_management_pf(&f, &format!("0000:03:00.{i}"), i % 2 == 0);
        }
        assert_eq!(detect_from(&f.sysfs), expected);
    }

    #[test]
    fn ordinary_nic_does_not_select_fabric_services() {
        let f = fake();
        add_management_pf(&f, "0000:03:00.0", false);
        assert_eq!(detect_from(&f.sysfs), Mode::Cpu);
    }

    // VFIO hands the guest the host's VPD, but QEMU gives each passed-through
    // PF its own slot, so the unmarked full-capability siblings never share a
    // slot with a marked PF there. The marked PFs alone must select the fabric.
    #[test]
    fn guest_layout_with_one_pf_per_slot_selects_connectx() {
        let f = fake();
        for (i, marked) in [true, true, false, false].into_iter().enumerate() {
            add_management_pf(&f, &format!("0000:{:02x}:00.0", 5 + i), marked);
        }
        assert_eq!(detect_from(&f.sysfs), Mode::ServiceVm(Fabric::ConnectX));
        f.add_pci_device("0000:09:00.0", 0x10de, 0x2901, 0x030200, None);
        assert_eq!(detect_from(&f.sysfs), gpu(Some(Fabric::ConnectX), 1));
    }

    #[rstest]
    #[case(Mode::Cpu, false, false)]
    #[case(gpu(None, 1), true, true)]
    #[case(gpu(None, 2), true, false)]
    #[case(gpu(Some(Fabric::DirectNvSwitch), 1), true, false)]
    #[case(gpu(Some(Fabric::ConnectX), 1), true, false)]
    #[case(Mode::ServiceVm(Fabric::DirectNvSwitch), true, false)]
    #[case(Mode::ServiceVm(Fabric::ConnectX), false, false)]
    fn driver_choices_follow_the_mode(
        #[case] mode: Mode,
        #[case] needs_nvidia_driver: bool,
        #[case] nvlink_disabled: bool,
    ) {
        assert_eq!(mode.needs_nvidia_driver(), needs_nvidia_driver);
        assert_eq!(mode.nvlink_disabled(), nvlink_disabled);
    }

    #[test]
    fn other_vendors_display_functions_do_not_select_gpu_services() {
        let f = fake();
        f.add_pci_device("0000:01:00.0", 0x1002, 0x2330, 0x030000, None);
        assert_eq!(detect_from(&f.sysfs), Mode::Cpu);
        f.add_pci_device("0000:02:00.0", 0x10de, 0x2330, 0x030200, None);
        assert_eq!(detect_from(&f.sysfs), gpu(None, 1));
    }

    #[test]
    #[should_panic(expected = "mixed direct NVSwitch and ConnectX")]
    fn mixed_interfaces_are_rejected() {
        let f = fake();
        f.add_pci_device("0000:02:00.0", 0x10de, 0x22a3, 0x068000, None);
        add_management_pf(&f, "0000:03:00.0", true);
        detect_from(&f.sysfs);
    }

    #[test]
    #[should_panic(expected = "discover PCI topology")]
    fn incomplete_scan_cannot_select_cpu_mode() {
        let f = fake();
        f.add_device("0000:01:00.0", None);
        detect_from(&f.sysfs);
    }

    #[test]
    #[should_panic(expected = "discover PCI topology")]
    fn malformed_vpd_cannot_hide_fabric_hardware() {
        let f = fake();
        add_management_pf(&f, "0000:03:00.0", true);
        fs::write(f.device("0000:03:00.0").join("vpd"), [0x90]).unwrap();
        detect_from(&f.sysfs);
    }
}
