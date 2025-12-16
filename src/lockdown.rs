// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{Context, Result};
use nix::sys::reboot::{reboot, RebootMode};
use nix::unistd::sync;
use std::fs;
use std::panic;

pub fn set_panic_hook() {
    panic::set_hook(Box::new(|panic_info| {
        log::error!("panic: {panic_info}");
        sync();
        let _ = reboot(RebootMode::RB_POWER_OFF);
    }));
}

pub fn disable_modules_loading() -> Result<()> {
    const PATH: &str = "/proc/sys/kernel/modules_disabled";

    // Kernel allows write-once only. Read first to make idempotent for hot-plug.
    if let Ok(current) = fs::read_to_string(PATH) {
        if current.trim() == "1" {
            return Ok(());
        }
    }

    fs::write(PATH, b"1\n").context("disable module loading")
}
