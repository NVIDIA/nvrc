// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{anyhow, Result};
use hardened_std::fs::{self, File, OpenOptions};
use std::sync::Once;

static KERNLOG_INIT: Once = Once::new();

/// Socket buffer size (16MB = 16 * 1024 * 1024 = 16777216 bytes).
/// Large buffers prevent message loss during high-throughput GPU operations
/// where NVIDIA drivers may emit bursts of diagnostic data.
const SOCKET_BUFFER_SIZE: &str = "16777216";

/// Initialize kernel logging and tune socket buffer sizes.
/// Large buffers (16MB) prevent message loss during high-throughput GPU operations
/// where drivers may emit bursts of diagnostic data.
pub fn kernlog_setup() -> Result<()> {
    KERNLOG_INIT.call_once(|| {
        let _ = kernlog::init();
    });
    log::set_max_level(log::LevelFilter::Off);
    for path in [
        "/proc/sys/net/core/rmem_default",
        "/proc/sys/net/core/wmem_default",
        "/proc/sys/net/core/rmem_max",
        "/proc/sys/net/core/wmem_max",
    ] {
        fs::write(path, SOCKET_BUFFER_SIZE.as_bytes())
            .map_err(|e| anyhow!("write {}: {}", path, e))?;
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
        .map_err(|e| anyhow!("open {}: {}", path, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use serial_test::serial;
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
        use std::os::unix::io::AsRawFd;

        // Create a temp file to verify we can actually write to it
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_str().unwrap();

        // Open via kmsg_at
        let file = kmsg_at(path).expect("kmsg_at should succeed for temp file");

        // Verify we can write to the returned File
        // This catches issues where the file handle is invalid or closed
        //
        // Note: We use unsafe libc::write() instead of std::io::Write because:
        // 1. hardened_std::fs::File doesn't implement Write trait (minimalist design)
        // 2. This test specifically validates the raw fd is usable, not high-level APIs
        // 3. Direct syscall testing ensures fd validity at lowest level
        // 4. Catching fd issues that might be hidden by higher-level wrappers
        let test_data = b"test write\n";
        let fd = file.as_raw_fd();
        assert!(fd >= 0, "File descriptor should be valid");

        // SAFETY: Direct fd write is safe here because:
        // 1. fd is valid (came from successful kmsg_at)
        // 2. test_data pointer and length are valid
        // 3. We don't use fd after this (file owns it)
        let write_result = unsafe {
            libc::write(
                fd,
                test_data.as_ptr() as *const libc::c_void,
                test_data.len(),
            )
        };
        assert_eq!(
            write_result,
            test_data.len() as isize,
            "Should write full test data"
        );
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

        const PATHS: [&str; 4] = [
            "/proc/sys/net/core/rmem_default",
            "/proc/sys/net/core/wmem_default",
            "/proc/sys/net/core/rmem_max",
            "/proc/sys/net/core/wmem_max",
        ];

        // RAII guard to restore original values after test
        struct Restore(Vec<(&'static str, String)>);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (path, value) in &self.0 {
                    let _ = std::fs::write(path, value.as_bytes());
                }
            }
        }

        let saved: Vec<_> = PATHS
            .iter()
            .filter_map(|&p| std::fs::read_to_string(p).ok().map(|v| (p, v)))
            .collect();
        let _restore = Restore(saved);

        assert!(kernlog_setup().is_ok());

        for &path in &PATHS {
            let v = std::fs::read_to_string(path).expect("should read sysctl");
            assert_eq!(
                v.trim(),
                SOCKET_BUFFER_SIZE,
                "sysctl {} should be {}",
                path,
                SOCKET_BUFFER_SIZE
            );
        }
    }
}
