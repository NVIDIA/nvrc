// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Startup depends on the assigned management interface, not a board's PF count.

use crate::macros::ResultExt;
use log::debug;
use pcilibs_rs::{
    nvlink,
    platform::{FabricInterface, Platform},
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
    Gpu(Option<Fabric>),
    ServiceVm(Fabric),
}

pub fn detect() -> Mode {
    detect_from(&Sysfs::default())
}

fn detect_from(sysfs: &Sysfs) -> Mode {
    let detected = nvlink::discover_platform(sysfs).or_panic("discover NVLink platform");
    select(&detected.platform)
}

fn select(platform: &Platform) -> Mode {
    debug!("platform: {platform:?}");
    let fabric = match platform.fabric {
        FabricInterface::None => None,
        FabricInterface::DirectNvSwitch => Some(Fabric::DirectNvSwitch),
        FabricInterface::ConnectX => Some(Fabric::ConnectX),
        FabricInterface::Mixed => panic!("mixed direct NVSwitch and ConnectX management topology"),
    };

    let mode = match (platform.gpu_count == 0, fabric) {
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
    use pcilibs_rs::{platform::Kind, testfs::fake};
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
        for i in 0..gpus {
            fs::write(
                f.device(&format!("0000:01:{i:02x}.0"))
                    .join("subsystem_device"),
                "0x16c0",
            )
            .unwrap();
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
    #[should_panic(expected = "discover NVLink platform")]
    fn incomplete_scan_cannot_select_cpu_mode() {
        let f = fake();
        f.add_device("0000:01:00.0", None);
        detect_from(&f.sysfs);
    }

    #[test]
    #[should_panic(expected = "discover NVLink platform")]
    fn malformed_vpd_cannot_hide_fabric_hardware() {
        let f = fake();
        add_management_pf(&f, "0000:03:00.0", true);
        fs::write(f.device("0000:03:00.0").join("vpd"), [0x90]).unwrap();
        detect_from(&f.sysfs);
    }

    #[rstest]
    #[case(0x2330, 0x16c0, true, Kind::HgxHx00, Fabric::DirectNvSwitch)]
    #[case(0x2901, 0x1999, false, Kind::HgxBx00, Fabric::ConnectX)]
    #[case(0x3002, 0x2277, false, Kind::HgxRx00, Fabric::ConnectX)]
    #[case(
        0x3041,
        0x221a,
        false,
        Kind::Coherent(pcilibs_rs::gpu::Family::Rubin),
        Fabric::ConnectX
    )]
    #[case(
        0x307e,
        0x221a,
        false,
        Kind::Coherent(pcilibs_rs::gpu::Family::Rubin),
        Fabric::ConnectX
    )]
    #[case(
        0x30ff,
        0x221b,
        false,
        Kind::Coherent(pcilibs_rs::gpu::Family::Rubin),
        Fabric::ConnectX
    )]
    fn shared_platform_profiles_choose_fabric_startup(
        #[case] device: u16,
        #[case] subsystem: u16,
        #[case] direct: bool,
        #[case] kind: Kind,
        #[case] fabric: Fabric,
    ) {
        let f = fake();
        f.add_pci_device("0000:01:00.0", 0x10de, device, 0x030200, None);
        fs::write(
            f.device("0000:01:00.0").join("subsystem_device"),
            format!("{subsystem:#06x}"),
        )
        .unwrap();
        if direct {
            f.add_pci_device("0000:02:00.0", 0x10de, 0x22a3, 0x068000, None);
        } else {
            add_management_pf(&f, "0000:03:00.0", true);
        }
        let detected = nvlink::discover_platform(&f.sysfs).unwrap();
        assert_eq!(detected.platform.kind, kind);
        assert_eq!(detect_from(&f.sysfs), Mode::Gpu(Some(fabric)));
    }

    #[test]
    fn mixed_gpu_families_without_a_fabric_still_use_gpu_mode() {
        assert_eq!(
            select(&Platform {
                kind: Kind::Mixed,
                fabric: FabricInterface::None,
                gpu_count: 2,
            }),
            Mode::Gpu(None)
        );
    }
}
