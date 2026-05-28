// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Lets operators correlate dmesg output against the cosign/Rekor digest
//! published in the release evidence bundle, so a running NVRC can be matched
//! to its release artifact independently of the build pipeline (see
//! ARCHITECTURE.md §"Provenance & Supply-Chain Security").
//!
//! Not a security primitive: binary integrity is already enforced before
//! `execve` by dm-verity (block layer), fs-verity (file layer), and IPE
//! (see ARCHITECTURE.md §"Measured Rootfs" and §"Integrity Policy
//! Enforcement"). A compromised NVRC could lie about its own hash — the
//! trustworthy digest is the one in Rekor, not the one in dmesg.

use crate::macros::ResultExt;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;

// TODO(hardened_std): add to fs path whitelist when hardened_std::fs lands
const SELF_EXE: &str = "/proc/self/exe";
const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn self_exe() {
    let digest = sha256().or_panic(format_args!("hash {SELF_EXE}"));
    debug!("NVRC version={VERSION} sha256={digest}");
}

fn sha256() -> std::io::Result<String> {
    fs::read(SELF_EXE).map(|data| hex_encode(&Sha256::digest(&data)))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_encode_empty() {
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn test_hex_encode_known_vector() {
        assert_eq!(hex_encode(&[0x00, 0xff, 0xab, 0x12, 0x9c]), "00ffab129c");
    }

    #[test]
    fn test_sha256_self_returns_64_hex_chars() {
        let digest = sha256().expect("hash self");
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_sha256_empty_string_known_vector() {
        // NIST: SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            hex_encode(&Sha256::digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_abc_known_vector() {
        // NIST: SHA-256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(
            hex_encode(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_self_exe_runs_to_completion() {
        self_exe();
    }
}
