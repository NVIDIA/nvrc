use crate::coreutils::{ln, mknod};
use anyhow::{Context, Result};
use nix::mount::{self, MsFlags};
use nix::sys::stat;
use std::fs;
use std::path::Path;

const COMMON_FLAGS: MsFlags = MsFlags::MS_NOSUID
    .union(MsFlags::MS_NOEXEC)
    .union(MsFlags::MS_NODEV)
    .union(MsFlags::MS_RELATIME);
const DEV_FLAGS: MsFlags = MsFlags::MS_NOSUID
    .union(MsFlags::MS_NOEXEC)
    .union(MsFlags::MS_RELATIME);
const TMP_FLAGS: MsFlags = MsFlags::MS_NOSUID
    .union(MsFlags::MS_NODEV)
    .union(MsFlags::MS_RELATIME);
const READONLY_FLAGS: MsFlags = MsFlags::MS_NOSUID
    .union(MsFlags::MS_NODEV)
    .union(MsFlags::MS_RDONLY)
    .union(MsFlags::MS_REMOUNT);

const MEM_MAJOR: u64 = 1;
const NULL_MINOR: u64 = 3;
const ZERO_MINOR: u64 = 5;
const RANDOM_MINOR: u64 = 8;
const URANDOM_MINOR: u64 = 9;

const PROC_MOUNTS: &str = "/proc/mounts";
const PROC_FILESYSTEMS: &str = "/proc/filesystems";
const SECURITY_FS_PATH: &str = "/sys/kernel/security";
const EFIVARFS_PATH: &str = "/sys/firmware/efi/efivars";

fn mount(
    source: &str,
    target: &str,
    fstype: &str,
    flags: MsFlags,
    data: Option<&str>,
) -> Result<()> {
    if !is_mounted(target) {
        mount::mount(Some(source), target, Some(fstype), flags, data)
            .with_context(|| format!("Failed to mount {} on {}", source, target))
    } else {
        Ok(())
    }
}

fn is_mounted(path: &str) -> bool {
    let proc_mounts_path = Path::new(PROC_MOUNTS);
    if proc_mounts_path.exists() {
        if let Ok(mounts) = fs::read_to_string(proc_mounts_path) {
            return mounts.lines().any(|line| line.contains(path));
        }
    }
    false
}

fn fs_available(fs: &str) -> bool {
    let path = Path::new(PROC_FILESYSTEMS);
    if path.exists() {
        if let Ok(filesystems) = fs::read_to_string(path) {
            return filesystems.lines().any(|line| line.contains(fs));
        }
    }
    false
}

pub fn readonly(target: &str) -> Result<()> {
    mount::mount(
        None::<&str>,
        target,
        None::<&str>,
        READONLY_FLAGS,
        None::<&str>,
    )
    .with_context(|| format!("Failed to remount {} readonly", target))
}

fn mount_conditional(
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

fn create_device_symlinks() -> Result<()> {
    let symlinks = [
        ("/proc/kcore", "/dev/core"),
        ("/proc/self/fd", "/dev/fd"),
        ("/proc/self/fd/0", "/dev/stdin"),
        ("/proc/self/fd/1", "/dev/stdout"),
        ("/proc/self/fd/2", "/dev/stderr"),
    ];
    for (source, target) in symlinks {
        ln(source, target)?;
    }
    Ok(())
}

fn create_device_nodes() -> Result<()> {
    let devices = [
        ("/dev/null", NULL_MINOR),
        ("/dev/zero", ZERO_MINOR),
        ("/dev/random", RANDOM_MINOR),
        ("/dev/urandom", URANDOM_MINOR),
    ];
    for (path, minor) in devices {
        mknod(path, stat::SFlag::S_IFCHR, MEM_MAJOR, minor)?;
    }
    Ok(())
}

pub fn setup() -> Result<()> {
    mount("proc", "/proc", "proc", COMMON_FLAGS, None)?;
    mount("dev", "/dev", "devtmpfs", DEV_FLAGS, Some("mode=0755"))?;
    mount("sysfs", "/sys", "sysfs", COMMON_FLAGS, None)?;
    mount("run", "/run", "tmpfs", COMMON_FLAGS, Some("mode=0755"))?;
    mount("tmpfs", "/tmp", "tmpfs", TMP_FLAGS, None)?;
    mount_conditional(
        "securityfs",
        "securityfs",
        SECURITY_FS_PATH,
        COMMON_FLAGS,
        None,
    )?;
    mount_conditional("efivarfs", "efivarfs", EFIVARFS_PATH, COMMON_FLAGS, None)?;
    create_device_symlinks()?;
    create_device_nodes()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mktemp::Temp;
    use nix::unistd::Uid;
    use std::env;
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
            Err(e) => {
                panic!("Failed to escalate privileges: {e:?}")
            }
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
        assert!(is_mounted("/dev"));
    }

    #[test]
    fn test_fs_available() {
        assert!(fs_available("proc"));
        assert!(fs_available("sysfs"));
        assert!(!fs_available("nonexistent_fs"));
    }

    #[test]
    fn test_constants() {
        assert_eq!(MEM_MAJOR, 1);
        assert_eq!(NULL_MINOR, 3);
        assert_eq!(ZERO_MINOR, 5);
        assert_eq!(RANDOM_MINOR, 8);
        assert_eq!(URANDOM_MINOR, 9);
    }
}
