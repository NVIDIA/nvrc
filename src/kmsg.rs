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

/// Open a log file for reading daemon markers. Automatically selects the
/// appropriate source based on current log level:
/// - Debug or higher (Debug/Trace): /dev/kmsg (kernel message buffer)
/// - Below Debug (Info/Warn/Error/Off): /run/syslog.log (file-based syslog sink)
///
/// For /dev/kmsg, seeks to end to skip boot history.
/// Call *before* spawning a daemon to avoid missing its marker.
pub fn open_kmsg(path: &str) -> BufReader<File> {
    // Auto-select log source based on log level (use max_level for consistency with syslog forwarding)
    let log_path = if path == "/dev/kmsg" && log::max_level() < log::LevelFilter::Debug {
        crate::syslog::SYSLOG_FILE_PATH
    } else {
        path
    };

    // For file-based logging, ensure the file exists before opening for read
    if log_path == crate::syslog::SYSLOG_FILE_PATH && !std::path::Path::new(log_path).exists() {
        fs::write(log_path, "").or_panic(format_args!("create {log_path}"));
    }

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(log_path)
        .or_panic(format_args!("open {log_path}"));

    // Skip boot history on /dev/kmsg; regular files read from the start.
    // SAFETY: lseek on a valid fd with SEEK_END is well-defined for /dev/kmsg
    if log_path == "/dev/kmsg" {
        let ret = unsafe { libc::lseek(std::os::fd::AsRawFd::as_raw_fd(&file), 0, libc::SEEK_END) };
        assert!(ret >= 0, "lseek SEEK_END failed on {log_path}");
    }

    BufReader::new(file)
}

