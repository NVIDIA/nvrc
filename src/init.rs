// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use crate::hash;

/// NVRC's init duties (mounts, module loads, daemon forks, the poweroff panic
/// hook) would wreck a normal host, so they must only run as PID 1. Anywhere
/// else (CI smoke test, dev shell) report identity and exit before the caller
/// touches anything.
pub fn as_pid1() {
    as_pid1_with(running_as_init(), || std::process::exit(0));
}

// Production exits the process; tests inject a no-op to observe the bail path
// without killing the runner (cf. lockdown::set_panic_hook_with).
fn as_pid1_with<F: FnOnce()>(is_init: bool, exit: F) {
    if is_init {
        return;
    }
    // No logger on this path, so print to stdout rather than via the dropped
    // log macros; this is the CI smoke test's only observable output.
    println!("{}", hash::version_line());
    exit();
}

// Raw SYS_getpid syscall: stays on the pure-syscall path hardened_std targets,
// and needs no /proc (unmounted this early, mount::setup runs later).
fn running_as_init() -> bool {
    unsafe { libc::syscall(libc::SYS_getpid) == 1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn test_running_as_init_false_for_test_harness() {
        // The test runner is never PID 1, so the real syscall must report so.
        assert!(!running_as_init());
    }

    #[test]
    fn test_as_pid1_returns_without_exiting_when_init() {
        let exited = Cell::new(false);
        as_pid1_with(true, || exited.set(true));
        assert!(
            !exited.get(),
            "must fall through to the caller's init sequence"
        );
    }

    #[test]
    fn test_as_pid1_exits_when_not_init() {
        let exited = Cell::new(false);
        as_pid1_with(false, || exited.set(true));
        assert!(exited.get(), "non-PID-1 must report identity and exit");
    }

    // The production guard calls std::process::exit(0), which no in-process
    // test survives; fork so the child runs the real thing.
    #[test]
    fn test_as_pid1_production_guard_exits_zero() {
        match unsafe { libc::fork() } {
            0 => {
                as_pid1();
                unsafe { libc::_exit(1) } // only reached if the guard fell through
            }
            pid if pid > 0 => {
                let mut status = 0;
                assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
                assert!(libc::WIFEXITED(status), "guard must exit, not crash");
                assert_eq!(libc::WEXITSTATUS(status), 0, "guard must exit(0)");
            }
            err => panic!("fork failed: {err}"),
        }
    }
}
