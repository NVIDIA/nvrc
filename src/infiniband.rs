// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! FM and NVLSM must use the same PCI-qualified management port.

use crate::macros::ResultExt;
use log::debug;
use pcilibs_rs::{nvlink, Sysfs};

pub fn detect_port_guid() -> Option<String> {
    detect_port_guid_from(&Sysfs::default())
}

fn detect_port_guid_from(sysfs: &Sysfs) -> Option<String> {
    nvlink::discover_management_ports(sysfs)
        .or_panic("discover NVLink management ports")
        .first()
        .map(|port| {
            let guid = format!("0x{:016x}", port.guid);
            debug!(
                "{} {} port {}: GUID {}",
                port.pci_bdf, port.ib_device, port.port, guid
            );
            guid
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::add_management_pf;
    use pcilibs_rs::testfs::{fake, Fake};
    use std::{fs, os::unix::fs::symlink};

    fn add_port(f: &Fake, bdf: &str, name: &str, port: u32, cap_mask: &str, gid: &str) {
        let ib = f.sysfs.infiniband().join(name);
        let path = ib.join(format!("ports/{port}"));
        fs::create_dir_all(path.join("gids")).unwrap();
        symlink(f.device(bdf), ib.join("device")).unwrap();
        fs::write(path.join("link_layer"), "InfiniBand\n").unwrap();
        fs::write(path.join("cap_mask"), cap_mask).unwrap();
        fs::write(path.join("gids/0"), gid).unwrap();
    }

    #[test]
    fn selects_management_pf_even_when_ordinary_nic_sorts_first() {
        let f = fake();
        add_management_pf(&f, "0000:01:00.0", false);
        add_port(&f, "0000:01:00.0", "mlx5_0", 1, "0x200", "fe80::1111");
        add_management_pf(&f, "0000:03:00.0", true);
        add_port(&f, "0000:03:00.0", "mlx5_1", 1, "0x400", "fe80::2222");
        add_management_pf(&f, "0000:03:00.1", false);
        add_port(
            &f,
            "0000:03:00.1",
            "mlx5_2",
            2,
            "0x200",
            "fe80::2:c903:29:7de1",
        );
        assert_eq!(
            detect_port_guid_from(&f.sysfs).as_deref(),
            Some("0x0002c90300297de1")
        );
    }

    #[test]
    fn selects_by_pci_address_instead_of_driver_registration_order() {
        let f = fake();
        for (bdf, name, gid) in [
            ("0000:03:00.0", "mlx5_0", "fe80::2222"),
            ("0000:02:00.0", "mlx5_9", "fe80::1111"),
        ] {
            add_management_pf(&f, bdf, true);
            add_port(&f, bdf, name, 1, "0x200", gid);
        }
        assert_eq!(
            detect_port_guid_from(&f.sysfs).as_deref(),
            Some("0x0000000000001111")
        );
    }

    #[test]
    fn no_sm_enabled_management_port_returns_none() {
        let f = fake();
        add_management_pf(&f, "0000:03:00.0", true);
        add_port(&f, "0000:03:00.0", "mlx5_0", 1, "0x400", "fe80::1111");
        assert_eq!(detect_port_guid_from(&f.sysfs), None);
    }

    #[test]
    #[should_panic(expected = "discover NVLink management ports")]
    fn malformed_capability_cannot_enable_sm() {
        let f = fake();
        add_management_pf(&f, "0000:03:00.0", true);
        add_port(&f, "0000:03:00.0", "mlx5_0", 1, "bad mask", "fe80::1111");
        detect_port_guid_from(&f.sysfs);
    }

    #[test]
    #[should_panic(expected = "discover NVLink management ports")]
    fn invalid_guid_cannot_reach_daemon_configuration() {
        let f = fake();
        add_management_pf(&f, "0000:03:00.0", true);
        add_port(&f, "0000:03:00.0", "mlx5_0", 1, "0x200", "not a GID");
        detect_port_guid_from(&f.sysfs);
    }

    #[test]
    #[should_panic(expected = "discover NVLink management ports")]
    fn missing_rdma_tree_fails_boot() {
        detect_port_guid_from(&fake().sysfs);
    }
}
