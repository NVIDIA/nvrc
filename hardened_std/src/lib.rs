// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! hardened_std - Security-hardened drop-in replacement for std library
//!
//! This crate provides minimal, security-constrained implementations of std
//! library APIs with built-in limits and validations to reduce attack surface.

#![no_std]

extern crate alloc;

// For tests and std-support feature, we need std
#[cfg(any(test, feature = "std-support"))]
extern crate std;

pub mod collections;
pub mod fs;
pub mod os;
pub mod path;
pub mod process;

use core::fmt;

/// Custom error type replacing anyhow
#[derive(Debug)]
pub enum Error {
    WriteTooLarge(usize),
    PathNotAllowed,
    BinaryNotAllowed,
    Io(i32), // errno
    WriteIncomplete,
    NotFound,
    PermissionDenied,
    AlreadyExists,
    InvalidInput(alloc::string::String),
    Other(alloc::string::String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::WriteTooLarge(size) => write!(f, "Write size {} exceeds maximum", size),
            Error::PathNotAllowed => write!(f, "Path not in allowed list"),
            Error::BinaryNotAllowed => write!(f, "Binary not in allowed list"),
            Error::Io(errno) => write!(f, "IO error: errno {}", errno),
            Error::WriteIncomplete => write!(f, "Write incomplete"),
            Error::NotFound => write!(f, "Not found"),
            Error::PermissionDenied => write!(f, "Permission denied"),
            Error::AlreadyExists => write!(f, "Already exists"),
            Error::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
            Error::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl Error {
    /// Returns self without adding context.
    /// This method exists for API compatibility with anyhow but does NOT store context.
    /// The context parameter is intentionally ignored to keep Error simple and Copy-compatible.
    ///
    /// # Note
    /// If you need context preservation, use Error::Other(String) instead.
    #[allow(unused_variables)]
    pub fn with_context(self, msg: &'static str) -> Self {
        self
    }
}

pub type Result<T> = core::result::Result<T, Error>;

/// Get last OS error from errno.
///
/// # Platform Support
/// This implementation uses libc::__errno_location() which is glibc-specific.
/// NVRC runs exclusively on Linux with glibc, so this is safe for our use case.
/// If portability to musl or other C libraries is needed in the future,
/// use libc::__errno() or platform-specific errno access methods.
///
/// # Safety
/// Safe because errno location is thread-local and guaranteed valid by libc.
pub(crate) fn last_os_error() -> Error {
    // SAFETY: __errno_location() returns a valid thread-local pointer to errno.
    // This is safe because:
    // 1. We're only running on glibc-based Linux (NVRC's target platform)
    // 2. The pointer is thread-local and always valid
    // 3. We're just reading, not modifying
    let errno = unsafe { *libc::__errno_location() };
    match errno {
        libc::ENOENT => Error::NotFound,
        libc::EACCES | libc::EPERM => Error::PermissionDenied,
        libc::EEXIST => Error::AlreadyExists,
        _ => Error::Io(errno),
    }
}
