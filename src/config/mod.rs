// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Configuration parsing from kernel parameters.
//!
//! This module provides idiomatic parsing of kernel command-line parameters
//! into a configuration object that can be consumed by the NVRC builder.
//!
//! # Design
//!
//! Instead of mutating NVRC after construction, we parse kernel parameters
//! into an immutable `KernelParams` object that the builder consumes.
//!
//! # Example
//!
//! ```no_run
//! use nvrc::config::KernelParams;
//! use nvrc::core::builder::NVRCBuilder;
//!
//! let config = KernelParams::from_cmdline(None)?;
//! let nvrc = NVRCBuilder::new()
//!     .with_auto_cc_provider()?
//!     .with_kernel_config(config)
//!     .build()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod parser;

use std::fs;
use std::str::FromStr;

use crate::core::error::Result;

/// PCI device ID override entry
///
/// Format: arch_name,vendor_id,device_id
/// Example: "hopper,10de,2334"
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PciDeviceOverride {
    pub arch_name: String,
    pub vendor_id: u16,
    pub device_id: u16,
}

/// Parsed kernel configuration
///
/// This struct represents the parsed kernel command-line parameters
/// relevant to NVRC operation. It's immutable and can be passed to
/// the NVRC builder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KernelParams {
    /// Log level setting (nvrc.log)
    pub log_level: Option<LogLevel>,
    /// UVM persistence mode (nvrc.uvm.persistence.mode)
    pub uvm_persistence_mode: Option<String>,
    /// DCGM enabled (nvrc.dcgm)
    pub dcgm_enabled: Option<bool>,
    /// Fabric Manager enabled (nvrc.fabricmanager)
    pub fabricmanager_enabled: Option<bool>,
    /// nvidia-smi SRS value (nvrc.smi.srs)
    pub nvidia_smi_srs: Option<String>,
    /// nvidia-smi LGC value (nvrc.smi.lgc) - for future use
    pub nvidia_smi_lgc: Option<String>,
    /// PCI device ID overrides (nvrc.pci.device.id)
    ///
    /// Allows adding device IDs not yet in PCI database.
    /// Format: "arch_name,vendor_id,device_id" (e.g., "hopper,10de,2334")
    /// Can be specified multiple times.
    pub pci_device_overrides: Vec<PciDeviceOverride>,
}

/// Log level setting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl FromStr for LogLevel {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "off" | "0" | "" => Self::Off,
            "error" => Self::Error,
            "warn" => Self::Warn,
            "info" => Self::Info,
            "debug" => Self::Debug,
            "trace" => Self::Trace,
            _ => Self::Off,
        })
    }
}

impl From<LogLevel> for log::LevelFilter {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Off => log::LevelFilter::Off,
            LogLevel::Error => log::LevelFilter::Error,
            LogLevel::Warn => log::LevelFilter::Warn,
            LogLevel::Info => log::LevelFilter::Info,
            LogLevel::Debug => log::LevelFilter::Debug,
            LogLevel::Trace => log::LevelFilter::Trace,
        }
    }
}

impl KernelParams {
    /// Parse kernel configuration from /proc/cmdline or provided string
    ///
    /// # Arguments
    ///
    /// * `cmdline` - Optional command line string. If None, reads from `/proc/cmdline`
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use nvrc::config::KernelParams;
    ///
    /// // From /proc/cmdline
    /// let config = KernelParams::from_cmdline(None)?;
    ///
    /// // From custom string
    /// let config = KernelParams::from_cmdline(Some("nvrc.dcgm=on nvrc.log=debug"))?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn from_cmdline(cmdline: Option<&str>) -> Result<Self> {
        let content = match cmdline {
            Some(c) => c.to_owned(),
            None => fs::read_to_string("/proc/cmdline").map_err(|e| {
                crate::core::error::NvrcError::FileOperationFailed {
                    path: "/proc/cmdline".into(),
                    source: e,
                }
            })?,
        };

