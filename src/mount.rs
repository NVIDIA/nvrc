use crate::coreutils::{ln, mknod, str_to_cstring, CoreUtilsError, Result, S_IFCHR};
use sc::syscall;

// Mount flags from <sys/mount.h>
const MS_RDONLY: usize = 1;
const MS_NOSUID: usize = 2;
const MS_NODEV: usize = 4;
const MS_NOEXEC: usize = 8;
const MS_REMOUNT: usize = 32;
const MS_RELATIME: usize = 1 << 21;

/// Performs a mount operation using a syscall.
fn mount(source: &str, target: &str, fstype: &str, flags: usize, data: Option<&str>) -> Result<()> {
    let mut source_buf = [0u8; 256];
    let mut target_buf = [0u8; 256];
    let mut fstype_buf = [0u8; 256];
    let mut data_buf = [0u8; 256];

    let source_ptr = str_to_cstring(source, &mut source_buf)?;
    let target_ptr = str_to_cstring(target, &mut target_buf)?;
    let fstype_ptr = str_to_cstring(fstype, &mut fstype_buf)?;
    let data_ptr = match data {
        Some(d) => str_to_cstring(d, &mut data_buf)?,
        None => core::ptr::null(),
    };

    let result = unsafe {
        syscall!(
            MOUNT,
            source_ptr as usize,
            target_ptr as usize,
            fstype_ptr as usize,
            flags,
            data_ptr as usize
        )
    } as isize;

    if result < 0 {
        Err(CoreUtilsError::Syscall(result))
    } else {
        Ok(())
    }
}

pub fn readonly(target: &str) -> Result<()> {
    let flags = MS_NOSUID | MS_NODEV | MS_RDONLY | MS_REMOUNT;
    let mut target_buf = [0u8; 256];
    let target_ptr = str_to_cstring(target, &mut target_buf)?;
    let result = unsafe {
        syscall!(
            MOUNT,
            0, // NULL source for remount
            target_ptr as usize,
            0, // NULL fstype for remount
            flags,
            0 // NULL data for remount
        )
    } as isize;

    if result < 0 {
        Err(CoreUtilsError::Syscall(result))
    } else {
        Ok(())
    }
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
        mknod(path, S_IFCHR, 1, minor)?; // major 1 for memory devices
    }
    Ok(())
}

pub fn setup() -> Result<()> {
    let common = MS_NOSUID | MS_NOEXEC | MS_NODEV | MS_RELATIME;
    mount("proc", "/proc", "proc", common, None)?;
    let dev_flags = MS_NOSUID | MS_NOEXEC | MS_RELATIME; // allow device nodes
    mount("dev", "/dev", "devtmpfs", dev_flags, Some("mode=0755"))?;
    mount("sysfs", "/sys", "sysfs", common, None)?;
    mount("run", "/run", "tmpfs", common, Some("mode=0755"))?;
    let tmp_flags = MS_NOSUID | MS_NODEV | MS_RELATIME;
    mount("tmpfs", "/tmp", "tmpfs", tmp_flags, None)?;
    // The mount_if calls are converted to unconditional mounts.
    // The kernel will fail them if the fs type is not available.
    // We ignore errors here as these filesystems might not be present.
    let _ = mount(
        "securityfs",
        "/sys/kernel/security",
        "securityfs",
        common,
        None,
    );
    let _ = mount(
        "efivarfs",
        "/sys/firmware/efi/efivars",
        "efivarfs",
        common,
        None,
    );
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
        mknod(device, S_IFCHR, 1, 3).expect("Failed to create device node");
        assert!(Path::new(device).exists());
        cleanup_path(device);
    }
}
