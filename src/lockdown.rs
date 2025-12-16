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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_disable_modules_idempotent() {
        // Regression test: function should be idempotent (hot-plug calls multiple times)
        use std::fs;

        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "0").unwrap();
        tmp.flush().unwrap();

        // Simulate idempotency check logic
        let check_and_write = |path: &std::path::Path| -> Result<()> {
            if let Ok(current) = fs::read_to_string(path) {
                if current.trim() == "1" {
                    return Ok(()); // Already set
                }
            }
            fs::write(path, b"1\n").context("write")
        };

        // First call: should write
        assert!(check_and_write(tmp.path()).is_ok());
        assert_eq!(fs::read_to_string(tmp.path()).unwrap().trim(), "1");

        // Second call: should succeed without writing (idempotent)
        assert!(check_and_write(tmp.path()).is_ok());
        assert_eq!(fs::read_to_string(tmp.path()).unwrap().trim(), "1");
    }
}
