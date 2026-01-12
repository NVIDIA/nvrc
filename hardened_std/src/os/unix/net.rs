// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Hardened Unix domain sockets
//!
//! **Security Model:**
//! - Only whitelisted paths can be bound (compile-time enforcement)
//! - Production: only `/dev/log` allowed for syslog
//! - Tests: `/tmp/*` paths allowed for testing
//!
//! This module provides Unix datagram sockets for receiving syslog messages
//! from daemons like nvidia-persistenced, nv-hostengine, etc.

use crate::{last_os_error, path::Path, Error, Result};
use core::ffi::c_int;

/// Maximum path length for Unix socket addresses (from sys/un.h)
const UNIX_PATH_MAX: usize = 108;

/// Check if socket path is allowed
fn is_socket_path_allowed(path: &str) -> bool {
    // Production: only /dev/log for syslog
    if path == "/dev/log" {
        return true;
    }

    // Tests: allow /tmp paths for testing
    #[cfg(test)]
    if path.starts_with("/tmp/") {
        return true;
    }

    false
}

/// Unix datagram socket with security restrictions.
///
/// **Allowed paths:**
/// - `/dev/log` - syslog socket (production)
/// - `/tmp/*` - temporary paths (test only)
pub struct UnixDatagram {
    fd: c_int,
}

impl UnixDatagram {
    /// Bind a Unix datagram socket to the given path.
    ///
    /// # Security
    /// Only whitelisted paths are allowed:
    /// - `/dev/log` (production syslog)
    /// - `/tmp/*` (tests only)
    ///
    /// If the socket file already exists, bind() will fail with EADDRINUSE.
    /// In an ephemeral VM with fresh filesystem, this indicates an error.
    ///
    /// # Errors
    /// - `PathNotAllowed` if path is not in whitelist
    /// - OS errors for socket/bind failures (including EADDRINUSE)
    pub fn bind<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().as_str();
        if !is_socket_path_allowed(path) {
            return Err(Error::PathNotAllowed);
        }

        if path.len() >= UNIX_PATH_MAX {
            return Err(Error::InvalidInput(alloc::string::String::from(
                "Socket path too long",
            )));
        }

        // Create socket with SOCK_CLOEXEC to prevent fd leak to child processes
        // SAFETY: socket() is safe, we check return value
        let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
        if fd < 0 {
            return Err(last_os_error());
        }

        // Build sockaddr_un (zeroed provides null termination)
        let mut addr: libc::sockaddr_un = unsafe { core::mem::zeroed() };
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;

        // Copy path into sun_path using safe iteration
        let path_bytes = path.as_bytes();
        for (i, &b) in path_bytes.iter().enumerate() {
            addr.sun_path[i] = b as i8;
        }

        // Bind the socket
        // SAFETY: bind() is safe with valid fd and address
        let ret = unsafe {
            libc::bind(
                fd,
                &addr as *const libc::sockaddr_un as *const libc::sockaddr,
                core::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t,
            )
        };

        if ret < 0 {
            let err = last_os_error();
            // Close fd on bind failure
            unsafe { libc::close(fd) };
            return Err(err);
        }

        Ok(Self { fd })
    }

    /// Receive a datagram from the socket.
    ///
    /// Returns the number of bytes read and the source address.
    /// This is the main method used by syslog to receive messages.
    pub fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        let mut addr: libc::sockaddr_un = unsafe { core::mem::zeroed() };
        let mut addr_len: libc::socklen_t =
            core::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;

        // SAFETY: recvfrom() is safe with valid fd, buffer, and address
        let ret = unsafe {
            libc::recvfrom(
                self.fd,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                0,
                &mut addr as *mut libc::sockaddr_un as *mut libc::sockaddr,
                &mut addr_len,
            )
        };

        if ret < 0 {
            return Err(last_os_error());
        }

        Ok((ret as usize, SocketAddr::from_raw(addr)))
    }
}

impl Drop for UnixDatagram {
    fn drop(&mut self) {
        // SAFETY: close() is safe with valid fd
        unsafe { libc::close(self.fd) };
    }
}

impl crate::os::fd::AsFd for UnixDatagram {
    fn as_fd(&self) -> i32 {
        self.fd
    }
}

/// Socket address for Unix domain sockets (opaque marker type).
pub struct SocketAddr {
    _addr: libc::sockaddr_un,
}

impl SocketAddr {
    fn from_raw(addr: libc::sockaddr_un) -> Self {
        Self { _addr: addr }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test]
    fn test_path_whitelist_dev_log() {
        assert!(is_socket_path_allowed("/dev/log"));
    }

    #[test]
    fn test_path_whitelist_tmp() {
        assert!(is_socket_path_allowed("/tmp/test.sock"));
        assert!(is_socket_path_allowed("/tmp/foo/bar.sock"));
    }

    #[test]
    fn test_path_whitelist_rejected() {
        assert!(!is_socket_path_allowed("/var/log/syslog"));
        assert!(!is_socket_path_allowed("/etc/passwd"));
        assert!(!is_socket_path_allowed("relative/path"));
        assert!(!is_socket_path_allowed("/home/user/sock"));
    }

    #[test]
    fn test_bind_and_recv() {
        let path = format!("/tmp/hardened_test_{}.sock", std::process::id());
        let _ = std::fs::remove_file(&path);

        // Bind server socket (hardened_std)
        let server = UnixDatagram::bind(&path).expect("bind failed");

        // Use std::os::unix::net for client (test-only, no restrictions needed)
        let client = std::os::unix::net::UnixDatagram::unbound().expect("unbound failed");

        let msg = b"<6>test message from client";
        client.send_to(msg, &path).expect("send_to failed");

        let mut buf = [0u8; 256];
        let (len, _addr) = server.recv_from(&mut buf).expect("recv_from failed");
        assert_eq!(&buf[..len], msg);

        drop(server);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_bind_disallowed_path() {
        let result = UnixDatagram::bind("/var/run/test.sock");
        assert!(matches!(result, Err(Error::PathNotAllowed)));
    }

    #[test]
    fn test_multiple_messages() {
        let path = format!("/tmp/hardened_multi_{}.sock", std::process::id());
        let _ = std::fs::remove_file(&path);

        let server = UnixDatagram::bind(&path).unwrap();
        let client = std::os::unix::net::UnixDatagram::unbound().unwrap();

        client.send_to(b"first", &path).unwrap();
        client.send_to(b"second", &path).unwrap();
        client.send_to(b"third", &path).unwrap();

        let mut buf = [0u8; 256];

        let (len, _) = server.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..len], b"first");

        let (len, _) = server.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..len], b"second");

        let (len, _) = server.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..len], b"third");

        drop(server);
        let _ = std::fs::remove_file(&path);
    }
}
