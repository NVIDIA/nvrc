// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! aarch64 platform implementations.
//!
//! This module provides CC detection for aarch64 platforms:
//! - ARM CCA (Confidential Compute Architecture)

use crate::core::traits::{CpuVendor, PlatformCCDetector};

/// Factory function to create aarch64 platform detector
///
/// Creates the appropriate detector based on CPU vendor.
#[allow(dead_code)] // Will be used in future PRs
pub fn create_detector(_vendor: CpuVendor) -> Box<dyn PlatformCCDetector> {
    // Placeholder implementation
    // Will be implemented in PR #6
    Box::new(Aarch64StandardDetector)
}

/// Standard (non-CC) aarch64 detector
#[allow(dead_code)] // Will be used in future PRs
#[derive(Debug)]
struct Aarch64StandardDetector;

impl PlatformCCDetector for Aarch64StandardDetector {
    fn is_cc_available(&self) -> bool {
        false
    }

    fn query_cc_mode(&self) -> crate::core::error::Result<crate::core::traits::CCMode> {
        Ok(crate::core::traits::CCMode::Off)
    }

    fn platform_description(&self) -> &str {
        "aarch64 (standard, no CC)"
    }
}
