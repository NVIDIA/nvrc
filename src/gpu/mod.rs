// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! GPU architecture detection and confidential computing support.
//!
//! This module provides GPU-specific implementations for detecting
//! GPU architectures and querying confidential computing capabilities.
//!
//! # Architecture
//!
//! The GPU module is organized into:
//! - **architectures**: GPU architecture-specific implementations (Hopper, Blackwell, etc.)
//! - **confidential**: CC-enabled GPU operations (feature-gated)
//! - **standard**: No-op implementations for non-CC builds
//!
//! # GPU Architectures
//!
//! Each GPU architecture has different register layouts and CC detection:
//! - **Hopper** (H100, H800): CC register at `0x001182cc`
//! - **Blackwell** (B100, B200): CC register at `0x590`
//!
//! # Usage
//!
//! ```no_run
//! use nvrc::gpu::architectures;
//!
//! let arch = architectures::detect_architecture(0x2330, "H100")?;
//! let cc_register = arch.cc_register_offset()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod architectures;
pub mod traits;

// Feature-gated modules
#[cfg(feature = "confidential")]
pub mod confidential;

#[cfg(not(feature = "confidential"))]
pub mod standard;

