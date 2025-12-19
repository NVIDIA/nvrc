// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Common macros for the init system.

/// Unwrap a Result or panic with a descriptive init failure message.
/// Used for operations that must succeed for init to proceed.
#[macro_export]
macro_rules! must {
    ($expr:expr) => {
        if let Err(e) = $expr {
            panic!("init failure: {} => {e}", stringify!($expr));
        }
    };
    ($expr:expr, $msg:literal) => {
        if let Err(e) = $expr {
            panic!("init failure: {}: {e}", $msg);
        }
    };
}

#[cfg(test)]
mod tests {
    /// Test must! macro with Ok result - should not panic
    #[test]
    fn test_must_ok() {
        must!(Ok::<(), &str>(()));
    }

    /// Test must! macro with custom message - should not panic on Ok
    #[test]
    fn test_must_ok_with_message() {
        must!(Ok::<(), &str>(()), "custom message");
    }

    /// Test must! macro panics on Err
    #[test]
    #[should_panic(expected = "init failure")]
    fn test_must_err_panics() {
        must!(Err::<(), _>("something went wrong"));
    }

    /// Test must! macro with custom message panics on Err
    #[test]
    #[should_panic(expected = "custom error")]
    fn test_must_err_with_message_panics() {
        must!(Err::<(), _>("boom"), "custom error");
    }
}
