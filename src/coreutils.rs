use sc::syscall;
/// Custom error type for no_std environment
#[derive(Debug)]
pub enum CoreUtilsError {
    /// Contains the raw error code from the syscall.
    Syscall(isize),
    /// Path was too long for the internal buffer.
    InvalidPath,
}

pub type Result<T> = core::result::Result<T, CoreUtilsError>;

const AT_FDCWD: i32 = -100;

// File type constants
pub const S_IFCHR: u32 = 0o020000;

/// Copies a rust string slice into a stack-allocated buffer and null-terminates it.
pub fn str_to_cstring(s: &str, buf: &mut [u8]) -> Result<*const u8> {
    if s.len() >= buf.len() {
        return Err(CoreUtilsError::InvalidPath);
    }
    buf[..s.len()].copy_from_slice(s.as_bytes());
    buf[s.len()] = 0;
    Ok(buf.as_ptr())
}

/// Replicates the `makedev` macro to create a device number.
fn makedev(major: u64, minor: u64) -> u64 {
    ((major & 0xfffff000) << 32)
        | ((major & 0xfff) << 8)
        | ((minor & 0xffffff00) << 12)
        | (minor & 0xff)
}

/// Create (or update) a symbolic link from target to linkpath.
/// Idempotent: if link already points to target, it is left unchanged.
pub fn ln(target: &str, linkpath: &str) -> Result<()> {
    let mut target_buf = [0u8; 256];
    let mut linkpath_buf = [0u8; 256];
    let mut readlink_buf = [0u8; 256];

    let target_ptr = str_to_cstring(target, &mut target_buf)?;
    let linkpath_ptr = str_to_cstring(linkpath, &mut linkpath_buf)?;

    // Check if symlink already exists and points to the correct target.
    let read_result = unsafe {
        syscall!(
            READLINKAT,
            AT_FDCWD as isize,
            linkpath_ptr as usize,
            readlink_buf.as_mut_ptr() as usize,
            readlink_buf.len()
        )
    } as isize;

    if read_result >= 0 {
        let bytes_read = read_result as usize;
        if bytes_read > 0 && &readlink_buf[..bytes_read] == target.as_bytes() {
            return Ok(()); // Already points to the correct target.
        }
        // If it exists but points elsewhere, we must remove it before creating the new one.
        // The unlinkat syscall will handle this.
    }

    // Remove existing file/symlink at linkpath. This is necessary if readlinkat failed
    // because linkpath is not a symlink (e.g., a regular file), or if it's a symlink
    // to the wrong target. We ignore the error, as the file might not exist (ENOENT).
    let _ = unsafe { syscall!(UNLINKAT, AT_FDCWD as isize, linkpath_ptr as usize, 0) };

    // Create the new symlink.
    let result = unsafe {
        syscall!(
            SYMLINKAT,
            target_ptr as usize,
            AT_FDCWD as isize,
            linkpath_ptr as usize
        )
    } as isize;

    if result < 0 {
        return Err(CoreUtilsError::Syscall(result));
    }

    Ok(())
}

/// Create (or replace) a character device node with desired major/minor.
/// Always recreates to avoid stale metadata/permissions.
pub fn mknod(path: &str, kind: u32, major: u64, minor: u64) -> Result<()> {
    let mut path_buf = [0u8; 256];
    let path_ptr = str_to_cstring(path, &mut path_buf)?;

    // Remove existing file if it exists. We ignore the error (e.g., if it doesn't exist).
    let _ = unsafe { syscall!(UNLINKAT, AT_FDCWD as isize, path_ptr as usize, 0) };

    // Create the device node.
    let dev = makedev(major, minor);
    let mode = kind | 0o666;

    let result = unsafe {
        syscall!(
            MKNODAT,
            AT_FDCWD as isize,
            path_ptr as usize,
            mode as usize,
            dev as usize
        )
    } as isize;

    if result < 0 {
        return Err(CoreUtilsError::Syscall(result));
    }

    Ok(())
}
