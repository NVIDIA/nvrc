// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! NVIDIA Blackwell GPU architecture support.
//!
//! This module implements CC detection for Blackwell-based GPUs (B100, B200 series).
//! Blackwell is NVIDIA's 10th generation data center GPU architecture.
//!
//! # CC Register Layout
//!
//! - **Register Offset**: `0x590`
//! - **CC State Bits**: `[1:0]`
//!   - `0x1`: CC On
//!   - `0x3`: CC Devtools
//!   - Other: CC Off

use crate::core::error::Result;
use crate::core::traits::{CCMode, GpuArchitecture};

/// Blackwell GPU architecture
///
/// Supports B100, B200, and GB100 family GPUs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlackwellArchitecture;

impl BlackwellArchitecture {
    /// CC register offset for Blackwell
    #[allow(dead_code)] // Used in tests
    pub const CC_REGISTER: u64 = 0x590;

    /// CC state mask (bits [1:0])
    #[allow(dead_code)] // Used in tests
    const CC_STATE_MASK: u32 = 0x3;

    /// Known Blackwell device IDs
    const DEVICE_IDS: &'static [u16] = &[
        // B100 family
        0x2900, // B100
        // B200 family
        0x2901, // B200
        0x2902, // B200A
        // GB100 base
        0x2910, // GB100
    ];
}

impl GpuArchitecture for BlackwellArchitecture {
    fn name(&self) -> &str {
        "Blackwell"
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
    fn test_blackwell_name() {
        let arch = BlackwellArchitecture;
        assert_eq!(arch.name(), "Blackwell");
    }

    #[test]
    fn test_blackwell_cc_register() {
        let arch = BlackwellArchitecture;
        assert_eq!(arch.cc_register_offset().unwrap(), 0x590);
    }

    #[test]
    fn test_blackwell_parse_cc_mode() {
        let arch = BlackwellArchitecture;

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
    fn test_blackwell_device_ids() {
        let arch = BlackwellArchitecture;

        // B100 family
        assert!(arch.matches_device_id(0x2900)); // B100

        // B200 family
        assert!(arch.matches_device_id(0x2901)); // B200
        assert!(arch.matches_device_id(0x2902)); // B200A

        // GB100 base
        assert!(arch.matches_device_id(0x2910)); // GB100

        // Not Blackwell
        assert!(!arch.matches_device_id(0x1234));
        assert!(!arch.matches_device_id(0x2330)); // Hopper
    }

    #[test]
    fn test_blackwell_const_values() {
        assert_eq!(BlackwellArchitecture::CC_REGISTER, 0x590);
        assert_eq!(BlackwellArchitecture::CC_STATE_MASK, 0x3);
    }
}

