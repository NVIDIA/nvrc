// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! aarch64 platform implementations.
//!
//! This module provides CC detection for aarch64 platforms:
//! - ARM CCA (Confidential Compute Architecture)

mod arm;
mod standard;

pub use arm::ArmCcaDetector;
pub use standard::Aarch64StandardDetector;

use crate::core::traits::{CpuVendor, PlatformCCDetector};

/// Factory function to create aarch64 platform detector
///
/// Creates the appropriate detector based on CPU vendor and feature flags.
///
/// # Arguments
///
/// * `vendor` - The detected CPU vendor (should be Arm for aarch64)
///
/// # Returns
///
/// A boxed platform detector appropriate for the vendor and build configuration:
/// - Arm + confidential feature: `ArmCcaDetector`
/// - Otherwise: `Aarch64StandardDetector`
pub fn create_detector(vendor: CpuVendor) -> Box<dyn PlatformCCDetector> {
    #[cfg(feature = "confidential")]
    {
        match vendor {
            CpuVendor::Arm => {
                debug!("Creating ARM CCA detector");
                Box::new(ArmCcaDetector::new())
            }
            _ => {
                debug!("Non-ARM vendor on aarch64, using standard detector");
                Box::new(Aarch64StandardDetector::new())
            }
        }
    }

    #[cfg(not(feature = "confidential"))]
    {
        let _ = vendor; // Suppress unused warning
        debug!("Standard build, using standard detector");
        Box::new(Aarch64StandardDetector::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_detector_standard() {
        // Standard build should always return Aarch64StandardDetector
        #[cfg(not(feature = "confidential"))]
        {
            let detector = create_detector(CpuVendor::Arm);
            assert_eq!(
                detector.platform_description(),
                "aarch64 (standard, no CC)"
            );
        }
    }

    #[test]
    fn test_create_detector_confidential() {
        // Confidential build should return ARM CCA detector
        #[cfg(feature = "confidential")]
        {
            let detector = create_detector(CpuVendor::Arm);
            assert!(detector.platform_description().contains("ARM CCA"));
        }
    }
}
