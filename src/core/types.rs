// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Common types used throughout NVRC.
//!
//! This module contains newtype wrappers and common type definitions
//! that provide type safety and better documentation.

use std::fmt;

/// PCI Device ID newtype for type safety
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(u16);

impl DeviceId {
    /// Create a new DeviceId
    pub const fn new(id: u16) -> Self {
        Self(id)
    }

    /// Get the raw u16 value
    pub const fn as_u16(self) -> u16 {
        self.0
    }

    /// Create from a hex string with optional 0x prefix
    pub fn from_hex_str(s: &str) -> Result<Self, std::num::ParseIntError> {
        let trimmed = s.trim().trim_start_matches("0x");
        u16::from_str_radix(trimmed, 16).map(Self)
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:04x}", self.0)
    }
}

impl fmt::LowerHex for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04x}", self.0)
    }
}

impl From<u16> for DeviceId {
    fn from(id: u16) -> Self {
        Self(id)
    }
}

impl From<DeviceId> for u16 {
    fn from(id: DeviceId) -> Self {
        id.0
    }
}

/// PCI Vendor ID newtype for type safety
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VendorId(u16);

impl VendorId {
    /// NVIDIA vendor ID constant
    pub const NVIDIA: Self = Self(0x10de);

    /// Create a new VendorId
    pub const fn new(id: u16) -> Self {
        Self(id)
    }

    /// Get the raw u16 value
    pub const fn as_u16(self) -> u16 {
        self.0
    }

    /// Create from a hex string with optional 0x prefix
    pub fn from_hex_str(s: &str) -> Result<Self, std::num::ParseIntError> {
        let trimmed = s.trim().trim_start_matches("0x");
        u16::from_str_radix(trimmed, 16).map(Self)
    }

    /// Check if this is the NVIDIA vendor ID
    pub const fn is_nvidia(self) -> bool {
        self.0 == 0x10de
    }
}

impl fmt::Display for VendorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:04x}", self.0)
    }
}

impl From<u16> for VendorId {
    fn from(id: u16) -> Self {
        Self(id)
    }
}

impl From<VendorId> for u16 {
    fn from(id: VendorId) -> Self {
        id.0
    }
}

/// PCI Class ID newtype for type safety
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClassId(u32);

impl ClassId {
    /// VGA controller class
    pub const VGA_CONTROLLER: Self = Self(0x030000);
    /// 3D display controller class
    pub const DISPLAY_3D_CONTROLLER: Self = Self(0x030200);
    /// Bridge device, other
    pub const BRIDGE_OTHER: Self = Self(0x068000);

    /// Create a new ClassId
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the raw u32 value
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    /// Create from a hex string with optional 0x prefix
    pub fn from_hex_str(s: &str) -> Result<Self, std::num::ParseIntError> {
        let trimmed = s.trim().trim_start_matches("0x");
        u32::from_str_radix(trimmed, 16).map(Self)
    }

    /// Check if this is a GPU class (VGA or 3D controller)
    pub const fn is_gpu(self) -> bool {
        matches!(self.0, 0x030000 | 0x030200)
    }

    /// Check if this is a bridge class
    pub const fn is_bridge(self) -> bool {
        self.0 == 0x068000
    }
}

impl fmt::Display for ClassId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:06x}", self.0)
    }
}

impl From<u32> for ClassId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<ClassId> for u32 {
    fn from(id: ClassId) -> Self {
        id.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_id() {
        let id = DeviceId::new(0x1234);
        assert_eq!(id.as_u16(), 0x1234);
        assert_eq!(format!("{}", id), "0x1234");
        assert_eq!(format!("{:x}", id), "1234");
    }

    #[test]
    fn test_device_id_from_hex_str() {
        assert_eq!(
            DeviceId::from_hex_str("0x1234").unwrap(),
            DeviceId::new(0x1234)
        );
        assert_eq!(
            DeviceId::from_hex_str("1234").unwrap(),
            DeviceId::new(0x1234)
        );
        assert_eq!(
            DeviceId::from_hex_str("  0x1234  ").unwrap(),
            DeviceId::new(0x1234)
        );
    }

    #[test]
    fn test_vendor_id_nvidia() {
        assert!(VendorId::NVIDIA.is_nvidia());
        assert_eq!(VendorId::NVIDIA.as_u16(), 0x10de);
        assert!(!VendorId::new(0x1234).is_nvidia());
    }

    #[test]
    fn test_vendor_id_from_hex_str() {
        assert_eq!(VendorId::from_hex_str("0x10de").unwrap(), VendorId::NVIDIA);
        assert_eq!(VendorId::from_hex_str("10de").unwrap(), VendorId::NVIDIA);
    }

    #[test]
    fn test_class_id_gpu() {
        assert!(ClassId::VGA_CONTROLLER.is_gpu());
        assert!(ClassId::DISPLAY_3D_CONTROLLER.is_gpu());
        assert!(!ClassId::BRIDGE_OTHER.is_gpu());
        assert!(!ClassId::new(0x123456).is_gpu());
    }

    #[test]
    fn test_class_id_bridge() {
        assert!(ClassId::BRIDGE_OTHER.is_bridge());
        assert!(!ClassId::VGA_CONTROLLER.is_bridge());
    }

    #[test]
    fn test_class_id_from_hex_str() {
        assert_eq!(
            ClassId::from_hex_str("0x030000").unwrap(),
            ClassId::VGA_CONTROLLER
        );
        assert_eq!(
            ClassId::from_hex_str("030000").unwrap(),
            ClassId::VGA_CONTROLLER
        );
    }

    #[test]
    fn test_type_conversions() {
        let device_id: DeviceId = 0x1234u16.into();
        assert_eq!(device_id, DeviceId::new(0x1234));

        let raw: u16 = device_id.into();
        assert_eq!(raw, 0x1234);
    }
}
