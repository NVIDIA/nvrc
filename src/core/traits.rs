// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Core traits for platform and GPU confidential computing detection.
//!
//! This module defines the trait hierarchy that allows runtime polymorphism
//! for different platform implementations (AMD SNP, Intel TDX, ARM CCA) and
//! GPU architectures (Hopper, Blackwell, etc.).

use std::fmt::Debug;

use crate::core::error::Result;
use crate::devices::NvidiaDevice;

/// Confidential Computing mode states
#[allow(dead_code)] // Will be used in future PRs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CCMode {
    /// Confidential computing is enabled
    On,
    /// Confidential computing is disabled
    Off,
    /// Development/debug mode (CC enabled but with reduced security)
    Devtools,
}

impl CCMode {
    /// Check if any form of CC is active
    #[allow(dead_code)] // Will be used in future PRs
    pub fn is_active(self) -> bool {
        !matches!(self, CCMode::Off)
    }
}

/// CPU vendor identification
#[allow(dead_code)] // Will be used in future PRs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuVendor {
    /// AMD processors
    Amd,
    /// Intel processors
    Intel,
    /// ARM processors
    Arm,
}

/// CPU architecture
#[allow(dead_code)] // Will be used in future PRs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuArch {
    /// x86_64 / amd64 architecture
    X86_64,
    /// ARM 64-bit architecture
    Aarch64,
}

/// Platform information combining vendor and architecture
#[allow(dead_code)] // Will be used in future PRs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformInfo {
    pub vendor: CpuVendor,
    pub arch: CpuArch,
}

impl PlatformInfo {
    /// Create new platform info
    #[allow(dead_code)] // Will be used in future PRs
    pub const fn new(vendor: CpuVendor, arch: CpuArch) -> Self {
        Self { vendor, arch }
    }
}

/// Trait for platform-specific confidential computing detection
///
/// Implementations exist for:
/// - AMD SEV-SNP (x86_64)
/// - Intel TDX (x86_64)
/// - ARM CCA (aarch64)
#[allow(dead_code)] // Will be used in future PRs
pub trait PlatformCCDetector: Send + Sync + Debug {
    /// Check if confidential computing is available on this platform
    ///
    /// This typically checks both hardware capabilities (CPUID/HWCAP)
    /// and software support (device nodes).
    fn is_cc_available(&self) -> bool;

    /// Query the current confidential computing mode
    fn query_cc_mode(&self) -> Result<CCMode>;

    /// Get a human-readable description of this platform
    ///
    /// # Examples
    ///
    /// - "AMD SEV-SNP (Secure Nested Paging)"
    /// - "Intel TDX (Trust Domain Extensions)"
    /// - "ARM CCA (Confidential Compute Architecture)"
    fn platform_description(&self) -> &str;

    /// Get the device node path for guest attestation, if any
    ///
    /// # Examples
    ///
    /// - AMD: `/dev/sev-guest`
    /// - Intel: `/dev/tdx-guest`
    /// - ARM: `/dev/cca-guest`
    fn guest_device_path(&self) -> Option<&str> {
        None
    }
}

/// Trait for GPU architecture-specific operations
///
/// Each GPU architecture (Hopper, Blackwell, etc.) has different
/// register layouts and CC mode detection mechanisms.
#[allow(dead_code)] // Will be used in future PRs
pub trait GpuArchitecture: Send + Sync + Debug {
    /// Get the name of this GPU architecture
    fn name(&self) -> &str;

    /// Get the CC register offset for this architecture
    ///
    /// # Examples
    ///
    /// - Hopper: `0x001182cc`
    /// - Blackwell: `0x590`
    fn cc_register_offset(&self) -> Result<u64>;

    /// Parse CC mode from a register value
    ///
    /// The register value is read from BAR0 at the offset
    /// returned by `cc_register_offset()`.
    fn parse_cc_mode(&self, register_value: u32) -> Result<CCMode>;

    /// Check if this device ID belongs to this architecture
    ///
    /// Used for device identification when creating architecture
    /// instances.
    fn matches_device_id(&self, device_id: u16) -> bool;
}

/// Trait for GPU confidential computing operations
#[allow(dead_code)] // Will be used in future PRs
pub trait GpuCCProvider: Send + Sync + Debug {
    /// Query CC mode for a specific GPU device
    ///
    /// # Arguments
    ///
    /// * `bdf` - Bus:Device.Function identifier (e.g., "0000:01:00.0")
    /// * `device_id` - PCI device ID
    fn query_device_cc_mode(&self, bdf: &str, device_id: u16) -> Result<CCMode>;

