// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Standard (non-CC) GPU provider.
//!
//! This module provides no-op implementations for non-confidential builds.

use crate::core::error::Result;
use crate::core::traits::{CCMode, GpuCCProvider};
use crate::devices::NvidiaDevice;

/// Standard GPU provider (no CC support)
#[derive(Debug)]
pub struct StandardGpuProvider;

impl GpuCCProvider for StandardGpuProvider {
    fn query_device_cc_mode(&self, _bdf: &str, _device_id: u16) -> Result<CCMode> {
        Ok(CCMode::Off)
    }

    fn query_all_gpus_cc_mode(&self, _devices: &[NvidiaDevice]) -> Result<Option<CCMode>> {
        Ok(None)
    }

    fn execute_srs_command(&self, _srs_value: Option<&str>) -> Result<()> {
        debug!("SRS command skipped (standard build)");
        Ok(())
    }
}
