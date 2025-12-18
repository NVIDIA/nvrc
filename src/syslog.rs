// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Minimal syslog sink for ephemeral init environments.
//!
//! Programs expect /dev/log to exist for logging. As a minimal init we provide
//! this socket and forward messages to the kernel log. We don't need severity
//! levels since all output goes to trace! anyway.

use log::trace;
use nix::poll::{PollFd, PollFlags, PollTimeout};
use once_cell::sync::OnceCell;
use std::os::fd::AsFd;
use std::os::unix::net::UnixDatagram;
use std::path::Path;

// Ephemeral init only runs once, no need for reset capability
static SYSLOG: OnceCell<UnixDatagram> = OnceCell::new();

/// Exposed for testing with tempdir paths instead of /dev/log
pub fn bind(path: &Path) -> std::io::Result<UnixDatagram> {
    UnixDatagram::bind(path)
}

/// Separated from poll() to enable testing without the global static
pub fn poll_socket(sock: &UnixDatagram) -> std::io::Result<Option<String>> {
    let mut fds = [PollFd::new(sock.as_fd(), PollFlags::POLLIN)];
    // Non-blocking - init loop calls this frequently, can't block
    let count = nix::poll::poll(&mut fds, PollTimeout::ZERO)
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    if count == 0 {
        return Ok(None);
    }

    let Some(revents) = fds[0].revents() else {
        return Ok(None);
    };

    if !revents.contains(PollFlags::POLLIN) {
        return Ok(None);
    }

    let mut buf = [0u8; 4096];
    let (len, _) = sock.recv_from(&mut buf)?;
    let msg = String::from_utf8_lossy(&buf[..len]);
    Ok(Some(strip_priority(msg.trim_end()).to_string()))
}

/// Drain one message per call - intentionally limited to prevent a rogue
/// process from DoS'ing init by flooding syslog. Caller loops at 2 msg/sec.
pub fn poll() -> std::io::Result<()> {
    let sock = SYSLOG.get_or_try_init(|| bind(Path::new("/dev/log")))?;

    if let Some(msg) = poll_socket(sock)? {
        trace!("{}", msg);
    }

    Ok(())
}

/// Priority prefix is just noise in our logs - we treat all messages equally
fn strip_priority(msg: &str) -> &str {
    msg.strip_prefix('<')
        .and_then(|s| s.find('>').map(|i| &s[i + 1..]))
        .unwrap_or(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_strip_priority() {
        assert_eq!(strip_priority("<6>test message"), "test message");
        assert_eq!(strip_priority("<13>another msg"), "another msg");
        assert_eq!(strip_priority("<191>high pri"), "high pri");
        assert_eq!(strip_priority("no prefix"), "no prefix");
        assert_eq!(strip_priority("<>empty"), "empty");
        assert_eq!(strip_priority("<6>"), "");
    }

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
}
