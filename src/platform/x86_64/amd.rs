// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! AMD SEV-SNP (Secure Encrypted Virtualization - Secure Nested Paging) detection.
//!
//! This module implements confidential computing detection for AMD platforms.
//! SEV-SNP is AMD's technology for securing virtual machines by encrypting
//! their memory and providing attestation capabilities.
//!
//! # Detection Strategy
//!
//! SEV-SNP detection requires both:
//! 1. **CPUID check**: Verify hardware support (CPUID.8000_001F.EAX[4])
//! 2. **Device node check**: Verify kernel support (`/dev/sev-guest`)
//!
//! Both must be present for SEV-SNP to be considered available.

use crate::core::error::Result;
use crate::core::traits::{CCMode, PlatformCCDetector};
use std::path::Path;

/// AMD SEV-SNP detector
#[derive(Debug, Default)]
pub struct AmdSnpDetector;

impl AmdSnpDetector {
    /// Create a new AMD SEV-SNP detector
    pub fn new() -> Self {
        Self
    }

    /// Check CPUID for SNP support (CPUID.8000_001F.EAX[4])
    fn check_cpuid(&self) -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            unsafe {
                use core::arch::x86_64::__cpuid_count;
                let result = __cpuid_count(0x8000_001f, 0);
                (result.eax & (1 << 4)) != 0
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            false
        }
    }

    /// Check for /dev/sev-guest device node
    fn check_device_node(&self) -> bool {
        Path::new("/dev/sev-guest").exists()
    }
}

impl PlatformCCDetector for AmdSnpDetector {
    fn is_cc_available(&self) -> bool {
        let cpuid = self.check_cpuid();
        let device = self.check_device_node();

        debug!("AMD SEV-SNP: cpuid={}, device={}", cpuid, device);

        if cpuid && !device {
            warn!("AMD SEV-SNP: CPUID bit set but device node missing");
        }
        if device && !cpuid {
            warn!("AMD SEV-SNP: Device node present but CPUID bit not set");
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

    fn cc_technology_name(&self) -> &str {
        "AMD SEV-SNP"
    }

    fn guest_device_path(&self) -> Option<&str> {
        Some("/dev/sev-guest")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_amd_snp_detector_creation() {
        let detector = AmdSnpDetector::new();
        assert_eq!(detector.cc_technology_name(), "AMD SEV-SNP");
        assert!(detector.platform_description().contains("AMD SEV-SNP"));
        assert_eq!(detector.guest_device_path(), Some("/dev/sev-guest"));
    }

    #[test]
    fn test_amd_snp_query_cc_mode() {
        let detector = AmdSnpDetector::new();
        let result = detector.query_cc_mode();
        assert!(result.is_ok());

        // Mode should be On or Off, never error
        let mode = result.unwrap();
        assert!(matches!(mode, CCMode::On | CCMode::Off));
    }

    #[test]
    fn test_amd_snp_detection_logic() {
        let detector = AmdSnpDetector::new();

        // is_cc_available should not panic
        let available = detector.is_cc_available();
        assert!(available == true || available == false);

        // If available, mode should be On
        if available {
            assert_eq!(detector.query_cc_mode().unwrap(), CCMode::On);
        }
    }

    #[test]
    fn test_amd_snp_cpuid_check() {
        let detector = AmdSnpDetector::new();
        let cpuid_result = detector.check_cpuid();

        // Should not panic
        assert!(cpuid_result == true || cpuid_result == false);
    }

    #[test]
    fn test_amd_snp_device_node_check() {
        let detector = AmdSnpDetector::new();
        let device_result = detector.check_device_node();

        // Should not panic
        assert!(device_result == true || device_result == false);
    }
}
