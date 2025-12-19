// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Shared test utilities. Only compiled during tests.

use nix::unistd::Uid;
use std::env;
use std::process::Command;

/// Ensure test runs as root.
///
/// Coverage builds: Panics immediately if not root. Coverage instrumentation
/// requires the entire test process to run as root from the start—there's no
/// way to escalate privileges mid-test. Run with: `sudo cargo llvm-cov`
///
/// Normal test builds: Re-executes the test binary via sudo, then exits with
/// the child's exit code. This allows `cargo test` to work without sudo.
pub fn require_root() {
    if Uid::effective().is_root() {
        return;
    }

    #[cfg(coverage)]
    panic!("coverage builds require root from start - run: sudo cargo llvm-cov");

    #[cfg(not(coverage))]
    {
        let args: Vec<String> = env::args().collect();
        match Command::new("sudo").args(&args).status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(e) => panic!("failed to run sudo: {}", e),
        }
    }
}
