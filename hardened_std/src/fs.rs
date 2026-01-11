// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Filesystem operations with security constraints

use crate::{last_os_error, path::PathBuf, Error, Result};

/// Maximum bytes allowed in a single write operation
/// Analysis: Production uses max 8 bytes ("16777216"), tests use max 11 bytes
/// Setting to 20 bytes provides safety margin while staying hardened
const MAX_WRITE_SIZE: usize = 20;

/// Maximum path length in bytes (excluding null terminator)
/// This is a security constraint to prevent path-based attacks and ensure
/// stack-allocated buffers remain safe. All NVRC paths are well under this limit.
const MAX_PATH_LEN: usize = 255;

/// Path buffer size including null terminator for C strings
const PATH_BUF_SIZE: usize = MAX_PATH_LEN + 1; // 256 bytes total

/// Exact allowed file paths for write operations (no wildcards, no directory prefixes)
/// This provides maximum security by explicitly whitelisting only the exact files nvrc needs
const ALLOWED_WRITE_PATHS: &[&str] = &[
    // kata_agent.rs - OOM score adjustment
    "/proc/self/oom_score_adj",
    // lockdown.rs - Disable kernel module loading
    "/proc/sys/kernel/modules_disabled",
    // kernel_params.rs - Kernel message logging
    "/proc/sys/kernel/printk_devkmsg",
    // kmsg.rs - Socket buffer tuning (4 files)
    "/proc/sys/net/core/rmem_default",
    "/proc/sys/net/core/wmem_default",
    "/proc/sys/net/core/rmem_max",
    "/proc/sys/net/core/wmem_max",
];

/// Exact allowed file paths for read operations
const ALLOWED_READ_PATHS: &[&str] = &[
    // kernel_params.rs - Kernel command line
    "/proc/cmdline",
    // mount.rs - Available filesystems
    "/proc/filesystems",
    // lockdown.rs - Module loading status
    "/proc/sys/kernel/modules_disabled",
    // kmsg.rs - Socket buffer sizes (4 files, for tests)
    "/proc/sys/net/core/rmem_default",
    "/proc/sys/net/core/wmem_default",
    "/proc/sys/net/core/rmem_max",
    "/proc/sys/net/core/wmem_max",
];

/// Allowed directory prefixes for create_dir_all operations
/// These are runtime directories that daemons need to create
const ALLOWED_DIR_PREFIXES: &[&str] = &[
    "/var/run/nvidia-persistenced", // daemon.rs - nvidia-persistenced runtime dir
];

/// Allowed path prefixes for test files only
/// Tests need to write to /tmp for verification
/// Note: Not behind #[cfg(test)] because dependent crates (NVRC) also run tests
#[cfg(test)]
const ALLOWED_TEST_PREFIXES: &[&str] = &[
    "/tmp/hardened_std_test_", // Only our test files
    "/tmp/.",                  // TempDir creates paths like /tmp/.tmpXXXXX and subdirs
];

/// Test path prefixes for dependent crate tests (always available)
/// TempDir creates paths like /tmp/.tmpXXXXX which are ephemeral and safe
const ALLOWED_TEMPDIR_PREFIXES: &[&str] = &[
    "/tmp/.", // TempDir paths for NVRC daemon tests
];

/// Write bytes to file with strict security constraints
///
/// # Security Constraints
/// - Maximum write size: 20 bytes
/// - Path must be in exact whitelist (no partial matches)
/// - Creates/truncates file with mode 0644
///
/// # Errors
/// - `Error::WriteTooLarge` if contents exceed MAX_WRITE_SIZE
/// - `Error::PathNotAllowed` if path not in whitelist
/// - `Error::Io` for system call failures
///
/// # Safety
/// Uses raw libc calls with proper error handling
pub fn write(path: &str, contents: &[u8]) -> Result<()> {
    // CONSTRAINT 1: Size limit enforcement
    if contents.len() > MAX_WRITE_SIZE {
        return Err(Error::WriteTooLarge(contents.len()));
    }

    // CONSTRAINT 2: Exact path whitelist enforcement
    let allowed = ALLOWED_WRITE_PATHS.contains(&path) || {
        #[cfg(test)]
        {
            ALLOWED_TEST_PREFIXES
                .iter()
                .any(|prefix| path.starts_with(prefix))
        }
        #[cfg(not(test))]
        {
            false
        }
    };

    if !allowed {
        return Err(Error::PathNotAllowed);
    }

    // SAFETY: Convert Rust string to C string for libc
    // C strings require null terminator, so max path is MAX_PATH_LEN bytes
    let mut path_buf = [0u8; PATH_BUF_SIZE];
    if path.len() > MAX_PATH_LEN {
        return Err(Error::InvalidInput(alloc::format!(
            "Path length {} exceeds maximum of {} bytes",
            path.len(),
            MAX_PATH_LEN
        )));
    }
    path_buf[..path.len()].copy_from_slice(path.as_bytes());
    // path_buf[path.len()] is already 0

    // SAFETY: Open file with O_WRONLY | O_CREAT | O_TRUNC
    // - O_WRONLY: write-only access
    // - O_CREAT: create if doesn't exist
    // - O_TRUNC: truncate to 0 if exists
    // - Mode 0644: rw-r--r--
    let fd = unsafe {
        libc::open(
            path_buf.as_ptr() as *const libc::c_char,
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
            0o644,
        )
    };

    if fd < 0 {
        return Err(last_os_error());
    }

    // SAFETY: Write contents to fd
    let written =
        unsafe { libc::write(fd, contents.as_ptr() as *const libc::c_void, contents.len()) };

    // SAFETY: Always close fd, even on write failure
    let close_result = unsafe { libc::close(fd) };

    // Check write result after closing
    if written < 0 {
        return Err(last_os_error());
    }

    if written != contents.len() as isize {
        return Err(Error::WriteIncomplete);
    }

    // Check close result
    if close_result < 0 {
        return Err(last_os_error());
    }

    Ok(())
}

