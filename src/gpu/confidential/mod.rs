// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Confidential computing GPU provider.
//!
//! This module provides GPU CC detection and management for confidential builds.

mod bar0;

#[allow(unused_imports)] // Will be used in existing code migration
pub use bar0::read_bar0_register;

use crate::core::error::{NvrcError, Result};
use crate::core::traits::{CCMode, GpuCCProvider};
use crate::devices::NvidiaDevice;
use crate::gpu::architectures;
use crate::pci_ids::DeviceType;
use anyhow::Context;

/// Confidential GPU provider
///
/// Provides GPU CC mode detection by:
/// 1. Detecting GPU architecture from device ID
/// 2. Reading CC register from BAR0
/// 3. Parsing CC mode from register value
#[derive(Debug, Default)]
pub struct ConfidentialGpuProvider;

impl ConfidentialGpuProvider {
    /// Create a new confidential GPU provider
    pub fn new() -> Self {
        Self
    }
}

impl GpuCCProvider for ConfidentialGpuProvider {
    fn query_device_cc_mode(&self, bdf: &str, device_id: u16) -> Result<CCMode> {
        // Get device name from PCI database
        let device_name = crate::pci_ids::get_pci_ids_database()
            .get(&device_id)
            .ok_or_else(|| NvrcError::GpuCCQueryFailed {
                bdf: bdf.to_string(),
                reason: format!("Device ID 0x{:04x} not found in PCI database", device_id),
            })?;

        // Detect GPU architecture
        let arch = architectures::detect_architecture(device_id, device_name)?;

        debug!(
            "GPU {}: architecture={}, device_id=0x{:04x}",
            bdf,
            arch.name(),
            device_id
        );

        // Get CC register offset
        let register_offset = arch.cc_register_offset()?;

        // Read BAR0 register
        let register_value = bar0::read_bar0_register(bdf, register_offset).map_err(|e| {
            NvrcError::GpuCCQueryFailed {
                bdf: bdf.to_string(),
                reason: format!("Failed to read BAR0 register: {}", e),
            }
        })?;

        // Parse CC mode
        let mode = arch.parse_cc_mode(register_value)?;

        debug!(
            "GPU {}: CC mode={:?}, register=0x{:x}",
            bdf, mode, register_value
        );

        Ok(mode)
    }

    fn query_all_gpus_cc_mode(&self, devices: &[NvidiaDevice]) -> Result<Option<CCMode>> {
        let mut aggregate: Option<CCMode> = None;

        for device in devices
            .iter()
            .filter(|d| matches!(d.device_type, DeviceType::Gpu))
        {
            let mode = self.query_device_cc_mode(&device.bdf, device.device_id)?;

            if let Some(prev) = aggregate {
                if prev != mode {
                    return Err(NvrcError::InconsistentGpuCCModes {
                        bdf: device.bdf.clone(),
                        actual: mode,
                        expected: prev,
                    });
                }
            } else {
                aggregate = Some(mode);
            }
        }

        if aggregate.is_none() {
            debug!("No GPUs found for CC mode query");
        }

        Ok(aggregate)
    }

    fn execute_srs_command(&self, srs_value: Option<&str>) -> Result<()> {
        // Import from existing daemon module
        crate::daemon::foreground(
            "/bin/nvidia-smi",
            &["conf-compute", "-srs", srs_value.unwrap_or("0")],
        )
        .context("nvidia-smi SRS command failed")
        .map_err(|e| NvrcError::CommandFailed {
            command: "nvidia-smi conf-compute -srs".to_string(),
            status: e.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidential_gpu_provider_creation() {
        let provider = ConfidentialGpuProvider::new();
        assert_eq!(format!("{:?}", provider), "ConfidentialGpuProvider");
    }

    #[test]
    fn test_query_all_gpus_no_gpus() {
        let provider = ConfidentialGpuProvider::new();
        let result = provider.query_all_gpus_cc_mode(&[]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    // Note: Full integration tests require actual GPU hardware
}
