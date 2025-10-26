// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use super::NVRC;
use crate::pci_ids::DeviceType;

impl NVRC {
    pub fn check_gpu_supported(&mut self, supported: Option<&Path>) -> Result<()> {
        // Find if we have at least one GPU
        let mut saw_gpu = false;
        let mut first_gpu_id: Option<u16> = None;
        for d in &self.nvidia_devices {
            if matches!(d.device_type, DeviceType::Gpu) {
                saw_gpu = true;
                first_gpu_id.get_or_insert(d.device_id);
            }
        }

        if !saw_gpu {
            debug!("No GPUs found, skipping GPU supported check");
            self.gpu_supported = false; // defined as not supported when none present
            return Ok(());
        }

        let default_path = Path::new("/supported-gpu.devids");
        let path = supported.unwrap_or(default_path);

        if !path.exists() {
            self.gpu_supported = false;
            return Err(anyhow::anyhow!(
                "{} missing (cannot verify GPU support)",
                path.display()
            ));
        }

        let supported_ids = load_supported_ids(path)?;
        // Verify all GPU device IDs are supported; short‑circuit on first miss
        if let Some(bad) = self
            .nvidia_devices
            .iter()
            .filter(|d| matches!(d.device_type, DeviceType::Gpu))
            .map(|d| d.device_id)
            .find(|id| !supported_ids.contains(&format!("0x{:04x}", id)))
        {
            self.gpu_supported = false;
            return Err(anyhow::anyhow!("GPU 0x{:04x} is not supported", bad));
        }

        self.gpu_supported = true;
        debug!(
            "All GPUs supported (example id: 0x{:04x})",
            first_gpu_id.unwrap_or(0)
        );
        Ok(())
    }
}

fn load_supported_ids(path: &Path) -> Result<HashSet<String>> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(content
        .lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir; // only needed in tests

    fn write_lines(path: &Path, lines: &[&str]) {
        let mut f = fs::File::create(path).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }

    #[test]
    fn test_check_gpu_supported() -> Result<()> {
        let dir = tempdir()?;
        let list = dir.path().join("supported-gpu.devids");

        // Supported GPU
        write_lines(&list, &["0x2330"]);
        let mut nvrc = NVRC::default();
        let dev = crate::devices::NvidiaDevice::new(
            "0000:01:00.0".into(),
            "0x2330",
            "0x10de",
            "0x030000",
        )?;
        nvrc.nvidia_devices = vec![dev];
        nvrc.check_gpu_supported(Some(&list))?;
        assert!(nvrc.gpu_supported);

        // Unsupported GPU
        write_lines(&list, &["0x2331"]);
        let mut nvrc = NVRC::default();
        let dev = crate::devices::NvidiaDevice::new(
            "0000:01:00.0".into(),
            "0x2330",
            "0x10de",
            "0x030000",
        )?;
        nvrc.nvidia_devices = vec![dev];
        assert!(nvrc.check_gpu_supported(Some(&list)).is_err());
        assert!(!nvrc.gpu_supported);

        // No GPUs present
        let mut nvrc = NVRC::default();
        nvrc.check_gpu_supported(Some(&list))?; // OK
        assert!(!nvrc.gpu_supported);

        // File with blanks
        write_lines(&list, &["0x2330", "", "   "]);
        let mut nvrc = NVRC::default();
        let dev = crate::devices::NvidiaDevice::new(
            "0000:01:00.0".into(),
            "0x2330",
            "0x10de",
            "0x030000",
        )?;
        nvrc.nvidia_devices = vec![dev];
        nvrc.check_gpu_supported(Some(&list))?;
        assert!(nvrc.gpu_supported);
        Ok(())
    }
}
