// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::Result;
use log::debug;
use std::collections::HashMap;
use std::sync::LazyLock;

// Embedded PCI IDs database
const EMBEDDED_PCI_IDS: &str = include_str!("pci_ids_embedded.txt");

// Cached PCI database - parsed once and reused
static PCI_DATABASE: LazyLock<HashMap<u16, String>> =
    LazyLock::new(|| parse_pci_database_content(EMBEDDED_PCI_IDS).expect("parse embedded PCI db"));

pub const NVIDIA_VENDOR_ID: u16 = 0x10de;

pub mod class_ids {
    pub const VGA_CONTROLLER: u32 = 0x030000;
    pub const DISPLAY_3D_CONTROLLER: u32 = 0x030200;
    pub const BRIDGE_OTHER: u32 = 0x068000;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    Gpu,
    NvSwitch,
    Unknown,
}

pub fn get_pci_ids_database() -> &'static HashMap<u16, String> {
    &PCI_DATABASE
}

fn parse_pci_database_content(content: &str) -> Result<HashMap<u16, String>> {
    let mut devs = HashMap::new();
    let mut nvidia = false;

    for line in content.lines() {
        if line.starts_with("10de  NVIDIA Corporation") {
            nvidia = true;
            continue;
        }

        if nvidia {
            match line {
                // Device entry: "\t<device_id>  <device_name>"
                l if l.starts_with('\t') && !l.starts_with("\t\t") => {
                    if let Some(dl) = l.strip_prefix('\t') {
                        if let Some((id, name)) = dl.split_once("  ") {
                            if let Ok(id) = u16::from_str_radix(id, 16) {
                                devs.insert(id, name.to_string());
                            }
                        }
                    }
                }
                // Subsystem entry (skip these)
                l if l.starts_with("\t\t") => continue,
                // End of NVIDIA section (new vendor) or comment
                l if !l.starts_with('\t') && !l.is_empty() && !l.starts_with('#') => {
                    break;
                }
                // Empty lines or other content
                _ => {}
            }
        }
    }

    Ok(devs)
}

fn is_nvswitch(name: &str) -> bool {
    name.to_ascii_lowercase().contains("nvswitch")
}

fn is_gpu_class(class_id: u32) -> bool {
    matches!(
        class_id,
        class_ids::VGA_CONTROLLER | class_ids::DISPLAY_3D_CONTROLLER
    )
}

const fn is_bridge_class(class_id: u32) -> bool {
    class_id == class_ids::BRIDGE_OTHER
}

