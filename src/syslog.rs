// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Minimal syslog sink for ephemeral init environments.
//!
//! Programs expect /dev/log to exist for logging. We provide this socket and
//! write all messages to /run/syslog.log. This file serves as the source of
//! truth for daemon synchronization - wait_for_marker() reads from it to detect
//! when daemons are ready. File-based approach works regardless of log level.

use log::debug;
use nix::poll::{PollFd, PollFlags, PollTimeout};
use once_cell::sync::OnceCell;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Global syslog socket—lazily initialized on first poll().
/// OnceCell ensures thread-safe one-time init. Ephemeral init runs once,
/// no need for reset capability.
static SYSLOG: OnceCell<UnixDatagram> = OnceCell::new();

/// Global log file for syslog messages - ALWAYS written for synchronization.
/// Mutex protects concurrent writes from multiple poll() calls.
static LOGFILE: OnceCell<Mutex<File>> = OnceCell::new();

const DEV_LOG: &str = "/dev/log";
const SYSLOG_FILE: &str = "/run/syslog.log";

/// Public path to the syslog file for cross-module access.
pub const SYSLOG_FILE_PATH: &str = SYSLOG_FILE;

/// Create and bind a Unix datagram socket at the given path.
fn bind(path: &Path) -> std::io::Result<UnixDatagram> {
    let sock = UnixDatagram::bind(path)?;
    // poll and recv are not atomic, a blocking recv could hang.
    sock.set_nonblocking(true)?;
    Ok(sock)
}

/// Wait up to `timeout` for the socket to have a message queued.
fn readable(sock: &UnixDatagram, timeout: PollTimeout) -> std::io::Result<bool> {
    let mut fds = [PollFd::new(sock.as_fd(), PollFlags::POLLIN)];
    nix::poll::poll(&mut fds, timeout).map_err(std::io::Error::from)?;
    ready(fds[0].revents())
}

/// Interpret poll events. A broken socket is an error, not "no message", so
/// callers back off instead of spinning on a socket that never blocks.
fn ready(revents: Option<PollFlags>) -> std::io::Result<bool> {
    let revents = revents.unwrap_or_else(PollFlags::empty);
    if revents.contains(PollFlags::POLLIN) {
        return Ok(true);
    }
    if revents.intersects(PollFlags::POLLERR | PollFlags::POLLHUP | PollFlags::POLLNVAL) {
        return Err(std::io::Error::other(format!(
            "syslog socket poll: {revents:?}"
        )));
    }
    Ok(false)
}

/// Read one message, waiting up to `timeout` for it to arrive.
fn poll_socket(sock: &UnixDatagram, timeout: PollTimeout) -> std::io::Result<Option<String>> {
    if !readable(sock, timeout)? {
        return Ok(None);
    }

    // Read the message—4KB buffer matches typical syslog max message size
    let mut buf = [0u8; 4096];
    let len = match sock.recv_from(&mut buf) {
        Ok((len, _)) => len,
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
        Err(e) => return Err(e),
    };
    let msg = String::from_utf8_lossy(&buf[..len]);
    Ok(Some(strip_priority(msg.trim_end()).to_string()))
}

/// Poll the global /dev/log socket, logging any message via trace!().
/// Lazily initializes /dev/log on first call.
/// Drains one message per call. The kata-agent handoff loop calls it every
/// 500ms, which rate-limits flooding; wait_for_marker drains as fast as daemons log.
pub fn poll() {
    use crate::macros::ResultExt;
    poll_at(Path::new(DEV_LOG), PollTimeout::ZERO).or_panic("syslog poll");
}

/// Best-effort syslog drain. Silently ignores errors (e.g. socket not bound yet).
/// Used by the kata-agent handoff loop, where I/O errors must not power off the VM.
pub fn try_poll() {
    let _ = poll_at(Path::new(DEV_LOG), PollTimeout::ZERO);
}

/// Like [`try_poll`], but waits up to `timeout` for a message. Sleeps instead
/// if syslog is broken, so callers never busy-loop.
pub fn try_poll_for(timeout: Duration) {
    let deadline = Instant::now() + timeout;
    let wait = PollTimeout::try_from(timeout).unwrap_or(PollTimeout::MAX);
    if poll_at(Path::new(DEV_LOG), wait).is_err() {
        // Only the time the failed poll left over, so the wait never doubles.
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
    }
}

