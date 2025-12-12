// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Error types for NVRC operations.
//!
//! This module defines domain-specific error types using `thiserror`,
//! providing better error messages and type-safe error handling compared
//! to generic `anyhow::Error`.
//!
//! # Error Categories
//!
//! - **Platform Errors**: CPU vendor detection, CC mode queries
//! - **Device Errors**: PCI device enumeration and parsing
//! - **GPU Errors**: GPU architecture detection, CC mode queries
//! - **Daemon Errors**: Process management operations
//! - **System Errors**: Mount, file I/O operations
//!
//! # Migration Strategy
//!
//! This module is designed to work alongside `anyhow` during migration:
//! - New code should use `NvrcError` and `Result<T>`
//! - Existing code can continue using `anyhow::Result`
//! - `NvrcError` implements `From<anyhow::Error>` for gradual migration

use std::io;
use std::num::ParseIntError;
use std::path::PathBuf;
use thiserror::Error;

/// Result type alias for NVRC operations
pub type Result<T> = std::result::Result<T, NvrcError>;

/// Main error type for NVRC operations
#[derive(Error, Debug)]
pub enum NvrcError {
    // ========================================================================
    // Platform Errors
    // ========================================================================
    /// CPU vendor could not be detected from /proc/cpuinfo
    #[error("CPU vendor detection failed: {details}")]
    CpuVendorDetectionFailed { details: String },

