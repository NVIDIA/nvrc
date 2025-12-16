// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{Context, Result};
use nix::fcntl::AT_FDCWD;
use nix::sys::stat::{self, Mode, SFlag};
use nix::unistd::symlinkat;
use std::fs;
use std::path::Path;

/// Create (or update) a symbolic link from target to linkpath.
/// Idempotent: if link already points to target, it is left unchanged.
pub fn ln(target: &str, linkpath: &str) -> Result<()> {
    if let Ok(existing) = fs::read_link(linkpath) {
        if existing == Path::new(target) {
            return Ok(());
        }
        let _ = fs::remove_file(linkpath);
    }

    // If path exists as non-symlink (file/dir), symlinkat will fail.
    // INTENTIONAL: in ephemeral VMs, obstacles indicate tampering/compromise.
    // Fail loudly rather than silently fixing potentially malicious state.
    symlinkat(target, AT_FDCWD, linkpath).with_context(|| format!("ln {} -> {}", linkpath, target))
}

/// Create (or replace) a character device node with desired major/minor.
/// Always recreates to avoid stale metadata/permissions.
pub fn mknod(path: &str, kind: SFlag, major: u64, minor: u64) -> Result<()> {
    if Path::new(path).exists() {
        fs::remove_file(path).with_context(|| format!("remove {} failed", path))?;
    }
    stat::mknod(
        path,
        kind,
        Mode::from_bits_truncate(0o666),
        stat::makedev(major, minor),
    )
    .with_context(|| format!("mknod {} failed", path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_ln_fails_on_regular_file_obstacle() {
        // Security test: ln should FAIL if non-symlink obstacle exists
        // In ephemeral VMs, this indicates tampering
        let tmpdir = TempDir::new().unwrap();
        let linkpath = tmpdir.path().join("stdin");
        let target = "/proc/self/fd/0";

        // Create regular file obstacle (simulates tampering)
        fs::write(&linkpath, "tampered").unwrap();
        assert!(linkpath.exists() && !linkpath.is_symlink());

        // ln should FAIL (not silently fix)
        let result = ln(target, linkpath.to_str().unwrap());
        assert!(result.is_err(), "ln must fail on non-symlink obstacle");
    }

    #[test]
    fn test_ln_fails_on_directory_obstacle() {
        // Security test: ln should FAIL if directory obstacle exists
        let tmpdir = TempDir::new().unwrap();
        let linkpath = tmpdir.path().join("stdin");
        let target = "/proc/self/fd/0";

        // Create directory obstacle
        fs::create_dir(&linkpath).unwrap();
        assert!(linkpath.is_dir());

        // ln should FAIL (not silently fix)
        let result = ln(target, linkpath.to_str().unwrap());
        assert!(result.is_err(), "ln must fail on directory obstacle");
    }

    #[test]
    fn test_ln_idempotent() {
        // Verify correct symlinks are left unchanged
        let tmpdir = TempDir::new().unwrap();
        let linkpath = tmpdir.path().join("test_link");
        let target = "/proc/self/fd/0";

        // Create correct symlink
        ln(target, linkpath.to_str().unwrap()).unwrap();
        assert!(linkpath.is_symlink());

        // Call again - should be idempotent
        ln(target, linkpath.to_str().unwrap()).unwrap();
        assert_eq!(fs::read_link(&linkpath).unwrap(), Path::new(target));
    }
}
