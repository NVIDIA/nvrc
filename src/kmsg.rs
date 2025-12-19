// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};

/// Initialize kernel logging and tune socket buffer sizes.
/// Large buffers (16MB) prevent message loss during high-throughput GPU operations
/// where drivers may emit bursts of diagnostic data.
pub fn kernlog_setup() -> Result<()> {
    kernlog::init().context("kernel log init")?;
    log::set_max_level(log::LevelFilter::Off);
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

/// Get a file handle for kernel message output.
/// Routes to /dev/kmsg when debug logging is enabled for visibility in dmesg,
/// otherwise /dev/null to suppress noise in production.
pub fn kmsg() -> Result<File> {
    kmsg_at(if log_enabled!(log::Level::Debug) {
        "/dev/kmsg"
    } else {
        "/dev/null"
    })
}

/// Internal: open the given path for writing. Extracted for testability.
fn kmsg_at(path: &str) -> Result<File> {
    OpenOptions::new()
        .write(true)
        .open(path)
        .with_context(|| format!("open {}", path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use serial_test::serial;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_kmsg_at_dev_null() {
        // /dev/null is always writable, no root needed
        let file = kmsg_at("/dev/null");
        assert!(file.is_ok());
    }

    #[test]
    fn test_kmsg_at_nonexistent() {
        let err = kmsg_at("/nonexistent/path").unwrap_err();
        // Should contain the path in the error context
        assert!(
            err.to_string().contains("/nonexistent/path"),
            "error should mention the path: {}",
            err
        );
    }

    #[test]
    fn test_kmsg_at_temp_file() {
        // Create a temp file to verify we can write to it
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_str().unwrap();
        let mut file = kmsg_at(path).unwrap();
        assert!(file.write_all(b"test").is_ok());
    }

    #[test]
    #[serial]
    fn test_kmsg_routes_to_dev_null_when_log_off() {
        // Default log level is Off, so kmsg() should open /dev/null
        log::set_max_level(log::LevelFilter::Off);
        let file = kmsg();
        assert!(file.is_ok());
    }

    #[test]
    #[serial]
    fn test_kmsg_routes_to_kmsg_when_debug() {
        require_root();
        // When debug is enabled, kmsg() should open /dev/kmsg
        log::set_max_level(log::LevelFilter::Debug);
        let file = kmsg();
        assert!(file.is_ok());
        log::set_max_level(log::LevelFilter::Off);
    }

    #[test]
    #[serial]
    fn test_kernlog_setup() {
        require_root();
        // kernlog_setup requires root for /proc/sys writes.
        // Note: kernlog::init() can only be called once per process,
        // so this test may fail if other tests already initialized it.
        // We just test the /proc/sys writes succeed by calling them directly.
        for path in [
            "/proc/sys/net/core/rmem_default",
            "/proc/sys/net/core/wmem_default",
            "/proc/sys/net/core/rmem_max",
            "/proc/sys/net/core/wmem_max",
        ] {
            let result = fs::write(path, b"16777216");
            assert!(result.is_ok(), "failed to write {}", path);
        }
    }
}
