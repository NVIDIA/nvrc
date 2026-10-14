// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Symbolic inputs shared by the `#[cfg(kani)]` proof modules.

/// Every byte value and every length up to N, so a harness taking this proves
/// its property for ALL UTF-8 strings of at most N bytes.
pub fn any_string<const N: usize>() -> String {
    let bytes: [u8; N] = kani::any();
    let len: usize = kani::any_where(|l| *l <= N);
    let s = std::str::from_utf8(&bytes[..len]);
    kani::assume(s.is_ok());
    s.unwrap().to_owned()
}

/// Stub for `std::fmt::format`: core::fmt is prohibitively slow under CBMC and
/// no proof inspects an error message. Drops the text, never the Err.
pub fn no_fmt(_: std::fmt::Arguments<'_>) -> String {
    String::new()
}

/// Stub for core's word-at-a-time memchr (behind `str::find`, `lines`,
/// `split_once`): the unaligned usize reads over symbolic bytes exhaust CBMC.
/// Same contract, byte loop.
pub fn naive_memchr(x: u8, text: &[u8]) -> Option<usize> {
    text.iter().position(|&b| b == x)
}

kani::stub_set!(pub cbmc,
    stub(std::fmt::format, crate::kani_support::no_fmt),
    stub(core::slice::memchr::memchr, crate::kani_support::naive_memchr),
);
