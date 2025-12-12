// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Top-level CC providers combining platform and GPU detection.
//!
//! This module provides unified CCProvider implementations that combine
//! platform-specific and GPU-specific CC detection into a single interface.
//!
//! # Providers
//!
//! - **ConfidentialProvider**: Full CC support (feature-gated)
//! - **StandardProvider**: No-op implementations for standard builds
//!
//! # Example
//!
//! ```no_run
//! use nvrc::providers::ConfidentialProvider;
//! use nvrc::core::traits::CCProvider;
//!
//! let provider = ConfidentialProvider::new()?;
//! let system_cc = provider.query_system_cc_mode(&devices)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod confidential;
mod standard;

#[allow(unused_imports)] // Used based on feature flag
pub use confidential::ConfidentialProvider;
#[allow(unused_imports)] // Used based on feature flag
pub use standard::StandardProvider;

// Feature-based default provider
#[allow(dead_code)] // Will be used in PR #11
#[cfg(feature = "confidential")]
pub type DefaultProvider = ConfidentialProvider;

#[allow(dead_code)] // Will be used in PR #11
#[cfg(not(feature = "confidential"))]
pub type DefaultProvider = StandardProvider;
