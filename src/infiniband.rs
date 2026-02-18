// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! HGX Bx00 uses CX7 bridges instead of direct GPU access for NVLink management.
//! The port GUID from these bridges is required to initialize NVLSM and FM.

use crate::macros::ResultExt;
use log::debug;
use std::fs;
use std::path::Path;

/// SM must be enabled on the port for NVLSM to manage the subnet.
const IS_SM_DISABLED_MASK: u32 = 1 << 10;

/// Returns port GUID from first CX7 bridge with SM enabled, or None.
pub fn detect_port_guid() -> Option<String> {
    detect_port_guid_from("/sys/class/infiniband")
}

fn detect_port_guid_from(ib_class_path: &str) -> Option<String> {
    if !Path::new(ib_class_path).is_dir() {
        panic!("{ib_class_path} not found — mlx5_ib module not loaded");
    }

    let mut entries: Vec<_> = fs::read_dir(ib_class_path)
        .or_panic(format_args!("read {ib_class_path}"))
        .flatten()
        .collect();

    if entries.is_empty() {
        panic!("{ib_class_path} is empty — mlx5_ib loaded but no IB devices registered");
    }

    // Deterministic selection: mlx5_0 before mlx5_1, so first valid SW_MNG device wins.
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let device_name = entry.file_name().to_string_lossy().to_string();
        let device_path = entry.path();

        if !is_sm_enabled(&device_path.join("ports/1/cap_mask")) {
            debug!("{}: SM disabled, skipping", device_name);
            continue;
        }

        if let Some(port_guid) = extract_port_guid(&device_path.join("ports/1/gids/0")) {
            debug!("{}: port GUID {}", device_name, port_guid);
            return Some(port_guid);
        }
    }

    None
}

/// NVLSM cannot manage a port with SM disabled.
fn is_sm_enabled(cap_mask_path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(cap_mask_path) else {
        return false;
    };
    let trimmed = content.trim().trim_start_matches("0x");
    let mask = u32::from_str_radix(trimmed, 16).unwrap_or(0);
    (mask & IS_SM_DISABLED_MASK) == 0
}

/// Port GUID is the last 64 bits of the GID, formatted as 0x-prefixed hex for FM/NVLSM.
fn extract_port_guid(gid_path: &Path) -> Option<String> {
    let content = fs::read_to_string(gid_path).ok()?;
    let parts: Vec<&str> = content.trim().split(':').collect();
    if parts.len() != 8 {
        return None;
    }
    Some(format!(
        "0x{}{}{}{}",
        parts[4], parts[5], parts[6], parts[7]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_ib_device(tmpdir: &TempDir, name: &str, cap_mask: &str, gid: &str) {
        let dev_path = tmpdir.path().join(name);
        let cap_path = dev_path.join("ports/1/cap_mask");
        let gid_path = dev_path.join("ports/1/gids/0");

        fs::create_dir_all(gid_path.parent().unwrap()).unwrap();

        fs::write(&cap_path, cap_mask).unwrap();
        fs::write(&gid_path, gid).unwrap();
    }

    #[test]
    fn test_detect_port_guid_found() {
        let tmpdir = TempDir::new().unwrap();
        create_ib_device(
            &tmpdir,
            "mlx5_0",
            "0x00000200\n", // bit 10 unset, SM enabled
            "fe80:0000:0000:0000:0002:c903:0029:7de1\n",
        );

        let guid = detect_port_guid_from(tmpdir.path().to_str().unwrap());
        assert_eq!(guid, Some("0x0002c90300297de1".to_owned()));
    }

    #[test]
    fn test_detect_port_guid_sm_disabled() {
        let tmpdir = TempDir::new().unwrap();
        create_ib_device(
            &tmpdir,
            "mlx5_0",
            "0x00000400\n", // bit 10 set, SM disabled
            "fe80:0000:0000:0000:0002:c903:0029:7de1\n",
        );

        let guid = detect_port_guid_from(tmpdir.path().to_str().unwrap());
        assert!(guid.is_none());
    }

    #[test]
    fn test_detect_port_guid_skips_sm_disabled() {
        let tmpdir = TempDir::new().unwrap();

        // First device: SM disabled
        create_ib_device(
            &tmpdir,
            "mlx5_0",
            "0x00000400\n",
            "fe80:0000:0000:0000:aaaa:bbbb:cccc:dddd\n",
        );

        // Second device: SM enabled
        create_ib_device(
            &tmpdir,
            "mlx5_1",
            "0x00000200\n",
            "fe80:0000:0000:0000:1111:2222:3333:4444\n",
        );

        let guid = detect_port_guid_from(tmpdir.path().to_str().unwrap());
        assert_eq!(guid, Some("0x1111222233334444".to_owned()));
    }

    #[test]
    #[should_panic(expected = "is empty")]
    fn test_detect_port_guid_empty_dir() {
        let tmpdir = TempDir::new().unwrap();
        detect_port_guid_from(tmpdir.path().to_str().unwrap());
    }

    #[test]
    #[should_panic(expected = "/nonexistent/path not found")]
    fn test_detect_port_guid_nonexistent_dir() {
        detect_port_guid_from("/nonexistent/path");
    }

    #[test]
    fn test_is_sm_enabled_bit_unset() {
        let tmpdir = TempDir::new().unwrap();
        let cap_path = tmpdir.path().join("cap_mask");
        // Bit 10 = 0x400, this mask has bit 10 unset
        fs::write(&cap_path, "0x00000200\n").unwrap();

        assert!(is_sm_enabled(&cap_path));
    }

    #[test]
    fn test_is_sm_enabled_bit_set() {
        let tmpdir = TempDir::new().unwrap();
        let cap_path = tmpdir.path().join("cap_mask");
        // Bit 10 = 0x400, this mask has bit 10 set (SM disabled)
        fs::write(&cap_path, "0x00000400\n").unwrap();

        assert!(!is_sm_enabled(&cap_path));
    }

    #[test]
    fn test_is_sm_enabled_no_file() {
        let tmpdir = TempDir::new().unwrap();
        let cap_path = tmpdir.path().join("nonexistent");

        assert!(!is_sm_enabled(&cap_path));
    }

    #[test]
    fn test_extract_port_guid_valid() {
        let tmpdir = TempDir::new().unwrap();
        let gid_path = tmpdir.path().join("gid");
        fs::write(&gid_path, "fe80:0000:0000:0000:0002:c903:0029:7de1\n").unwrap();

        let guid = extract_port_guid(&gid_path);
        assert_eq!(guid, Some("0x0002c90300297de1".to_owned()));
    }

    #[test]
    fn test_extract_port_guid_invalid_format() {
        let tmpdir = TempDir::new().unwrap();
        let gid_path = tmpdir.path().join("gid");
        fs::write(&gid_path, "invalid:format\n").unwrap();

        assert!(extract_port_guid(&gid_path).is_none());
    }

    #[test]
    fn test_extract_port_guid_no_file() {
        let tmpdir = TempDir::new().unwrap();
        let gid_path = tmpdir.path().join("nonexistent");

        assert!(extract_port_guid(&gid_path).is_none());
    }
}
