use anyhow::{Context, Result};
use nix::fcntl::AT_FDCWD;
use nix::sys::stat::{self, Mode, SFlag};
use nix::unistd::symlinkat;
use std::path::Path;

/// Create a symbolic link from target to linkpath
pub fn ln(target: &str, linkpath: &str) -> Result<()> {
    symlinkat(target, AT_FDCWD, linkpath)
        .with_context(|| format!("Failed to create symlink {} -> {}", linkpath, target))
}

/// Create a device node if it doesn't already exist
pub fn mknod(path: &str, kind: SFlag, major: u64, minor: u64) -> Result<()> {
    if Path::new(path).exists() {
        debug!("Device node {} already exists, skipping", path);
        return Ok(());
    }

    let dev = stat::makedev(major, minor);
    let mode = Mode::from_bits_truncate(0o666);

    stat::mknod(path, kind, mode, dev)
        .with_context(|| format!("Failed to create device node {}", path))
}
