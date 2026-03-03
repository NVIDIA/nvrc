// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use crate::macros::ResultExt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::os::unix::fs::OpenOptionsExt;
use std::sync::Once;
use std::time::{Duration, Instant};

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

/// Block until `marker` appears in kmsg or `timeout_secs` expires.
/// Opens the file, seeks to end to skip history, then reads new entries.
/// Drains syslog on each iteration so messages stay visible during the wait.
pub fn wait_for_marker(path: &str, marker: &str, timeout_secs: u32) {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .or_panic(format_args!("open {path}"));
    // SAFETY: lseek on a valid fd with SEEK_END is well-defined for /dev/kmsg
    unsafe { libc::lseek(std::os::fd::AsRawFd::as_raw_fd(&file), 0, libc::SEEK_END) };

    let deadline = Instant::now() + Duration::from_secs(timeout_secs as u64);
    let mut reader = BufReader::new(file);
    let mut line = String::new();

    loop {
        crate::syslog::poll();
        if Instant::now() > deadline {
            panic!("timeout waiting for: {marker}");
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => std::thread::sleep(Duration::from_millis(500)),
            Ok(_) => {
                if line.contains(marker) {
                    info!("{marker}");
                    return;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(500));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(500)),
        }
    }
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

    // === wait_for_marker tests ===

    #[test]
    fn test_wait_for_marker_finds_marker() {
        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "some noise").unwrap();
        writeln!(tmp, "FM starting NvLink Inband foo").unwrap();
        writeln!(tmp, "more noise").unwrap();
        tmp.flush().unwrap();

        wait_for_marker(tmp.path().to_str().unwrap(), "FM starting NvLink Inband", 5);
    }

    #[test]
    fn test_wait_for_marker_finds_marker_at_end() {
        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "line 1").unwrap();
        writeln!(tmp, "line 2").unwrap();
        writeln!(tmp, "FM starting NvLink Inband").unwrap();
        tmp.flush().unwrap();

        wait_for_marker(tmp.path().to_str().unwrap(), "FM starting NvLink Inband", 5);
    }

    #[test]
    fn test_wait_for_marker_no_marker_panics() {
        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "no match here").unwrap();
        tmp.flush().unwrap();

        let result = panic::catch_unwind(|| {
            wait_for_marker(tmp.path().to_str().unwrap(), "FM starting NvLink Inband", 1);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_wait_for_marker_empty_file_panics() {
        let tmp = NamedTempFile::new().unwrap();

        let result = panic::catch_unwind(|| {
            wait_for_marker(tmp.path().to_str().unwrap(), "FM starting NvLink Inband", 1);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_wait_for_marker_nonexistent_file_panics() {
        let result = panic::catch_unwind(|| {
            wait_for_marker("/nonexistent/path", "marker", 1);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_wait_for_marker_partial_match_not_enough() {
        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "FM starting").unwrap();
        writeln!(tmp, "NvLink Inband").unwrap();
        tmp.flush().unwrap();

        // Marker spans two lines — should not match
        let result = panic::catch_unwind(|| {
            wait_for_marker(tmp.path().to_str().unwrap(), "FM starting NvLink Inband", 1);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_wait_for_marker_on_dev_kmsg() {
        require_root();

        let marker = "NVRC_TEST_MARKER_12345";
        fs::write("/dev/kmsg", format!("{marker}\n")).expect("write /dev/kmsg");
        wait_for_marker("/dev/kmsg", marker, 5);
    }
}
