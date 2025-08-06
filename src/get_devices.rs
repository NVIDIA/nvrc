use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use super::NVRC;
use crate::pci_ids::{self, DeviceType};

/// Represents an NVIDIA device (GPU or NvSwitch) with its associated PCI information
#[derive(Debug, Clone, PartialEq)]
pub struct NvidiaDevice {
    /// Bus-Device-Function identifier (e.g., "0000:01:00.0")
    pub bdf: String,
    /// PCI device ID as a 16-bit integer
    pub device_id: u16,
    /// PCI vendor ID as a 16-bit integer
    pub vendor_id: u16,
    /// PCI class ID as a 32-bit integer
    pub class_id: u32,
    /// Type of NVIDIA device
    pub device_type: DeviceType,
}

impl NvidiaDevice {
    /// Create a new NvidiaDevice from string values
    pub fn new(
        bdf: String,
        device_id_str: &str,
        vendor_id_str: &str,
        class_id_str: &str,
    ) -> Result<Self> {
        // Parse device ID (handle both "0x1234" and "1234" formats)
        let device_id_str = device_id_str
            .trim()
            .strip_prefix("0x")
            .unwrap_or(device_id_str);
        let device_id = u16::from_str_radix(device_id_str, 16)
            .with_context(|| format!("Failed to parse device ID: {}", device_id_str))?;

        // Parse vendor ID (handle both "0x10de" and "10de" formats)
        let vendor_id_str = vendor_id_str
            .trim()
            .strip_prefix("0x")
            .unwrap_or(vendor_id_str);
        let vendor_id = u16::from_str_radix(vendor_id_str, 16)
            .with_context(|| format!("Failed to parse vendor ID: {}", vendor_id_str))?;

        // Parse class ID (handle both "0x030000" and "030000" formats)
        let class_id_str = class_id_str
            .trim()
            .strip_prefix("0x")
            .unwrap_or(class_id_str);
        let class_id = u32::from_str_radix(class_id_str, 16)
            .with_context(|| format!("Failed to parse class ID: {}", class_id_str))?;

        // Determine device type based on class ID and device ID
        let device_type = Self::determine_device_type(vendor_id, device_id, class_id)?;

        Ok(NvidiaDevice {
            bdf,
            device_id,
            vendor_id,
            class_id,
            device_type,
        })
    }

    /// Determine if this is a valid NVIDIA device and what type
    fn determine_device_type(vendor_id: u16, device_id: u16, class_id: u32) -> Result<DeviceType> {
        pci_ids::classify_device_type(vendor_id, device_id, class_id)
    }
}

impl NVRC {
    pub fn get_nvidia_devices(&mut self, base_path: Option<&Path>) -> Result<()> {
        let base_path = match base_path {
            Some(path) => path.to_path_buf(),
            None => Path::new("/sys/bus/pci").to_path_buf(),
        };

        let mut nvidia_devices = Vec::new();

        // Iterate over PCI devices in the provided base path
        for entry in
            fs::read_dir(base_path.join("devices")).context("Failed to read devices directory")?
        {
            let entry = entry.context("Failed to read entry in devices directory")?;
            let device_dir = entry.path();

            // Read the vendor ID
            let vendor_path = device_dir.join("vendor");
            let vendor = fs::read_to_string(&vendor_path)
                .unwrap_or_default()
                .trim()
                .to_string();

            // Read the class ID
            let class_path = device_dir.join("class");
            let class = fs::read_to_string(&class_path)
                .unwrap_or_default()
                .trim()
                .to_string();

            // Read the device ID
            let device_path = device_dir.join("device");
            let device_id = fs::read_to_string(&device_path)
                .unwrap_or_default()
                .trim()
                .to_string();

            // Extract the BDF (bus, device, function) using the directory name
            if let Some(bdf) = device_dir.file_name().and_then(|bdf| bdf.to_str()) {
                // Try to create a NvidiaDevice
                if let Ok(nvidia_device) =
                    NvidiaDevice::new(bdf.to_string(), &device_id, &vendor, &class)
                {
                    match nvidia_device.device_type {
                        DeviceType::Gpu => {
                            debug!(
                                "Found NVIDIA GPU: BDF={}, DeviceID=0x{:04x}",
                                nvidia_device.bdf, nvidia_device.device_id
                            );
                        }
                        DeviceType::NvSwitch => {
                            debug!(
                                "Found NVIDIA NvSwitch: BDF={}, DeviceID=0x{:04x}",
                                nvidia_device.bdf, nvidia_device.device_id
                            );
                        }
                        DeviceType::Unknown => {
                            debug!(
                                "Found unknown NVIDIA device: BDF={}, DeviceID=0x{:04x}",
                                nvidia_device.bdf, nvidia_device.device_id
                            );
                        }
                    }
                    nvidia_devices.push(nvidia_device);
                }
            }
        }

        self.nvidia_devices = nvidia_devices;

        if self.nvidia_devices.is_empty() {
            debug!("No NVIDIA devices found");
            self.cold_plug = false;
        } else {
            debug!(
                "Device BDFs: {:?}",
                self.nvidia_devices
                    .iter()
                    .map(|d| &d.bdf)
                    .collect::<Vec<_>>()
            );
            self.cold_plug = true;
        }

        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};
    use tempfile::tempdir;

