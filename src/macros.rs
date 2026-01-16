// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Error handling extensions for fail-fast init semantics.
//!
//! NVRC runs as PID 1 in an ephemeral VM. On any error, we panic (which
//! triggers VM power-off via our panic hook). This trait provides a clean
//! `.or_panic(msg)` method instead of verbose `.unwrap_or_else(|e| panic!(...))`.

use std::fmt::Display;

/// Extension trait for Result types that panic on error with context.
pub trait ResultExt<T> {
    /// Unwrap the value or panic with the given message and error details.
    /// Use with static strings or pre-formatted messages.
    fn or_panic(self, msg: impl Display) -> T;
}

impl<T, E: Display> ResultExt<T> for Result<T, E> {
    #[cold]
    #[inline(never)]
    fn or_panic(self, msg: impl Display) -> T {
        match self {
            Ok(v) => v,
            Err(e) => panic!("{msg}: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::catch_unwind;

    #[test]
    fn test_or_panic_ok() {
        let result: Result<i32, &str> = Ok(42);
        assert_eq!(result.or_panic("should not panic"), 42);
    }

    #[test]
    fn test_or_panic_err() {
        let result = catch_unwind(|| {
            let r: Result<i32, &str> = Err("boom");
            r.or_panic("operation failed");
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_or_panic_with_io_error() {
        let result = catch_unwind(|| {
            let r: Result<(), std::io::Error> = Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "file missing",
            ));
            r.or_panic("read config");
        });
        assert!(result.is_err());
    }
}
