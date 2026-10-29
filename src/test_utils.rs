// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Shared test utilities. Only compiled during tests.

use nix::mount::MntFlags;
use nix::unistd::Uid;
use std::env;
use std::panic::{self, AssertUnwindSafe, PanicHookInfo};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
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

/// Lazy unmount: a parallel test's fork can hold the mount busy (EBUSY).
pub fn unmount(path: &Path) {
    nix::mount::umount2(path, MntFlags::MNT_DETACH).unwrap();
}

/// Serializes panic hook swaps.
static HOOK_SWAP: Mutex<()> = Mutex::new(());

/// Hold while swapping the panic hook.
pub fn hook_swap_guard() -> MutexGuard<'static, ()> {
    HOOK_SWAP.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Run `f` and report whether it panicked, with `hook` handling this thread's
/// panics only: the hook is process-global, so a slow (`sync()`) or fatal
/// (power-off) one would hit parallel tests.
pub fn panics_under_hook(
    hook: impl Fn(&PanicHookInfo<'_>) + Send + Sync + 'static,
    f: impl FnOnce(),
) -> bool {
    let _swap = hook_swap_guard();
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

// Only some management PFs carry the role; siblings must still be discovered.
pub fn add_management_pf(f: &pcilibs_rs::testfs::Fake, bdf: &str, marked: bool) {
    f.add_pci_device(bdf, 0x15b3, 0x1021, 0x020700, None);
    let vpd: &[u8] = if marked {
        b"\x90\x0e\x00VA\x0bSMDL=SW_MNG\x78"
    } else {
        b"\x78"
    };
    std::fs::write(f.device(bdf).join("vpd"), vpd).unwrap();
}

/// A port of a registered InfiniBand device; `cap_mask` bit 10 set means its
/// SM is disabled, and the GUID is the low half of GID 0.
pub fn add_ib_port(f: &pcilibs_rs::testfs::Fake, ib: &str, port: u32, cap_mask: &str, gid: &str) {
    let path = f
        .sysfs
        .infiniband()
        .join(ib)
        .join("ports")
        .join(port.to_string());
    std::fs::create_dir_all(path.join("gids")).unwrap();
    std::fs::write(path.join("link_layer"), "InfiniBand\n").unwrap();
    std::fs::write(path.join("cap_mask"), cap_mask).unwrap();
    std::fs::write(path.join("gids/0"), gid).unwrap();
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