/// Read entire file to string with path whitelist
///
/// # Security Constraints
/// - Path must be in exact whitelist
/// - Maximum read size: 4096 bytes (typical page size)
///
/// # Errors
/// - `Error::PathNotAllowed` if path not in whitelist
/// - `Error::Io` for system call failures
pub fn read_to_string(path: &str) -> Result<alloc::string::String> {
    const MAX_READ_SIZE: usize = 4096;

    // Path whitelist enforcement
    let allowed = ALLOWED_READ_PATHS.contains(&path) || {
        #[cfg(test)]
        {
            ALLOWED_TEST_PREFIXES
                .iter()
                .any(|prefix| path.starts_with(prefix))
        }
        #[cfg(not(test))]
        {
            false
        }
    };

    if !allowed {
        return Err(Error::PathNotAllowed);
    }

    // Convert path to C string (requires null terminator)
    let mut path_buf = [0u8; PATH_BUF_SIZE];
    if path.len() > MAX_PATH_LEN {
        return Err(Error::InvalidInput(alloc::format!(
            "Path length {} exceeds maximum of {} bytes",
            path.len(),
            MAX_PATH_LEN
        )));
    }
    path_buf[..path.len()].copy_from_slice(path.as_bytes());

    // SAFETY: Open file read-only
    let fd = unsafe { libc::open(path_buf.as_ptr() as *const libc::c_char, libc::O_RDONLY) };

    if fd < 0 {
        return Err(last_os_error());
    }

    // Read into buffer
    let mut buffer = alloc::vec![0u8; MAX_READ_SIZE];
    let bytes_read =
        unsafe { libc::read(fd, buffer.as_mut_ptr() as *mut libc::c_void, MAX_READ_SIZE) };

    // Always close fd
    let close_result = unsafe { libc::close(fd) };

    if bytes_read < 0 {
        return Err(last_os_error());
    }

    if close_result < 0 {
        return Err(last_os_error());
    }

    // Truncate to actual bytes read
    buffer.truncate(bytes_read as usize);

    // Convert to String
    alloc::string::String::from_utf8(buffer)
        .map_err(|_| Error::InvalidInput(alloc::format!("File contains invalid UTF-8")))
}

/// Create directory and all parents with security constraints
///
/// # Security Constraints
/// - Path must match allowed directory prefixes
/// - Maximum path length: MAX_PATH_LEN bytes
/// - Creates directories with mode 0755 (rwxr-xr-x)
///
/// # Errors
/// - `Error::PathNotAllowed` if path not in whitelist
/// - `Error::Io` for system call failures
///
/// # Safety
/// Uses raw libc mkdir calls with proper error handling
pub fn create_dir_all(path: &str) -> Result<()> {
    // STRICT PATH VALIDATION: Only exact prefixes allowed
    let allowed = ALLOWED_DIR_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
        || ALLOWED_TEMPDIR_PREFIXES
            .iter()
            .any(|prefix| path.starts_with(prefix))
        || {
            #[cfg(test)]
            {
                ALLOWED_TEST_PREFIXES
                    .iter()
                    .any(|prefix| path.starts_with(prefix))
            }
            #[cfg(not(test))]
            {
                false
            }
        };

    if !allowed {
        return Err(Error::PathNotAllowed);
    }

    // Path length check
    if path.len() > MAX_PATH_LEN {
        return Err(Error::InvalidInput(alloc::format!(
            "Path length {} exceeds maximum of {} bytes",
            path.len(),
            MAX_PATH_LEN
        )));
    }

    // Convert path to C string
    let mut path_buf = [0u8; PATH_BUF_SIZE];
    path_buf[..path.len()].copy_from_slice(path.as_bytes());

    // SAFETY: Create directory with mkdir -p semantics
    // Mode 0755 = rwxr-xr-x (standard directory permissions)
    // We iterate through path components and create each one
    let mut current_path = alloc::vec::Vec::new();
    for component in path.split('/') {
        if component.is_empty() {
            continue; // Skip empty components (leading / or //)
        }

        current_path.push(b'/');
        current_path.extend_from_slice(component.as_bytes());

        // Null-terminate for C
        let mut c_path = [0u8; PATH_BUF_SIZE];
        if current_path.len() >= PATH_BUF_SIZE {
            return Err(Error::InvalidInput(alloc::format!(
                "Path component too long: {}",
                current_path.len()
            )));
        }
        c_path[..current_path.len()].copy_from_slice(&current_path);

        // SAFETY: mkdir syscall - safe to call even if directory exists
        let result = unsafe { libc::mkdir(c_path.as_ptr() as *const libc::c_char, 0o755) };

        // Ignore EEXIST (directory already exists), fail on other errors
        if result < 0 {
            let errno = unsafe { *libc::__errno_location() };
            if errno != libc::EEXIST {
                return Err(last_os_error());
            }
        }
    }

    Ok(())
}