        Self::parse(&content)
    }

    /// Parse kernel configuration from a command-line string
    ///
    /// This is the core parsing logic that extracts NVRC-specific parameters.
    pub fn parse(cmdline: &str) -> Result<Self> {
        let mut config = Self::default();

        for (key, value) in cmdline.split_whitespace().filter_map(|p| p.split_once('=')) {
            match key {
                "nvrc.log" => {
                    config.log_level = LogLevel::from_str(value).ok();
                }
                "nvrc.uvm.persistence.mode" => {
                    config.uvm_persistence_mode = Some(value.to_owned());
                }
                "nvrc.dcgm" => {
                    config.dcgm_enabled = Some(parser::parse_boolean(value));
                }
                "nvrc.fabricmanager" => {
                    config.fabricmanager_enabled = Some(parser::parse_boolean(value));
                }
                "nvrc.smi.srs" => {
                    config.nvidia_smi_srs = Some(value.to_owned());
                }
                "nvrc.smi.lgc" => {
                    config.nvidia_smi_lgc = Some(value.to_owned());
                }
                "nvrc.pci.device.id" => {
                    // Parse: "arch_name,vendor_id,device_id"
                    if let Some(override_entry) = Self::parse_pci_override(value) {
                        config.pci_device_overrides.push(override_entry);
                    } else {
                        warn!("Invalid PCI device override format: {}", value);
                    }
                }
                _ => {} // Ignore unknown parameters
            }
        }

        debug!("Parsed kernel config: {:?}", config);
        Ok(config)
    }

    /// Parse PCI device override from kernel parameter
    ///
    /// Format: "arch_name,vendor_id,device_id"
    /// Example: "hopper,10de,2334"
    fn parse_pci_override(value: &str) -> Option<PciDeviceOverride> {
        let parts: Vec<&str> = value.split(',').collect();
        if parts.len() != 3 {
            return None;
        }

        let arch_name = parts[0].to_string();
        let vendor_id = u16::from_str_radix(parts[1].trim_start_matches("0x"), 16).ok()?;
        let device_id = u16::from_str_radix(parts[2].trim_start_matches("0x"), 16).ok()?;

        Some(PciDeviceOverride {
            arch_name,
            vendor_id,
            device_id,
        })
    }

    /// Apply this configuration to the log system
    ///
    /// This is separated because log configuration needs to happen early
    /// and affects global state.
    pub fn apply_log_config(&self) -> Result<()> {
        if let Some(level) = self.log_level {
            let filter: log::LevelFilter = level.into();
            log::set_max_level(filter);
            debug!("Log level set to: {}", log::max_level());

            // Enable kernel message logging
            fs::write("/proc/sys/kernel/printk_devkmsg", b"on\n").map_err(|e| {
                crate::core::error::NvrcError::FileOperationFailed {
                    path: "/proc/sys/kernel/printk_devkmsg".into(),
                    source: e,
                }
            })?;
        }

        Ok(())
    }

    /// Merge another config into this one
    ///
    /// Values from `other` override values in `self` if they are `Some`.
    #[allow(dead_code)] // Useful for config composition
    pub fn merge(mut self, other: Self) -> Self {
        if other.log_level.is_some() {
            self.log_level = other.log_level;
        }
        if other.uvm_persistence_mode.is_some() {
            self.uvm_persistence_mode = other.uvm_persistence_mode;
        }
        if other.dcgm_enabled.is_some() {
            self.dcgm_enabled = other.dcgm_enabled;
        }
        if other.fabricmanager_enabled.is_some() {
            self.fabricmanager_enabled = other.fabricmanager_enabled;
        }
        if other.nvidia_smi_srs.is_some() {
            self.nvidia_smi_srs = other.nvidia_smi_srs;
        }
        if other.nvidia_smi_lgc.is_some() {
            self.nvidia_smi_lgc = other.nvidia_smi_lgc;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty() {
        let config = KernelParams::parse("").unwrap();
        assert_eq!(config, KernelParams::default());
    }

    #[test]
    fn test_parse_log_level() {
        let config = KernelParams::parse("nvrc.log=debug").unwrap();
        assert_eq!(config.log_level, Some(LogLevel::Debug));

        let config = KernelParams::parse("nvrc.log=info").unwrap();
        assert_eq!(config.log_level, Some(LogLevel::Info));
    }

    #[test]
    fn test_parse_dcgm() {
        let config = KernelParams::parse("nvrc.dcgm=on").unwrap();
        assert_eq!(config.dcgm_enabled, Some(true));

        let config = KernelParams::parse("nvrc.dcgm=off").unwrap();
        assert_eq!(config.dcgm_enabled, Some(false));
    }

    #[test]
    fn test_parse_multiple() {
        let config =
            KernelParams::parse("nvrc.log=debug nvrc.dcgm=on nvrc.fabricmanager=off").unwrap();
        assert_eq!(config.log_level, Some(LogLevel::Debug));
        assert_eq!(config.dcgm_enabled, Some(true));
        assert_eq!(config.fabricmanager_enabled, Some(false));
    }

    #[test]
    fn test_parse_with_other_params() {
        let config = KernelParams::parse("root=/dev/sda nvrc.dcgm=on quiet splash").unwrap();
        assert_eq!(config.dcgm_enabled, Some(true));
        // Other parameters ignored
    }

    #[test]
    fn test_log_level_from_str() {
        assert_eq!(LogLevel::from_str("debug").unwrap(), LogLevel::Debug);
        assert_eq!(LogLevel::from_str("0").unwrap(), LogLevel::Off);
        assert_eq!(LogLevel::from_str("").unwrap(), LogLevel::Off);
        assert_eq!(LogLevel::from_str("invalid").unwrap(), LogLevel::Off);
    }

    #[test]
    fn test_log_level_to_filter() {
        let filter: log::LevelFilter = LogLevel::Debug.into();
        assert_eq!(filter, log::LevelFilter::Debug);
    }

    #[test]
    fn test_config_merge() {
        let base = KernelParams {
            dcgm_enabled: Some(true),
            log_level: Some(LogLevel::Info),
            ..Default::default()
        };

        let override_config = KernelParams {
            dcgm_enabled: Some(false),
            fabricmanager_enabled: Some(true),
            ..Default::default()
        };

        let merged = base.merge(override_config);
        assert_eq!(merged.dcgm_enabled, Some(false)); // Overridden
        assert_eq!(merged.log_level, Some(LogLevel::Info)); // Kept
        assert_eq!(merged.fabricmanager_enabled, Some(true)); // Added
    }

    #[test]
    fn test_parse_pci_override() {
        // Valid formats
        let override1 = KernelParams::parse_pci_override("hopper,10de,2334");
        assert!(override1.is_some());
        let o = override1.unwrap();
        assert_eq!(o.arch_name, "hopper");
        assert_eq!(o.vendor_id, 0x10de);
        assert_eq!(o.device_id, 0x2334);

        // With 0x prefix
        let override2 = KernelParams::parse_pci_override("blackwell,0x10de,0x2900");
        assert!(override2.is_some());
        let o = override2.unwrap();
        assert_eq!(o.vendor_id, 0x10de);
        assert_eq!(o.device_id, 0x2900);

        // Invalid formats
        assert!(KernelParams::parse_pci_override("invalid").is_none());
        assert!(KernelParams::parse_pci_override("hopper,10de").is_none());
        assert!(KernelParams::parse_pci_override("hopper,XXXX,2334").is_none());
    }

    #[test]
    fn test_parse_with_pci_overrides() {
        let config = KernelParams::parse(
            "nvrc.pci.device.id=hopper,10de,2334 nvrc.pci.device.id=blackwell,10de,2900",
        )
        .unwrap();
        assert_eq!(config.pci_device_overrides.len(), 2);
        assert_eq!(config.pci_device_overrides[0].arch_name, "hopper");
        assert_eq!(config.pci_device_overrides[1].arch_name, "blackwell");
    }
}
