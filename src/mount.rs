use anyhow::{Context, Result};
use nix::mount; //::{mount, MsFlags};
use nix::mount::MsFlags;
use nix::sys::stat;

use std::fs;
use std::path::Path;

use crate::coreutils::{ln, mknod};

/// Common mount flags for most filesystems
const COMMON_FLAGS: MsFlags = MsFlags::MS_NOSUID
    .union(MsFlags::MS_NOEXEC)
    .union(MsFlags::MS_NODEV)
    .union(MsFlags::MS_RELATIME);

/// Mount flags for device filesystems
const DEV_FLAGS: MsFlags = MsFlags::MS_NOSUID
    .union(MsFlags::MS_NOEXEC)
    .union(MsFlags::MS_RELATIME);

/// Mount flags for temporary filesystems
const TMP_FLAGS: MsFlags = MsFlags::MS_NOSUID
    .union(MsFlags::MS_NODEV)
    .union(MsFlags::MS_RELATIME);

/// Readonly remount flags
const READONLY_FLAGS: MsFlags = MsFlags::MS_NOSUID
    .union(MsFlags::MS_NODEV)
    .union(MsFlags::MS_RDONLY)
    .union(MsFlags::MS_REMOUNT);

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
        // Skip mounting if already mounted
        Ok(())
    }
}

fn is_mounted(path: &str) -> bool {
    let proc_mounts_path = Path::new("/proc/mounts");
    if proc_mounts_path.exists() {
        if let Ok(mounts) = fs::read_to_string(proc_mounts_path) {
            return mounts.lines().any(|line| line.contains(path));
        }
    }
    false
}

fn fs_available(fs: &str) -> bool {
    let path = Path::new("/proc/filesystems");
    if path.exists() {
        if let Ok(filesystems) = fs::read_to_string(path) {
            return filesystems.lines().any(|line| line.contains(fs));
        }
    }
    false
}

/// Remount a filesystem as read-only.
pub fn readonly(target: &str) -> Result<()> {
    // TODO how to mount it MsFlags::MS_NOEXEC
    //MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_RDONLY,
    mount::mount(
        None::<&str>,
        target,
        None::<&str>,
        READONLY_FLAGS,
        None::<&str>,
    )
    .with_context(|| format!("Failed to remount {} readonly", target))
}
/// Set up the basic filesystem hierarchy.
pub fn setup() -> Result<()> {
    mount("proc", "/proc", "proc", COMMON_FLAGS, None)?;
    mount("dev", "/dev", "devtmpfs", DEV_FLAGS, Some("mode=0755"))?;
    mount("sysfs", "/sys", "sysfs", COMMON_FLAGS, None)?;
    mount("run", "/run", "tmpfs", COMMON_FLAGS, Some("mode=0755"))?;
    mount("tmpfs", "/tmp", "tmpfs", TMP_FLAGS, None)?;

    if fs_available("securityfs")
        && Path::new("/sys/kernel/security").exists()
        && !is_mounted("/sys/kernel/security")
    {
        mount(
            "securityfs",
            "/sys/kernel/security",
            "securityfs",
            COMMON_FLAGS,
            None,
        )?;
    }

    if fs_available("efivarfs")
        && Path::new("/sys/firmware/efi/efivars").exists()
        && !is_mounted("/sys/firmware/efi/efivars")
    {
        mount(
            "efivarfs",
            "/sys/firmware/efi/efivars",
            "efivarfs",
            COMMON_FLAGS,
            None,
        )?;
    }

    ln("/proc/kcore", "/dev/core")?;
    ln("/proc/self/fd", "/dev/fd")?;
    ln("/proc/self/fd/0", "/dev/stdin")?;
    ln("/proc/self/fd/1", "/dev/stdout")?;
    ln("/proc/self/fd/2", "/dev/stderr")?;

    mknod("/dev/null", stat::SFlag::S_IFCHR, 1, 3)?;
    mknod("/dev/zero", stat::SFlag::S_IFCHR, 1, 5)?;
    mknod("/dev/random", stat::SFlag::S_IFCHR, 1, 8)?;
    mknod("/dev/urandom", stat::SFlag::S_IFCHR, 1, 9)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mktemp::Temp;
    use nix::unistd::Uid;
    use std::env;
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

    #[test]
    fn test_ln_dir() {
        let target = Temp::new_dir().unwrap();
        let linkpath = Temp::new_dir().unwrap();

        fs::remove_dir(&linkpath).expect("Failed to remove test link");

        let src = target.to_str().unwrap();
        let dst = linkpath.to_str().unwrap();
        ln(src, dst).expect("Failed to create symbolic link");

        assert!(Path::new(dst).exists());
        fs::remove_dir(target).expect("Failed to remove test directory");
        fs::remove_file(linkpath).expect("Failed to remove test link");
    }
    #[test]
    fn test_ln_file() {
        let target = Temp::new_file().unwrap();
        let linkpath = Temp::new_file().unwrap();

        fs::write(&target, "test").expect("Failed to create test file");

        fs::remove_file(&linkpath).expect("Failed to remove test directory");

        let src = target.to_str().unwrap();
        let dst = linkpath.to_str().unwrap();

        ln(src, dst).expect("Failed to create symbolic link");

        assert!(Path::new(dst).exists());
        fs::remove_file(target).expect("Failed to remove test file");
        fs::remove_file(linkpath).expect("Failed to remove test link");
    }

    #[test]
    fn test_mknod() {
        if !Uid::effective().is_root() {
            // Re-run the test with sudo
            return rerun_with_sudo();
        }

        let device = "/tmp/test_node";
        if Path::new(device).exists() {
            fs::remove_file(device).expect("Failed to remove test node");
        }
        mknod(device, stat::SFlag::S_IFCHR, 1, 3).expect("Failed to create device node");
        assert!(Path::new(device).exists());
        fs::remove_file(device).expect("Failed to remove test node");
    }

    #[test]
    fn test_is_mounted() {
        assert!(is_mounted("/"));
        assert!(!is_mounted("/nonexistent"));
        assert!(is_mounted("/dev"));
    }
    #[allow(dead_code)]
    fn test_mount() {
        //if !Uid::effective().is_root() {
        // Re-run the test with sudo
        //    return rerun_with_sudo();
        //}

        let source = "tmpfs";
        let target = Temp::new_file().unwrap();
        let fstype = "tmpfs";
        let flags = MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV;
        let data = Some("mode=0755");

        let dst = target.to_str().unwrap();

        mount(source, dst, fstype, flags, data).expect("Failed to mount filesystem");
        assert!(is_mounted(dst));
        fs::remove_dir(target).expect("Failed to remove test mount");
    }
}
