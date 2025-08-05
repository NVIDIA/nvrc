use anyhow::Result;
use std::collections::HashMap;

// Re-export embedded PCI IDs from query_cc_mode module
const EMBEDDED_PCI_IDS: &str = include_str!("pci_ids_embedded.txt");

pub const NVIDIA_VENDOR_ID: u16 = 0x10de;

pub mod class_ids {
    pub const VGA_CONTROLLER: u32 = 0x030000;
    pub const DISPLAY_3D_CONTROLLER: u32 = 0x030200;
    pub const BRIDGE_OTHER: u32 = 0x068000;
}

/// Device types for NVIDIA hardware
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceType {
    Gpu,
    NvSwitch,
    Unknown,
}

pub fn get_pci_ids_database() -> Result<HashMap<u16, String>> {
    parse_pci_database_content(EMBEDDED_PCI_IDS)
}

fn parse_pci_database_content(content: &str) -> Result<HashMap<u16, String>> {
    let mut nvidia_devices = HashMap::new();
    let mut in_nvidia_section = false;

    for line in content.lines() {
        if line.starts_with("10de  NVIDIA Corporation") {
            in_nvidia_section = true;
            continue;
        }

        if in_nvidia_section {
            if line.starts_with('\t') && !line.starts_with("\t\t") {
                // Device entry: "\t<device_id>  <device_name>"
                if let Some(parts) = line.strip_prefix('\t') {
                    if let Some((id_str, name)) = parts.split_once("  ") {
                        if let Ok(device_id) = u16::from_str_radix(id_str, 16) {
                            nvidia_devices.insert(device_id, name.to_string());
                        }
                    }
                }
            } else if line.starts_with("\t\t") {
                // Subsystem entry (skip these)
                continue;
            } else if !line.starts_with('\t') && !line.is_empty() && !line.starts_with('#') {
                // End of NVIDIA section (new vendor)
                break;
            }
        }
    }

    Ok(nvidia_devices)
}

fn is_nvswitch(device_name: &str) -> bool {
    let name_lower = device_name.to_lowercase();

    // NvSwitch devices typically have "nvswitch" in their name
    // Examples: "GA100 [A100 NVSwitch]", "GH100 [H100 NVSwitch]"
    name_lower.contains("nvswitch")
}

fn is_gpu_class(class_id: u32) -> bool {
    class_id == class_ids::VGA_CONTROLLER || class_id == class_ids::DISPLAY_3D_CONTROLLER
}

fn is_bridge_class(class_id: u32) -> bool {
    class_id == class_ids::BRIDGE_OTHER
}

/// Determine device type based on device name from PCI database
pub fn classify_device_type_by_name(device_name: &str) -> DeviceType {
    if is_nvswitch(device_name) {
        DeviceType::NvSwitch
    } else {
        // For all other NVIDIA devices, assume they are GPUs
        // This is a reasonable default since most NVIDIA devices are GPUs
        DeviceType::Gpu
    }
}

