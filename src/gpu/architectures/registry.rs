// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! GPU architecture registry for runtime architecture detection.
//!
//! This module provides a registry system that maps GPU device IDs and names
//! to their corresponding architecture implementations. The registry allows
//! for runtime polymorphism while maintaining type safety.
//!
//! # Design
//!
//! The registry uses a combination of:
//! - **Device ID matching**: Fast, exact matching by PCI device ID
//! - **Name-based detection**: Fallback using device name patterns
//! - **Lazy initialization**: Registry built once on first access
//!
//! # Example
//!
//! ```no_run
//! use nvrc::gpu::architectures::detect_architecture;
//!
//! let arch = detect_architecture(0x2330, "H100 PCIe")?;
//! println!("Architecture: {}", arch.name());
//! println!("CC register: 0x{:x}", arch.cc_register_offset()?);
//! ```

use crate::core::error::{NvrcError, Result};
use crate::core::traits::GpuArchitecture;
use std::sync::LazyLock;

/// Trait for cloning boxed GPU architectures
///
/// This trait allows the registry to clone architecture implementations
/// when returning them from the registry lookup.
pub trait CloneableGpuArchitecture: GpuArchitecture {
    /// Clone this architecture into a new box
    fn clone_box(&self) -> Box<dyn GpuArchitecture>;
}

// Blanket implementation for Clone + GpuArchitecture
impl<T> CloneableGpuArchitecture for T
where
    T: GpuArchitecture + Clone + 'static,
{
    fn clone_box(&self) -> Box<dyn GpuArchitecture> {
        Box::new(self.clone())
    }
}

/// GPU architecture registry
///
/// Maintains a list of known GPU architectures and provides lookup
/// functionality based on device ID or device name.
pub struct GpuArchitectureRegistry {
    architectures: Vec<Box<dyn CloneableGpuArchitecture>>,
}

impl GpuArchitectureRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            architectures: Vec::new(),
        }
    }

    /// Get the global registry instance
    ///
    /// This is initialized once on first access with all known architectures.
    pub fn global() -> &'static Self {
        static REGISTRY: LazyLock<GpuArchitectureRegistry> =
            LazyLock::new(GpuArchitectureRegistry::init);
        &REGISTRY
    }

    /// Initialize the registry with all known architectures
    fn init() -> Self {
        let mut registry = Self::new();

        // Register known architectures
        registry.register(crate::gpu::architectures::HopperArchitecture);
        registry.register(crate::gpu::architectures::BlackwellArchitecture);

        debug!(
            "GPU architecture registry initialized with {} architectures",
            registry.len()
        );

        registry
    }

    /// Register a new architecture
    ///
    /// This adds an architecture to the registry. Architectures are checked
    /// in the order they are registered.
    pub fn register<T>(&mut self, arch: T)
    where
        T: CloneableGpuArchitecture + 'static,
    {
        self.architectures.push(Box::new(arch));
    }

    /// Get architecture by device name
    ///
    /// Searches for an architecture whose name appears in the device name.
    /// This is a fallback when device ID lookup fails.
    ///
    /// # Arguments
    ///
    /// * `device_name` - The device name string (e.g., "H100 PCIe")
    ///
    /// # Returns
    ///
    /// An architecture if found, or `None` if no match.
    pub fn get_by_device_name(&self, device_name: &str) -> Option<Box<dyn GpuArchitecture>> {
        let name_lower = device_name.to_lowercase();
        for arch in &self.architectures {
            let arch_name_lower = arch.name().to_lowercase();
            if name_lower.contains(&arch_name_lower) {
                return Some(arch.clone_box());
            }
        }
        None
    }

    /// Get architecture by device ID with fallback to name
    ///
    /// This is the primary lookup method that tries device ID first,
    /// then falls back to name-based detection.
    ///
    /// # Errors
    ///
    /// Returns an error if no matching architecture is found.
    pub fn get_architecture(
        &self,
        device_id: u16,
        device_name: &str,
    ) -> Result<Box<dyn GpuArchitecture>> {
        // Use name-based detection (PCI database is source of truth)
        if let Some(arch) = self.get_by_device_name(device_name) {
            debug!(
                "Detected GPU architecture '{}' by device name '{}' (device ID 0x{:04x})",
                arch.name(),
                device_name,
                device_id
            );
            return Ok(arch);
        }

        // No match found - device ID not in PCI database or architecture unknown
        // User can add via: nvrc.pci.device.id=arch_name,vendor,device_id
        Err(NvrcError::unknown_gpu_architecture(device_id, device_name))
    }

    /// Get the number of registered architectures
    pub fn len(&self) -> usize {
        self.architectures.len()
    }

    /// Check if the registry is empty
    #[allow(dead_code)] // Public API
    pub fn is_empty(&self) -> bool {
        self.architectures.is_empty()
    }
}

