// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Intel TDX (Trust Domain Extensions) detection.
//!
//! This module implements confidential computing detection for Intel platforms.
//! TDX is Intel's technology for creating hardware-isolated virtual machines
//! called Trust Domains (TDs) with encrypted memory and attestation.
//!
//! # Detection Strategy
//!
//! TDX detection requires both:
//! 1. **CPUID check**: Verify hardware support (CPUID.0x21.EAX != 0)
//! 2. **Device node check**: Verify kernel support (`/dev/tdx-guest`)
//!
//! Both must be present for TDX to be considered available.

use crate::core::error::Result;
use crate::core::traits::{CCMode, PlatformCCDetector};
use std::path::Path;

/// Intel TDX detector
#[derive(Debug, Default)]
pub struct IntelTdxDetector;

impl IntelTdxDetector {
    /// Create a new Intel TDX detector
    pub fn new() -> Self {
        Self
    }

    /// Check CPUID for TDX support (CPUID.0x21.EAX != 0)
    fn check_cpuid(&self) -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            unsafe {
                use core::arch::x86_64::__cpuid_count;
                let result = __cpuid_count(0x21, 0);
                result.eax != 0
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            false
        }
    }

    /// Check for /dev/tdx-guest device node
    fn check_device_node(&self) -> bool {
        Path::new("/dev/tdx-guest").exists()
    }
}

impl PlatformCCDetector for IntelTdxDetector {
    fn is_cc_available(&self) -> bool {
        let cpuid = self.check_cpuid();
        let device = self.check_device_node();

        debug!("Intel TDX: cpuid={}, device={}", cpuid, device);

        if cpuid && !device {
            warn!("Intel TDX: CPUID leaf present but device node missing");
        }
        if device && !cpuid {
            warn!("Intel TDX: Device node present but CPUID leaf missing");
        }

        cpuid && device
    }

    fn query_cc_mode(&self) -> Result<CCMode> {
        Ok(if self.is_cc_available() {
            CCMode::On
        } else {
            CCMode::Off
        })
    }

    fn platform_description(&self) -> &str {
        "Intel TDX (Trust Domain Extensions)"
    }

    fn guest_device_path(&self) -> Option<&str> {
        Some("/dev/tdx-guest")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intel_tdx_detector_creation() {
        let detector = IntelTdxDetector::new();
        assert_eq!(
            detector.platform_description(),
            "Intel TDX (Trust Domain Extensions)"
        );
        assert_eq!(detector.guest_device_path(), Some("/dev/tdx-guest"));
    }

    #[test]
    fn test_intel_tdx_query_cc_mode() {
        let detector = IntelTdxDetector::new();
        let result = detector.query_cc_mode();
        assert!(result.is_ok());

        // Mode should be On or Off, never error
        let mode = result.unwrap();
        assert!(matches!(mode, CCMode::On | CCMode::Off));
    }

    #[test]
    fn test_intel_tdx_detection_logic() {
        let detector = IntelTdxDetector::new();

        // is_cc_available should not panic
        let available = detector.is_cc_available();
        assert!(available == true || available == false);

        // If available, mode should be On
        if available {
            assert_eq!(detector.query_cc_mode().unwrap(), CCMode::On);
        }
    }

    #[test]
    fn test_intel_tdx_cpuid_check() {
        let detector = IntelTdxDetector::new();
        let cpuid_result = detector.check_cpuid();

        // Should not panic
        assert!(cpuid_result == true || cpuid_result == false);
    }

    #[test]
    fn test_intel_tdx_device_node_check() {
        let detector = IntelTdxDetector::new();
        let device_result = detector.check_device_node();

        // Should not panic
        assert!(device_result == true || device_result == false);
    }
}
