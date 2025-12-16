// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use crate::coreutils::{ln, mknod};
use anyhow::{Context, Result};
use nix::mount::{self, MsFlags};
use nix::sys::stat;
use std::fs;
use std::path::Path;

// Simplified helper: perform mount only if target not already mounted
fn mount(
    source: &str,
    target: &str,
    fstype: &str,
    flags: MsFlags,
    data: Option<&str>,
) -> Result<()> {
    if !is_mounted(target) {
        mount::mount(Some(source), target, Some(fstype), flags, data)
            .with_context(|| format!("Failed to mount {source} on {target}"))?;
    }
    Ok(())
}

fn is_mounted(path: &str) -> bool {
    fs::read_to_string("/proc/mounts")
        .map(|mounts| {
            mounts.lines().any(|line| {
                // Field 2 is mountpoint. Avoid substring match (/dev vs /dev/pts).
                line.split_whitespace().nth(1) == Some(path)
            })
        })
        .unwrap_or(false)
}

fn fs_available(fs: &str) -> bool {
    fs::read_to_string("/proc/filesystems")
        .map(|filesystems| filesystems.lines().any(|line| line.contains(fs)))
        .unwrap_or(false)
}

pub fn readonly(target: &str) -> Result<()> {
    let flags = MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_RDONLY | MsFlags::MS_REMOUNT;
    mount::mount(None::<&str>, target, None::<&str>, flags, None::<&str>)
        .with_context(|| format!("Failed to remount {target} readonly"))
}

fn mount_if(
    fstype: &str,
    source: &str,
    target: &str,
    flags: MsFlags,
    data: Option<&str>,
) -> Result<()> {
    if fs_available(fstype) && Path::new(target).exists() && !is_mounted(target) {
        mount(source, target, fstype, flags, data)?;
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
    let common = MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV | MsFlags::MS_RELATIME;
    mount("proc", "/proc", "proc", common, None)?;
    let dev_flags = MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_RELATIME; // allow device nodes
    mount("dev", "/dev", "devtmpfs", dev_flags, Some("mode=0755"))?;
    mount("sysfs", "/sys", "sysfs", common, None)?;
    mount("run", "/run", "tmpfs", common, Some("mode=0755"))?;
    let tmp_flags = MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_RELATIME;
    mount("tmpfs", "/tmp", "tmpfs", tmp_flags, None)?;
    mount_if(
        "securityfs",
        "securityfs",
        "/sys/kernel/security",
        common,
        None,
    )?;
    mount_if(
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
    use mktemp::Temp;
    use nix::unistd::Uid;
    use std::env;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    fn rerun_with_sudo() {
        let args: Vec<String> = env::args().collect();
        let output = Command::new("sudo").args(&args).status();
        match output {
            Ok(output) => {
                if output.success() {
                    println!("running with sudo")
                } else {
                    panic!("not running with sudo")
                }
            }
            Err(e) => panic!("Failed to escalate privileges: {e:?}"),
        }
    }

    fn cleanup_path<P: AsRef<Path>>(path: P) {
        let path = path.as_ref();
        if path.exists() {
            if path.is_dir() {
                let _ = fs::remove_dir_all(path);
            } else {
                let _ = fs::remove_file(path);
            }
        }
    }

    #[test]
    fn test_ln_dir() {
        let target = Temp::new_dir().unwrap();
        let linkpath = Temp::new_dir().unwrap();
        cleanup_path(&linkpath);
        let src = target.to_str().unwrap();
        let dst = linkpath.to_str().unwrap();
        ln(src, dst).expect("Failed to create symbolic link");
        assert!(Path::new(dst).exists());
        cleanup_path(target);
        cleanup_path(linkpath);
    }

    #[test]
    fn test_ln_file() {
        let target = Temp::new_file().unwrap();
        let linkpath = Temp::new_file().unwrap();
        fs::write(&target, "test").expect("Failed to create test file");
        cleanup_path(&linkpath);
        let src = target.to_str().unwrap();
        let dst = linkpath.to_str().unwrap();
        ln(src, dst).expect("Failed to create symbolic link");
        assert!(Path::new(dst).exists());
        cleanup_path(target);
        cleanup_path(linkpath);
    }

    #[test]
    fn test_mknod() {
        if !Uid::effective().is_root() {
            return rerun_with_sudo();
        }
        let device = "/tmp/test_node";
        if Path::new(device).exists() {
            cleanup_path(device);
        }
        mknod(device, stat::SFlag::S_IFCHR, 1, 3).expect("Failed to create device node");
        assert!(Path::new(device).exists());
        cleanup_path(device);
    }

    #[test]
    fn test_is_mounted() {
        assert!(is_mounted("/"));
        assert!(!is_mounted("/nonexistent"));
    }

    #[test]
    fn test_is_mounted_exact_match() {
        // Regression test for substring match bug
        // /dev/pts should NOT match when checking /dev
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "devpts /dev/pts devpts rw 0 0").unwrap();
        writeln!(tmp, "tmpfs /dev/shm tmpfs rw 0 0").unwrap();
        writeln!(tmp, "proc /proc proc rw 0 0").unwrap();

        let content = std::fs::read_to_string(tmp.path()).unwrap();

        // Test exact matching by simulating is_mounted logic
        let is_dev_mounted = content
            .lines()
            .any(|line| line.split_whitespace().nth(1) == Some("/dev"));

        let is_dev_pts_mounted = content
            .lines()
            .any(|line| line.split_whitespace().nth(1) == Some("/dev/pts"));

        assert!(
            !is_dev_mounted,
            "/dev should NOT match when only /dev/pts is mounted"
        );
        assert!(is_dev_pts_mounted, "/dev/pts should match");
    }

    #[test]
    fn test_fs_available() {
        assert!(fs_available("proc"));
        assert!(fs_available("sysfs"));
        assert!(!fs_available("nonexistent_fs"));
    }
}