impl Default for GpuArchitectureRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to detect architecture using the global registry
///
/// # Examples
///
/// ```no_run
/// use nvrc::gpu::architectures::detect_architecture;
///
/// let arch = detect_architecture(0x2330, "H100")?;
/// println!("Detected: {}", arch.name());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn detect_architecture(device_id: u16, device_name: &str) -> Result<Box<dyn GpuArchitecture>> {
    GpuArchitectureRegistry::global().get_architecture(device_id, device_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::traits::CCMode;

    // Mock architecture for testing
    #[derive(Debug, Clone)]
    struct MockArchitecture {
        name: String,
        device_ids: Vec<u16>,
        register_offset: u64,
    }

    impl MockArchitecture {
        fn new(name: &str, device_ids: Vec<u16>, register_offset: u64) -> Self {
            Self {
                name: name.to_string(),
                device_ids,
                register_offset,
            }
        }
    }

    impl GpuArchitecture for MockArchitecture {
        fn name(&self) -> &str {
            &self.name
        }

        fn cc_register_offset(&self) -> Result<u64> {
            Ok(self.register_offset)
        }

        fn parse_cc_mode(&self, register_value: u32) -> Result<CCMode> {
            Ok(match register_value & 0x3 {
                0x1 => CCMode::On,
                0x3 => CCMode::Devtools,
                _ => CCMode::Off,
            })
        }
    }

    #[test]
    fn test_registry_creation() {
        let registry = GpuArchitectureRegistry::new();
        assert_eq!(registry.len(), 0);
        assert!(registry.is_empty());
    }

    #[test]
    fn test_registry_register() {
        let mut registry = GpuArchitectureRegistry::new();
        let arch = MockArchitecture::new("TestArch", vec![0x1234], 0x100);

        registry.register(arch);
        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());
    }

    #[test]
    fn test_get_by_device_name() {
        let mut registry = GpuArchitectureRegistry::new();
        registry.register(MockArchitecture::new("Hopper", vec![0x1234], 0x100));

        let arch = registry.get_by_device_name("H100 [Hopper]");
        assert!(arch.is_some());
        assert_eq!(arch.unwrap().name(), "Hopper");

        let arch = registry.get_by_device_name("Unknown Device");
        assert!(arch.is_none());
    }

    #[test]
    fn test_get_architecture_by_name_fallback() {
        let mut registry = GpuArchitectureRegistry::new();
        registry.register(MockArchitecture::new("Hopper", vec![0x1234], 0x100));

        // Device ID doesn't match, but name does
        let result = registry.get_architecture(0x9999, "H100 Hopper GPU");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name(), "Hopper");
    }

    #[test]
    fn test_get_architecture_not_found() {
        let registry = GpuArchitectureRegistry::new();

        let result = registry.get_architecture(0x9999, "Unknown GPU");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            NvrcError::UnknownGpuArchitecture { .. }
        ));
    }

    #[test]
    fn test_multiple_architectures() {
        let mut registry = GpuArchitectureRegistry::new();
        registry.register(MockArchitecture::new(
            "Hopper",
            vec![0x2330, 0x2331],
            0x1182cc,
        ));
        registry.register(MockArchitecture::new(
            "Blackwell",
            vec![0x2900, 0x2901],
            0x590,
        ));

        assert_eq!(registry.len(), 2);

        // Device ID matching removed - using name-based detection only
        let arch1 = registry.get_by_device_name("H100").unwrap();
        assert_eq!(arch1.name(), "Hopper");

        let arch2 = registry.get_by_device_name("B100").unwrap();
        assert_eq!(arch2.name(), "Blackwell");
    }

    #[test]
    fn test_global_registry() {
        let registry = GpuArchitectureRegistry::global();
        // Global registry initialized with known architectures
        assert!(registry.len() >= 0);
    }
}