/// Internal: poll a specific socket path (for unit tests).
/// Production code uses poll() which hardcodes /dev/log.
fn poll_at(path: &Path, timeout: PollTimeout) -> std::io::Result<()> {
    if path == Path::new(DEV_LOG) {
        forward_next(SYSLOG.get_or_try_init(|| bind(path))?, timeout)
    } else {
        // For testing: create a one-shot socket (caller manages lifecycle)
        forward_next(&bind(path)?, timeout)
    }
}

/// Move one pending message, if any arrives within `timeout`, into the log file.
fn forward_next(sock: &UnixDatagram, timeout: PollTimeout) -> std::io::Result<()> {
    poll_socket(sock, timeout)?.map_or(Ok(()), |msg| forward_message(&msg))
}

/// Write syslog message to persistent file for daemon synchronization.
/// Daemons like nvidia-persistenced signal readiness via syslog. File-based
/// approach works regardless of log level and survives for post-mortem debugging.
fn forward_message(msg: &str) -> std::io::Result<()> {
    let logfile = LOGFILE.get_or_try_init(|| {
        OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600) // Restrict to owner only
            .open(SYSLOG_FILE)
            .map(Mutex::new)
    })?;

    let mut file = logfile
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    writeln!(file, "{}", msg)?;
    file.flush()?;

    // Also log when debug enabled - may appear in dmesg depending on logger config
    debug!("{}", msg);

    Ok(())
}

