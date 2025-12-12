// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Platform-specific traits and capabilities.

/// Platform-specific capabilities
///
/// This trait allows querying platform information at compile time.
#[allow(dead_code)]
pub trait PlatformCapabilities {
    /// Check if running on this platform
    fn is_current_platform() -> bool;

    /// Get platform name
    fn platform_name() -> &'static str;
}
