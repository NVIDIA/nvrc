// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Minimal syslog sink for ephemeral init environments.
//!
//! Programs expect /dev/log to exist for logging. As a minimal init we provide
//! this socket and forward messages to the kernel log via trace!(). Severity
//! levels are stripped since all messages go to the same destination anyway.

use log::trace;
use nix::poll::{PollFd, PollFlags, PollTimeout};
use once_cell::sync::OnceCell;
use std::os::fd::AsFd;
use std::os::unix::net::UnixDatagram;
use std::path::Path;

/// Global syslog socket—lazily initialized on first poll().
/// OnceCell ensures thread-safe one-time init. Ephemeral init runs once,
/// no need for reset capability.
static SYSLOG: OnceCell<UnixDatagram> = OnceCell::new();

const DEV_LOG: &str = "/dev/log";

/// Create and bind a Unix datagram socket at the given path.
fn bind(path: &Path) -> std::io::Result<UnixDatagram> {
    UnixDatagram::bind(path)
}

/// Check socket for pending messages (non-blocking).
/// Returns None if no data available, Some(msg) if a message was read.
fn poll_socket(sock: &UnixDatagram) -> std::io::Result<Option<String>> {
    let mut fds = [PollFd::new(sock.as_fd(), PollFlags::POLLIN)];
    // Non-blocking poll—init loop calls this frequently, can't afford to block
    let count = nix::poll::poll(&mut fds, PollTimeout::ZERO)
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    if count == 0 {
        return Ok(None); // No events, no data waiting
    }

    let Some(revents) = fds[0].revents() else {
        return Ok(None); // Shouldn't happen, but handle gracefully
    };

    if !revents.contains(PollFlags::POLLIN) {
        return Ok(None); // Event wasn't POLLIN (e.g., error flag)
    }

    // Read the message—4KB buffer matches typical syslog max message size
    let mut buf = [0u8; 4096];
    let (len, _) = sock.recv_from(&mut buf)?;
    let msg = String::from_utf8_lossy(&buf[..len]);
    Ok(Some(strip_priority(msg.trim_end()).to_string()))
}

/// Poll the global /dev/log socket, logging any message via trace!().
/// Lazily initializes /dev/log on first call.
/// Drains one message per call—rate-limited to prevent DoS by syslog flooding.
/// Caller loops at ~2 msg/sec (500ms sleep between calls).
pub fn poll() {
    use crate::macros::ResultExt;
    poll_at(Path::new(DEV_LOG)).or_panic("syslog poll");
}

/// Internal: poll a specific socket path (for unit tests).
/// Production code uses poll() which hardcodes /dev/log.
fn poll_at(path: &Path) -> std::io::Result<()> {
    let sock: &UnixDatagram = if path == Path::new(DEV_LOG) {
        SYSLOG.get_or_try_init(|| bind(path))?
    } else {
        // For testing: create a one-shot socket (caller manages lifecycle)
        return poll_once(path);
    };

    if let Some(msg) = poll_socket(sock)? {
        trace!("{}", msg);
    }

    Ok(())
}

/// One-shot poll for testing: bind, poll once, return.
/// Socket is dropped after call—suitable for tests with temp paths.
fn poll_once(path: &Path) -> std::io::Result<()> {
    let sock = bind(path)?;
    if let Some(msg) = poll_socket(&sock)? {
        trace!("{}", msg);
    }
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
    fn test_bind_success() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");
        let sock = bind(&path);
        assert!(sock.is_ok());
    }

    #[test]
    fn test_bind_nonexistent_dir() {
        let path = Path::new("/nonexistent/dir/test.sock");
        let err = bind(path).unwrap_err();
        // Should fail with "No such file or directory" (ENOENT)
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
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
    fn test_poll_socket_no_data() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");
        let sock = bind(&path).unwrap();

        let result = poll_socket(&sock).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_poll_socket_with_data() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");
        let server = bind(&path).unwrap();

        let client = UnixDatagram::unbound().unwrap();
        client.send_to(b"<6>hello world", &path).unwrap();

        let result = poll_socket(&server).unwrap();
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    fn test_poll_socket_strips_priority() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");
        let server = bind(&path).unwrap();

        let client = UnixDatagram::unbound().unwrap();
        client.send_to(b"<3>error message", &path).unwrap();

        let result = poll_socket(&server).unwrap();
        assert_eq!(result, Some("error message".to_string()));
    }

    #[test]
    fn test_poll_socket_multiple_messages() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");
        let server = bind(&path).unwrap();

        let client = UnixDatagram::unbound().unwrap();
        client.send_to(b"<6>first", &path).unwrap();
        client.send_to(b"<6>second", &path).unwrap();

        // poll_socket drains one at a time
        let result1 = poll_socket(&server).unwrap();
        assert_eq!(result1, Some("first".to_string()));

        let result2 = poll_socket(&server).unwrap();
        assert_eq!(result2, Some("second".to_string()));

        // No more messages
        let result3 = poll_socket(&server).unwrap();
        assert_eq!(result3, None);
    }

    #[test]
    fn test_poll_socket_trims_trailing_whitespace() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");
        let server = bind(&path).unwrap();

        let client = UnixDatagram::unbound().unwrap();
        client.send_to(b"<6>message with newline\n", &path).unwrap();

        let result = poll_socket(&server).unwrap();
        assert_eq!(result, Some("message with newline".to_string()));
    }

    // === poll_at / poll_once tests ===

    #[test]
    fn test_poll_once_no_data() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");

        // poll_once will bind and poll - should succeed with no messages
        let result = poll_once(&path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_poll_once_with_data() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.sock");

        // Create server socket first
        let server = bind(&path).unwrap();

        // Send data
        let client = UnixDatagram::unbound().unwrap();
        client.send_to(b"<6>poll_once test", &path).unwrap();

        // poll_socket on the server
        let result = poll_socket(&server).unwrap();
        assert_eq!(result, Some("poll_once test".to_string()));
    }

    #[test]
    fn test_poll_at_custom_path() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("custom.sock");

        // poll_at with non-/dev/log path uses poll_once internally
        let result = poll_at(&path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_poll_dev_log() {
        use std::panic;
        // poll() tries to bind /dev/log - may panic if already bound or no permission
        // Just exercise the code path, don't assert success
        let _ = panic::catch_unwind(poll);
    }
}
