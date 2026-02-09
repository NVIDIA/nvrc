// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Library interface for fuzzing and testing.

#![allow(non_snake_case)]
//! The main binary uses these modules internally.

pub mod config;
pub mod daemon;
pub mod execute;
pub mod kata_agent;
pub mod kernel_params;
pub mod kmsg;
pub mod lockdown;
#[macro_use]
pub mod macros;
pub mod modprobe;
pub mod mount;
pub mod nvrc;
pub mod smi;
pub mod syslog;
pub mod toolkit;

#[cfg(test)]
pub mod test_utils;

#[macro_use]
extern crate log;
extern crate kernlog;
