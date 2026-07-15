//! Fuzz kernel command line parsing.
//!
//! Fuzzes the try_ variant because cargo-fuzz builds with panic=abort, so
//! catch_unwind cannot filter the expected validation panics. Err is an
//! expected rejection; any panic is a real bug and becomes a libFuzzer crash.

#![no_main]

use libfuzzer_sys::fuzz_target;
use NVRC::nvrc::NVRC;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let _ = NVRC::default().try_process_kernel_params(Some(input));
    }
});
