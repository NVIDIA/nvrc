// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use crate::coreutils::{ln, mknod};
use anyhow::{Context, Result};
use nix::mount::{self, MsFlags};
use nix::sys::stat;
use std::fs;
use std::path::Path;

/// Check if path is mounted (exact match on mountpoint, not substring)
fn is_mounted_in(mounts: &str, path: &str) -> bool {
    mounts
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some(path))
}

fn fs_available_in(filesystems: &str, fs: &str) -> bool {
    filesystems.lines().any(|line| line.contains(fs))
}

/// Mount if not already mounted (uses cached mounts snapshot)
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

pub fn readonly(target: &str) -> Result<()> {
    let flags = MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_RDONLY | MsFlags::MS_REMOUNT;
    mount::mount(None::<&str>, target, None::<&str>, flags, None::<&str>)
        .with_context(|| format!("Failed to remount {target} readonly"))
}

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

fn proc_symlinks() -> Result<()> {
    for (src, dst) in [
        ("/proc/kcore", "/dev/core"),
        ("/proc/self/fd", "/dev/fd"),
        ("/proc/self/fd/0", "/dev/stdin"),
        ("/proc/self/fd/1", "/dev/stdout"),
        ("/proc/self/fd/2", "/dev/stderr"),
    ] {
        ln(src, dst)?;
    }
    Ok(())
}

fn device_nodes() -> Result<()> {
    // (path, minor)
    for (path, minor) in [
        ("/dev/null", 3u64),
        ("/dev/zero", 5u64),
        ("/dev/random", 8u64),
        ("/dev/urandom", 9u64),
    ] {
        mknod(path, stat::SFlag::S_IFCHR, 1, minor)?; // major 1 for memory devices
    }
    Ok(())
}

pub fn setup() -> Result<()> {
    // Snapshot mount state once - consistent view, no TOCTOU
    let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
    let filesystems = fs::read_to_string("/proc/filesystems").unwrap_or_default();

    let common = MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV | MsFlags::MS_RELATIME;
    mount_cached(&mounts, "proc", "/proc", "proc", common, None)?;
    let dev_flags = MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_RELATIME;
    mount_cached(
        &mounts,
        "dev",
        "/dev",
        "devtmpfs",
        dev_flags,
        Some("mode=0755"),
    )?;
    mount_cached(&mounts, "sysfs", "/sys", "sysfs", common, None)?;
    mount_cached(&mounts, "run", "/run", "tmpfs", common, Some("mode=0755"))?;
    let tmp_flags = MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_RELATIME;
    mount_cached(&mounts, "tmpfs", "/tmp", "tmpfs", tmp_flags, None)?;
    mount_if_cached(
        &mounts,
        &filesystems,
        "securityfs",
        "securityfs",
        "/sys/kernel/security",
        common,
        None,
    )?;
    mount_if_cached(
        &mounts,
        &filesystems,
        "efivarfs",
        "efivarfs",
        "/sys/firmware/efi/efivars",
        common,
        None,
    )?;
    proc_symlinks()?;
    device_nodes()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_is_mounted_in() {
        let mounts = fs::read_to_string("/proc/mounts").unwrap();
        assert!(is_mounted_in(&mounts, "/"));
        assert!(!is_mounted_in(&mounts, "/nonexistent"));
    }

    #[test]
    fn test_is_mounted_exact_match() {
        // /dev/pts mounted should NOT match /dev
        let mounts = "devpts /dev/pts devpts rw 0 0\ntmpfs /tmp tmpfs rw 0 0\n";
        assert!(!is_mounted_in(mounts, "/dev"));
        assert!(is_mounted_in(mounts, "/dev/pts"));
        assert!(is_mounted_in(mounts, "/tmp"));
    }

    #[test]
    fn test_fs_available_in() {
        let filesystems = fs::read_to_string("/proc/filesystems").unwrap();
        assert!(fs_available_in(&filesystems, "proc"));
        assert!(fs_available_in(&filesystems, "sysfs"));
        assert!(!fs_available_in(&filesystems, "nonexistent_fs"));
    }
}
