// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Standard (non-confidential) aarch64 platform detector.

use crate::core::error::Result;
use crate::core::traits::{CCMode, PlatformCCDetector};

/// Standard aarch64 detector (no CC support)
#[derive(Debug, Default)]
pub struct Aarch64StandardDetector;

impl Aarch64StandardDetector {
    /// Create a new standard aarch64 detector
    pub fn new() -> Self {
        Self
    }
}

impl PlatformCCDetector for Aarch64StandardDetector {
    fn is_cc_available(&self) -> bool {
        false
    }

    fn query_cc_mode(&self) -> Result<CCMode> {
        Ok(CCMode::Off)
    }

    fn platform_description(&self) -> &str {
        "aarch64 (standard, no CC)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aarch64_standard_detector() {
        let detector = Aarch64StandardDetector::new();
        assert!(!detector.is_cc_available());
        assert_eq!(detector.query_cc_mode().unwrap(), CCMode::Off);
        assert_eq!(detector.platform_description(), "aarch64 (standard, no CC)");
        assert_eq!(detector.guest_device_path(), None);
    }
}
