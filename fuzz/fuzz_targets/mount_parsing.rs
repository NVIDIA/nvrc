//! Fuzz target for /proc/mounts parsing.
//!
//! Tests that arbitrary input to is_mounted_in() doesn't panic.
//! The function parses mount table format with whitespace splitting.

#![no_main]

use libfuzzer_sys::fuzz_target;
use NVRC::mount::is_mounted_in;

fuzz_target!(|data: &[u8]| {
    // Only fuzz valid UTF-8 strings
    if let Ok(input) = std::str::from_utf8(data) {
        // Test with various target paths
        let _ = is_mounted_in(input, "/");
        let _ = is_mounted_in(input, "/dev");
        let _ = is_mounted_in(input, "/proc");
        let _ = is_mounted_in(input, "/sys/kernel/security");
        let _ = is_mounted_in(input, "");

        // Also fuzz the path parameter
        let _ = is_mounted_in("tmpfs /tmp tmpfs rw 0 0\n", input);
    }
});

