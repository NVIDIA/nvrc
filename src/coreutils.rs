// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{Context, Result};
use nix::fcntl::AT_FDCWD;
use nix::sys::stat::{self, Mode, SFlag};
use nix::unistd::symlinkat;
use std::fs;
use std::path::Path;

#[cfg(test)]
use serial_test::serial;
/// Create (or update) a symbolic link from target to linkpath.
/// Idempotent: if link already points to target, it is left unchanged.
pub fn ln(target: &str, linkpath: &str) -> Result<()> {
    let path = Path::new(linkpath);

    // Check if it's already a correct symlink
    if let Ok(existing) = fs::read_link(path) {
        if existing == Path::new(target) {
            return Ok(()); // already correct
        }
    }

    // Remove whatever exists at linkpath (file, symlink, etc.)
    if path.exists() || path.is_symlink() {
        let _ = fs::remove_file(path);
    }

    symlinkat(target, AT_FDCWD, linkpath).with_context(|| format!("ln {} -> {}", linkpath, target))
}

/// Create (or replace) a character device node with desired major/minor.
/// Always recreates to avoid stale metadata/permissions.
pub fn mknod(path: &str, kind: SFlag, major: u64, minor: u64) -> Result<()> {
    if Path::new(path).exists() {
        fs::remove_file(path).with_context(|| format!("remove {} failed", path))?;
    }

    let perm = Mode::from_bits_truncate(0o666);

    // Temporarily clear umask so we get exact permissions requested
    let old_umask = stat::umask(Mode::empty());
    let result = stat::mknod(path, kind, perm, stat::makedev(major, minor));
    stat::umask(old_umask); // Restore original umask

    result.with_context(|| format!("mknod {} failed", path))
}
#[cfg(test)]
mod tests {
    use super::*;
    use nix::unistd::Uid;
    use std::env;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use std::process::Command;
    use tempfile::TempDir;

    /// During coverage (must run as root), just asserts we're root.
    /// During normal tests, re-executes via sudo and exits with child's code.
    fn require_root() {
        if Uid::effective().is_root() {
            return;
        }

        #[cfg(coverage)]
        panic!("coverage tests must run as root - use: sudo cargo llvm-cov");

        #[cfg(not(coverage))]
        {
            // Re-run this test as root; exit with child's status to propagate pass/fail
            let args: Vec<String> = env::args().collect();
            match Command::new("sudo").args(&args).status() {
                Ok(status) => std::process::exit(status.code().unwrap_or(1)),
                Err(e) => panic!("failed to run sudo: {}", e),
            }
        }
    }

    // ==================== ln tests ====================

    #[test]
    fn test_ln_creates_symlink() {
        let tmpdir = TempDir::new().unwrap();
        let target = tmpdir.path().join("target.txt");
        let link = tmpdir.path().join("link.txt");

        // Create target file
        fs::write(&target, "hello").unwrap();

        // Create symlink
        ln(target.to_str().unwrap(), link.to_str().unwrap()).unwrap();

        // Verify symlink exists and points to target
        assert!(link.is_symlink());
        assert_eq!(fs::read_link(&link).unwrap(), target);
    }

    #[test]
    fn test_ln_idempotent() {
        let tmpdir = TempDir::new().unwrap();
        let target = tmpdir.path().join("target.txt");
        let link = tmpdir.path().join("link.txt");

        fs::write(&target, "hello").unwrap();

        // Create symlink twice - should succeed both times
        ln(target.to_str().unwrap(), link.to_str().unwrap()).unwrap();
        ln(target.to_str().unwrap(), link.to_str().unwrap()).unwrap();

        assert!(link.is_symlink());
        assert_eq!(fs::read_link(&link).unwrap(), target);
    }

