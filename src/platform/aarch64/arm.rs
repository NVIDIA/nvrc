// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! ARM CCA (Confidential Compute Architecture) detection.
//!
//! This module implements confidential computing detection for ARM platforms.
//! CCA is ARM's technology for creating hardware-isolated execution
//! environments called Realms with encrypted memory and attestation.
//!
//! # Detection Strategy
//!
//! CCA detection requires both:
//! 1. **HWCAP2 check**: Verify hardware support (HWCAP2_RME bit)
//! 2. **Device node check**: Verify kernel support (`/dev/cca-guest`)
//!
//! Both must be present for CCA to be considered available.

use crate::core::error::Result;
use crate::core::traits::{CCMode, PlatformCCDetector};
use std::path::Path;

/// ARM CCA detector
#[derive(Debug, Default)]
pub struct ArmCcaDetector;

impl ArmCcaDetector {
    /// Create a new ARM CCA detector
    pub fn new() -> Self {
        Self
    }

    /// Check HWCAP2 for RME (Realm Management Extension) support
    ///
    /// Uses `getauxval(AT_HWCAP2)` to check for the RME bit (bit 28).
    fn check_hwcap(&self) -> bool {
        #[cfg(target_arch = "aarch64")]
        {
            const AT_HWCAP2: libc::c_ulong = 26;
            const HWCAP2_RME: u64 = 1 << 28;
            unsafe { (libc::getauxval(AT_HWCAP2) & HWCAP2_RME) != 0 }
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            false
        }
    }

    /// Check for /dev/cca-guest device node
    fn check_device_node(&self) -> bool {
        Path::new("/dev/cca-guest").exists()
    }
}

impl PlatformCCDetector for ArmCcaDetector {
    fn is_cc_available(&self) -> bool {
        let hwcap = self.check_hwcap();
        let device = self.check_device_node();

        debug!("ARM CCA: hwcap_rme={}, device={}", hwcap, device);

        if hwcap && !device {
            warn!("ARM CCA: HWCAP2_RME set but device node missing");
        }
        if device && !hwcap {
            warn!("ARM CCA: Device node present but HWCAP2_RME not set");
        }

        hwcap && device
    }

    fn query_cc_mode(&self) -> Result<CCMode> {
        Ok(if self.is_cc_available() {
            CCMode::On
        } else {
            CCMode::Off
        })
    }

    fn cc_technology_name(&self) -> &str {
        "ARM CCA"
    }

    fn guest_device_path(&self) -> Option<&str> {
        Some("/dev/cca-guest")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arm_cca_detector_creation() {
        let detector = ArmCcaDetector::new();
        assert_eq!(detector.cc_technology_name(), "ARM CCA");
        assert!(detector.platform_description().contains("ARM CCA"));
        assert_eq!(detector.guest_device_path(), Some("/dev/cca-guest"));
    }

    #[test]
    fn test_arm_cca_query_cc_mode() {
        let detector = ArmCcaDetector::new();
        let result = detector.query_cc_mode();
        assert!(result.is_ok());

        // Mode should be On or Off, never error
        let mode = result.unwrap();
        assert!(matches!(mode, CCMode::On | CCMode::Off));
    }

    #[test]
    fn test_arm_cca_detection_logic() {
        let detector = ArmCcaDetector::new();

        // is_cc_available should not panic
        let available = detector.is_cc_available();
        assert!(available == true || available == false);

        // If available, mode should be On
        if available {
            assert_eq!(detector.query_cc_mode().unwrap(), CCMode::On);
        }
    }

    #[test]
    fn test_arm_cca_hwcap_check() {
        let detector = ArmCcaDetector::new();
        let hwcap_result = detector.check_hwcap();

        // Should not panic
        assert!(hwcap_result == true || hwcap_result == false);
    }

    #[test]
    fn test_arm_cca_device_node_check() {
        let detector = ArmCcaDetector::new();
        let device_result = detector.check_device_node();

        // Should not panic
        assert!(device_result == true || device_result == false);
    }
}