    #[test]
    fn test_get_nvidia_devices() -> Result<()> {
        let mut init = NVRC::default();
        let temp_dir = tempdir()?;
        let base_path = temp_dir.path();

        // Create mock PCI devices
        let device_1_path = base_path.join("devices/0000:01:00.0");
        let device_2_path = base_path.join("devices/0000:02:00.0");
        let nvswitch_path = base_path.join("devices/0000:03:00.0");
        let non_nvidia_device_path = base_path.join("devices/0000:04:00.0");

        // Create directories for devices
        create_dir_all(&device_1_path)?;
        create_dir_all(&device_2_path)?;
        create_dir_all(&nvswitch_path)?;
        create_dir_all(&non_nvidia_device_path)?;

        // Create mock files for device 1 (NVIDIA GPU)
        write(device_1_path.join("vendor"), "0x10de")?;
        write(device_1_path.join("class"), "0x030000")?;
        write(device_1_path.join("device"), "0x1234")?;

        // Create mock files for device 2 (NVIDIA GPU)
        write(device_2_path.join("vendor"), "0x10de")?;
        write(device_2_path.join("class"), "0x030200")?;
        write(device_2_path.join("device"), "0x5678")?;

        // Create mock files for NvSwitch device
        write(nvswitch_path.join("vendor"), "0x10de")?;
        write(nvswitch_path.join("class"), "0x068000")?;
        write(nvswitch_path.join("device"), "0x1af1")?;

        // Create mock files for non-NVIDIA device
        write(non_nvidia_device_path.join("vendor"), "0x1234")?;
        write(non_nvidia_device_path.join("class"), "0x567800")?;
        write(non_nvidia_device_path.join("device"), "abcd")?;

        // Run the function with the mock PCI space
        init.get_nvidia_devices(Some(base_path)).unwrap();

        // Check the results
        assert_eq!(init.nvidia_devices.len(), 3); // 2 GPUs + 1 NvSwitch

        let gpu_devices: Vec<_> = init
            .nvidia_devices
            .iter()
            .filter(|d| matches!(d.device_type, DeviceType::Gpu))
            .collect();
        let nvswitch_devices: Vec<_> = init
            .nvidia_devices
            .iter()
            .filter(|d| matches!(d.device_type, DeviceType::NvSwitch))
            .collect();

        assert_eq!(gpu_devices.len(), 2);
        assert_eq!(nvswitch_devices.len(), 1);

        let gpu_bdfs: Vec<String> = gpu_devices.iter().map(|d| d.bdf.clone()).collect();
        let nvswitch_bdfs: Vec<String> = nvswitch_devices.iter().map(|d| d.bdf.clone()).collect();

        assert!(gpu_bdfs.contains(&"0000:01:00.0".to_string()));
        assert!(gpu_bdfs.contains(&"0000:02:00.0".to_string()));
        assert!(nvswitch_bdfs.contains(&"0000:03:00.0".to_string()));

        // Check device IDs (order may vary)
        let gpu_device_ids: Vec<u16> = gpu_devices.iter().map(|d| d.device_id).collect();
        assert!(gpu_device_ids.contains(&0x1234)); // "1234" hex = 4660
        assert!(gpu_device_ids.contains(&0x5678)); // "5678" hex = 22136
        assert_eq!(nvswitch_devices[0].device_id, 0x1af1); // "1AF1" hex

        Ok(())
    }

    #[test]
    fn test_get_nvidia_devices_baremetal() {
        let mut init = NVRC::default();
        init.get_nvidia_devices(None).unwrap();

        let gpu_devices: Vec<_> = init
            .nvidia_devices
            .iter()
            .filter(|d| matches!(d.device_type, DeviceType::Gpu))
            .collect();
        let nvswitch_devices: Vec<_> = init
            .nvidia_devices
            .iter()
            .filter(|d| matches!(d.device_type, DeviceType::NvSwitch))
            .collect();

        let gpu_bdfs: Vec<String> = gpu_devices.iter().map(|d| d.bdf.clone()).collect();
        let gpu_devids: Vec<String> = gpu_devices
            .iter()
            .map(|d| format!("0x{:04x}", d.device_id))
            .collect();
        let nvswitch_bdfs: Vec<String> = nvswitch_devices.iter().map(|d| d.bdf.clone()).collect();
        let nvswitch_devids: Vec<String> = nvswitch_devices
            .iter()
            .map(|d| format!("0x{:04x}", d.device_id))
            .collect();

        println!("GPU BDFs: {:?}", gpu_bdfs);
        println!("GPU Device IDs: {:?}", gpu_devids);
        println!("NvSwitch BDFs: {:?}", nvswitch_bdfs);
        println!("NvSwitch Device IDs: {:?}", nvswitch_devids);
        println!("Total NVIDIA devices: {}", init.nvidia_devices.len());
    }
}
