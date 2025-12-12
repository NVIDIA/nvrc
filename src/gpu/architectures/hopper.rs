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
    #[allow(dead_code)] // Used in tests
    pub const CC_REGISTER: u64 = 0x001182cc;

    /// CC state mask (bits [1:0])
    #[allow(dead_code)] // Used in tests
    const CC_STATE_MASK: u32 = 0x3;

    /// Known Hopper device IDs
    const DEVICE_IDS: &'static [u16] = &[
        // H100 family
        0x2330, // H100 PCIe
        0x2331, // H100 SXM5 80GB
        0x2332, // H100 SXM5 64GB
        0x2336, // H100 NVL
        // H800 family
        0x233A, // H800 PCIe
        0x233B, // H800 SXM5
        // GH100 base
        0x2302, // GH100
        0x2303, // GH100 GL
    ];
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

    fn matches_device_id(&self, device_id: u16) -> bool {
        Self::DEVICE_IDS.contains(&device_id)
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
    fn test_hopper_device_ids() {
        let arch = HopperArchitecture;

        // H100 family
        assert!(arch.matches_device_id(0x2330)); // H100 PCIe
        assert!(arch.matches_device_id(0x2331)); // H100 SXM5 80GB
        assert!(arch.matches_device_id(0x2332)); // H100 SXM5 64GB
        assert!(arch.matches_device_id(0x2336)); // H100 NVL

        // H800 family
        assert!(arch.matches_device_id(0x233A)); // H800 PCIe
        assert!(arch.matches_device_id(0x233B)); // H800 SXM5

        // GH100 base
        assert!(arch.matches_device_id(0x2302)); // GH100
        assert!(arch.matches_device_id(0x2303)); // GH100 GL

        // Not Hopper
        assert!(!arch.matches_device_id(0x1234));
        assert!(!arch.matches_device_id(0x2900)); // Blackwell
    }

    #[test]
    fn test_hopper_const_values() {
        assert_eq!(HopperArchitecture::CC_REGISTER, 0x001182cc);
        assert_eq!(HopperArchitecture::CC_STATE_MASK, 0x3);
    }
}

