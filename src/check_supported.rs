use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs::File;
use std::io::BufRead;
use std::path::Path;

use super::NVRC;
use crate::pci_ids::DeviceType;

const DEFAULT_SUPPORTED_GPU_FILE: &str = "/supported-gpu.devids";

impl NVRC {
    pub fn check_gpu_supported(&mut self, supported: Option<&Path>) -> Result<()> {
        let gpu_devices: Vec<_> = self
            .nvidia_devices
            .iter()
            .filter(|d| matches!(d.device_type, DeviceType::Gpu))
            .collect();

        if gpu_devices.is_empty() {
            debug!("No GPUs found, skipping GPU supported check");
            self.gpu_supported = false;
            return Ok(());
        }

        let supported_path = supported.unwrap_or_else(|| Path::new(DEFAULT_SUPPORTED_GPU_FILE));

        if !supported_path.exists() {
            self.gpu_supported = false;
            return Err(anyhow::anyhow!(
                "{} file not found, cannot verify GPU support",
                supported_path.display()
            ));
        }

        let supported_ids = self.load_supported_ids(supported_path)?;

        if let Some(gpu) = gpu_devices
            .iter()
            .find(|gpu| !supported_ids.contains(&format!("0x{:04x}", gpu.device_id).to_lowercase()))
        {
            self.gpu_supported = false;
            Err(anyhow::anyhow!(
                "GPU 0x{:04x} is not supported",
                gpu.device_id
            ))
        } else {
            self.gpu_supported = true;
            Ok(())
        }
    }

    /// Load and parse supported device IDs from file
    fn load_supported_ids(&self, path: &Path) -> Result<HashSet<String>> {
        let reader = std::io::BufReader::new(
            File::open(path).with_context(|| format!("Failed to open {}", path.display()))?,
        );

        Ok(reader
            .lines()
            .map_while(Result::ok)
            .map(|line| line.trim().to_lowercase())
            .filter(|line| !line.is_empty())
            .collect())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_check_gpu_supported() -> Result<()> {
        let supported_dir = tempdir()?;
        let supported_path = supported_dir.path().join("supported-gpu.devids");

        // Test with supported GPU
        {
            let mut file = File::create(&supported_path)?;
            writeln!(file, "0x2330")?;

            let mut nvrc = NVRC::default();
            let nvidia_device = crate::get_devices::NvidiaDevice::new(
                "0000:01:00.0".to_string(),
                "0x2330",
                "0x10de",
                "0x030000",
            )?;
            nvrc.nvidia_devices = vec![nvidia_device];

            nvrc.check_gpu_supported(Some(&supported_path))?;
            assert!(nvrc.gpu_supported);
        }

        // Test with unsupported GPU
        {
            let mut file = File::create(&supported_path)?;
            writeln!(file, "0x2331")?; // Different device ID

            let mut nvrc = NVRC::default();
            let nvidia_device = crate::get_devices::NvidiaDevice::new(
                "0000:01:00.0".to_string(),
                "0x2330", // This won't match the supported ID
                "0x10de",
                "0x030000",
            )?;
            nvrc.nvidia_devices = vec![nvidia_device];

            let result = nvrc.check_gpu_supported(Some(&supported_path));
            assert!(result.is_err());
            assert!(!nvrc.gpu_supported);
        }

        // Test with no GPUs (should be considered "not supported")
        {
            let mut nvrc = NVRC::default();
            nvrc.nvidia_devices = vec![]; // No devices

            nvrc.check_gpu_supported(Some(&supported_path))?;
            assert!(!nvrc.gpu_supported); // No GPUs means not supported
        }

        // Test with empty lines in support file
        {
            let mut file = File::create(&supported_path)?;
            writeln!(file, "0x2330")?;
            writeln!(file, "")?; // Empty line
            writeln!(file, "  ")?; // Whitespace line

            let mut nvrc = NVRC::default();
            let nvidia_device = crate::get_devices::NvidiaDevice::new(
                "0000:01:00.0".to_string(),
                "0x2330",
                "0x10de",
                "0x030000",
            )?;
            nvrc.nvidia_devices = vec![nvidia_device];

            nvrc.check_gpu_supported(Some(&supported_path))?;
            assert!(nvrc.gpu_supported);
        }

        Ok(())
    }
}
