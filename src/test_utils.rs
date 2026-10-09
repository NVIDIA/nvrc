// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Shared test utilities. Only compiled during tests.

use nix::mount::MntFlags;
use nix::unistd::Uid;
use std::env;
use std::panic::{self, AssertUnwindSafe, PanicHookInfo};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::thread;

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

/// Unmount lazily: a test forking in parallel can hold a file open on the
/// mount, which would make a plain `umount` fail with EBUSY.
pub fn unmount(path: &Path) {
    nix::mount::umount2(path, MntFlags::MNT_DETACH).unwrap();
}

/// Run `f` and report whether it panicked, with `hook` handling panics on this
/// thread only. The panic hook is process-global: a slow hook (`sync()`) would
/// stall parallel tests that panic meanwhile, and a fatal one (power-off) would
/// kill them. Callers must be `#[serial]` so no other test swaps the hook.
pub fn panics_under_hook(
    hook: impl Fn(&PanicHookInfo<'_>) + Send + Sync + 'static,
    f: impl FnOnce(),
) -> bool {
    let this_thread = thread::current().id();
    let previous: Arc<dyn Fn(&PanicHookInfo<'_>) + Send + Sync> = Arc::from(panic::take_hook());
    let for_others = Arc::clone(&previous);
    panic::set_hook(Box::new(move |info| match thread::current().id() {
        id if id == this_thread => hook(info),
        _ => for_others(info),
    }));

    let panicked = panic::catch_unwind(AssertUnwindSafe(f)).is_err();

    panic::set_hook(Box::new(move |info| previous(info)));
    panicked
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
        let mut args: Vec<String> = env::args().collect();

        // Add --test-threads=1 to force serial execution when running under sudo
        // This prevents multiple root-requiring tests from interfering with each other
        let has_test_threads = args.iter().any(|arg| arg.starts_with("--test-threads"));
        if !has_test_threads {
            args.push("--test-threads=1".to_string());
        }

        // If a test filter is present but --exact is not, add it to prevent
        // matching multiple tests when re-running with sudo
        let has_exact = args.iter().any(|arg| arg == "--exact");
        // A filter is any non-flag argument (doesn't start with -)
        let has_filter = args.iter().skip(1).any(|arg| !arg.starts_with('-'));

        if has_filter && !has_exact {
            args.push("--exact".to_string());
        }

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
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    fn test_require_root_when_actually_root() {
        // We're running as root for coverage, so this should succeed
        require_root();
    }
}
