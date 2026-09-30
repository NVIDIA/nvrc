// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Startup depends on the assigned management interface, not a board's PF count.

use crate::macros::ResultExt;
use log::debug;
use pcilibs_rs::{nvlink, Sysfs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fabric {
    DirectNvSwitch,
    ConnectX,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Cpu,
    Gpu(Option<Fabric>),
    ServiceVm(Fabric),
}

pub fn detect() -> Mode {
    detect_from(&Sysfs::default())
}

fn detect_from(sysfs: &Sysfs) -> Mode {
    select(&nvlink::discover(sysfs).or_panic("discover NVLink topology"))
}

fn select(topology: &nvlink::Topology) -> Mode {
    debug!(
        "topology: {} GPU, {} NVSwitch, {} management PF",
        topology.gpus.len(),
        topology.switches.len(),
        topology.management_functions.len()
    );

    let fabric = match (
        topology.switches.is_empty(),
        topology.management_functions.is_empty(),
    ) {
        (true, true) => None,
        (false, true) => Some(Fabric::DirectNvSwitch),
        (true, false) => Some(Fabric::ConnectX),
        // A single startup path cannot manage both interfaces.
        (false, false) => panic!("mixed direct NVSwitch and ConnectX management topology"),
    };

    let mode = match (topology.gpus.is_empty(), fabric) {
        (true, None) => Mode::Cpu,
        (true, Some(fabric)) => Mode::ServiceVm(fabric),
        (false, fabric) => Mode::Gpu(fabric),
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

    #[rstest]
    #[case(0, 0, 0, Mode::Cpu)]
    #[case(1, 0, 0, Mode::Gpu(None))]
    #[case(8, 0, 0, Mode::Gpu(None))]
    #[case(0, 4, 0, Mode::ServiceVm(Fabric::DirectNvSwitch))]
    #[case(8, 4, 0, Mode::Gpu(Some(Fabric::DirectNvSwitch)))]
    #[case(0, 1, 0, Mode::ServiceVm(Fabric::DirectNvSwitch))]
    #[case(2, 2, 0, Mode::Gpu(Some(Fabric::DirectNvSwitch)))]
    #[case(0, 0, 1, Mode::ServiceVm(Fabric::ConnectX))]
    #[case(0, 0, 2, Mode::ServiceVm(Fabric::ConnectX))]
    #[case(0, 0, 4, Mode::ServiceVm(Fabric::ConnectX))]
    #[case(0, 0, 6, Mode::ServiceVm(Fabric::ConnectX))]
    #[case(8, 0, 2, Mode::Gpu(Some(Fabric::ConnectX)))]
    #[case(8, 0, 4, Mode::Gpu(Some(Fabric::ConnectX)))]
    #[case(1, 0, 1, Mode::Gpu(Some(Fabric::ConnectX)))]
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
    #[should_panic(expected = "discover NVLink topology")]
    fn incomplete_scan_cannot_select_cpu_mode() {
        let f = fake();
        f.add_device("0000:01:00.0", None);
        detect_from(&f.sysfs);
    }

    #[test]
    #[should_panic(expected = "discover NVLink topology")]
    fn malformed_vpd_cannot_hide_fabric_hardware() {
        let f = fake();
        add_management_pf(&f, "0000:03:00.0", true);
        fs::write(f.device("0000:03:00.0").join("vpd"), [0x90]).unwrap();
        detect_from(&f.sysfs);
    }
}
