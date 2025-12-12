// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Platform detection utilities.
//!
//! This module provides functions to detect the current CPU vendor
//! and architecture at runtime.

use crate::core::error::{NvrcError, Result};
use crate::core::traits::{CpuArch, CpuVendor, PlatformInfo};
use std::fs;

/// Detect CPU vendor from /proc/cpuinfo
///
/// Reads `/proc/cpuinfo` and searches for vendor identification strings:
/// - AMD: "AuthenticAMD"
/// - Intel: "GenuineIntel"
/// - ARM: "CPU implementer" with value "0x41"
///
/// # Errors
///
/// Returns an error if:
/// - `/proc/cpuinfo` cannot be read
/// - No recognized vendor string is found
#[allow(dead_code)] // Used in tests and will be used in future PRs
pub fn detect_cpu_vendor() -> Result<CpuVendor> {
    let data = fs::read_to_string("/proc/cpuinfo").map_err(|e| NvrcError::FileOperationFailed {
        path: "/proc/cpuinfo".into(),
        source: e,
    })?;

    for line in data.lines() {
        if line.contains("AuthenticAMD") {
            return Ok(CpuVendor::Amd);
        }
        if line.contains("GenuineIntel") {
            return Ok(CpuVendor::Intel);
        }
        if line.contains("CPU implementer") && line.contains("0x41") {
            return Ok(CpuVendor::Arm);
        }
    }

    Err(NvrcError::cpu_vendor_detection_failed(
        "No recognized vendor string found in /proc/cpuinfo",
    ))
}

/// Detect CPU architecture at compile time
///
/// Returns the current architecture based on the compilation target.
/// This is a zero-cost abstraction resolved at compile time.
#[allow(dead_code)] // Used in tests and will be used in future PRs
pub const fn detect_cpu_arch() -> CpuArch {
    #[cfg(target_arch = "x86_64")]
    return CpuArch::X86_64;

    #[cfg(target_arch = "aarch64")]
    return CpuArch::Aarch64;

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    compile_error!("Unsupported architecture");
}

/// Detect full platform information
///
/// Combines runtime CPU vendor detection with compile-time architecture
/// detection to provide complete platform information.
///
/// # Examples
///
/// ```no_run
/// use nvrc::platform::detector::detect_platform;
///
/// let platform = detect_platform()?;
/// println!("Running on {:?} {:?}", platform.vendor, platform.arch);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[allow(dead_code)] // Used in tests and will be used in future PRs
pub fn detect_platform() -> Result<PlatformInfo> {
    let vendor = detect_cpu_vendor()?;
    let arch = detect_cpu_arch();

    Ok(PlatformInfo { vendor, arch })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_cpu_vendor() {
        // Should succeed on any supported platform
        let result = detect_cpu_vendor();
        assert!(result.is_ok(), "Failed to detect CPU vendor: {:?}", result);

        let vendor = result.unwrap();
        assert!(
            matches!(vendor, CpuVendor::Amd | CpuVendor::Intel | CpuVendor::Arm),
            "Unexpected vendor: {:?}",
            vendor
        );
    }

    #[test]
    fn test_detect_cpu_arch() {
        let arch = detect_cpu_arch();

        #[cfg(target_arch = "x86_64")]
        assert_eq!(arch, CpuArch::X86_64);

        #[cfg(target_arch = "aarch64")]
        assert_eq!(arch, CpuArch::Aarch64);
    }

    #[test]
    fn test_detect_platform() {
        let result = detect_platform();
        assert!(result.is_ok(), "Failed to detect platform: {:?}", result);

        let platform = result.unwrap();

        // Verify architecture matches compile target
        #[cfg(target_arch = "x86_64")]
        {
            assert_eq!(platform.arch, CpuArch::X86_64);
            assert!(matches!(platform.vendor, CpuVendor::Amd | CpuVendor::Intel));
        }

        #[cfg(target_arch = "aarch64")]
        {
            assert_eq!(platform.arch, CpuArch::Aarch64);
            assert_eq!(platform.vendor, CpuVendor::Arm);
        }
    }
}
