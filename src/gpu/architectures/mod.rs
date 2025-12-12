// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! GPU architecture implementations.
//!
//! This module contains implementations for specific GPU architectures.
//! Each architecture knows its CC register layout and device ID mappings.
//!
//! # Architecture Registry
//!
//! The registry pattern allows runtime detection of GPU architectures
//! based on PCI device IDs and device names.
//!
//! # Example
//!
//! ```no_run
//! use nvrc::gpu::architectures::detect_architecture;
//!
//! let arch = detect_architecture(0x2330, "H100")?;
//! let cc_register = arch.cc_register_offset()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod blackwell;
pub mod hopper;
pub mod registry;

// Re-export architectures
pub use blackwell::BlackwellArchitecture;
pub use hopper::HopperArchitecture;

// Re-export main functions
#[allow(unused_imports)]
pub use registry::{detect_architecture, GpuArchitectureRegistry};
