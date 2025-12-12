// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Standard (non-confidential) x86_64 platform detector.

use crate::core::error::Result;
use crate::core::traits::{CCMode, PlatformCCDetector};

/// Standard x86_64 detector (no CC support)
#[derive(Debug, Default)]
pub struct X86StandardDetector;

impl X86StandardDetector {
    /// Create a new standard x86_64 detector
    pub fn new() -> Self {
        Self
    }
}

impl PlatformCCDetector for X86StandardDetector {
    fn is_cc_available(&self) -> bool {
        false
    }

    fn query_cc_mode(&self) -> Result<CCMode> {
        Ok(CCMode::Off)
    }

    fn platform_description(&self) -> &str {
        "x86_64 (standard, no CC)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_x86_standard_detector() {
        let detector = X86StandardDetector::new();
        assert!(!detector.is_cc_available());
        assert_eq!(detector.query_cc_mode().unwrap(), CCMode::Off);
        assert_eq!(detector.platform_description(), "x86_64 (standard, no CC)");
        assert_eq!(detector.guest_device_path(), None);
    }
}
