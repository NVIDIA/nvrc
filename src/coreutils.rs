use nix::fcntl::AT_FDCWD;
use nix::sys::stat;
use nix::unistd::symlinkat;
use std::path::Path;

pub fn ln(target: &str, linkpath: &str) {
    if let Err(e) = symlinkat(target, AT_FDCWD, linkpath) {
        panic!("Failed to create symlink {linkpath} -> {target}: {e}");
    }
}

pub fn mknod(path: &str, kind: stat::SFlag, major: u64, minor: u64) {
    if !Path::new(path).exists() {
        let dev = nix::sys::stat::makedev(major, minor);
        if let Err(e) = stat::mknod(path, kind, stat::Mode::from_bits_truncate(0o666), dev) {
            panic!("Failed to create device node {path}: {e}");
        }
    }
}
