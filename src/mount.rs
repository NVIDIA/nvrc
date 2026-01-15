// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Filesystem setup for the minimal init environment.

use crate::macros::ResultExt;
use nix::mount::MsFlags;
use std::fs;
use std::path::Path;

/// Mount a filesystem. Errors if mount fails.
fn mount(source: &str, target: &str, fstype: &str, flags: MsFlags, data: Option<&str>) {
    nix::mount::mount(Some(source), target, Some(fstype), flags, data)
        .or_panic(format_args!("mount {source} on {target}"));
}

/// Remount a filesystem as read-only.
/// Security hardening: prevents writes to the root filesystem after init,
/// reducing attack surface in the confidential VM.
pub fn readonly(target: &str) {
    let flags = MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_RDONLY | MsFlags::MS_REMOUNT;
    nix::mount::mount(None::<&str>, target, None::<&str>, flags, None::<&str>)
        .or_panic(format_args!("remount {target} readonly"));
}

/// Check if a filesystem type is available in the kernel.
fn fs_available(filesystems: &str, fstype: &str) -> bool {
    filesystems.lines().any(|line| line.contains(fstype))
}

/// Mount optional filesystem if the fstype is available AND the target exists.
/// Used for securityfs and efivarfs that may not be present on all kernels.
fn mount_optional(filesystems: &str, source: &str, target: &str, fstype: &str, flags: MsFlags) {
    if fs_available(filesystems, fstype) && Path::new(target).exists() {
        mount(source, target, fstype, flags, None);
    }
}

/// Set up the minimal filesystem hierarchy required for GPU initialization.
/// Creates /proc, /dev, /sys, /run, /tmp mounts.
/// devtmpfs automatically creates standard device nodes; symlinks
/// (/dev/stdin, /dev/stdout, /dev/stderr, /dev/fd, /dev/core) are
/// created later by kata-agent.
pub fn setup() {
    setup_at("")
}

/// Internal: setup with configurable root path (for testing with temp directories).
fn setup_at(root: &str) {
    let common = MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV | MsFlags::MS_RELATIME;

    mount("proc", &format!("{root}/proc"), "proc", common, None);

    // devtmpfs automatically creates /dev/null, /dev/zero, /dev/random, /dev/urandom
    // Symlinks (/dev/stdin, /dev/stdout, /dev/stderr, /dev/fd, /dev/core) are created by kata-agent
    let dev_flags = MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_RELATIME;
    mount(
        "dev",
        &format!("{root}/dev"),
        "devtmpfs",
        dev_flags,
        Some("mode=0755"),
    );

    mount("sysfs", &format!("{root}/sys"), "sysfs", common, None);
    mount(
        "run",
        &format!("{root}/run"),
        "tmpfs",
        common,
        Some("mode=0755"),
    );

    let tmp_flags = MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_RELATIME;
    mount("tmpfs", &format!("{root}/tmp"), "tmpfs", tmp_flags, None);

    // Read once for all optional mounts
    let filesystems = fs::read_to_string("/proc/filesystems").unwrap_or_default();

    mount_optional(
        &filesystems,
        "securityfs",
        &format!("{root}/sys/kernel/security"),
        "securityfs",
        common,
    );
    mount_optional(
        &filesystems,
        "efivarfs",
        &format!("{root}/sys/firmware/efi/efivars"),
        "efivarfs",
        common,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;

    // === fs_available tests ===

    #[test]
    fn test_fs_available() {
        let filesystems = fs::read_to_string("/proc/filesystems").unwrap();
        assert!(fs_available(&filesystems, "proc"));
        assert!(fs_available(&filesystems, "sysfs"));
        assert!(fs_available(&filesystems, "tmpfs"));
        assert!(!fs_available(&filesystems, "nonexistent_fs"));
    }

    #[test]
    fn test_fs_available_empty() {
        assert!(!fs_available("", "proc"));
        assert!(!fs_available("", "tmpfs"));
    }

    // === mount_optional tests ===

    #[test]
    fn test_mount_optional_target_not_exists() {
        // When target path doesn't exist, should be no-op
        let filesystems = "nodev tmpfs\n";
        mount_optional(
            filesystems,
            "tmpfs",
            "/nonexistent/path",
            "tmpfs",
            MsFlags::empty(),
        );
    }

    // === Error path tests ===

    #[test]
    fn test_mount_fails_nonexistent_target() {
        use std::panic;

        let result = panic::catch_unwind(|| {
            mount(
                "tmpfs",
                "/nonexistent/mount/point",
                "tmpfs",
                MsFlags::empty(),
                None,
            );
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_readonly_fails_nonexistent() {
        use std::panic;

        let result = panic::catch_unwind(|| {
            readonly("/nonexistent/path");
        });
        assert!(result.is_err());
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
        setup_at(root);

        // devtmpfs creates these automatically
        assert!(Path::new(&format!("{root}/dev/null")).exists());
        assert!(Path::new(&format!("{root}/dev/zero")).exists());
        assert!(Path::new(&format!("{root}/dev/random")).exists());
        assert!(Path::new(&format!("{root}/dev/urandom")).exists());

        // Cleanup: unmount in reverse order
        for dir in ["tmp", "run", "sys", "dev", "proc"] {
            let _ = umount(format!("{root}/{dir}").as_str());
        }
    }
}
