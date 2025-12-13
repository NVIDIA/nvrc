// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! NVIDIA Hopper GPU architecture support.
//!
//! This module implements CC detection for Hopper-based GPUs (H100, H800 series).
//! Hopper is NVIDIA's 9th generation data center GPU architecture.
//!
//! # CC Register Layout
//!
//! - **Register Offset**: `0x001182cc`
//! - **CC State Bits**: `[1:0]`
//!   - `0x1`: CC On
//!   - `0x3`: CC Devtools
//!   - Other: CC Off

use crate::core::error::Result;
use crate::core::traits::{CCMode, GpuArchitecture};

/// Hopper GPU architecture
///
/// Supports H100, H800, and GH100 family GPUs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HopperArchitecture;

impl HopperArchitecture {
    /// CC register offset for Hopper
    #[allow(dead_code)]
    pub const CC_REGISTER: u64 = 0x001182cc;

    /// CC state mask (bits [1:0])
    #[allow(dead_code)]
    const CC_STATE_MASK: u32 = 0x3;
}

impl GpuArchitecture for HopperArchitecture {
    fn name(&self) -> &str {
        "Hopper"
    }

    fn cc_register_offset(&self) -> Result<u64> {
        Ok(Self::CC_REGISTER)
    }

    fn parse_cc_mode(&self, register_value: u32) -> Result<CCMode> {
        let cc_state = register_value & Self::CC_STATE_MASK;

        Ok(match cc_state {
            0x1 => CCMode::On,
            0x3 => CCMode::Devtools,
            _ => CCMode::Off,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hopper_name() {
        let arch = HopperArchitecture;
        assert_eq!(arch.name(), "Hopper");
    }

    #[test]
    fn test_hopper_cc_register() {
        let arch = HopperArchitecture;
        assert_eq!(arch.cc_register_offset().unwrap(), 0x001182cc);
    }

    #[test]
    fn test_hopper_parse_cc_mode() {
        let arch = HopperArchitecture;

        // CC Off (state = 0x0)
        assert_eq!(arch.parse_cc_mode(0x0).unwrap(), CCMode::Off);

        // CC On (state = 0x1)
        assert_eq!(arch.parse_cc_mode(0x1).unwrap(), CCMode::On);

        // CC Devtools (state = 0x3)
        assert_eq!(arch.parse_cc_mode(0x3).unwrap(), CCMode::Devtools);

        // Other bits set but CC Off (state = 0x2)
        assert_eq!(arch.parse_cc_mode(0x2).unwrap(), CCMode::Off);

        // CC On with other bits set
        assert_eq!(arch.parse_cc_mode(0xFFFF_FFF1).unwrap(), CCMode::On);
    }

    #[test]
    fn test_hopper_name_detection() {
        let arch = HopperArchitecture;

        // matches_device_id() is not used - we use name-based detection
        // The registry calls get_by_device_name() which checks if arch.name()
        // appears in the device name from PCI database

        // Test that arch.name() returns correct value
        assert_eq!(arch.name(), "Hopper");

        // For device IDs not in PCI database, use kernel parameter:
        // nvrc.pci.device.id=hopper,10de,XXXX
    }

    #[test]
    fn test_hopper_const_values() {
        assert_eq!(HopperArchitecture::CC_REGISTER, 0x001182cc);
        assert_eq!(HopperArchitecture::CC_STATE_MASK, 0x3);
    }
}