// NOTE: remove_file is NOT needed in hardened_std
// In production, NVRC runs as PID 1 in ephemeral VMs with fresh filesystems.
// Existing files are errors, not something to silently remove.
// See coreutils.rs ln() and mknod() - both fail fast if files exist.

/// Read symlink target
pub fn read_link(path: &str) -> Result<PathBuf> {
    todo!("fs::read_link")
}

/// Get file metadata
pub fn metadata(path: &str) -> Result<Metadata> {
    todo!("fs::metadata")
}

/// File handle
pub struct File {
    fd: i32,
}

impl File {
    pub fn open(path: &str) -> Result<Self> {
        todo!("File::open")
    }
}

/// File open options
pub struct OpenOptions {
    write: bool,
}

impl OpenOptions {
    pub fn new() -> Self {
        todo!("OpenOptions::new")
    }

    pub fn write(&mut self, write: bool) -> &mut Self {
        todo!("OpenOptions::write")
    }

    pub fn open(&self, path: &str) -> Result<File> {
        todo!("OpenOptions::open")
    }
}

/// File metadata
pub struct Metadata {
    _private: (),
}

impl Metadata {
    pub fn file_type(&self) -> FileType {
        todo!("Metadata::file_type")
    }

    pub fn mode(&self) -> u32 {
        todo!("Metadata::mode")
    }
}

/// File type
pub struct FileType {
    _private: (),
}

impl FileType {
    pub fn is_fifo(&self) -> bool {
        todo!("FileType::is_fifo")
    }

    pub fn is_char_device(&self) -> bool {
        todo!("FileType::is_char_device")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path(name: &str) -> alloc::string::String {
        alloc::format!("/tmp/hardened_std_test_{}", name)
    }

    #[test]
    fn test_write_security_constraints() {
        let path = test_path("write");

        // Size limit: at limit OK, over limit fails
        assert!(write(&path, &[b'X'; MAX_WRITE_SIZE]).is_ok());
        assert!(matches!(
            write(&path, &[b'X'; MAX_WRITE_SIZE + 1]),
            Err(Error::WriteTooLarge(_))
        ));

        // Path whitelist: exact match OK, similar path rejected
        assert!(!matches!(
            write("/proc/self/oom_score_adj", b"test"),
            Err(Error::PathNotAllowed)
        ));
        assert!(matches!(
            write("/etc/passwd", b"test"),
            Err(Error::PathNotAllowed)
        ));

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_write_behavior() {
        let path = test_path("behavior");

        // Creates file, truncates existing, correct content
        write(&path, b"hello").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");

        write(&path, b"hi").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hi");

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_read_to_string() {
        let path = test_path("read");

        // Write then read back
        std::fs::write(&path, b"test content").unwrap();
        assert_eq!(read_to_string(&path).unwrap(), "test content");

        // Path whitelist enforcement
        assert!(matches!(
            read_to_string("/etc/passwd"),
            Err(Error::PathNotAllowed)
        ));

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_path_length_limits() {
        // Path of exactly MAX_PATH_LEN (255) bytes should succeed
        let max_path = alloc::format!(
            "/tmp/hardened_std_test_{}",
            "x".repeat(MAX_PATH_LEN - "/tmp/hardened_std_test_".len())
        );
        assert_eq!(max_path.len(), MAX_PATH_LEN);
        assert!(write(&max_path, b"ok").is_ok());
        let _ = std::fs::remove_file(&max_path);

        // Path of MAX_PATH_LEN + 1 (256) bytes should fail
        let too_long = alloc::format!(
            "/tmp/hardened_std_test_{}",
            "x".repeat(MAX_PATH_LEN + 1 - "/tmp/hardened_std_test_".len())
        );
        assert_eq!(too_long.len(), MAX_PATH_LEN + 1);
        assert!(matches!(
            write(&too_long, b"fail"),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn test_create_dir_all() {
        let base = test_path("dir_test");
        let nested = alloc::format!("{}/a/b/c", base);

        // Create nested directories
        assert!(create_dir_all(&nested).is_ok());
        assert!(std::path::Path::new(&nested).exists());

        // Idempotent - calling again should succeed
        assert!(create_dir_all(&nested).is_ok());

        // Cleanup
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_create_dir_all_whitelist() {
        // Path not in whitelist should fail
        assert!(matches!(
            create_dir_all("/etc/forbidden"),
            Err(Error::PathNotAllowed)
        ));
    }
}

#[cfg(test)]
mod bench {
    // Future: Add criterion benchmarks comparing std::fs vs hardened_std
}
