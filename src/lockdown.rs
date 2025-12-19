// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Production-only lockdown primitives for confidential VM security.
//!
//! WARNING: This module cannot be unit tested on a development machine.
//! - `set_panic_hook` calls `reboot(RB_POWER_OFF)` which will shut down the host
//! - `disable_modules_loading` permanently disables module loading until reboot
//!
//! These functions are designed for ephemeral confidential VMs where such
//! drastic actions are appropriate. Test only in disposable VM environments.

use anyhow::{Context, Result};
use nix::sys::reboot::{reboot, RebootMode};
use nix::unistd::sync;
use std::fs;
use std::panic;

/// Install a panic handler that powers off the VM instead of unwinding.
/// In a confidential VM, a panic could leave the system in an undefined state
/// with potential data exposure. Power-off ensures clean termination—the host
/// hypervisor will see the VM exit and can handle cleanup appropriately.
/// sync() flushes pending writes before power-off to preserve any logs.
pub fn set_panic_hook() {
    panic::set_hook(Box::new(|panic_info| {
        log::error!("panic: {panic_info}");
        sync();
        let _ = reboot(RebootMode::RB_POWER_OFF);
    }));
}

/// Permanently disable kernel module loading for this boot.
/// Once all required GPU drivers are loaded, this prevents any further
/// module insertion—a security hardening measure for confidential VMs
/// that blocks potential kernel-level attacks via malicious modules.
/// This is a one-way operation: once set, it cannot be undone without reboot.
pub fn disable_modules_loading() -> Result<()> {
    fs::write("/proc/sys/kernel/modules_disabled", b"1\n").context("disable module loading")
}