/// Strip the syslog priority prefix <N> from a message.
/// Priority levels are noise for us—all messages go to trace!() equally.
/// Example: "<6>hello" → "hello"
fn strip_priority(msg: &str) -> &str {
    msg.strip_prefix('<')
        .and_then(|s| s.find('>').map(|i| &s[i + 1..]))
        .unwrap_or(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use serial_test::serial;
    use std::time::Instant;
    use tempfile::TempDir;

    // === strip_priority tests ===

    #[test]
    fn test_strip_priority_normal() {
        assert_eq!(strip_priority("<6>test message"), "test message");
        assert_eq!(strip_priority("<13>another msg"), "another msg");
        assert_eq!(strip_priority("<191>high pri"), "high pri");
    }

    #[test]
    fn test_strip_priority_no_prefix() {
        assert_eq!(strip_priority("no prefix"), "no prefix");
    }

    #[test]
    fn test_strip_priority_edge_cases() {
        assert_eq!(strip_priority("<>empty"), "empty");
        assert_eq!(strip_priority("<6>"), "");
        assert_eq!(strip_priority(""), "");
        assert_eq!(strip_priority("<"), "<");
        assert_eq!(strip_priority("<6"), "<6"); // No closing >
    }

    // === bind tests ===

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    fn test_bind_success() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");
        let sock = bind(&path);
        assert!(sock.is_ok());
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    fn test_bind_nonexistent_dir() {
        let path = Path::new("/nonexistent/dir/test.sock");
        let err = bind(path).unwrap_err();
        // Should fail with "No such file or directory" (ENOENT)
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    fn test_bind_already_exists() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");
        let _sock1 = bind(&path).unwrap();
        // Binding again to same path should fail with "Address already in use"
        let err = bind(&path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
    }

    // === poll_socket tests ===

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    fn test_poll_socket_no_data() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");
        let sock = bind(&path).unwrap();

        let result = poll_socket(&sock, PollTimeout::ZERO).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    fn test_poll_socket_with_data() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");
        let server = bind(&path).unwrap();

        let client = UnixDatagram::unbound().unwrap();
        client.send_to(b"<6>hello world", &path).unwrap();

        let result = poll_socket(&server, PollTimeout::ZERO).unwrap();
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    fn test_poll_socket_strips_priority() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");
        let server = bind(&path).unwrap();

        let client = UnixDatagram::unbound().unwrap();
        client.send_to(b"<3>error message", &path).unwrap();

        let result = poll_socket(&server, PollTimeout::ZERO).unwrap();
        assert_eq!(result, Some("error message".to_string()));
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    fn test_poll_socket_multiple_messages() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");
        let server = bind(&path).unwrap();

        let client = UnixDatagram::unbound().unwrap();
        client.send_to(b"<6>first", &path).unwrap();
        client.send_to(b"<6>second", &path).unwrap();

        // poll_socket drains one at a time
        let result1 = poll_socket(&server, PollTimeout::ZERO).unwrap();
        assert_eq!(result1, Some("first".to_string()));

        let result2 = poll_socket(&server, PollTimeout::ZERO).unwrap();
        assert_eq!(result2, Some("second".to_string()));

        // No more messages
        let result3 = poll_socket(&server, PollTimeout::ZERO).unwrap();
        assert_eq!(result3, None);
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    fn test_poll_socket_trims_trailing_whitespace() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");
        let server = bind(&path).unwrap();

        let client = UnixDatagram::unbound().unwrap();
        client.send_to(b"<6>message with newline\n", &path).unwrap();

        let result = poll_socket(&server, PollTimeout::ZERO).unwrap();
        assert_eq!(result, Some("message with newline".to_string()));
    }

    #[rstest]
    #[case::nothing(None, false)]
    #[case::idle(Some(PollFlags::empty()), false)]
    #[case::message(Some(PollFlags::POLLIN), true)]
    #[case::message_then_hangup(Some(PollFlags::POLLIN | PollFlags::POLLHUP), true)]
    fn test_ready_ok(#[case] revents: Option<PollFlags>, #[case] expected: bool) {
        assert_eq!(ready(revents).unwrap(), expected);
    }

    #[rstest]
    #[case::error(PollFlags::POLLERR)]
    #[case::hangup(PollFlags::POLLHUP)]
    #[case::invalid(PollFlags::POLLNVAL)]
    fn test_ready_broken_socket_is_an_error(#[case] revents: PollFlags) {
        assert!(ready(Some(revents)).is_err());
    }

    // === blocking wait ===

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    fn test_poll_socket_times_out_when_idle() {
        let tmp = TempDir::new().unwrap();
        let server = bind(&tmp.path().join("test.sock")).unwrap();

        let start = Instant::now();
        let result = poll_socket(&server, PollTimeout::from(50u16)).unwrap();

        assert_eq!(result, None);
        assert!(start.elapsed() >= Duration::from_millis(50));
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    fn test_poll_socket_wakes_when_message_arrives() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");
        let server = bind(&path).unwrap();

        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            UnixDatagram::unbound()
                .unwrap()
                .send_to(b"<6>late arrival", &path)
                .unwrap();
        });

        let start = Instant::now();
        let result = poll_socket(&server, PollTimeout::from(10_000u16)).unwrap();
        sender.join().unwrap();

        assert_eq!(result, Some("late arrival".to_string()));
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    // === poll_at tests ===

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    fn test_poll_at_custom_path() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("custom.sock");

        // poll_at with non-/dev/log path binds a one-shot socket
        let result = poll_at(&path, PollTimeout::ZERO);
        assert!(result.is_ok());
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    fn test_poll_dev_log() {
        use std::panic;
        // poll() tries to bind /dev/log - may panic if already bound or no permission
        // Just exercise the code path, don't assert success
        let _ = panic::catch_unwind(poll);
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    fn test_try_poll_swallows_errors() {
        // /dev/log may be foreign (bind fails) or already ours; both must be
        // non-fatal for the best-effort drain.
        try_poll();
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "fork/socket syscalls are foreign functions miri cannot emulate"
    )]
    fn test_try_poll_for_waits_out_the_timeout_when_idle() {
        let start = Instant::now();
        try_poll_for(Duration::from_millis(100));
        let elapsed = start.elapsed();

        assert!(elapsed >= Duration::from_millis(100));
        assert!(elapsed < Duration::from_secs(5));
    }

    // Serialized with the kmsg test that removes and recreates the same file.
    #[test]
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    #[cfg_attr(not(miri), serial)]
    fn test_forward_message_appends_to_syslog_file() {
        crate::test_utils::require_root();
        // Nonce keeps the assertion honest against /run/syslog.log contents
        // accumulated by earlier runs on the same machine.
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let marker = format!("<test> forward_message smoke {nonce}");
        forward_message(&marker).unwrap();
        let content = std::fs::read_to_string(SYSLOG_FILE).unwrap();
        assert!(content.contains(&marker));
    }
}
