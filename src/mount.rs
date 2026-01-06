// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Filesystem setup for the minimal init environment.
//!
//! Coverage note: ~80% is the safe maximum. `setup()` mounts filesystems
//! and can only be tested in ephemeral VMs.

use crate::coreutils::{ln, mknod};
use anyhow::{Context, Result};
use nix::mount::{self, MsFlags};
use nix::sys::stat;
use std::fs;
use std::path::Path;

/// Check if path is mounted (exact match on mountpoint, not substring).
/// Uses exact field matching to avoid false positives like "/dev" matching "/dev/pts".
/// Public for fuzzing: parses arbitrary /proc/mounts content.
pub fn is_mounted_in(mounts: &str, path: &str) -> bool {
    mounts
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some(path))
}

/// Check if a filesystem type is available in the kernel.
/// Some filesystems (securityfs, efivarfs) may not be present in all kernels.
fn fs_available_in(filesystems: &str, fs: &str) -> bool {
    filesystems.lines().any(|line| line.contains(fs))
}

/// Mount filesystem only if not already mounted.
/// Idempotent: safe to call multiple times. Uses pre-read mounts snapshot
/// to avoid TOCTOU races between check and mount.
fn mount_cached(
    mounts: &str,
    source: &str,
    target: &str,
    fstype: &str,
    flags: MsFlags,
    data: Option<&str>,
) -> Result<()> {
    if !is_mounted_in(mounts, target) {
        mount::mount(Some(source), target, Some(fstype), flags, data)
            .with_context(|| format!("Failed to mount {source} on {target}"))?;
    }
    Ok(())
}

/// Remount a filesystem as read-only.
/// Security hardening: prevents writes to the root filesystem after init,
/// reducing attack surface in the confidential VM.
pub fn readonly(target: &str) -> Result<()> {
    let flags = MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_RDONLY | MsFlags::MS_REMOUNT;
    mount::mount(None::<&str>, target, None::<&str>, flags, None::<&str>)
        .with_context(|| format!("Failed to remount {target} readonly"))
}

/// Mount filesystem only if the fstype is available AND the target exists.
/// Used for optional filesystems like securityfs and efivarfs that may not
/// be present on all systems or kernel configurations.
fn mount_if_cached(
    mounts: &str,
    filesystems: &str,
    fstype: &str,
    source: &str,
    target: &str,
    flags: MsFlags,
    data: Option<&str>,
) -> Result<()> {
    if fs_available_in(filesystems, fstype) && Path::new(target).exists() {
        mount_cached(mounts, source, target, fstype, flags, data)?;
    }
    Ok(())
}

/// Create /dev symlinks pointing to /proc entries.
/// Standard Unix convention: /dev/stdin, /dev/stdout, /dev/stderr should
/// exist for programs that expect them. /dev/fd provides access to open
/// file descriptors via /proc/self/fd.
fn proc_symlinks(root: &str) -> Result<()> {
    for (src, dst) in [
        ("/proc/kcore", "dev/core"),
        ("/proc/self/fd", "dev/fd"),
        ("/proc/self/fd/0", "dev/stdin"),
        ("/proc/self/fd/1", "dev/stdout"),
        ("/proc/self/fd/2", "dev/stderr"),
    ] {
        ln(src, &format!("{root}/{dst}"))?;
    }
    Ok(())
}

/// Create essential /dev device nodes for basic I/O.
/// These character devices are fundamental Unix primitives:
/// - /dev/null: discard output, read returns EOF
/// - /dev/zero: infinite stream of zeros
/// - /dev/random, /dev/urandom: cryptographic randomness
fn device_nodes(root: &str) -> Result<()> {
    for (path, minor) in [
        ("dev/null", 3u64),
        ("dev/zero", 5u64),
        ("dev/random", 8u64),
        ("dev/urandom", 9u64),
    ] {
        mknod(&format!("{root}/{path}"), stat::SFlag::S_IFCHR, 1, minor)?; // major 1 = memory devices
    }
    Ok(())
}