    /// Query CC mode for all GPUs, ensuring consistency
    ///
    /// Returns an error if GPUs have inconsistent CC modes.
    /// Returns `None` if no GPUs are present.
    fn query_all_gpus_cc_mode(&self, devices: &[NvidiaDevice]) -> Result<Option<CCMode>>;

    /// Execute nvidia-smi secure remote services (SRS) command
    ///
    /// Only applicable when GPU is in CC mode.
    fn execute_srs_command(&self, srs_value: Option<&str>) -> Result<()>;
}

/// Combined provider for all confidential computing operations
///
/// This is the main trait that NVRC uses to interact with both
/// platform and GPU CC detection.
#[allow(dead_code)] // Will be used in future PRs
pub trait CCProvider: Send + Sync + Debug {
    /// Get the platform CC detector
    fn platform(&self) -> &dyn PlatformCCDetector;

    /// Get the GPU CC provider
    fn gpu(&self) -> &dyn GpuCCProvider;

    /// Query overall system CC mode (combines platform + GPU)
    ///
    /// This is a convenience method that queries both platform and GPU
    /// and returns a combined view.
    fn query_system_cc_mode(&self, devices: &[NvidiaDevice]) -> Result<SystemCCMode> {
        let platform_mode = self.platform().query_cc_mode().unwrap_or(CCMode::Off);
        let gpu_mode = self.gpu().query_all_gpus_cc_mode(devices)?;

        Ok(SystemCCMode {
            platform: platform_mode,
            gpu: gpu_mode,
        })
    }
}

/// System-wide CC mode combining platform and GPU states
#[allow(dead_code)] // Will be used in future PRs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemCCMode {
    /// Platform (CPU) CC mode
    pub platform: CCMode,
    /// GPU CC mode (None if no GPUs present)
    pub gpu: Option<CCMode>,
}

impl SystemCCMode {
    /// Check if the entire system is in CC mode
    ///
    /// Returns true only if both platform and all GPUs have CC enabled.
    #[allow(dead_code)] // Will be used in future PRs
    pub fn is_fully_enabled(&self) -> bool {
        self.platform == CCMode::On && self.gpu == Some(CCMode::On)
    }

    /// Check if any CC is enabled (platform or GPU)
    #[allow(dead_code)] // Will be used in future PRs
    pub fn has_any_cc(&self) -> bool {
        self.platform.is_active() || self.gpu.is_some_and(|m| m.is_active())
    }

    /// Check if platform and GPU CC modes are consistent
    #[allow(dead_code)] // Will be used in future PRs
    pub fn is_consistent(&self) -> bool {
        match self.gpu {
            Some(gpu_mode) => self.platform == gpu_mode,
            None => true, // No GPU is not inconsistent
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cc_mode_is_active() {
        assert!(CCMode::On.is_active());
        assert!(CCMode::Devtools.is_active());
        assert!(!CCMode::Off.is_active());
    }

    #[test]
    fn test_system_cc_mode_fully_enabled() {
        let mode = SystemCCMode {
            platform: CCMode::On,
            gpu: Some(CCMode::On),
        };
        assert!(mode.is_fully_enabled());

        let mode = SystemCCMode {
            platform: CCMode::On,
            gpu: Some(CCMode::Off),
        };
        assert!(!mode.is_fully_enabled());

        let mode = SystemCCMode {
            platform: CCMode::Off,
            gpu: Some(CCMode::On),
        };
        assert!(!mode.is_fully_enabled());
    }

    #[test]
    fn test_system_cc_mode_has_any_cc() {
        let mode = SystemCCMode {
            platform: CCMode::On,
            gpu: Some(CCMode::Off),
        };
        assert!(mode.has_any_cc());

        let mode = SystemCCMode {
            platform: CCMode::Off,
            gpu: Some(CCMode::On),
        };
        assert!(mode.has_any_cc());

        let mode = SystemCCMode {
            platform: CCMode::Off,
            gpu: None,
        };
        assert!(!mode.has_any_cc());
    }

    #[test]
    fn test_system_cc_mode_consistency() {
        let mode = SystemCCMode {
            platform: CCMode::On,
            gpu: Some(CCMode::On),
        };
        assert!(mode.is_consistent());

        let mode = SystemCCMode {
            platform: CCMode::On,
            gpu: Some(CCMode::Off),
        };
        assert!(!mode.is_consistent());

        let mode = SystemCCMode {
            platform: CCMode::On,
            gpu: None,
        };
        assert!(mode.is_consistent());
    }

    #[test]
    fn test_platform_info_new() {
        let info = PlatformInfo::new(CpuVendor::Amd, CpuArch::X86_64);
        assert_eq!(info.vendor, CpuVendor::Amd);
        assert_eq!(info.arch, CpuArch::X86_64);
    }
}
