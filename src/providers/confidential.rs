// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Confidential computing provider implementation.
//!
//! This provider combines platform-specific CC detection (AMD SNP, Intel TDX, ARM CCA)
//! with GPU CC detection (Hopper, Blackwell) into a unified interface.

use crate::core::error::Result;
use crate::core::traits::{CCProvider, PlatformCCDetector};
use crate::platform;

#[cfg(feature = "confidential")]
use crate::gpu::confidential::ConfidentialGpuProvider;

#[cfg(not(feature = "confidential"))]
// Use standard GPU provider when confidential feature is not enabled
type ConfidentialGpuProvider = crate::gpu::standard::StandardGpuProvider;

/// Top-level confidential computing provider
///
/// Combines platform and GPU CC detection into a single provider.
/// Automatically detects the current platform and creates the
/// appropriate platform detector.
#[allow(dead_code)] // Will be used in PR #11
#[derive(Debug)]
pub struct ConfidentialProvider {
    platform_detector: Box<dyn PlatformCCDetector>,
    gpu_provider: ConfidentialGpuProvider,
}

impl ConfidentialProvider {
    /// Create a new confidential provider with auto-detected platform
    ///
    /// This detects the current CPU vendor and architecture, then creates
    /// the appropriate platform-specific CC detector.
    ///
    /// # Errors
    ///
    /// Returns an error if platform detection fails.
    #[allow(dead_code)] // Will be used in PR #11
    pub fn new() -> Result<Self> {
        let platform_info = platform::detector::detect_platform()?;
        Self::with_platform(platform_info)
    }

    /// Create with specific platform info
    ///
    /// Useful for testing or when platform info is already known.
    #[allow(dead_code)] // Will be used in PR #11
    pub fn with_platform(platform_info: crate::core::traits::PlatformInfo) -> Result<Self> {
        let platform_detector = platform::create_platform_detector(platform_info);
        let gpu_provider = ConfidentialGpuProvider::new();

        debug!(
            "Created ConfidentialProvider with platform: {}",
            platform_detector.platform_description()
        );

        Ok(Self {
            platform_detector,
            gpu_provider,
        })
    }

    /// Create with custom detectors (for testing)
    ///
    /// Allows injecting custom platform and GPU detectors for unit testing.
    #[allow(dead_code)] // Used in tests
    pub fn with_detectors(
        platform_detector: Box<dyn PlatformCCDetector>,
        gpu_provider: ConfidentialGpuProvider,
    ) -> Self {
        Self {
            platform_detector,
            gpu_provider,
        }
    }
}

impl CCProvider for ConfidentialProvider {
    fn platform(&self) -> &dyn PlatformCCDetector {
        &*self.platform_detector
    }

    fn gpu(&self) -> &dyn crate::core::traits::GpuCCProvider {
        &self.gpu_provider
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidential_provider_creation() {
        let result = ConfidentialProvider::new();
        // Should not panic, may fail if platform detection fails
        match result {
            Ok(provider) => {
                assert!(provider.platform().platform_description().len() > 0);
            }
            Err(e) => {
                // Platform detection failure is acceptable in test environment
                println!(
                    "Platform detection failed (expected in some environments): {}",
                    e
                );
            }
        }
    }

    #[test]
    fn test_confidential_provider_platform_access() {
        if let Ok(provider) = ConfidentialProvider::new() {
            let platform = provider.platform();
            let description = platform.platform_description();
            assert!(!description.is_empty());

            // Should be able to query CC mode
            let mode = platform.query_cc_mode();
            assert!(mode.is_ok());
        }
    }

    #[test]
    fn test_confidential_provider_gpu_access() {
        if let Ok(provider) = ConfidentialProvider::new() {
            let gpu = provider.gpu();

            // Should be able to query with empty device list
            let mode = gpu.query_all_gpus_cc_mode(&[]);
            assert!(mode.is_ok());
            assert_eq!(mode.unwrap(), None);
        }
    }
}
