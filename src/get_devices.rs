use anyhow::{Context, Result};
use std::fmt;
use std::fs;
use std::path::Path;

use super::NVRC;
use crate::pci_ids::{self, DeviceType};

/// Default PCI devices directory path
const DEFAULT_PCI_PATH: &str = "/sys/bus/pci";

/// Helper function to parse hexadecimal strings with optional "0x" prefix
fn parse_hex_u16(hex_str: &str, field_name: &str) -> Result<u16> {
    let cleaned = hex_str.trim().strip_prefix("0x").unwrap_or(hex_str.trim());
    u16::from_str_radix(cleaned, 16)
        .with_context(|| format!("Failed to parse {}: {}", field_name, hex_str))
}

/// Helper function to parse hexadecimal strings with optional "0x" prefix for u32
fn parse_hex_u32(hex_str: &str, field_name: &str) -> Result<u32> {
    let cleaned = hex_str.trim().strip_prefix("0x").unwrap_or(hex_str.trim());
    u32::from_str_radix(cleaned, 16)
        .with_context(|| format!("Failed to parse {}: {}", field_name, hex_str))
}

/// Represents an NVIDIA device (GPU or NvSwitch) with its associated PCI information
#[derive(Debug, Clone, PartialEq)]
pub struct NvidiaDevice {
    /// Bus-Device-Function identifier (e.g., "0000:01:00.0")
    pub bdf: String,
    pub device_id: u16,
    pub vendor_id: u16,
    pub class_id: u32,
    pub device_type: DeviceType,
}

impl NvidiaDevice {
    pub fn new(
        bdf: String,
        device_id_str: &str,
        vendor_id_str: &str,
        class_id_str: &str,
    ) -> Result<Self> {
        let device_id = parse_hex_u16(device_id_str, "device ID")?;
        let vendor_id = parse_hex_u16(vendor_id_str, "vendor ID")?;
        let class_id = parse_hex_u32(class_id_str, "class ID")?;

        let device_type = Self::determine_device_type(vendor_id, device_id, class_id)?;

        Ok(NvidiaDevice {
            bdf,
            device_id,
            vendor_id,
            class_id,
            device_type,
        })
    }

    fn determine_device_type(vendor_id: u16, device_id: u16, class_id: u32) -> Result<DeviceType> {
        pci_ids::classify_device_type(vendor_id, device_id, class_id)
    }
}

impl fmt::Display for NvidiaDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let device_type_name = match self.device_type {
            DeviceType::Gpu => "GPU",
            DeviceType::NvSwitch => "NvSwitch",
            DeviceType::Unknown => "unknown device",
        };
        write!(
            f,
            "Found NVIDIA {}: BDF={}, DeviceID=0x{:04x}",
            device_type_name, self.bdf, self.device_id
        )
    }
}

impl NVRC {
    /// Get all NVIDIA devices from the PCI bus
    pub fn get_nvidia_devices(&mut self, base_path: Option<&Path>) -> Result<()> {
        let base_path = base_path
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| Path::new(DEFAULT_PCI_PATH).to_path_buf());

        let devices_dir = base_path.join("devices");
        let entries = fs::read_dir(&devices_dir)
            .with_context(|| format!("Failed to read devices directory: {:?}", devices_dir))?;

        let nvidia_devices: Vec<NvidiaDevice> = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let device_dir = entry.path();

                // Extract BDF from directory name
                let bdf = device_dir.file_name()?.to_str()?.to_string();

                // Try to read device information
                self.read_device_info(&device_dir, bdf).ok()
            })
            .filter_map(|(bdf, device_info)| {
                // Try to create NvidiaDevice
                NvidiaDevice::new(
                    bdf,
                    &device_info.device_id,
                    &device_info.vendor_id,
                    &device_info.class_id,
                )
                .ok()
            })
            .inspect(|device| debug!("{}", device))
            .collect();

        self.update_device_state(nvidia_devices);
        Ok(())
    }

    /// Read device information from sysfs files
    fn read_device_info(&self, device_dir: &Path, bdf: String) -> Result<(String, DeviceInfo)> {
        let device_info = DeviceInfo {
            vendor_id: Self::read_sysfs_file(&device_dir.join("vendor"))?,
            class_id: Self::read_sysfs_file(&device_dir.join("class"))?,
            device_id: Self::read_sysfs_file(&device_dir.join("device"))?,
        };
        Ok((bdf, device_info))
    }

    /// Read a sysfs file and return trimmed content
    fn read_sysfs_file(path: &Path) -> Result<String> {
        fs::read_to_string(path)
            .with_context(|| format!("Failed to read file: {:?}", path))
            .map(|content| content.trim().to_string())
    }

    /// Update the NVRC state with discovered devices
    fn update_device_state(&mut self, nvidia_devices: Vec<NvidiaDevice>) {
        let device_count = nvidia_devices.len();
        self.nvidia_devices = nvidia_devices;

        if device_count == 0 {
            debug!("No NVIDIA devices found");
            self.cold_plug = false;
        } else {
            let bdfs: Vec<&String> = self.nvidia_devices.iter().map(|d| &d.bdf).collect();
            debug!("Device BDFs: {:?}", bdfs);
            debug!("Total NVIDIA devices: {}", device_count);
            self.cold_plug = true;
        }
    }
}

