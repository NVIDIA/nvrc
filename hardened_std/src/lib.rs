//! hardened_std - Security-hardened drop-in replacement for std library
//!
//! This crate provides minimal, security-constrained implementations of std
//! library APIs with built-in limits and validations to reduce attack surface.

#![no_std]

extern crate alloc;

pub mod fs;
pub mod path;
pub mod process;
pub mod collections;
pub mod os;

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
    pub fn with_context(self, _msg: &'static str) -> Self {
        // For now, just return self
        // Could extend to store context in the future
        self
    }
}

pub type Result<T> = core::result::Result<T, Error>;

/// Get last OS error from errno
pub(crate) fn last_os_error() -> Error {
    let errno = unsafe { *libc::__errno_location() };
    match errno {
        libc::ENOENT => Error::NotFound,
        libc::EACCES | libc::EPERM => Error::PermissionDenied,
        libc::EEXIST => Error::AlreadyExists,
        _ => Error::Io(errno),
    }
}
