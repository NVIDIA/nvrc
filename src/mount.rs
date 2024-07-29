use nix::mount; //::{mount, MsFlags};
use nix::mount::MsFlags;
use nix::sys::stat;
use nix::unistd::symlinkat;
use std::fs;
use std::path::Path;

use super::NVRC;

fn is_mounted(path: &str) -> bool {
    let proc_mounts_path = Path::new("/proc/mounts");
    if proc_mounts_path.exists() {
        if let Ok(mounts) = fs::read_to_string(proc_mounts_path) {
            return mounts.lines().any(|line| line.contains(path));
        }
    }
    false
}

fn mount(source: &str, target: &str, fstype: &str, flags: MsFlags, data: Option<&str>) {
    if !is_mounted(target) {
        match mount::mount(Some(source), target, Some(fstype), flags, data) {
            Ok(_) => {}
            Err(e) => panic!("Failed to mount {} on {}: {}", source, target, e),
        }
    }
}

fn ln(target: &str, linkpath: &str) {
    if let Err(e) = symlinkat(target, None, linkpath) {
        panic!("Failed to create symlink {} -> {}: {}", linkpath, target, e);
    }
}

fn mknod(path: &str, kind: stat::SFlag, major: u64, minor: u64) {
    if !Path::new(path).exists() {
        let dev = nix::sys::stat::makedev(major, minor);
        if let Err(e) = stat::mknod(path, kind, stat::Mode::from_bits_truncate(0o666), dev) {
            panic!("Failed to create device node {}: {}", path, e);
        }
    }
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
impl NVRC {
    #[allow(dead_code)]
    pub fn mount_readonly(&self, target: &str) {
        match mount::mount(
            None::<&str>,
            target,
            None::<&str>,
            // TODO how to mount it MsFlags::MS_NOEXEC
            //MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_RDONLY,
            MsFlags::MS_RDONLY,
            None::<&str>,
        ) {
            Ok(_) => {}
            Err(e) => panic!("failed to remount {} readonly: {}", target, e),
        }
    }
    pub fn mount_setup(&self) {
        mount(
            "proc",
            "/proc",
            "proc",
            MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV,
            None,
        );
        mount(
            "dev",
            "/dev",
            "devtmpfs",
            MsFlags::MS_NOSUID,
            Some("mode=0755"),
        );
        mount(
            "sysfs",
            "/sys",
            "sysfs",
            MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV,
            None,
        );
        mount(
            "run",
            "/run",
            "tmpfs",
            MsFlags::MS_NOSUID | MsFlags::MS_NODEV,
            Some("mode=0755"),
        );
        mount(
            "tmpfs",
            "/tmp",
            "tmpfs",
            MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV,
            None,
        );

        if fs_available("securityfs")
            && Path::new("/sys/kernel/security").exists()
            && !is_mounted("/sys/kernel/security")
        {
            mount(
                "securityfs",
                "/sys/kernel/security",
                "securityfs",
                MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV | MsFlags::MS_RELATIME,
                None,
            );
        }

        if fs_available("efivarfs")
            && Path::new("/sys/firmware/efi/efivars").exists()
            && !is_mounted("/sys/firmware/efi/efivars")
        {
            mount(
                "efivarfs",
                "/sys/firmware/efi/efivars",
                "efivarfs",
                MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC,
                None,
            );
        }

        //self.mount_readonly("/");

        ln("/proc/kcore", "/dev/core");
        ln("/proc/self/fd", "/dev/fd");
        ln("/proc/self/fd/0", "/dev/stdin");
        ln("/proc/self/fd/1", "/dev/stdout");
        ln("/proc/self/fd/2", "/dev/stderr");

        mknod("/dev/null", stat::SFlag::S_IFCHR, 1, 3);
        mknod("/dev/zero", stat::SFlag::S_IFCHR, 1, 5);
        mknod("/dev/random", stat::SFlag::S_IFCHR, 1, 8);
        mknod("/dev/urandom", stat::SFlag::S_IFCHR, 1, 9);
    }
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
                panic!("Failed to escalate privileges: {:?}", e)
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
        ln(src, dst);

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

        ln(src, dst);

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
        mknod(device, stat::SFlag::S_IFCHR, 1, 3);
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

        mount(source, dst, fstype, flags, data);
        assert!(is_mounted(dst));
        fs::remove_dir(target).expect("Failed to remove test mount");
    }
}
