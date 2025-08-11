use anyhow::{Context, Result};
use nix::fcntl::AT_FDCWD;
use nix::sys::stat::{self, Mode, SFlag};
use nix::unistd::symlinkat;
use std::fs;
use std::path::Path;

/// Create (or update) a symbolic link from target to linkpath.
/// Idempotent: if link already points to target, it is left unchanged.
pub fn ln(target: &str, linkpath: &str) -> Result<()> {
    if let Ok(existing) = fs::read_link(linkpath) {
        if existing == Path::new(target) {
            return Ok(()); // already correct
        }
        // Existing link/file points elsewhere; remove it so we can replace
        let _ = fs::remove_file(linkpath);
    }
    symlinkat(target, AT_FDCWD, linkpath).with_context(|| format!("ln {} -> {}", linkpath, target))
}

/// Create (or replace) a character device node with desired major/minor.
/// Always recreates to avoid stale metadata/permissions.
pub fn mknod(path: &str, kind: SFlag, major: u64, minor: u64) -> Result<()> {
    if Path::new(path).exists() {
        fs::remove_file(path).with_context(|| format!("remove {} failed", path))?;
    }
    stat::mknod(
        path,
        kind,
        Mode::from_bits_truncate(0o666),
        stat::makedev(major, minor),
    )
    .with_context(|| format!("mknod {} failed", path))
}