/// Helper struct to hold device information read from sysfs
#[derive(Debug)]
struct DeviceInfo {
    vendor_id: String,
    class_id: String,
    device_id: String,
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};
    use tempfile::tempdir;

    /// Test device configurations for mock PCI devices
    struct TestDevice {
        bdf: &'static str,
        vendor: &'static str,
        class: &'static str,
        device: &'static str,
    }

    const TEST_DEVICES: &[TestDevice] = &[
        TestDevice {
            bdf: "0000:01:00.0",
            vendor: "0x10de",
            class: "0x030000",
            device: "0x1234",
        },
        TestDevice {
            bdf: "0000:02:00.0",
            vendor: "0x10de",
            class: "0x030200",
            device: "0x5678",
        },
        TestDevice {
            bdf: "0000:03:00.0",
            vendor: "0x10de",
            class: "0x068000",
            device: "0x1af1",
        },
    ];

    const NON_NVIDIA_DEVICE: TestDevice = TestDevice {
        bdf: "0000:04:00.0",
        vendor: "0x1234",
        class: "0x567800",
        device: "abcd",
    };

    fn create_mock_device(base_path: &Path, device: &TestDevice) -> Result<()> {
        let device_path = base_path.join("devices").join(device.bdf);
        create_dir_all(&device_path)?;
        write(device_path.join("vendor"), device.vendor)?;
        write(device_path.join("class"), device.class)?;
        write(device_path.join("device"), device.device)?;
        Ok(())
    }

    #[test]
    fn test_get_nvidia_devices() -> Result<()> {
        let mut nvrc = NVRC::default();
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();

        // Create mock NVIDIA devices
        for device in TEST_DEVICES {
            create_mock_device(base_path, device)?;
        }

        // Create non-NVIDIA device (should be ignored)
        create_mock_device(base_path, &NON_NVIDIA_DEVICE)?;

        // Run the function with the mock PCI space
        nvrc.get_nvidia_devices(Some(base_path))?;

        // Verify results
        assert_eq!(nvrc.nvidia_devices.len(), TEST_DEVICES.len());
        assert!(nvrc.cold_plug);

        // Group devices by type for verification
        let (gpu_devices, nvswitch_devices): (Vec<_>, Vec<_>) = nvrc
            .nvidia_devices
            .iter()
            .partition(|d| matches!(d.device_type, DeviceType::Gpu));

        assert_eq!(gpu_devices.len(), 2);
        assert_eq!(nvswitch_devices.len(), 1);

        // Verify specific device details
        let gpu_bdfs: Vec<&String> = gpu_devices.iter().map(|d| &d.bdf).collect();
        let gpu_device_ids: Vec<u16> = gpu_devices.iter().map(|d| d.device_id).collect();

        assert!(gpu_bdfs.contains(&&"0000:01:00.0".to_string()));
        assert!(gpu_bdfs.contains(&&"0000:02:00.0".to_string()));
        assert!(gpu_device_ids.contains(&0x1234));
        assert!(gpu_device_ids.contains(&0x5678));

        assert_eq!(nvswitch_devices[0].bdf, "0000:03:00.0");
        assert_eq!(nvswitch_devices[0].device_id, 0x1af1);

        Ok(())
    }

    #[test]
    fn test_get_nvidia_devices_baremetal() {
        let mut nvrc = NVRC::default();
        nvrc.get_nvidia_devices(None).unwrap();

        let (gpu_devices, nvswitch_devices): (Vec<_>, Vec<_>) = nvrc
            .nvidia_devices
            .iter()
            .partition(|d| matches!(d.device_type, DeviceType::Gpu));

        let summary = format!(
            "GPU devices: {} (BDFs: {:?}, IDs: {:?}), NvSwitch devices: {} (BDFs: {:?}, IDs: {:?}), Total: {}",
            gpu_devices.len(),
            gpu_devices.iter().map(|d| &d.bdf).collect::<Vec<_>>(),
            gpu_devices.iter().map(|d| format!("0x{:04x}", d.device_id)).collect::<Vec<_>>(),
            nvswitch_devices.len(),
            nvswitch_devices.iter().map(|d| &d.bdf).collect::<Vec<_>>(),
            nvswitch_devices.iter().map(|d| format!("0x{:04x}", d.device_id)).collect::<Vec<_>>(),
            nvrc.nvidia_devices.len()
        );

        println!("{}", summary);
    }
}
