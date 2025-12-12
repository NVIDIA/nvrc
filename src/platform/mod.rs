// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Platform-specific confidential computing detection.
//!
//! This module provides platform-specific implementations for detecting
//! and querying confidential computing capabilities across different
//! CPU vendors and architectures.
//!
//! # Supported Platforms
//!
//! - **x86_64**:
//!   - AMD SEV-SNP (Secure Encrypted Virtualization - Secure Nested Paging)
//!   - Intel TDX (Trust Domain Extensions)
//! - **aarch64**:
//!   - ARM CCA (Confidential Compute Architecture)
//!
//! # Architecture
//!
//! The platform module uses a factory pattern to create the appropriate
//! detector based on runtime platform detection:
//!
//! ```text
//! detect_platform() -> PlatformInfo
//!         ↓
//! create_platform_detector(PlatformInfo) -> Box<dyn PlatformCCDetector>
//!         ↓
//! detector.query_cc_mode() -> CCMode
//! ```

pub mod detector;
pub mod traits;

// Platform-specific modules (conditionally compiled)
#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(target_arch = "aarch64")]
pub mod aarch64;

use crate::core::traits::{PlatformCCDetector, PlatformInfo};

/// Factory function to create the appropriate platform CC detector
///
/// This function creates a platform-specific detector based on the
/// provided platform information. The detector implements the
/// `PlatformCCDetector` trait.
///
/// # Examples
///
/// ```no_run
/// use nvrc::platform::{detector, create_platform_detector};
///
/// let platform = detector::detect_platform()?;
/// let detector = create_platform_detector(platform);
/// let cc_mode = detector.query_cc_mode()?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[allow(dead_code)]
pub fn create_platform_detector(platform: PlatformInfo) -> Box<dyn PlatformCCDetector> {
    #[cfg(target_arch = "x86_64")]
    {
        x86_64::create_detector(platform.vendor)
    }

    #[cfg(target_arch = "aarch64")]
    {
        aarch64::create_detector(platform.vendor)
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        compile_error!("Unsupported architecture. Only x86_64 and aarch64 are supported.");
    }
}
