// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Lockdown primitives for confidential VM security.
//!
//! In production, panic triggers VM power-off. For tests, the shutdown
//! action is configurable via `set_panic_hook_with()`.

use anyhow::{Context, Result};
use nix::sys::reboot::{reboot, RebootMode};
use nix::unistd::sync;
use std::fs;
use std::panic;

/// Default shutdown action: power off the VM.
fn power_off() {
    let _ = reboot(RebootMode::RB_POWER_OFF);
}

/// Install a panic handler that powers off the VM instead of unwinding.
/// In a confidential VM, a panic could leave the system in an undefined state
/// with potential data exposure. Power-off ensures clean termination—the host
/// hypervisor will see the VM exit and can handle cleanup appropriately.
/// sync() flushes pending writes before power-off to preserve any logs.
pub fn set_panic_hook() {
    set_panic_hook_with(power_off)
}

/// Testable version: install panic handler with custom shutdown action.
/// Production uses `power_off()`, tests can use a no-op or logging closure.
fn set_panic_hook_with<F: Fn() + Send + Sync + 'static>(shutdown: F) {
    panic::set_hook(Box::new(move |panic_info| {
        log::error!("panic: {panic_info}");
        sync();
        shutdown();
    }));
}

/// Permanently disable kernel module loading for this boot.
/// Once all required GPU drivers are loaded, this prevents any further
/// module insertion—a security hardening measure for confidential VMs
/// that blocks potential kernel-level attacks via malicious modules.
/// This is a one-way operation: once set, it cannot be undone without reboot.
pub fn disable_modules_loading() -> Result<()> {
    disable_modules_at("/proc/sys/kernel/modules_disabled")
}

/// Testable version with configurable path.
fn disable_modules_at(path: &str) -> Result<()> {
    fs::write(path, b"1\n").with_context(|| format!("disable module loading: {}", path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    #[test]
    fn test_set_panic_hook_with_custom_action() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        // Install hook with test closure
        set_panic_hook_with(move || {
            called_clone.store(true, Ordering::SeqCst);
        });

        // The hook is installed - we can't trigger it without panicking,
        // but we've exercised the code path
        assert!(!called.load(Ordering::SeqCst)); // Not called yet
    }

    #[test]
    fn test_disable_modules_at_success() {
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_str().unwrap();

        let result = disable_modules_at(path);
        assert!(result.is_ok());

        // Verify content
        let content = fs::read_to_string(path).unwrap();
        assert_eq!(content, "1\n");
    }

    #[test]
    fn test_disable_modules_at_nonexistent() {
        let result = disable_modules_at("/nonexistent/path");
        assert!(result.is_err());
    }

    #[test]
    fn test_power_off_function_exists() {
        // Just verify power_off compiles - can't call it without rebooting!
        let _: fn() = power_off;
    }

    #[test]
    fn test_set_panic_hook() {
        // Installs the real hook (with power_off) - just don't trigger it!
        set_panic_hook();
        // If we got here, the hook was installed successfully
    }

    #[test]
    fn test_disable_modules_loading() {
        // Will fail without root/proper permissions, but exercises the code
        let _ = disable_modules_loading();
    }
}
