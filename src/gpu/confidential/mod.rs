// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Confidential computing GPU provider.
//!
//! This module provides GPU CC detection and management for confidential builds.
//! It will be fully implemented in PR #9.

use crate::core::error::Result;
use crate::core::traits::{CCMode, GpuCCProvider};
use crate::devices::NvidiaDevice;

/// Confidential GPU provider (placeholder)
#[allow(dead_code)] // Will be implemented in PR #9
#[derive(Debug)]
pub struct ConfidentialGpuProvider;

impl GpuCCProvider for ConfidentialGpuProvider {
    fn query_device_cc_mode(&self, _bdf: &str, _device_id: u16) -> Result<CCMode> {
        // Placeholder - will be implemented in PR #9
        Ok(CCMode::Off)
    }

    fn query_all_gpus_cc_mode(&self, _devices: &[NvidiaDevice]) -> Result<Option<CCMode>> {
        // Placeholder - will be implemented in PR #9
        Ok(None)
    }

    fn execute_srs_command(&self, _srs_value: Option<&str>) -> Result<()> {
        // Placeholder - will be implemented in PR #9
        debug!("SRS command (placeholder)");
        Ok(())
    }
}
