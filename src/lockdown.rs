// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Lockdown primitives for confidential VM security.
//!
//! In production, panic triggers VM power-off. For tests, the shutdown
//! action is configurable via `set_panic_hook_with()`.

use crate::macros::ResultExt;
use nix::sys::reboot::{reboot, RebootMode};
use nix::unistd::sync;
use std::fs;
use std::io::Write;
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

/// Internal: panic handler with configurable shutdown (for unit tests).
/// Production uses power_off(); tests inject a no-op to avoid rebooting.
pub(crate) fn set_panic_hook_with<F: Fn() + Send + Sync + 'static>(shutdown: F) {
    panic::set_hook(Box::new(move |panic_info| {
        let msg = format!("NVRC panic: {panic_info}");
        // /dev/kmsg lands in the kernel ring buffer, which the kernel flushes
        // to the console during power-off. That is what keeps a panic visible
        // in the guest console (and thus in the kata journal).
        if let Ok(mut kmsg) = fs::OpenOptions::new().write(true).open("/dev/kmsg") {
            let _ = writeln!(kmsg, "{msg}");
        }
        sync();
        shutdown();
    }));
}

/// Permanently disable kernel module loading for this boot.
/// Once all required GPU drivers are loaded, this prevents any further
/// module insertion—a security hardening measure for confidential VMs
/// that blocks potential kernel-level attacks via malicious modules.
/// This is a one-way operation: once set, it cannot be undone without reboot.
pub fn disable_modules_loading() {
    const PATH: &str = "/proc/sys/kernel/modules_disabled";
    fs::write(PATH, b"1\n").or_panic(format_args!("disable module loading {PATH}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    #[ignore] // Permanently disables module loading until reboot - run with --include-ignored on CI
    fn test_disable_modules_loading() {
        require_root();

        // This permanently disables module loading until reboot.
        // Only run on dedicated test runners!
        disable_modules_loading();

        // Verify it was set
        let content = fs::read_to_string("/proc/sys/kernel/modules_disabled").unwrap();
        assert_eq!(content.trim(), "1");
    }

    #[test]
    fn test_power_off_function_exists() {
        // Just verify power_off compiles - can't call it without rebooting!
        let _: fn() = power_off;
    }

    #[test]
    #[cfg_attr(not(miri), serial_test::serial)]
    #[cfg_attr(miri, ignore = "the hook calls sync(), which miri cannot emulate")]
    fn test_panic_hook_invokes_shutdown_on_panic() {
        let saved_hook = panic::take_hook();
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();
        set_panic_hook_with(move || called_clone.store(true, Ordering::SeqCst));

        assert!(!called.load(Ordering::SeqCst)); // must not fire on install

        let result = panic::catch_unwind(|| panic!("boom"));
        panic::set_hook(saved_hook);

        assert!(result.is_err());
        assert!(
            called.load(Ordering::SeqCst),
            "panic must reach the shutdown action"
        );
    }

    #[test]
    #[ignore] // Installs real power_off hook - run with --include-ignored on CI
    fn test_set_panic_hook() {
        // Restore the previous hook: leaving power_off installed powers
        // off the machine on the next caught panic.
        let saved_hook = panic::take_hook();
        set_panic_hook();
        panic::set_hook(saved_hook);
    }
}
