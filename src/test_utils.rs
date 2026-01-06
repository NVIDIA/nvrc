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
    require_root_impl(Uid::effective().is_root())
}

/// Internal: testable implementation with injected root status.
fn require_root_impl(is_root: bool) {
    if is_root {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_require_root_impl_when_root() {
        // Should return immediately without panic
        require_root_impl(true);
    }

    #[test]
    #[cfg(coverage)]
    fn test_require_root_impl_when_not_root_coverage() {
        // In coverage builds, not being root should panic
        let result = std::panic::catch_unwind(|| require_root_impl(false));
        assert!(result.is_err());
    }

    #[test]
    fn test_require_root_when_actually_root() {
        // We're running as root for coverage, so this should succeed
        require_root();
    }
}
