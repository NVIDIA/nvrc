// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Lockdown primitives for confidential VM security.
//!
//! In production, panic triggers VM power-off. For tests, the shutdown
//! action is configurable via `set_panic_hook_with()`.

use anyhow::{anyhow, Result};
use hardened_std::fs;
use nix::sys::reboot::{reboot, RebootMode};
use nix::unistd::sync;
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
pub fn set_panic_hook() -> Result<()> {
    set_panic_hook_with(power_off)
}

/// Internal: panic handler with configurable shutdown (for unit tests).
/// Production uses power_off(); tests inject a no-op to avoid rebooting.
fn set_panic_hook_with<F: Fn() + Send + Sync + 'static>(shutdown: F) -> Result<()> {
    panic::set_hook(Box::new(move |panic_info| {
        // fd 1,2 are always available from the kernel
        eprintln!("panic: {panic_info}");
        sync();
        shutdown();
    }));
    Ok(())
}

/// Permanently disable kernel module loading for this boot.
/// Once all required GPU drivers are loaded, this prevents any further
/// module insertion—a security hardening measure for confidential VMs
/// that blocks potential kernel-level attacks via malicious modules.
/// This is a one-way operation: once set, it cannot be undone without reboot.
pub fn disable_modules_loading() -> Result<()> {
    const PATH: &str = "/proc/sys/kernel/modules_disabled";
    fs::write(PATH, b"1\n").map_err(|e| anyhow!("disable module loading {}: {}", PATH, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_set_panic_hook_with_custom_action() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        // Install hook with test closure
        let _ = set_panic_hook_with(move || {
            called_clone.store(true, Ordering::SeqCst);
        });

        // The hook is installed - we can't trigger it without panicking,
        // but we've exercised the code path
        assert!(!called.load(Ordering::SeqCst)); // Not called yet
    }

    #[test]
    fn test_disable_modules_loading() {
        require_root();

        // This permanently disables module loading until reboot.
        // Only run on dedicated test runners!
        let result = disable_modules_loading();
        assert!(result.is_ok());

        // Verify it was set (use std::fs in tests to verify hardened_std wrote correctly)
        let content = std::fs::read_to_string("/proc/sys/kernel/modules_disabled").unwrap();
        assert_eq!(content.trim(), "1");
    }

    #[test]
    fn test_power_off_function_exists() {
        // Just verify power_off compiles - can't call it without rebooting!
        let _: fn() = power_off;
    }

    #[test]
    fn test_set_panic_hook() {
        // Installs the real hook (with power_off) - just don't trigger it!
        let _ = set_panic_hook();
        // If we got here, the hook was installed successfully
    }
}
