// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

// Old CPU detection code - replaced by platform module
// This file is kept for the Cpu enum which may still be referenced
// but the actual detection is now in src/platform/detector.rs

// Note: The Cpu enum here is being replaced by core::traits::CpuVendor
// Kept temporarily for backward compatibility
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Cpu {
    Amd,
    Intel,
    Arm,
}

// Old confidential module removed - all CC detection now in platform module
// (src/platform/x86_64/amd.rs, src/platform/x86_64/intel.rs, src/platform/aarch64/arm.rs)

// Old tests removed - see platform::detector::tests for CPU vendor detection tests
