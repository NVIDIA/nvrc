// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Standard (non-confidential) provider implementation.
//!
//! This provider provides no-op implementations for non-confidential builds.
//! All CC queries return "Off" or "None".

use crate::core::error::Result;
use crate::core::traits::{CCMode, CCProvider, GpuCCProvider, PlatformCCDetector};
use crate::devices::NvidiaDevice;

/// Standard (non-CC) provider
///
/// Always reports CC as disabled. Used for standard (non-confidential) builds.
#[derive(Debug)]
pub struct StandardProvider {
    platform_detector: StandardPlatformDetector,
    gpu_provider: StandardGpuProvider,
}

/// Standard GPU provider (always returns CC Off)
#[allow(dead_code)] // Used by StandardProvider
#[derive(Debug)]
struct StandardGpuProvider;

impl StandardProvider {
    /// Create a new standard provider
    pub fn new() -> Self {
        debug!("Created StandardProvider (no CC support)");
        Self {
            platform_detector: StandardPlatformDetector,
            gpu_provider: StandardGpuProvider,
        }
    }
}

impl Default for StandardProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CCProvider for StandardProvider {
    fn platform(&self) -> &dyn PlatformCCDetector {
        &self.platform_detector
    }

    fn gpu(&self) -> &dyn GpuCCProvider {
        &self.gpu_provider
    }
}

impl GpuCCProvider for StandardGpuProvider {
    fn query_device_cc_mode(&self, _bdf: &str, _device_id: u16) -> Result<CCMode> {
        Ok(CCMode::Off)
    }

    fn query_all_gpus_cc_mode(&self, _devices: &[NvidiaDevice]) -> Result<Option<CCMode>> {
        Ok(None)
    }

    fn execute_srs_command(&self, _srs_value: Option<&str>) -> Result<()> {
        debug!("SRS command skipped (standard build)");
        Ok(())
    }
}

/// Standard platform detector (always returns CC Off)
#[allow(dead_code)] // Used by StandardProvider
#[derive(Debug)]
struct StandardPlatformDetector;

impl PlatformCCDetector for StandardPlatformDetector {
    fn is_cc_available(&self) -> bool {
        false
    }

    fn query_cc_mode(&self) -> Result<CCMode> {
        Ok(CCMode::Off)
    }

    fn cc_technology_name(&self) -> &str {
        "Standard (no CC)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_provider_creation() {
        let provider = StandardProvider::new();
        assert_eq!(provider.platform().cc_technology_name(), "Standard (no CC)");
        assert!(provider
            .platform()
            .platform_description()
            .contains("Standard (no CC)"));
    }

    #[test]
    fn test_standard_provider_platform() {
        let provider = StandardProvider::new();
        let platform = provider.platform();

        assert!(!platform.is_cc_available());
        assert_eq!(platform.query_cc_mode().unwrap(), CCMode::Off);
        assert_eq!(platform.guest_device_path(), None);
    }

    #[test]
    fn test_standard_provider_gpu() {
        let provider = StandardProvider::new();
        let gpu = provider.gpu();

        let mode = gpu.query_all_gpus_cc_mode(&[]);
        assert!(mode.is_ok());
        assert_eq!(mode.unwrap(), None);
    }

    #[test]
    fn test_standard_provider_system_cc_mode() {
        let provider = StandardProvider::new();
        let system_mode = provider.query_system_cc_mode(&[]).unwrap();

        assert_eq!(system_mode.platform, CCMode::Off);
        assert_eq!(system_mode.gpu, None);
        assert!(!system_mode.is_fully_enabled());
        assert!(!system_mode.has_any_cc());
    }

    #[test]
    fn test_standard_provider_default() {
        let provider = StandardProvider::default();
        assert_eq!(provider.platform().cc_technology_name(), "Standard (no CC)");
        assert!(provider
            .platform()
            .platform_description()
            .contains("Standard (no CC)"));
    }
}
