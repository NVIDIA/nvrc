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
        // NVRC is fail-fast: invalid numeric params must panic and reboot the VM.
        // Swallow only those known validation panics; re-raise anything else so
        // libFuzzer still catches genuine parser bugs.
        if let Err(payload) = std::panic::catch_unwind(|| {
            let mut nvrc = NVRC::default();
            nvrc.process_kernel_params(Some(input));
        }) {
            let msg = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("");
            if !msg.contains("nvrc.smi.") {
                std::panic::resume_unwind(payload);
            }
        }
    }
});

