// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! DMI (Desktop Management Interface) information reader.
//!
//! Reads hardware identification information from sysfs DMI interface.
//! This provides vendor, product, and board information useful for
//! identifying the specific hardware platform.

use std::fs;
use std::path::Path;

/// DMI hardware information
#[allow(dead_code)] // Public API
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DmiInfo {
    /// Board/motherboard vendor (e.g., "Dell", "Supermicro", "HP")
    pub board_vendor: Option<String>,
    /// Product/system name (e.g., "PowerEdge R750", "ProLiant DL380")
    pub product_name: Option<String>,
    /// System vendor (may differ from board vendor)
    pub system_vendor: Option<String>,
}

impl DmiInfo {
    /// Read DMI information from sysfs
    ///
    /// Reads from `/sys/class/dmi/id/` which provides hardware identification.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use nvrc::platform::dmi::DmiInfo;
    ///
    /// let dmi = DmiInfo::from_sysfs();
    /// println!("Vendor: {:?}", dmi.board_vendor);
    /// println!("Product: {:?}", dmi.product_name);
    /// ```
    #[allow(dead_code)] // Public API
    pub fn from_sysfs() -> Self {
        Self::from_path("/sys/class/dmi/id")
    }

    /// Read DMI information from a specific path
    ///
    /// Useful for testing with mock sysfs data.
    pub fn from_path(base_path: &str) -> Self {
        let base = Path::new(base_path);

        Self {
            board_vendor: read_dmi_field(&base.join("board_vendor")),
            product_name: read_dmi_field(&base.join("product_name")),
            system_vendor: read_dmi_field(&base.join("sys_vendor")),
        }
    }

    /// Get a formatted hardware description
    ///
    /// Returns a string like "Dell PowerEdge R750" or just the vendor/product
    /// if only one is available.
    ///
    /// # Examples
    ///
    /// ```
    /// use nvrc::platform::dmi::DmiInfo;
    ///
    /// let dmi = DmiInfo {
    ///     board_vendor: Some("Dell".to_string()),
    ///     product_name: Some("PowerEdge R750".to_string()),
    ///     system_vendor: None,
    /// };
    ///
    /// assert_eq!(dmi.hardware_description(), "Dell PowerEdge R750");
    /// ```
    pub fn hardware_description(&self) -> String {
        match (&self.board_vendor, &self.product_name) {
            (Some(vendor), Some(product)) => format!("{} {}", vendor, product),
            (Some(vendor), None) => vendor.clone(),
            (None, Some(product)) => product.clone(),
            (None, None) => "Unknown Hardware".to_string(),
        }
    }

    /// Check if any DMI information is available
    #[allow(dead_code)] // Public API
    pub fn is_available(&self) -> bool {
        self.board_vendor.is_some() || self.product_name.is_some()
    }
}

/// Read a single DMI field from sysfs
///
/// Returns None if the file doesn't exist or can't be read.
/// Trims whitespace and returns None for empty strings.
fn read_dmi_field(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_dmi_hardware_description() {
        let dmi = DmiInfo {
            board_vendor: Some("Dell".to_string()),
            product_name: Some("PowerEdge R750".to_string()),
            system_vendor: None,
        };
        assert_eq!(dmi.hardware_description(), "Dell PowerEdge R750");

        let dmi = DmiInfo {
            board_vendor: Some("Supermicro".to_string()),
            product_name: None,
            system_vendor: None,
        };
        assert_eq!(dmi.hardware_description(), "Supermicro");

        let dmi = DmiInfo {
            board_vendor: None,
            product_name: Some("Custom Server".to_string()),
            system_vendor: None,
        };
        assert_eq!(dmi.hardware_description(), "Custom Server");

        let dmi = DmiInfo::default();
        assert_eq!(dmi.hardware_description(), "Unknown Hardware");
    }

    #[test]
    fn test_dmi_is_available() {
        let dmi = DmiInfo {
            board_vendor: Some("Dell".to_string()),
            product_name: None,
            system_vendor: None,
        };
        assert!(dmi.is_available());

        let dmi = DmiInfo::default();
        assert!(!dmi.is_available());
    }

    #[test]
    fn test_dmi_from_path() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        fs::write(path.join("board_vendor"), "TestVendor\n").unwrap();
        fs::write(path.join("product_name"), "  TestProduct  \n").unwrap();

        let dmi = DmiInfo::from_path(path.to_str().unwrap());
        assert_eq!(dmi.board_vendor, Some("TestVendor".to_string()));
        assert_eq!(dmi.product_name, Some("TestProduct".to_string()));
    }

    #[test]
    fn test_dmi_from_sysfs() {
        // Should not panic even if sysfs is not available
        let dmi = DmiInfo::from_sysfs();
        // In most environments, at least some DMI info should be available
        // But we don't assert anything specific as it depends on the system
        let _ = dmi.hardware_description();
    }

    #[test]
    fn test_read_dmi_field_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // Empty file
        fs::write(path.join("empty"), "").unwrap();
        let result = read_dmi_field(&path.join("empty"));
        assert_eq!(result, None);

        // Whitespace only
        fs::write(path.join("whitespace"), "   \n  ").unwrap();
        let result = read_dmi_field(&path.join("whitespace"));
        assert_eq!(result, None);

        // Nonexistent file
        let result = read_dmi_field(&path.join("nonexistent"));
        assert_eq!(result, None);
    }
}
