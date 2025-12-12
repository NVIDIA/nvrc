// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! x86_64 platform implementations.
//!
//! This module provides CC detection for x86_64 platforms:
//! - AMD SEV-SNP
//! - Intel TDX

use crate::core::traits::{CpuVendor, PlatformCCDetector};

/// Factory function to create x86_64 platform detector
///
/// Creates the appropriate detector based on CPU vendor.
pub fn create_detector(_vendor: CpuVendor) -> Box<dyn PlatformCCDetector> {
    // Placeholder implementation
    // Will be implemented in PR #5
    Box::new(X86StandardDetector)
}

/// Standard (non-CC) x86_64 detector
#[derive(Debug)]
struct X86StandardDetector;

impl PlatformCCDetector for X86StandardDetector {
    fn is_cc_available(&self) -> bool {
        false
    }

    fn query_cc_mode(&self) -> crate::core::error::Result<crate::core::traits::CCMode> {
        Ok(crate::core::traits::CCMode::Off)
    }

    fn platform_description(&self) -> &str {
        "x86_64 (standard, no CC)"
    }
}