/// Set up the minimal filesystem hierarchy required for GPU initialization.
/// Creates /proc, /dev, /sys, /run, /tmp mounts and essential device nodes.
/// Snapshot-based: reads mount state once to avoid TOCTOU races.
pub fn setup() -> Result<()> {
    setup_at("")
}

/// Internal: setup with configurable root path (for testing with temp directories).
fn setup_at(root: &str) -> Result<()> {
    // Snapshot mount state once - consistent view, no TOCTOU
    let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
    let filesystems = fs::read_to_string("/proc/filesystems").unwrap_or_default();

    let common = MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV | MsFlags::MS_RELATIME;
    mount_cached(
        &mounts,
        "proc",
        &format!("{root}/proc"),
        "proc",
        common,
        None,
    )?;
    let dev_flags = MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_RELATIME;
    mount_cached(
        &mounts,
        "dev",
        &format!("{root}/dev"),
        "devtmpfs",
        dev_flags,
        Some("mode=0755"),
    )?;
    mount_cached(
        &mounts,
        "sysfs",
        &format!("{root}/sys"),
        "sysfs",
        common,
        None,
    )?;
    mount_cached(
        &mounts,
        "run",
        &format!("{root}/run"),
        "tmpfs",
        common,
        Some("mode=0755"),
    )?;
    let tmp_flags = MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_RELATIME;
    mount_cached(
        &mounts,
        "tmpfs",
        &format!("{root}/tmp"),
        "tmpfs",
        tmp_flags,
        None,
    )?;
    mount_if_cached(
        &mounts,
        &filesystems,
        "securityfs",
        "securityfs",
        &format!("{root}/sys/kernel/security"),
        common,
        None,
    )?;
    mount_if_cached(
        &mounts,
        &filesystems,
        "efivarfs",
        "efivarfs",
        &format!("{root}/sys/firmware/efi/efivars"),
        common,
        None,
    )?;
    proc_symlinks(root)?;
    device_nodes(root)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use std::fs;

    // === Safe parsing function tests ===

    #[test]
    fn test_is_mounted_in() {
        let mounts = fs::read_to_string("/proc/mounts").unwrap();
        assert!(is_mounted_in(&mounts, "/"));
        assert!(!is_mounted_in(&mounts, "/nonexistent"));
    }

    #[test]
    fn test_is_mounted_exact_match() {
        // /dev/pts mounted should NOT match /dev (substring matching bug fix)
        let mounts = "devpts /dev/pts devpts rw 0 0\ntmpfs /tmp tmpfs rw 0 0\n";
        assert!(!is_mounted_in(mounts, "/dev"));
        assert!(is_mounted_in(mounts, "/dev/pts"));
        assert!(is_mounted_in(mounts, "/tmp"));
    }

    #[test]
    fn test_is_mounted_empty() {
        assert!(!is_mounted_in("", "/"));
        assert!(!is_mounted_in("", "/dev"));
    }

    #[test]
    fn test_fs_available_in() {
        let filesystems = fs::read_to_string("/proc/filesystems").unwrap();
        assert!(fs_available_in(&filesystems, "proc"));
        assert!(fs_available_in(&filesystems, "sysfs"));
        assert!(!fs_available_in(&filesystems, "nonexistent_fs"));
    }

    #[test]
    fn test_fs_available_empty() {
        assert!(!fs_available_in("", "proc"));
    }

    // === mount_cached tests (safe: no-op when already mounted) ===

    #[test]
    fn test_mount_cached_already_mounted() {
        // When target is already in mounts, mount_cached is a no-op
        let mounts = "proc /proc proc rw 0 0\n";
        let result = mount_cached(mounts, "proc", "/proc", "proc", MsFlags::empty(), None);
        assert!(result.is_ok());
    }

    // === mount_if_cached tests (safe: no-op when conditions not met) ===

    #[test]
    fn test_mount_if_cached_fs_not_available() {
        // When filesystem is not available, should be no-op
        let mounts = "";
        let filesystems = "nodev tmpfs\n";
        let result = mount_if_cached(
            mounts,
            filesystems,
            "nonexistent_fs",
            "src",
            "/tmp", // exists but fs not available
            MsFlags::empty(),
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_mount_if_cached_target_not_exists() {
        // When target path doesn't exist, should be no-op
        let mounts = "";
        let filesystems = "nodev tmpfs\n";
        let result = mount_if_cached(
            mounts,
            filesystems,
            "tmpfs",
            "src",
            "/nonexistent/path",
            MsFlags::empty(),
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_mount_if_cached_already_mounted() {
        // When already mounted, should be no-op
        let mounts = "tmpfs /tmp tmpfs rw 0 0\n";
        let filesystems = "nodev tmpfs\n";
        let result = mount_if_cached(
            mounts,
            filesystems,
            "tmpfs",
            "tmpfs",
            "/tmp",
            MsFlags::empty(),
            None,
        );
        assert!(result.is_ok());
    }

    // === Error path tests (safe: mount fails, no changes made) ===

    #[test]
    fn test_mount_cached_fails_nonexistent_target() {
        let mounts = "";
        let err = mount_cached(
            mounts,
            "tmpfs",
            "/nonexistent/mount/point",
            "tmpfs",
            MsFlags::empty(),
            None,
        )
        .unwrap_err();
        // Should contain the mount target in error context
        assert!(
            err.to_string().contains("/nonexistent/mount/point"),
            "error should mention the path: {}",
            err
        );
    }

    #[test]
    fn test_readonly_fails_nonexistent() {
        let err = readonly("/nonexistent/path").unwrap_err();
        // Should contain the path in error context
        assert!(
            err.to_string().contains("/nonexistent/path"),
            "error should mention the path: {}",
            err
        );
    }

    // === Functions that need root but are safe ===

    #[test]
    fn test_proc_symlinks() {
        // These symlinks already exist on any Linux system.
        // ln() is idempotent - returns Ok if already correct.
        require_root();
        assert!(proc_symlinks("").is_ok());
    }

    #[test]
    fn test_device_nodes() {
        // mknod() removes existing nodes first, then recreates.
        // Safe: just recreates /dev/null, /dev/zero, etc. with same params.
        require_root();
        assert!(device_nodes("").is_ok());
    }

    // === setup_at() tests with temp directory ===

    #[test]
    fn test_setup_at_with_temp_root() {
        use nix::mount::umount;
        use tempfile::TempDir;

        require_root();

        let tmpdir = TempDir::new().unwrap();
        let root = tmpdir.path().to_str().unwrap();

        // Create required directories
        for dir in ["proc", "dev", "sys", "run", "tmp"] {
            fs::create_dir_all(format!("{root}/{dir}")).unwrap();
        }

        // Run setup_at with temp root
        let result = setup_at(root);
        assert!(result.is_ok(), "setup_at failed: {:?}", result);

        // Verify mounts happened
        let mounts = fs::read_to_string("/proc/mounts").unwrap();
        assert!(is_mounted_in(&mounts, &format!("{root}/run")));
        assert!(is_mounted_in(&mounts, &format!("{root}/tmp")));

        // Verify device nodes were created
        assert!(Path::new(&format!("{root}/dev/null")).exists());
        assert!(Path::new(&format!("{root}/dev/zero")).exists());

        // Verify symlinks were created
        assert!(Path::new(&format!("{root}/dev/stdin")).is_symlink());
        assert!(Path::new(&format!("{root}/dev/stdout")).is_symlink());

        // Cleanup: unmount in reverse order
        for dir in ["tmp", "run", "sys", "dev", "proc"] {
            let _ = umount(format!("{root}/{dir}").as_str());
        }
    }
}
