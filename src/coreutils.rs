// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{anyhow, Context, Result};
use nix::fcntl::AT_FDCWD;
use nix::unistd::symlinkat;
use std::path::Path;
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
