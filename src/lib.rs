// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! NVRC's logic; `main.rs` is only the mode dispatch.

#![allow(non_snake_case)]

pub mod config;
pub mod daemon;
pub mod device;
pub mod execute;
pub mod gpu_extension;
pub mod guest_extension_image;
pub mod hash;
pub mod infiniband;
pub mod init;
pub mod kata_agent;
pub mod kernel_params;
pub mod kmsg;
pub mod lockdown;
pub mod macros;
pub mod mode;
pub mod modprobe;
pub mod mount;
pub mod net;
pub mod nvrc;
pub mod smi;
pub mod syslog;
pub mod toolkit;
pub mod uvm;

#[cfg(test)]
pub mod test_utils;

#[macro_use]
extern crate log;
extern crate kernlog;
