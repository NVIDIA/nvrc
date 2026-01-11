// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{anyhow, Context, Result};
use nix::fcntl::AT_FDCWD;
use nix::sys::stat::{self, Mode, SFlag};
use nix::unistd::symlinkat;
use std::path::Path;

#[cfg(test)]
use serial_test::serial;
/// Create symbolic link from target to linkpath.
/// In production (PID 1 init), filesystem is fresh - any existing path is an error.
pub fn ln(target: &str, linkpath: &str) -> Result<()> {
    // In ephemeral VM as PID 1, nothing should exist at linkpath yet
    // If it does, something is wrong - fail fast
    let path = Path::new(linkpath);
    if path.exists() || path.is_symlink() {
        return Err(anyhow!(
            "Cannot create symlink at {} - path already exists (fresh filesystem expected)",
            linkpath
        ));
    }

    symlinkat(target, AT_FDCWD, linkpath).with_context(|| format!("ln {} -> {}", linkpath, target))
}

/// Create a character device node with desired major/minor.
/// In production (PID 1 init), filesystem is fresh - existing files are errors.
pub fn mknod(path: &str, kind: SFlag, major: u64, minor: u64) -> Result<()> {
    // Fail fast if file already exists (shouldn't happen in clean ephemeral VM)
    if Path::new(path).exists() {
        return Err(anyhow!(
            "Cannot create device node at {} - path already exists",
            path
        ));
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
    use crate::test_utils::require_root;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use tempfile::TempDir;

    // ==================== ln tests ====================

    #[test]
    fn test_ln_creates_symlink() {
        let tmpdir = TempDir::new().unwrap();
        let target = tmpdir.path().join("target.txt");
        let link = tmpdir.path().join("link.txt");

        // Create target file
        std::fs::write(&target, "hello").unwrap();

        // Create symlink
        ln(target.to_str().unwrap(), link.to_str().unwrap()).unwrap();

        // Verify symlink exists and points to target
        assert!(link.is_symlink());
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
    }

    #[test]
    fn test_ln_fails_if_exists() {
        let tmpdir = TempDir::new().unwrap();
        let target = tmpdir.path().join("target.txt");
        let link = tmpdir.path().join("link.txt");

        std::fs::write(&target, "hello").unwrap();

        // Create symlink once - succeeds
        ln(target.to_str().unwrap(), link.to_str().unwrap()).unwrap();

        // Try to create again - should fail (not idempotent in production)
        let result = ln(target.to_str().unwrap(), link.to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
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
    fn test_ln_fails_if_regular_file_exists() {
        let tmpdir = TempDir::new().unwrap();
        let target = tmpdir.path().join("target.txt");
        let link = tmpdir.path().join("link.txt");

        std::fs::write(&target, "target").unwrap();
        std::fs::write(&link, "was a file").unwrap(); // Regular file at link path

        // ln should fail if file exists (production behavior)
        let result = ln(target.to_str().unwrap(), link.to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    // ==================== mknod tests ====================
    // FIFO (named pipe) can be created without root, char devices need root

    #[test]
    #[serial] // umask is process-global
    fn test_mknod_creates_fifo() {
        // FIFO doesn't require root - tests the mknod logic
        let tmpdir = TempDir::new().unwrap();
        let fifopath = tmpdir.path().join("test_fifo");

        mknod(fifopath.to_str().unwrap(), SFlag::S_IFIFO, 0, 0).unwrap();

        assert!(fifopath.exists());
        let meta = std::fs::metadata(&fifopath).unwrap();
        assert!(meta.file_type().is_fifo());
    }

    #[test]
    #[serial] // umask is process-global
    fn test_mknod_fifo_permissions() {
        let tmpdir = TempDir::new().unwrap();
        let fifopath = tmpdir.path().join("test_fifo_perm");

        mknod(fifopath.to_str().unwrap(), SFlag::S_IFIFO, 0, 0).unwrap();

        let meta = std::fs::metadata(&fifopath).unwrap();
        let mode = meta.mode() & 0o777;
        assert_eq!(mode, 0o666);
    }

    #[test]
    #[serial] // umask is process-global
    fn test_mknod_fails_if_exists() {
        let tmpdir = TempDir::new().unwrap();
        let fifopath = tmpdir.path().join("test_replace_fifo");

        // Create a regular file first
        std::fs::write(&fifopath, "placeholder").unwrap();
        assert!(fifopath.is_file());

        // mknod should fail if file exists (production behavior)
        let result = mknod(fifopath.to_str().unwrap(), SFlag::S_IFIFO, 0, 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    #[serial] // umask is process-global
    fn test_mknod_umask_not_applied() {
        let tmpdir = TempDir::new().unwrap();
        let fifopath = tmpdir.path().join("test_umask_fifo");

        // Set a restrictive umask
        let old_umask = stat::umask(Mode::from_bits_truncate(0o077));

        // Create FIFO - should get exact permissions despite umask
        mknod(fifopath.to_str().unwrap(), SFlag::S_IFIFO, 0, 0).unwrap();

        // Restore umask
        stat::umask(old_umask);

        let meta = std::fs::metadata(&fifopath).unwrap();
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

        let meta = std::fs::metadata(&devpath).unwrap();
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
    #[serial] // umask is process-global
    fn test_mknod_error_nonexistent_parent() {
        // mknod fails when parent directory doesn't exist
        let result = mknod("/nonexistent/dir/fifo", SFlag::S_IFIFO, 0, 0);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("mknod"), "error should mention mknod: {}", err);
    }
}
