// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use crate::macros::ResultExt;
use std::fs::{self, File, OpenOptions};
use std::sync::Once;

static KERNLOG_INIT: Once = Once::new();

/// Socket buffer size (16MB = 16 * 1024 * 1024 = 16777216 bytes).
/// Large buffers prevent message loss during high-throughput GPU operations
/// where NVIDIA drivers may emit bursts of diagnostic data.
const SOCKET_BUFFER_SIZE: &str = "16777216";

/// Initialize kernel logging and tune socket buffer sizes.
/// Large buffers (16MB) prevent message loss during high-throughput GPU operations
/// where drivers may emit bursts of diagnostic data.
pub fn kernlog_setup() {
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
        fs::write(path, SOCKET_BUFFER_SIZE.as_bytes()).or_panic(format_args!("write {path}"));
    }
}

/// Get a file handle for kernel message output.
/// Routes to /dev/kmsg when debug logging is enabled for visibility in dmesg,
/// otherwise /dev/null to suppress noise in production.
pub fn kmsg() -> File {
    kmsg_at(if log_enabled!(log::Level::Debug) {
        "/dev/kmsg"
    } else {
        "/dev/null"
    })
}

/// Internal: open the given path for writing. Extracted for testability.
fn kmsg_at(path: &str) -> File {
    OpenOptions::new()
        .write(true)
        .open(path)
        .or_panic(format_args!("open {path}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use serial_test::serial;
    use std::io::Write;
    use std::panic;
    use tempfile::NamedTempFile;

    #[test]
    fn test_kmsg_at_dev_null() {
        // /dev/null is always writable, no root needed
        let _file = kmsg_at("/dev/null");
    }

    #[test]
    fn test_kmsg_at_nonexistent() {
        let result = panic::catch_unwind(|| {
            kmsg_at("/nonexistent/path");
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_kmsg_at_temp_file() {
        // Create a temp file to verify we can write to it
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_str().unwrap();
        let mut file = kmsg_at(path);
        assert!(file.write_all(b"test").is_ok());
    }

    #[test]
    #[serial]
    fn test_kmsg_routes_to_dev_null_when_log_off() {
        // Default log level is Off, so kmsg() should open /dev/null
        log::set_max_level(log::LevelFilter::Off);
        let _file = kmsg();
    }

    #[test]
    #[serial]
    fn test_kmsg_routes_to_kmsg_when_debug() {
        require_root();
        // When debug is enabled, kmsg() should open /dev/kmsg
        log::set_max_level(log::LevelFilter::Debug);
        let _file = kmsg();
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
                    let _ = fs::write(path, value.as_bytes());
                }
            }
        }

        let saved: Vec<_> = PATHS
            .iter()
            .filter_map(|&p| fs::read_to_string(p).ok().map(|v| (p, v)))
            .collect();
        let _restore = Restore(saved);

        kernlog_setup();

        for &path in &PATHS {
            let v = fs::read_to_string(path).expect("should read sysctl");
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
