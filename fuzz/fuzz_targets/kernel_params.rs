//! Fuzz target for kernel command line parameter parsing.
//!
//! Tests that arbitrary input to process_kernel_params() doesn't panic.
//! Catches integer overflows, malformed UTF-8 handling, and edge cases
//! in split/parse logic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use NVRC::nvrc::NVRC;

fuzz_target!(|data: &[u8]| {
    // Only fuzz valid UTF-8 strings (kernel cmdline is always ASCII/UTF-8)
    if let Ok(input) = std::str::from_utf8(data) {
        let mut nvrc = NVRC::default();
        // Ignore result - we're testing for panics, not correctness
        let _ = nvrc.process_kernel_params(Some(input));
    }
});

