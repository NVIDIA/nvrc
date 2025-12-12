// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Core module containing fundamental traits and types for NVRC.
//!
//! This module defines the trait system that allows flexible implementation
//! of confidential computing detection across different platforms and GPU
//! architectures.

pub mod error;
pub mod traits;
pub mod types;

// Re-export commonly used items
pub use error::{NvrcError, Result};
