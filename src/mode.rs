// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Startup depends on the assigned management interface, not a board's PF count.

use crate::macros::ResultExt;
use log::debug;
use pcilibs_rs::{
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
        /// One GPU and no fabric: nothing sits at the other end of its NVLinks.
        isolated: bool,
    },
    ServiceVm(Fabric),
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

    let mode = match (topology.gpus.len(), fabric) {
        (0, None) => Mode::Cpu,
        (0, Some(fabric)) => Mode::ServiceVm(fabric),
        (gpus, fabric) => Mode::Gpu {
            fabric,
            isolated: gpus == 1 && fabric.is_none(),
        },
    };
    debug!("mode: {mode:?}");
    mode
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::add_management_pf;
    use pcilibs_rs::testfs::fake;
    use rstest::rstest;
    use std::fs;

    const ISOLATED_GPU: Mode = Mode::Gpu {
        fabric: None,
        isolated: true,
    };

    fn linked(fabric: Option<Fabric>) -> Mode {
        Mode::Gpu {
            fabric,
            isolated: false,
        }
    }

    #[rstest]
    #[case(0, 0, 0, Mode::Cpu)]
    #[case(1, 0, 0, ISOLATED_GPU)]
    #[case(2, 0, 0, linked(None))]
    #[case(8, 0, 0, linked(None))]
    #[case(0, 4, 0, Mode::ServiceVm(Fabric::DirectNvSwitch))]
    #[case(8, 4, 0, linked(Some(Fabric::DirectNvSwitch)))]
    #[case(0, 1, 0, Mode::ServiceVm(Fabric::DirectNvSwitch))]
    #[case(1, 1, 0, linked(Some(Fabric::DirectNvSwitch)))]
    #[case(2, 2, 0, linked(Some(Fabric::DirectNvSwitch)))]
    #[case(0, 0, 1, Mode::ServiceVm(Fabric::ConnectX))]
    #[case(0, 0, 2, Mode::ServiceVm(Fabric::ConnectX))]
    #[case(0, 0, 4, Mode::ServiceVm(Fabric::ConnectX))]
    #[case(0, 0, 6, Mode::ServiceVm(Fabric::ConnectX))]
    #[case(8, 0, 2, linked(Some(Fabric::ConnectX)))]
    #[case(8, 0, 4, linked(Some(Fabric::ConnectX)))]
    #[case(1, 0, 1, linked(Some(Fabric::ConnectX)))]
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
