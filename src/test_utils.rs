// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Shared test utilities. Only compiled during tests.

use nix::unistd::Uid;
use std::env;
use std::process::Command;

/// Ensure test runs as root.
/// - During coverage builds: panics if not root (coverage must run as root)
/// - During normal tests: re-executes via sudo and exits with child's code
///
/// This allows privileged tests to run in CI while ensuring coverage
/// reports accurately reflect execution as root.
pub fn require_root() {
    if Uid::effective().is_root() {
        return;
    }

    #[cfg(coverage)]
    panic!("coverage tests must run as root - use: sudo cargo llvm-cov");

    #[cfg(not(coverage))]
    {
        let args: Vec<String> = env::args().collect();
        match Command::new("sudo").args(&args).status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(e) => panic!("failed to run sudo: {}", e),
        }
    }
}
