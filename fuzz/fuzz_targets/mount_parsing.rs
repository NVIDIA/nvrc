//! Fuzz target for /proc/filesystems parsing.
//!
//! Tests that arbitrary /proc/filesystems content and fstype strings never
//! panic or produce memory-safety bugs in the availability check logic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use NVRC::mount::fs_available;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        // Split at the first NUL-like separator to get two independent string
        // arguments; fall back to the whole input as filesystems with a fixed
        // fstype so the fuzzer still exercises the line-scan path.
        let (filesystems, fstype) = input.split_once('\x00').unwrap_or((input, "tmpfs"));
        let _ = fs_available(filesystems, fstype);
    }
});
