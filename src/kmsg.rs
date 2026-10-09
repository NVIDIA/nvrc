// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use crate::macros::ResultExt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind};
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

/// Open syslog file for reading daemon startup markers.
/// Maps /dev/kmsg to /run/syslog.log because daemon synchronization needs to
/// work without trace logging enabled. File-based sync is simpler and more
/// reliable than trying to coordinate log levels between writer and reader.
pub fn open_kmsg(path: &str) -> BufReader<File> {
    let log_path = if path == "/dev/kmsg" {
        crate::syslog::SYSLOG_FILE_PATH
    } else {
        path
    };

    let open_for_reading = || {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(log_path)
    };

    let file = open_for_reading()
        .or_else(|e| match e.kind() {
            ErrorKind::NotFound => OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(log_path)
                .and_then(|_| open_for_reading()),
            _ => Err(e),
        })
        .or_panic(format_args!("open {log_path}"));

    BufReader::new(file)
}

/// Run `start`, then wait for the `marker` it makes a daemon log. The log is
/// opened first, so the marker cannot be missed.
pub fn wait_for_marker_after(marker: &str, timeout_secs: u32, start: impl FnOnce()) {
    let mut reader = open_kmsg("/dev/kmsg");
    start();
    wait_for_marker(&mut reader, marker, timeout_secs);
}

/// Longest single wait on the syslog socket.
const MAX_WAIT: Duration = Duration::from_millis(500);

/// Block until `marker` appears in `reader` or `timeout_secs` expires.
/// At end of file, waits on /dev/log (which forwards into the file) rather
/// than sleeping, so the marker is seen as soon as it is logged.
pub fn wait_for_marker(reader: &mut BufReader<File>, marker: &str, timeout_secs: u32) {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs as u64);
    let mut line = String::new();

    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or_else(|| panic!("timeout waiting for: {marker}"));
        line.clear();
        match reader.read_line(&mut line) {
            Ok(n) if n > 0 && line.contains(marker) => {
                info!("{marker}");
                return;
            }
            Ok(n) if n > 0 => {}
            // EOF, WouldBlock or error: wait for the next message.
            _ => crate::syslog::try_poll_for(remaining.min(MAX_WAIT)),
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
    use rstest::rstest;
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
    #[cfg_attr(not(miri), serial)]
    fn test_kmsg_routes_to_dev_null_when_log_off() {
        // Default log level is Off, so kmsg() should open /dev/null
        log::set_max_level(log::LevelFilter::Off);
        let _file = kmsg();
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    #[cfg_attr(not(miri), serial)]
    fn test_kmsg_routes_to_kmsg_when_debug() {
        require_root();
        // When debug is enabled, kmsg() should open /dev/kmsg
        log::set_max_level(log::LevelFilter::Debug);
        let _file = kmsg();
        log::set_max_level(log::LevelFilter::Off);
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    #[cfg_attr(not(miri), serial)]
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

    const MARKER: &str = "FM starting NvLink Inband";

    /// Whether wait_for_marker found the marker in `contents` (a timeout
    /// panics), and how long it took.
    fn wait_over(contents: &str, timeout_secs: u32) -> (bool, Duration) {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(contents.as_bytes()).unwrap();
        tmp.flush().unwrap();
        let mut reader = open_kmsg(tmp.path().to_str().unwrap());

        let start = Instant::now();
        let found = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            wait_for_marker(&mut reader, MARKER, timeout_secs)
        }))
        .is_ok();
        (found, start.elapsed())
    }

    #[rstest]
    #[case::amid_noise("some noise\nFM starting NvLink Inband foo\nmore noise\n")]
    #[case::last_line("line 1\nline 2\nFM starting NvLink Inband\n")]
    #[cfg_attr(
        miri,
        ignore = "open_kmsg uses O_NONBLOCK, an open flag miri does not support"
    )]
    fn test_wait_for_marker_finds_marker(#[case] contents: &str) {
        assert!(wait_over(contents, 5).0);
    }

    #[rstest]
    #[case::no_marker("no match here\n")]
    #[case::empty_file("")]
    #[case::marker_split_across_lines("FM starting\nNvLink Inband\n")]
    #[cfg_attr(
        miri,
        ignore = "open_kmsg uses O_NONBLOCK, an open flag miri does not support"
    )]
    fn test_wait_for_marker_times_out_on_the_deadline(#[case] contents: &str) {
        let (found, elapsed) = wait_over(contents, 1);

        assert!(!found);
        assert!(elapsed >= Duration::from_secs(1));
        assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "open_kmsg uses O_NONBLOCK, an open flag miri does not support"
    )]
    fn test_wait_for_marker_nonexistent_file_panics() {
        let result = panic::catch_unwind(|| {
            wait_for_marker(&mut open_kmsg("/nonexistent/path"), "marker", 1);
        });
        assert!(result.is_err());
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    #[cfg_attr(not(miri), serial)]
    fn test_wait_for_marker_on_dev_kmsg() {
        require_root();

        // Clear any previous test data to avoid false positives
        let _ = fs::remove_file(crate::syslog::SYSLOG_FILE_PATH);

        let marker = "NVRC_TEST_MARKER_12345";

        // /dev/kmsg is read from the syslog file; log the marker as syslog.rs would.
        wait_for_marker_after(marker, 5, || {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(crate::syslog::SYSLOG_FILE_PATH)
                .expect("open syslog file");
            writeln!(file, "{}", marker).expect("write marker");
        });
    }
}