    #[test]
    fn test_ln_updates_existing_link() {
        let tmpdir = TempDir::new().unwrap();
        let target1 = tmpdir.path().join("target1.txt");
        let target2 = tmpdir.path().join("target2.txt");
        let link = tmpdir.path().join("link.txt");

        fs::write(&target1, "first").unwrap();
        fs::write(&target2, "second").unwrap();

        // Create link to target1
        ln(target1.to_str().unwrap(), link.to_str().unwrap()).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), target1);

        // Update link to target2
        ln(target2.to_str().unwrap(), link.to_str().unwrap()).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), target2);
    }

    #[test]
    fn test_ln_to_nonexistent_target() {
        let tmpdir = TempDir::new().unwrap();
        let target = tmpdir.path().join("nonexistent");
        let link = tmpdir.path().join("link.txt");

        // Symlinks can point to nonexistent targets (dangling symlinks)
        ln(target.to_str().unwrap(), link.to_str().unwrap()).unwrap();

        assert!(link.is_symlink());
        assert!(!link.exists()); // Dangling symlink
    }

    #[test]
    fn test_ln_replaces_regular_file() {
        let tmpdir = TempDir::new().unwrap();
        let target = tmpdir.path().join("target.txt");
        let link = tmpdir.path().join("link.txt");

        fs::write(&target, "target").unwrap();
        fs::write(&link, "was a file").unwrap(); // Regular file at link path

        // ln should replace the regular file with a symlink
        ln(target.to_str().unwrap(), link.to_str().unwrap()).unwrap();

        assert!(link.is_symlink());
        assert_eq!(fs::read_link(&link).unwrap(), target);
    }

    // ==================== mknod tests ====================
    // FIFO (named pipe) can be created without root, char devices need root

    #[test]
    fn test_mknod_creates_fifo() {
        // FIFO doesn't require root - tests the mknod logic
        let tmpdir = TempDir::new().unwrap();
        let fifopath = tmpdir.path().join("test_fifo");

        mknod(fifopath.to_str().unwrap(), SFlag::S_IFIFO, 0, 0).unwrap();

        assert!(fifopath.exists());
        let meta = fs::metadata(&fifopath).unwrap();
        assert!(meta.file_type().is_fifo());
    }

    #[test]
    #[serial]  // umask is process-global
    fn test_mknod_fifo_permissions() {
        let tmpdir = TempDir::new().unwrap();
        let fifopath = tmpdir.path().join("test_fifo_perm");

        mknod(fifopath.to_str().unwrap(), SFlag::S_IFIFO, 0, 0).unwrap();

        let meta = fs::metadata(&fifopath).unwrap();
        let mode = meta.mode() & 0o777;
        assert_eq!(mode, 0o666);
    }

    #[test]
    fn test_mknod_replaces_existing_with_fifo() {
        let tmpdir = TempDir::new().unwrap();
        let fifopath = tmpdir.path().join("test_replace_fifo");

        // Create a regular file first
        fs::write(&fifopath, "placeholder").unwrap();
        assert!(fifopath.is_file());

        // mknod should replace it with a FIFO
        mknod(fifopath.to_str().unwrap(), SFlag::S_IFIFO, 0, 0).unwrap();

        let meta = fs::metadata(&fifopath).unwrap();
        assert!(meta.file_type().is_fifo());
    }

    #[test]
    #[serial]  // umask is process-global
    fn test_mknod_umask_not_applied() {
        let tmpdir = TempDir::new().unwrap();
        let fifopath = tmpdir.path().join("test_umask_fifo");

        // Set a restrictive umask
        let old_umask = stat::umask(Mode::from_bits_truncate(0o077));

        // Create FIFO - should get exact permissions despite umask
        mknod(fifopath.to_str().unwrap(), SFlag::S_IFIFO, 0, 0).unwrap();

        // Restore umask
        stat::umask(old_umask);

        let meta = fs::metadata(&fifopath).unwrap();
        let mode = meta.mode() & 0o777;
        assert_eq!(mode, 0o666, "umask should not affect mknod permissions");
    }

    // Char device tests - require root, will rerun with sudo if needed
    #[test]
    #[serial]
    fn test_mknod_creates_char_device() {
        require_root();

        let tmpdir = TempDir::new().unwrap();
        let devpath = tmpdir.path().join("test_null");

        mknod(devpath.to_str().unwrap(), SFlag::S_IFCHR, 1, 3).unwrap();

        let meta = fs::metadata(&devpath).unwrap();
        assert!(meta.file_type().is_char_device());
    }

    // ==================== error path tests ====================
    // These tests trigger the .with_context() closures for coverage

    #[test]
    fn test_ln_error_nonexistent_parent() {
        // symlinkat fails when parent directory doesn't exist
        let result = ln("/target", "/nonexistent/dir/link");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("ln"), "error should mention ln: {}", err);
    }

    #[test]
    fn test_mknod_error_nonexistent_parent() {
        // mknod fails when parent directory doesn't exist
        let result = mknod("/nonexistent/dir/fifo", SFlag::S_IFIFO, 0, 0);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("mknod"), "error should mention mknod: {}", err);
    }

}