/// Determine device type based on PCI class ID and device ID
pub fn classify_device_type(vendor_id: u16, device_id: u16, class_id: u32) -> Result<DeviceType> {
    // NVIDIA vendor ID is 0x10de
    if vendor_id != NVIDIA_VENDOR_ID {
        return Err(anyhow::anyhow!(
            "Not an NVIDIA device (vendor ID: 0x{:04x})",
            vendor_id
        ));
    }

    // GPU class IDs are 0x030000 (VGA controller) or 0x030200 (3D controller)
    if is_gpu_class(class_id) {
        return Ok(DeviceType::Gpu);
    }

    // NvSwitch devices have class ID 0x068000 (Bridge device, Other bridge device)
    // Use the PCI database to verify if it's actually an NvSwitch
    if is_bridge_class(class_id) {
        // Try to get device name from PCI database
        if let Ok(pci_db) = get_pci_ids_database() {
            if let Some(device_name) = pci_db.get(&device_id) {
                let device_type = classify_device_type_by_name(device_name);
                if device_type == DeviceType::NvSwitch {
                    return Ok(DeviceType::NvSwitch);
                }
            }
        }

        // If we can't find it in the database or it's not an NvSwitch, it's unknown but not an error
        debug!("Unknown NVIDIA bridge device (device ID: 0x{:04x}, class: 0x{:06x})", device_id, class_id);
        return Ok(DeviceType::Unknown);
    }

    // For other unknown device types, also return Unknown instead of error
    debug!("Unknown NVIDIA device type (device ID: 0x{:04x}, class: 0x{:06x})", device_id, class_id);
    Ok(DeviceType::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let pci_db = get_pci_ids_database().expect("Should be able to load embedded PCI database");
        assert!(!pci_db.is_empty(), "PCI database should not be empty");

        // Check for known NvSwitch entries
        assert!(
            pci_db.contains_key(&0x1AF1),
            "Should contain A100 NvSwitch device ID"
        );
        assert!(
            pci_db.contains_key(&0x22A3),
            "Should contain H100 NvSwitch device ID"
        );
    }

    #[test]
    fn test_nvswitch_device_classification_by_class() {
        // Test NvSwitch detection using the main classify_device_type function
        // This tests the proper flow: class ID -> PCI database lookup -> name classification

        // Test that known NvSwitch devices are properly classified
        // Note: We need bridge class ID (0x068000) for NvSwitch detection
        let nvswitch_result =
            classify_device_type(NVIDIA_VENDOR_ID, 0x1AF1, class_ids::BRIDGE_OTHER);
        assert!(nvswitch_result.is_ok());
        assert_eq!(nvswitch_result.unwrap(), DeviceType::NvSwitch);

        let nvswitch_result2 =
            classify_device_type(NVIDIA_VENDOR_ID, 0x22A3, class_ids::BRIDGE_OTHER);
        assert!(nvswitch_result2.is_ok());
        assert_eq!(nvswitch_result2.unwrap(), DeviceType::NvSwitch);

        // Test that GPU devices with GPU class IDs are properly classified as GPUs
        let gpu_result = classify_device_type(NVIDIA_VENDOR_ID, 0x2230, class_ids::VGA_CONTROLLER);
        assert!(gpu_result.is_ok());
        assert_eq!(gpu_result.unwrap(), DeviceType::Gpu);
    }

    #[test]
    fn test_device_type_classification() {
        // Test GPU classification
        let gpu_result = classify_device_type(NVIDIA_VENDOR_ID, 0x2684, class_ids::VGA_CONTROLLER);
        assert!(gpu_result.is_ok());
        assert_eq!(gpu_result.unwrap(), DeviceType::Gpu);

        // Test non-NVIDIA device rejection
        let non_nvidia_result = classify_device_type(0x1234, 0x5678, class_ids::VGA_CONTROLLER);
        assert!(non_nvidia_result.is_err());
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
        // Test is_nvswitch function
        assert!(is_nvswitch("GA100 [A100 NVSwitch]"));
        assert!(is_nvswitch("GH100 [H100 NVSwitch]"));
        assert!(!is_nvswitch("GeForce RTX 4090"));
        assert!(!is_nvswitch("GA102GL [RTX A6000]"));

        // Test is_gpu_class function
        assert!(is_gpu_class(class_ids::VGA_CONTROLLER));
        assert!(is_gpu_class(class_ids::DISPLAY_3D_CONTROLLER));
        assert!(!is_gpu_class(class_ids::BRIDGE_OTHER));
        assert!(!is_gpu_class(0x123456));

        // Test is_bridge_class function
        assert!(is_bridge_class(class_ids::BRIDGE_OTHER));
        assert!(!is_bridge_class(class_ids::VGA_CONTROLLER));
        assert!(!is_bridge_class(class_ids::DISPLAY_3D_CONTROLLER));
        assert!(!is_bridge_class(0x123456));
    }

    #[test]
    fn test_unknown_device_handling() {
        // Test that unknown bridge devices return Unknown instead of error
        let unknown_bridge_result = classify_device_type(NVIDIA_VENDOR_ID, 0x9999, class_ids::BRIDGE_OTHER);
        assert!(unknown_bridge_result.is_ok());
        assert_eq!(unknown_bridge_result.unwrap(), DeviceType::Unknown);

        // Test that unknown device classes return Unknown instead of error
        let unknown_class_result = classify_device_type(NVIDIA_VENDOR_ID, 0x1234, 0x999999);
        assert!(unknown_class_result.is_ok());
        assert_eq!(unknown_class_result.unwrap(), DeviceType::Unknown);
    }
}