/// Determine device type based on PCI class ID and device ID
pub fn classify_device_type(vendor_id: u16, device_id: u16, class_id: u32) -> Result<DeviceType> {
    // Ensure this is an NVIDIA device
    if vendor_id != NVIDIA_VENDOR_ID {
        return Err(anyhow::anyhow!("not nvidia: 0x{vendor_id:04x}"));
    }

    // GPU class IDs are 0x030000 (VGA controller) or 0x030200 (3D controller)
    if is_gpu_class(class_id) {
        return Ok(DeviceType::Gpu);
    }

    // NvSwitch devices have class ID 0x068000 (Bridge device, Other bridge device)
    // Use the PCI database to verify if it's actually an NvSwitch
    if is_bridge_class(class_id) {
        if let Some(name) = get_pci_ids_database().get(&device_id) {
            if is_nvswitch(name) {
                return Ok(DeviceType::NvSwitch);
            }
        }

        // If we can't find it in the database or it's not an NvSwitch, it's unknown but not an error
        debug!("unknown nvidia bridge 0x{device_id:04x} class 0x{class_id:06x}");
        return Ok(DeviceType::Unknown);
    }

    // For other unknown device types, also return Unknown instead of error
    debug!("unknown nvidia device 0x{device_id:04x} class 0x{class_id:06x}");
    Ok(DeviceType::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Determine device type based on device name from PCI database
    pub fn classify_device_type_by_name(n: &str) -> DeviceType {
        if is_nvswitch(n) {
            DeviceType::NvSwitch
        } else {
            DeviceType::Gpu
        }
    }

    #[test]
    fn test_nvswitch_device_classification() {
        // Test known NvSwitch device names
        assert_eq!(
            classify_device_type_by_name("GA100 [A100 NVSwitch]"),
            DeviceType::NvSwitch
        );
        assert_eq!(
            classify_device_type_by_name("GH100 [H100 NVSwitch]"),
            DeviceType::NvSwitch
        );

        // Test GPU device names
        assert_eq!(
            classify_device_type_by_name("GA102GL [RTX A6000]"),
            DeviceType::Gpu
        );
        assert_eq!(
            classify_device_type_by_name("GeForce RTX 4090"),
            DeviceType::Gpu
        );
    }

    #[test]
    fn test_pci_database_access() {
        let db = get_pci_ids_database();
        assert!(!db.is_empty());

        // Check for known NvSwitch entries
        assert!(db.contains_key(&0x1AF1));
        assert!(db.contains_key(&0x22A3));
    }

    #[test]
    fn test_nvswitch_device_classification_by_class() {
        let r = classify_device_type(NVIDIA_VENDOR_ID, 0x1AF1, class_ids::BRIDGE_OTHER);
        assert!(r.is_ok() && r.unwrap() == DeviceType::NvSwitch);

        let r2 = classify_device_type(NVIDIA_VENDOR_ID, 0x22A3, class_ids::BRIDGE_OTHER);
        assert!(r2.is_ok() && r2.unwrap() == DeviceType::NvSwitch);

        let g = classify_device_type(NVIDIA_VENDOR_ID, 0x2230, class_ids::VGA_CONTROLLER);
        assert!(g.is_ok() && g.unwrap() == DeviceType::Gpu);
    }

    #[test]
    fn test_device_type_classification() {
        let g = classify_device_type(NVIDIA_VENDOR_ID, 0x2684, class_ids::VGA_CONTROLLER);
        assert!(g.is_ok() && g.unwrap() == DeviceType::Gpu);

        // Test non-NVIDIA device rejection
        assert!(classify_device_type(0x1234, 0x5678, class_ids::VGA_CONTROLLER).is_err());
    }

    #[test]
    fn test_constants() {
        assert_eq!(NVIDIA_VENDOR_ID, 0x10de);
        assert_eq!(class_ids::VGA_CONTROLLER, 0x030000);
        assert_eq!(class_ids::DISPLAY_3D_CONTROLLER, 0x030200);
        assert_eq!(class_ids::BRIDGE_OTHER, 0x068000);
    }

    #[test]
    fn test_helper_functions() {
        assert!(is_nvswitch("GA100 [A100 NVSwitch]"));
        assert!(is_nvswitch("GH100 [H100 NVSwitch]"));
        assert!(!is_nvswitch("GeForce RTX 4090"));
        assert!(!is_nvswitch("GA102GL [RTX A6000]"));

        assert!(is_gpu_class(class_ids::VGA_CONTROLLER));
        assert!(is_gpu_class(class_ids::DISPLAY_3D_CONTROLLER));
        assert!(!is_gpu_class(class_ids::BRIDGE_OTHER));
        assert!(!is_gpu_class(0x123456));

        assert!(is_bridge_class(class_ids::BRIDGE_OTHER));
        assert!(!is_bridge_class(class_ids::VGA_CONTROLLER));
        assert!(!is_bridge_class(class_ids::DISPLAY_3D_CONTROLLER));
        assert!(!is_bridge_class(0x123456));
    }

    #[test]
    fn test_unknown_device_handling() {
        let u = classify_device_type(NVIDIA_VENDOR_ID, 0x9999, class_ids::BRIDGE_OTHER);
        assert!(u.is_ok() && u.unwrap() == DeviceType::Unknown);

        let u2 = classify_device_type(NVIDIA_VENDOR_ID, 0x1234, 0x999999);
        assert!(u2.is_ok() && u2.unwrap() == DeviceType::Unknown);
    }
}
