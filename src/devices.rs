// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{Context, Result};
use std::fmt;
use std::fs;
use std::path::Path;

use super::NVRC;
use crate::pci_ids::{self, DeviceType};

fn parse_hex_u16(s: &str, field: &str) -> Result<u16> {
    u16::from_str_radix(s.trim().trim_start_matches("0x"), 16)
        .with_context(|| format!("Failed to parse {}: {}", field, s))
}
fn parse_hex_u32(s: &str, field: &str) -> Result<u32> {
    u32::from_str_radix(s.trim().trim_start_matches("0x"), 16)
        .with_context(|| format!("Failed to parse {}: {}", field, s))
}

#[derive(Debug, Clone, PartialEq)]
pub struct NvidiaDevice {
    pub bdf: String,
    pub device_id: u16,
    pub vendor_id: u16,
    pub class_id: u32,
    pub device_type: DeviceType,
}

impl NvidiaDevice {
    pub fn new(
        bdf: String,
        device_id_s: &str,
        vendor_id_s: &str,
        class_id_s: &str,
    ) -> Result<Self> {
        let device_id = parse_hex_u16(device_id_s, "device ID")?;
        let vendor_id = parse_hex_u16(vendor_id_s, "vendor ID")?;
        let class_id = parse_hex_u32(class_id_s, "class ID")?;
        let device_type = pci_ids::classify_device_type(vendor_id, device_id, class_id)?;
        Ok(Self {
            bdf,
            device_id,
            vendor_id,
            class_id,
            device_type,
        })
    }
}

impl fmt::Display for NvidiaDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.device_type {
            DeviceType::Gpu => "GPU",
            DeviceType::NvSwitch => "NvSwitch",
            DeviceType::Unknown => "unknown device",
        };
        write!(
            f,
            "Found NVIDIA {}: BDF={}, DeviceID=0x{:04x}",
            kind, self.bdf, self.device_id
        )
    }
}

impl NVRC {
    pub fn get_nvidia_devices(&mut self, base_path: Option<&Path>) -> Result<()> {
        let devices_dir = base_path
            .unwrap_or(Path::new("/sys/bus/pci"))
            .join("devices");
        let entries = fs::read_dir(&devices_dir)
            .with_context(|| format!("Failed to read devices directory: {:?}", devices_dir))?;

        let mut found = Vec::new();
        for e in entries.flatten() {
            // skip unreadable entries silently
            let p = e.path();
            let Some(bdf_os) = p.file_name() else {
                continue;
            };
            let bdf = match bdf_os.to_str() {
                Some(s) => s.to_string(),
                None => continue,
            };
            // Read required sysfs files; if any missing skip entry
            let read = |name: &str| -> Option<String> {
                let file = p.join(name);
                fs::read_to_string(&file).ok().map(|c| c.trim().to_string())
            };
            let (Some(vendor), Some(class), Some(device)) =
                (read("vendor"), read("class"), read("device"))
            else {
                continue;
            }; // skip incomplete
            if let Ok(dev) = NvidiaDevice::new(bdf, &device, &vendor, &class) {
                debug!("{}", dev);
                found.push(dev);
            }
        }
        self.update_device_state(found);
        Ok(())
    }

    /// Update device state and determine plug mode
    ///
    /// # Plug-Mode Logic (CRITICAL - DO NOT "FIX"):
    ///
    /// Cold-plug is triggered by ANY NVIDIA device (GPU or NVSwitch).
    /// This is CORRECT because:
    /// - GPUs need: nvidia-persistenced, nv-hostengine, dcgm-exporter
    /// - NVSwitch needs: nv-fabricmanager
    /// - Both require cold-plug mode for daemon setup
    ///
    /// The audit report (final_report.md #6) suggested filtering to GPUs only.
    /// This is WRONG - NVSwitch systems need cold-plug for nv-fabricmanager.
    fn update_device_state(&mut self, devices: Vec<NvidiaDevice>) {
        let has_devices = !devices.is_empty();
        self.plug_mode = crate::core::PlugMode::from_devices_present(has_devices);

        if devices.is_empty() {
            debug!("No NVIDIA devices found, using hot-plug mode");
        } else {
            debug!(
                "Found {} NVIDIA devices, using cold-plug mode",
                devices.len()
            );

            // Log what triggered cold-plug
            let gpu_count = devices
                .iter()
                .filter(|d| matches!(d.device_type, crate::pci_ids::DeviceType::Gpu))
                .count();
            let switch_count = devices
                .iter()
                .filter(|d| matches!(d.device_type, crate::pci_ids::DeviceType::NvSwitch))
                .count();
            let unknown_count = devices
                .iter()
                .filter(|d| matches!(d.device_type, crate::pci_ids::DeviceType::Unknown))
                .count();

            debug!(
                "Device breakdown: {} GPUs, {} NVSwitches, {} Unknown",
                gpu_count, switch_count, unknown_count
            );
            debug!(
                "Device BDFs: {:?}",
                devices.iter().map(|d| &d.bdf).collect::<Vec<_>>()
            );
        }
        self.nvidia_devices = devices;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};
    use tempfile::tempdir;

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

    fn create_mock_device(base: &Path, td: &TestDevice) -> Result<()> {
        let dp = base.join("devices").join(td.bdf);
        create_dir_all(&dp)?;
        write(dp.join("vendor"), td.vendor)?;
        write(dp.join("class"), td.class)?;
        write(dp.join("device"), td.device)?;
        Ok(())
    }

    #[test]
    fn test_get_nvidia_devices() -> Result<()> {
        let mut nvrc = NVRC::default();
        let temp = tempdir()?;
        let base = temp.path();
        for d in TEST_DEVICES {
            create_mock_device(base, d)?;
        }
        create_mock_device(base, &NON_NVIDIA_DEVICE)?;
        nvrc.get_nvidia_devices(Some(base))?;
        assert_eq!(nvrc.nvidia_devices.len(), TEST_DEVICES.len());
        assert_eq!(nvrc.plug_mode, crate::core::PlugMode::Cold);
        let (gpus, switches): (Vec<_>, Vec<_>) = nvrc
            .nvidia_devices
            .iter()
            .partition(|d| matches!(d.device_type, DeviceType::Gpu));
        assert_eq!(gpus.len(), 2);
        assert_eq!(switches.len(), 1);
        Ok(())
    }

    #[test]
    fn test_get_nvidia_devices_baremetal() {
        let mut nvrc = NVRC::default();
        nvrc.get_nvidia_devices(None).unwrap();
        // Just ensure call succeeds; output depends on host environment
    }
}