    /// Platform CC mode query failed
    #[error("Platform CC mode query failed for {platform}: {source}")]
    PlatformCCQueryFailed {
        platform: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Unsupported platform for CC operations
    #[error("Unsupported platform: {arch} with {vendor}")]
    UnsupportedPlatform { arch: String, vendor: String },

    // ========================================================================
    // Device Errors
    // ========================================================================
    /// PCI device not found
    #[error("PCI device not found: {bdf}")]
    DeviceNotFound { bdf: String },

    /// Failed to read device information from sysfs
    #[error("Failed to read device info from sysfs: {path}")]
    DeviceInfoReadFailed {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Failed to parse PCI identifier (device ID, vendor ID, class ID)
    #[error("Failed to parse PCI {field}: {value}")]
    PciParseError {
        field: String,
        value: String,
        #[source]
        source: ParseIntError,
    },

    /// Device is not an NVIDIA device
    #[error("Not an NVIDIA device: vendor ID {vendor_id:#06x}")]
    NotNvidiaDevice { vendor_id: u16 },

    // ========================================================================
    // GPU Errors
    // ========================================================================
    /// GPU is not supported (not in supported device list)
    #[error("GPU not supported: device ID {device_id:#06x}")]
    UnsupportedGpu { device_id: u16 },

    /// GPU architecture could not be determined
    #[error("Unknown GPU architecture for device {device_id:#06x} ({device_name})")]
    UnknownGpuArchitecture { device_id: u16, device_name: String },

    /// Failed to query GPU CC mode
    #[error("GPU CC mode query failed for {bdf}: {reason}")]
    GpuCCQueryFailed { bdf: String, reason: String },

    /// GPUs have inconsistent CC modes
    #[error("Inconsistent GPU CC modes: {bdf} has {actual:?}, expected {expected:?}")]
    InconsistentGpuCCModes {
        bdf: String,
        actual: crate::core::traits::CCMode,
        expected: crate::core::traits::CCMode,
    },

    /// BAR0 access failed
    #[error("BAR0 access failed for {bdf} at offset {offset:#x}: {reason}")]
    Bar0AccessFailed {
        bdf: String,
        offset: u64,
        reason: String,
    },

    /// Register offset out of bounds
    #[error("Register offset {offset:#x} exceeds BAR0 size {size:#x} for {bdf}")]
    RegisterOutOfBounds {
        bdf: String,
        offset: u64,
        size: usize,
    },

    // ========================================================================
    // Daemon Errors
    // ========================================================================
    /// Failed to start a daemon
    #[error("Failed to start daemon {daemon}: {source}")]
    DaemonStartFailed {
        daemon: String,
        #[source]
        source: io::Error,
    },

    /// Failed to stop a daemon
    #[error("Failed to stop daemon {daemon}: {source}")]
    DaemonStopFailed {
        daemon: String,
        #[source]
        source: io::Error,
    },

    /// Daemon process exited unexpectedly
    #[error("Daemon {daemon} exited with status: {status}")]
    DaemonExitedUnexpectedly { daemon: String, status: String },

    /// Failed to execute a command
    #[error("Command execution failed: {command}")]
    CommandExecutionFailed {
        command: String,
        #[source]
        source: io::Error,
    },

    /// Command returned non-zero exit code
    #[error("Command failed with status {status}: {command}")]
    CommandFailed { command: String, status: String },

    // ========================================================================
    // System Errors
    // ========================================================================
    /// Mount operation failed
    #[error("Mount failed: {mount_source} on {target}")]
    MountFailed {
        mount_source: String,
        target: String,
        #[source]
        source: nix::errno::Errno,
    },

    /// File operation failed
    #[error("File operation failed: {path}")]
    FileOperationFailed {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Directory operation failed
    #[error("Directory operation failed: {path}")]
    DirectoryOperationFailed {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Failed to create device node
    #[error("Device node creation failed: {path}")]
    DeviceNodeCreationFailed {
        path: String,
        #[source]
        source: nix::errno::Errno,
    },

    /// Kernel parameter parsing failed
    #[error("Kernel parameter parsing failed: {param}={value}")]
    KernelParamParseFailed { param: String, value: String },

    // ========================================================================
    // Configuration Errors
    // ========================================================================
    /// Invalid configuration value
    #[error("Invalid configuration: {field} = {value}")]
    InvalidConfiguration { field: String, value: String },

    /// Missing required configuration
    #[error("Missing required configuration: {field}")]
    MissingConfiguration { field: String },

    /// Supported GPU device list not found
    #[error("Supported GPU device list not found: {path}")]
    SupportedDeviceListNotFound { path: PathBuf },

    // ========================================================================
    // Generic Errors
    // ========================================================================
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Nix (POSIX) error
    #[error("System error: {0}")]
    Nix(#[from] nix::errno::Errno),

    /// Parse integer error
    #[error("Integer parse error: {0}")]
    ParseInt(#[from] ParseIntError),

    /// Generic error with context
    #[error("{context}: {source}")]
    Other {
        context: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl NvrcError {
    /// Create a new "other" error with context
    pub fn other(
        context: impl Into<String>,
        source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self::Other {
            context: context.into(),
            source: source.into(),
        }
    }

    /// Create a device not found error
    pub fn device_not_found(bdf: impl Into<String>) -> Self {
        Self::DeviceNotFound { bdf: bdf.into() }
    }

    /// Create a CPU vendor detection failed error
    pub fn cpu_vendor_detection_failed(details: impl Into<String>) -> Self {
        Self::CpuVendorDetectionFailed {
            details: details.into(),
        }
    }

    /// Create an unsupported GPU error
    pub fn unsupported_gpu(device_id: u16) -> Self {
        Self::UnsupportedGpu { device_id }
    }

    /// Create an unknown GPU architecture error
    pub fn unknown_gpu_architecture(device_id: u16, device_name: impl Into<String>) -> Self {
        Self::UnknownGpuArchitecture {
            device_id,
            device_name: device_name.into(),
        }
    }
}

// Allow conversion from anyhow::Error for gradual migration
impl From<anyhow::Error> for NvrcError {
    fn from(err: anyhow::Error) -> Self {
        Self::Other {
            context: "anyhow error".to_string(),
            source: err.into(),
        }
    }
}

// Note: We don't implement From<NvrcError> for anyhow::Error
// because anyhow already provides a blanket impl for all std::error::Error types

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::traits::CCMode;
    use std::error::Error;
    use std::io;

    #[test]
    fn test_error_display() {
        let err = NvrcError::device_not_found("0000:01:00.0");
        assert_eq!(err.to_string(), "PCI device not found: 0000:01:00.0");

        let err = NvrcError::unsupported_gpu(0x1234);
        assert_eq!(err.to_string(), "GPU not supported: device ID 0x1234");

        let err = NvrcError::unknown_gpu_architecture(0x1234, "Test GPU");
        assert_eq!(
            err.to_string(),
            "Unknown GPU architecture for device 0x1234 (Test GPU)"
        );
    }

    #[test]
    fn test_inconsistent_gpu_cc_modes() {
        let err = NvrcError::InconsistentGpuCCModes {
            bdf: "0000:02:00.0".to_string(),
            actual: CCMode::Off,
            expected: CCMode::On,
        };
        assert!(err
            .to_string()
            .contains("Inconsistent GPU CC modes: 0000:02:00.0"));
        assert!(err.to_string().contains("Off"));
        assert!(err.to_string().contains("On"));
    }

    #[test]
    fn test_register_out_of_bounds() {
        let err = NvrcError::RegisterOutOfBounds {
            bdf: "0000:01:00.0".to_string(),
            offset: 0x1000,
            size: 0x800,
        };
        assert!(err.to_string().contains("0x1000"));
        assert!(err.to_string().contains("0x800"));
    }

    #[test]
    fn test_daemon_errors() {
        let err = NvrcError::DaemonStartFailed {
            daemon: "nvidia-persistenced".to_string(),
            source: io::Error::new(io::ErrorKind::NotFound, "not found"),
        };
        assert!(err.to_string().contains("nvidia-persistenced"));

        let err = NvrcError::DaemonExitedUnexpectedly {
            daemon: "dcgm-exporter".to_string(),
            status: "exit code: 1".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Daemon dcgm-exporter exited with status: exit code: 1"
        );
    }

    #[test]
    fn test_mount_error() {
        let err = NvrcError::MountFailed {
            mount_source: "proc".to_string(),
            target: "/proc".to_string(),
            source: nix::errno::Errno::EPERM,
        };
        assert!(err.to_string().contains("/proc"));
    }

    #[test]
    fn test_pci_parse_error() {
        let err = NvrcError::PciParseError {
            field: "device ID".to_string(),
            value: "invalid".to_string(),
            source: "invalid".parse::<u16>().unwrap_err(),
        };
        assert!(err.to_string().contains("device ID"));
        assert!(err.to_string().contains("invalid"));
    }

    #[test]
    fn test_kernel_param_parse_failed() {
        let err = NvrcError::KernelParamParseFailed {
            param: "nvrc.log".to_string(),
            value: "invalid_level".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Kernel parameter parsing failed: nvrc.log=invalid_level"
        );
    }

    #[test]
    fn test_configuration_errors() {
        let err = NvrcError::InvalidConfiguration {
            field: "dcgm_enabled".to_string(),
            value: "maybe".to_string(),
        };
        assert!(err.to_string().contains("dcgm_enabled"));

        let err = NvrcError::MissingConfiguration {
            field: "cc_provider".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Missing required configuration: cc_provider"
        );
    }

    #[test]
    fn test_from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let nvrc_err: NvrcError = io_err.into();
        assert!(nvrc_err.to_string().contains("file not found"));
    }

    #[test]
    fn test_from_parse_int_error() {
        let parse_err = "invalid".parse::<u16>().unwrap_err();
        let nvrc_err: NvrcError = parse_err.into();
        assert!(matches!(nvrc_err, NvrcError::ParseInt(_)));
    }

    #[test]
    fn test_error_source_chain() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "permission denied");
        let err = NvrcError::FileOperationFailed {
            path: PathBuf::from("/proc/cmdline"),
            source: io_err,
        };

        // Test that the error source chain is preserved
        assert!(err.source().is_some());
        assert!(err.source().unwrap().to_string().contains("permission"));
    }

    #[test]
    fn test_anyhow_compatibility() {
        // Test conversion from anyhow
        let anyhow_err = anyhow::anyhow!("test error");
        let nvrc_err: NvrcError = anyhow_err.into();
        assert!(matches!(nvrc_err, NvrcError::Other { .. }));

        // Test conversion to anyhow (via std::error::Error)
        let nvrc_err = NvrcError::device_not_found("0000:01:00.0");
        let anyhow_err = anyhow::Error::new(nvrc_err);
        assert!(anyhow_err.to_string().contains("0000:01:00.0"));
    }

    #[test]
    fn test_helper_constructors() {
        let err = NvrcError::other("test context", io::Error::new(io::ErrorKind::Other, "test"));
        assert!(err.to_string().contains("test context"));

        let err = NvrcError::cpu_vendor_detection_failed("no vendor string found");
        assert!(err.to_string().contains("no vendor string found"));
    }
}
