// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! x86_64 platform implementations.
//!
//! This module provides CC detection for x86_64 platforms:
//! - AMD SEV-SNP
//! - Intel TDX

mod amd;
mod intel;
mod standard;

pub use amd::AmdSnpDetector;
pub use intel::IntelTdxDetector;
pub use standard::X86StandardDetector;

use crate::core::traits::{CpuVendor, PlatformCCDetector};

/// Factory function to create x86_64 platform detector
///
/// Creates the appropriate detector based on CPU vendor and feature flags.
///
/// # Arguments
///
/// * `vendor` - The detected CPU vendor (AMD, Intel, or Arm)
///
/// # Returns
///
/// A boxed platform detector appropriate for the vendor and build configuration:
/// - AMD + confidential feature: `AmdSnpDetector`
/// - Intel + confidential feature: `IntelTdxDetector`
/// - Otherwise: `X86StandardDetector`
pub fn create_detector(vendor: CpuVendor) -> Box<dyn PlatformCCDetector> {
    #[cfg(feature = "confidential")]
    {
        match vendor {
            CpuVendor::Amd => {
                debug!("Creating AMD SEV-SNP detector");
                Box::new(AmdSnpDetector::new())
            }
            CpuVendor::Intel => {
                debug!("Creating Intel TDX detector");
                Box::new(IntelTdxDetector::new())
            }
            _ => {
                debug!("Non-x86 vendor on x86_64, using standard detector");
                Box::new(X86StandardDetector::new())
            }
        }
    }

    #[cfg(not(feature = "confidential"))]
    {
        let _ = vendor; // Suppress unused warning
        debug!("Standard build, using standard detector");
        Box::new(X86StandardDetector::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_detector_standard() {
        // Standard build should always return X86StandardDetector
        #[cfg(not(feature = "confidential"))]
        {
            let detector = create_detector(CpuVendor::Amd);
            assert_eq!(detector.platform_description(), "x86_64 (standard, no CC)");

            let detector = create_detector(CpuVendor::Intel);
            assert_eq!(detector.platform_description(), "x86_64 (standard, no CC)");
        }
    }

    #[test]
    fn test_create_detector_confidential() {
        // Confidential build should return vendor-specific detectors
        #[cfg(feature = "confidential")]
        {
            let detector = create_detector(CpuVendor::Amd);
            assert!(detector.platform_description().contains("AMD SEV-SNP"));

            let detector = create_detector(CpuVendor::Intel);
            assert!(detector.platform_description().contains("Intel TDX"));
        }
    }
}
