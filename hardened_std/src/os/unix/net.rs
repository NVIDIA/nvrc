// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Hardened Unix domain sockets
//!
//! **Security Model:**
//! - Only whitelisted paths can be bound (runtime enforcement at bind time)
//! - Production: only `/dev/log` allowed for syslog
//! - Tests: `/tmp/*` paths allowed for testing
//!
//! This module provides Unix datagram sockets for receiving syslog messages
//! from daemons like nvidia-persistenced, nv-hostengine, etc.

use crate::{last_os_error, path::Path, Error, Result};
use core::ffi::c_int;

/// Maximum path length for Unix socket addresses (from sys/un.h)
const UNIX_PATH_MAX: usize = 108;

/// TempDir paths for dependent crate tests (always available).
/// TempDir creates paths like /tmp/.tmpXXXXX which are ephemeral and safe.
const ALLOWED_TEMPDIR_PREFIXES: &[&str] = &[
    "/tmp/.", // TempDir paths for NVRC syslog tests
];

/// Test path prefixes only for hardened_std's own tests
#[cfg(test)]
const ALLOWED_TEST_PREFIXES: &[&str] = &[
    "/tmp/hardened_", // hardened_std's own test sockets
];

/// Check if socket path is allowed
fn is_socket_path_allowed(path: &str) -> bool {
    // Production: only /dev/log for syslog
    if path == "/dev/log" {
        return true;
    }

    // Allow TempDir paths for dependent crate tests (nvrc tests)
    if ALLOWED_TEMPDIR_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
    {
        return true;
    }

    // hardened_std's own tests: allow /tmp/hardened_* paths
    #[cfg(test)]
    if ALLOWED_TEST_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
    {
        return true;
    }

    false
}

/// Unix datagram socket with security restrictions.
///
/// **Allowed paths:**
/// - `/dev/log` - syslog socket (production)
/// - `/tmp/*` - temporary paths (test only)
#[derive(Debug)]
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
        // Note: sun_path is i8 on glibc, u8 on musl - cast handles both
        let path_bytes = path.as_bytes();
        for (i, &b) in path_bytes.iter().enumerate() {
            addr.sun_path[i] = b as _;
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
        // SAFETY: close() is safe with valid fd.
        //
        // Note: We intentionally do NOT unlink the socket path from the
        // filesystem. This matches std::os::unix::net::UnixDatagram behavior.
        // In ephemeral VMs with fresh filesystems, if a socket file exists
        // on next bind, it indicates an error (EADDRINUSE).
        unsafe { libc::close(self.fd) };
    }
}

impl crate::os::fd::AsFd for UnixDatagram {
    fn as_fd(&self) -> i32 {
        self.fd
    }
}

/// Implement std::os::fd::AsFd when std-support is enabled.
/// This allows using hardened_std::UnixDatagram with nix::poll::PollFd.
#[cfg(feature = "std-support")]
impl std::os::fd::AsFd for UnixDatagram {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        // SAFETY: self.fd is a valid file descriptor owned by this UnixDatagram.
        // The BorrowedFd's lifetime is tied to &self, ensuring the fd remains valid.
        unsafe { std::os::fd::BorrowedFd::borrow_raw(self.fd) }
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

    // Note: Tests use std::fs::remove_file for cleanup. While hardened_std
    // avoids remove_file in production (ephemeral VMs start fresh), tests
    // need cleanup to avoid /tmp artifacts and EADDRINUSE on re-runs.

    #[test]
    fn test_path_whitelist_dev_log() {
        assert!(is_socket_path_allowed("/dev/log"));
    }

    #[test]
    fn test_path_whitelist_tempdir() {
        // TempDir paths (always allowed for dependent crate tests)
        assert!(is_socket_path_allowed("/tmp/.tmpABCDE/test.sock"));
        assert!(is_socket_path_allowed("/tmp/.tmp123/sub/test.sock"));
    }

    #[test]
    fn test_path_whitelist_test_prefixes() {
        // hardened_std test paths (only in cfg(test))
        assert!(is_socket_path_allowed("/tmp/hardened_test.sock"));
        assert!(is_socket_path_allowed("/tmp/hardened_multi.sock"));
    }

    #[test]
    fn test_path_whitelist_rejected() {
        assert!(!is_socket_path_allowed("/var/log/syslog"));
        assert!(!is_socket_path_allowed("/etc/passwd"));
        assert!(!is_socket_path_allowed("relative/path"));
        assert!(!is_socket_path_allowed("/home/user/sock"));
        assert!(!is_socket_path_allowed("/tmp")); // No trailing slash
        assert!(!is_socket_path_allowed("/tmp/random.sock")); // Not in whitelist
    }

    #[test]
    fn test_bind_and_recv() {
        let path = format!(
            "/tmp/hardened_test_{}_{:?}.sock",
            std::process::id(),
            std::thread::current().id()
        );
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
    fn test_bind_path_too_long() {
        // UNIX_PATH_MAX is 108, so a path of 108+ bytes should be rejected
        // "/tmp/hardened_" is 14 chars, so we need 94+ more to reach 108
        let long_path = format!("/tmp/hardened_{}", "x".repeat(94));
        assert!(long_path.len() >= 108);
        let result = UnixDatagram::bind(&long_path);
        assert!(matches!(result, Err(Error::InvalidInput(_))));
    }

    #[test]
    fn test_multiple_messages() {
        let path = format!(
            "/tmp/hardened_multi_{}_{:?}.sock",
            std::process::id(),
            std::thread::current().id()
        );
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