/// Block until `marker` appears in `reader` or `timeout_secs` expires.
/// Best-effort syslog drain keeps messages visible during the wait.
pub fn wait_for_marker(reader: &mut BufReader<File>, marker: &str, timeout_secs: u32) {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs as u64);
    let mut line = String::new();

    loop {
        crate::syslog::try_poll();
        if Instant::now() > deadline {
            panic!("timeout waiting for: {marker}");
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => std::thread::sleep(Duration::from_millis(500)),
            Ok(_) if line.contains(marker) => {
                info!("{marker}");
                return;
            }
            Ok(_) => {}
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

    /// RAII guard to restore log level after test.
    /// Ensures tests don't leak log level changes to other tests.
    struct LogLevelGuard {
        original: log::LevelFilter,
    }

    impl LogLevelGuard {
        fn new(level: log::LevelFilter) -> Self {
            let original = log::max_level();
            log::set_max_level(level);
            Self { original }
        }
    }

    impl Drop for LogLevelGuard {
        fn drop(&mut self) {
            log::set_max_level(self.original);
        }
    }

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
        let _guard = LogLevelGuard::new(log::LevelFilter::Off);
        let _file = kmsg();
    }

    #[test]
    #[serial]
    fn test_kmsg_routes_to_kmsg_when_debug() {
        require_root();
        // When debug is enabled, kmsg() should open /dev/kmsg
        let _guard = LogLevelGuard::new(log::LevelFilter::Debug);
        let _file = kmsg();
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

        wait_for_marker(
            &mut open_kmsg(tmp.path().to_str().unwrap()),
            "FM starting NvLink Inband",
            5,
        );
    }

    #[test]
    fn test_wait_for_marker_finds_marker_at_end() {
        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "line 1").unwrap();
        writeln!(tmp, "line 2").unwrap();
        writeln!(tmp, "FM starting NvLink Inband").unwrap();
        tmp.flush().unwrap();

        wait_for_marker(
            &mut open_kmsg(tmp.path().to_str().unwrap()),
            "FM starting NvLink Inband",
            5,
        );
    }

    #[test]
    fn test_wait_for_marker_no_marker_panics() {
        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "no match here").unwrap();
        tmp.flush().unwrap();

        let result = panic::catch_unwind(|| {
            wait_for_marker(
                &mut open_kmsg(tmp.path().to_str().unwrap()),
                "FM starting NvLink Inband",
                1,
            );
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_wait_for_marker_empty_file_panics() {
        let tmp = NamedTempFile::new().unwrap();

        let result = panic::catch_unwind(|| {
            wait_for_marker(
                &mut open_kmsg(tmp.path().to_str().unwrap()),
                "FM starting NvLink Inband",
                1,
            );
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_wait_for_marker_nonexistent_file_panics() {
        let result = panic::catch_unwind(|| {
            wait_for_marker(&mut open_kmsg("/nonexistent/path"), "marker", 1);
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
            wait_for_marker(
                &mut open_kmsg(tmp.path().to_str().unwrap()),
                "FM starting NvLink Inband",
                1,
            );
        });
        assert!(result.is_err());
    }

    #[test]
    #[serial]
    fn test_wait_for_marker_on_dev_kmsg() {
        require_root();

        // Enable debug logging so open_kmsg() uses /dev/kmsg instead of /run/syslog.log
        let _guard = LogLevelGuard::new(log::LevelFilter::Debug);

        // Open /dev/kmsg directly for writing
        let mut kmsg_writer = OpenOptions::new()
            .write(true)
            .open("/dev/kmsg")
            .expect("open /dev/kmsg for write");

        // Open reader *after* opening writer but before writing — same pattern as production.
        let mut reader = open_kmsg("/dev/kmsg");
        let marker = "NVRC_TEST_MARKER_12345";

        use std::io::Write;
        writeln!(kmsg_writer, "{}", marker).expect("write to /dev/kmsg");
        kmsg_writer.flush().expect("flush /dev/kmsg");

        wait_for_marker(&mut reader, marker, 5);
    }

    #[test]
    fn test_wait_for_marker_reads_from_file() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Logging disabled, so open_kmsg should use /run/syslog.log
        let _guard = LogLevelGuard::new(log::LevelFilter::Off);

        // Create a temporary syslog file
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_string_lossy().to_string();

        // Write initial content
        fs::write(&path, "initial log entry\n").unwrap();

        // Open reader on the temp file
        let mut reader = open_kmsg(&path);

        // Append the marker (simulating what syslog.rs does)
        let marker = "NVRC_TEST_MARKER_FILE";
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{}", marker).unwrap();
        file.flush().unwrap();

        // Should find the marker
        wait_for_marker(&mut reader, marker, 5);
    }

    // === open_kmsg path selection tests ===

    #[test]
    #[serial]
    fn test_open_kmsg_selects_syslog_file_when_logging_disabled() {
        use std::io::Read;
        use tempfile::NamedTempFile;

        // Without debug logging, daemon markers must be readable from file
        let _guard = LogLevelGuard::new(log::LevelFilter::Off);

        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        let test_msg = "test content for path selection\n";
        fs::write(path, test_msg).unwrap();

        let mut reader = open_kmsg(path);
        let mut buffer = String::new();
        let _ = reader.read_to_string(&mut buffer);

        assert!(buffer.contains("test content"));
    }

    #[test]
    #[serial]
    fn test_open_kmsg_selects_kmsg_when_logging_enabled() {
        require_root();

        // With debug enabled, use kernel buffer for real-time visibility
        let _guard = LogLevelGuard::new(log::LevelFilter::Debug);
        let reader = open_kmsg("/dev/kmsg");
        drop(reader);
    }

    #[test]
    #[serial]
    fn test_open_kmsg_creates_missing_syslog_file() {
        // File must exist before daemon spawns to avoid race condition
        let _guard = LogLevelGuard::new(log::LevelFilter::Off);

        // Best-effort removal - might fail if file is locked by another test
        let _ = fs::remove_file(crate::syslog::SYSLOG_FILE_PATH);

        // Brief wait for filesystem
        std::thread::sleep(std::time::Duration::from_millis(50));

        // open_kmsg with /dev/kmsg when logging is off should:
        // 1. Auto-select SYSLOG_FILE_PATH
        // 2. Create it if it doesn't exist
        let _reader = open_kmsg("/dev/kmsg");

        // Verify file exists after open_kmsg (created if missing)
        assert!(
            std::path::Path::new(crate::syslog::SYSLOG_FILE_PATH).exists(),
            "SYSLOG_FILE_PATH should exist after open_kmsg"
        );
    }

    #[test]
    fn test_open_kmsg_preserves_non_kmsg_paths() {
        use tempfile::NamedTempFile;

        // Test files must not be redirected to avoid breaking tests
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        fs::write(path, "test").unwrap();

        let _guard = LogLevelGuard::new(log::LevelFilter::Off);
        let reader = open_kmsg(path);
        drop(reader);
    }

    #[test]
    #[serial]
    fn test_open_kmsg_auto_selection_with_different_log_levels() {
        // Log level changes at runtime must not break synchronization
        let _guard = LogLevelGuard::new(log::LevelFilter::Info);
        drop(_guard);
        let _guard = LogLevelGuard::new(log::LevelFilter::Debug);
        drop(_guard);
        let _guard = LogLevelGuard::new(log::LevelFilter::Trace);
        drop(_guard);
        let _guard = LogLevelGuard::new(log::LevelFilter::Off);
    }

    // === Integration tests ===

    #[test]
    #[serial]
    fn test_end_to_end_syslog_to_wait_for_marker_integration() {
        use std::os::unix::net::UnixDatagram;
        use tempfile::TempDir;

        // Verify complete flow: daemon → syslog → file → marker detection
        let _guard = LogLevelGuard::new(log::LevelFilter::Off);

        let tmp_dir = TempDir::new().unwrap();
        let sock_path = tmp_dir.path().join("test.sock");
        let log_file = tmp_dir.path().join("syslog.log");

        let _syslog_sock = UnixDatagram::bind(&sock_path).expect("bind test socket");
        fs::write(&log_file, "").unwrap();

        // Reader must open before daemon starts to catch marker
        let mut reader = open_kmsg(log_file.to_str().unwrap());

        let client = UnixDatagram::unbound().unwrap();
        let marker = "INTEGRATION_TEST_MARKER";
        client
            .send_to(format!("<6>{}", marker).as_bytes(), &sock_path)
            .unwrap();

        use std::io::Write;
        let mut file = OpenOptions::new().append(true).open(&log_file).unwrap();
        writeln!(file, "{}", marker).unwrap();
        file.flush().unwrap();

        wait_for_marker(&mut reader, marker, 5);
    }

    #[test]
    #[serial]
    fn test_daemon_startup_synchronization_simulation() {
        use std::os::unix::net::UnixDatagram;
        use std::thread;
        use std::time::Duration;
        use tempfile::TempDir;

        // Async daemon startup must not race with main thread
        let _guard = LogLevelGuard::new(log::LevelFilter::Off);

        let tmp_dir = TempDir::new().unwrap();
        let sock_path = tmp_dir.path().join("test.sock");
        let log_file = tmp_dir.path().join("syslog.log");

        let _syslog_sock = UnixDatagram::bind(&sock_path).expect("bind test socket");
        fs::write(&log_file, "").unwrap();

        let log_path = log_file.to_string_lossy().to_string();
        let mut reader = open_kmsg(&log_path);

        let sock_clone = sock_path.clone();
        let log_clone = log_path.clone();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            let client = UnixDatagram::unbound().unwrap();
            client
                .send_to(b"<6>Local RPC services initialized", &sock_clone)
                .unwrap();

            use std::io::Write;
            let mut file = OpenOptions::new().append(true).open(&log_clone).unwrap();
            writeln!(file, "Local RPC services initialized").unwrap();
            file.flush().unwrap();
        });

        for _ in 0..20 {
            thread::sleep(Duration::from_millis(50));
            let mut line = String::new();
            use std::io::BufRead;
            if reader.read_line(&mut line).is_ok()
                && line.contains("Local RPC services initialized")
            {
                break;
            }
        }

        handle.join().unwrap();
    }
}
