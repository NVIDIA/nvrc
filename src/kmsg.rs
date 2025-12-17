// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};

pub fn kernlog_setup() -> Result<()> {
    kernlog::init().context("kernel log init")?;
    log::set_max_level(log::LevelFilter::Off);
    // Write large buffer size to related kernel params
    for path in [
        "/proc/sys/net/core/rmem_default",
        "/proc/sys/net/core/wmem_default",
        "/proc/sys/net/core/rmem_max",
        "/proc/sys/net/core/wmem_max",
    ] {
        fs::write(path, b"16777216").with_context(|| format!("write {}", path))?;
    }
    Ok(())
}

pub fn kmsg() -> Result<File> {
    let path = if log_enabled!(log::Level::Debug) {
        "/dev/kmsg"
    } else {
        "/dev/null"
    };
    OpenOptions::new()
        .write(true)
        .open(path)
        .with_context(|| format!("open {}", path))
}
