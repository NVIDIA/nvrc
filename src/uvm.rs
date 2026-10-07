// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use crate::{device, macros::ResultExt, nvrc::NVRC};
use nix::sys::stat::{makedev, mknod, Mode, SFlag};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

// NVIDIA's kernel-open/nvidia-uvm/uvm_common.h assigns tools minor 1;
// uvm_tools_init() shares the dynamically allocated nvidia-uvm major.
const UVM_TOOLS_MINOR: u64 = 1;

impl NVRC {
    pub fn setup_uvm_tools(&self) {
        if !self.uvm_tools_enabled {
            return;
        }
        create_tools_node(
            Path::new("/proc/devices"),
            Path::new("/dev/nvidia-uvm-tools"),
        );
    }
}

fn create_tools_node(devices: &Path, node: &Path) {
    let contents = fs::read_to_string(devices).or_panic("read UVM device major");
    let major = device::character_major(&contents, "nvidia-uvm")
        .expect("nvidia-uvm character device major missing or invalid");
    mknod(
        node,
        SFlag::S_IFCHR,
        Mode::from_bits_truncate(0o666),
        makedev(u64::from(major), UVM_TOOLS_MINOR),
    )
    .or_panic("create nvidia-uvm-tools");
    // Match NVIDIA device permissions regardless of the init process umask.
    fs::set_permissions(node, fs::Permissions::from_mode(0o666))
        .or_panic("set nvidia-uvm-tools permissions");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    #[test]
    fn disabled_tools_skip_setup() {
        // The fixed /proc and /dev paths would panic without the early return.
        NVRC::default().setup_uvm_tools();
    }

    #[test]
    #[should_panic(expected = "read UVM device major")]
    fn tools_node_requires_readable_driver_state() {
        let dir = tempfile::tempdir().unwrap();
        create_tools_node(&dir.path().join("missing"), &dir.path().join("tools"));
    }

    #[test]
    #[should_panic(expected = "nvidia-uvm character device major missing or invalid")]
    fn tools_node_requires_a_registered_uvm_driver() {
        let dir = tempfile::tempdir().unwrap();
        let devices = dir.path().join("devices");
        fs::write(&devices, "Character devices:\n195 nvidia\n").unwrap();
        create_tools_node(&devices, &dir.path().join("tools"));
    }

    #[test]
    #[cfg_attr(miri, ignore = "mknod requires CAP_MKNOD and is not emulated by miri")]
    fn tools_node_is_a_character_device_that_is_never_replaced() {
        crate::test_utils::require_root();
        let dir = tempfile::tempdir().unwrap();
        let devices = dir.path().join("devices");
        let node = dir.path().join("nvidia-uvm-tools");
        fs::write(&devices, "Character devices:\n509 nvidia-uvm\n").unwrap();
        create_tools_node(&devices, &node);
        let metadata = fs::symlink_metadata(&node).unwrap();
        assert!(metadata.file_type().is_char_device());
        assert_eq!(nix::sys::stat::major(metadata.rdev()), 509);
        assert_eq!(nix::sys::stat::minor(metadata.rdev()), 1);
        assert_eq!(metadata.mode() & 0o777, 0o666);

        assert!(std::panic::catch_unwind(|| create_tools_node(&devices, &node)).is_err());
        assert_eq!(fs::symlink_metadata(&node).unwrap().ino(), metadata.ino());
    }
}
