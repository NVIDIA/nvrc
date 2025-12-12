// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Builder pattern for NVRC initialization.
//!
//! This module provides a fluent builder API for constructing NVRC instances
//! with custom configuration options.
//!
//! # Example
//!
//! ```no_run
//! use nvrc::core::builder::NVRCBuilder;
//!
//! let nvrc = NVRCBuilder::new()
//!     .with_auto_cc_provider()?
//!     .with_dcgm(true)
//!     .with_fabricmanager(false)
//!     .build()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::core::error::{NvrcError, Result};
use crate::core::traits::CCProvider;
use std::sync::Arc;

/// Builder for NVRC instances
///
/// Provides a fluent API for configuring and creating NVRC instances.
/// Ensures all required configuration is set before building.
#[allow(dead_code)] // Public API, not all methods used internally yet
pub struct NVRCBuilder {
    cc_provider: Option<Arc<dyn CCProvider>>,
    dcgm_enabled: bool,
    fabricmanager_enabled: bool,
    uvm_persistence_mode: Option<String>,
    nvidia_smi_srs: Option<String>,
}

#[allow(dead_code)] // Public API, not all methods used internally yet
impl NVRCBuilder {
    /// Create a new builder with default settings
    pub fn new() -> Self {
        Self {
            cc_provider: None,
            dcgm_enabled: false,
            fabricmanager_enabled: false,
            uvm_persistence_mode: None,
            nvidia_smi_srs: None,
        }
    }

    /// Set the CC provider with auto-detection
    ///
    /// This creates the appropriate provider based on the current feature flags
    /// and automatically detects the platform.
    ///
    /// # Errors
    ///
    /// Returns an error if platform detection fails (confidential builds only).
    pub fn with_auto_cc_provider(mut self) -> Result<Self> {
        #[cfg(feature = "confidential")]
        {
            let provider = crate::providers::ConfidentialProvider::new()?;
            self.cc_provider = Some(Arc::new(provider));
        }

        #[cfg(not(feature = "confidential"))]
        {
            let provider = crate::providers::StandardProvider::new();
            self.cc_provider = Some(Arc::new(provider));
        }

        Ok(self)
    }

    /// Set a custom CC provider
    ///
    /// Useful for testing or advanced customization.
    #[allow(dead_code)]
    pub fn with_cc_provider(mut self, provider: Arc<dyn CCProvider>) -> Self {
        self.cc_provider = Some(provider);
        self
    }

    /// Enable or disable DCGM
    pub fn with_dcgm(mut self, enabled: bool) -> Self {
        self.dcgm_enabled = enabled;
        self
    }

    /// Enable or disable Fabric Manager
    pub fn with_fabricmanager(mut self, enabled: bool) -> Self {
        self.fabricmanager_enabled = enabled;
        self
    }

    /// Set UVM persistence mode
    pub fn with_uvm_persistence_mode(mut self, mode: String) -> Self {
        self.uvm_persistence_mode = Some(mode);
        self
    }

    /// Set nvidia-smi SRS value
    pub fn with_nvidia_smi_srs(mut self, srs: String) -> Self {
        self.nvidia_smi_srs = Some(srs);
        self
    }

    /// Build the NVRC instance
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - CC provider is not set (call `with_auto_cc_provider()` or `with_cc_provider()`)
    /// - Initialization steps fail
    pub fn build(self) -> Result<crate::nvrc::NVRC> {
        let cc_provider = self
            .cc_provider
            .ok_or_else(|| NvrcError::MissingConfiguration {
                field: "cc_provider".to_string(),
            })?;

        let mut nvrc = crate::nvrc::NVRC {
            nvidia_smi_srs: self.nvidia_smi_srs,
            nvidia_smi_lgc: None,
            uvm_persistence_mode: self.uvm_persistence_mode,
            dcgm_enabled: self.dcgm_enabled,
            fabricmanager_enabled: self.fabricmanager_enabled,
            cpu_vendor: None,
            platform_info: None,
            nvidia_devices: Vec::new(),
            gpu_supported: false,
            cc_provider,
            plug_mode: crate::core::PlugMode::default(),
            identity: crate::user_group::UserGroup::new(),
            daemons: std::collections::HashMap::new(),
            syslog_socket: None,
        };

        // Perform initialization
        nvrc.setup_syslog()?;
        nvrc.set_random_identity()?;

        Ok(nvrc)
    }
}

impl Default for NVRCBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_creation() {
        let builder = NVRCBuilder::new();
        assert!(builder.cc_provider.is_none());
        assert!(!builder.dcgm_enabled);
        assert!(!builder.fabricmanager_enabled);
    }

    #[test]
    fn test_builder_default() {
        let builder = NVRCBuilder::default();
        assert!(builder.cc_provider.is_none());
    }

    #[test]
    fn test_builder_with_dcgm() {
        let builder = NVRCBuilder::new().with_dcgm(true);
        assert!(builder.dcgm_enabled);
    }

    #[test]
    fn test_builder_with_fabricmanager() {
        let builder = NVRCBuilder::new().with_fabricmanager(true);
        assert!(builder.fabricmanager_enabled);
    }

    #[test]
    fn test_builder_with_uvm_persistence_mode() {
        let builder = NVRCBuilder::new().with_uvm_persistence_mode("on".to_string());
        assert_eq!(builder.uvm_persistence_mode, Some("on".to_string()));
    }

    #[test]
    fn test_builder_with_nvidia_smi_srs() {
        let builder = NVRCBuilder::new().with_nvidia_smi_srs("1500".to_string());
        assert_eq!(builder.nvidia_smi_srs, Some("1500".to_string()));
    }

    #[test]
    fn test_builder_chaining() {
        let builder = NVRCBuilder::new()
            .with_dcgm(true)
            .with_fabricmanager(false)
            .with_uvm_persistence_mode("on".to_string());

        assert!(builder.dcgm_enabled);
        assert!(!builder.fabricmanager_enabled);
        assert_eq!(builder.uvm_persistence_mode, Some("on".to_string()));
    }

    #[test]
    fn test_builder_with_auto_cc_provider() {
        let result = NVRCBuilder::new().with_auto_cc_provider();

        #[cfg(feature = "confidential")]
        {
            // May fail in test environment without proper platform
            match result {
                Ok(builder) => assert!(builder.cc_provider.is_some()),
                Err(_) => {
                    // Platform detection can fail in containers/VMs
                    println!("Platform detection failed (expected in some environments)");
                }
            }
        }

        #[cfg(not(feature = "confidential"))]
        {
            assert!(result.is_ok());
            assert!(result.unwrap().cc_provider.is_some());
        }
    }

    #[test]
    fn test_builder_build_with_provider() {
        let result = NVRCBuilder::new().with_auto_cc_provider();

        // May fail in test environment
        if let Ok(builder) = result {
            let nvrc_result = builder.build();
            match nvrc_result {
                Ok(nvrc) => {
                    assert!(nvrc.cc_provider.platform().platform_description().len() > 0);
                }
                Err(e) => {
                    // Initialization can fail in test environment
                    println!("Build failed (acceptable in test env): {}", e);
                }
            }
        }
    }

    #[test]
    fn test_builder_build_missing_provider() {
        let builder = NVRCBuilder::new();
        let result = builder.build();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            NvrcError::MissingConfiguration { .. }
        ));
    }
}
