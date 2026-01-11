//! Filesystem operations with security constraints

use crate::{last_os_error, path::PathBuf, Error, Result};

/// Maximum bytes allowed in a single write operation
/// Analysis: Production uses max 8 bytes ("16777216"), tests use max 11 bytes
/// Setting to 20 bytes provides safety margin while staying hardened
const MAX_WRITE_SIZE: usize = 20;

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

/// Allowed path prefixes for test files only
/// Tests need to write to /tmp for verification
#[cfg(test)]
const ALLOWED_TEST_PREFIXES: &[&str] = &[
    "/tmp/hardened_std_test_", // Only our test files
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
    // Path must be null-terminated for C functions
    let mut path_buf = [0u8; 256]; // Stack-allocated, max path 255 + null
    if path.len() >= path_buf.len() {
        return Err(Error::InvalidInput(alloc::format!(
            "Path too long: {}",
            path.len()
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

/// Read entire file to string (path whitelist enforced)
pub fn read_to_string(path: &str) -> Result<alloc::string::String> {
    todo!("fs::read_to_string")
}

/// Create directory and all parents
pub fn create_dir_all(path: &str) -> Result<()> {
    todo!("fs::create_dir_all")
}

/// Remove file
pub fn remove_file(path: &str) -> Result<()> {
    todo!("fs::remove_file")
}

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
    use alloc::vec;

    // Helper to create test file path in /tmp
    fn test_path(name: &str) -> alloc::string::String {
        alloc::format!("/tmp/hardened_std_test_{}", name)
    }

    // ==================== Security Constraint Tests ====================

    #[test]
    fn test_write_size_limit_enforced() {
        // Test exactly at limit (should succeed)
        let at_limit = [b'X'; MAX_WRITE_SIZE];
        let path = test_path("size_at_limit");
        let result = write(&path, &at_limit);
        assert!(result.is_ok(), "Should allow write at MAX_WRITE_SIZE");

        // Test one byte over limit (should fail)
        let over_limit = [b'X'; MAX_WRITE_SIZE + 1];
        let result = write(&path, &over_limit);
        assert!(
            matches!(result, Err(Error::WriteTooLarge(size)) if size == MAX_WRITE_SIZE + 1),
            "Should reject write over MAX_WRITE_SIZE"
        );
    }

    #[test]
    fn test_path_whitelist_enforced() {
        let content = b"test";

        // Production allowed paths - should succeed or fail on permissions, NOT PathNotAllowed
        let production_paths = vec![
            "/proc/self/oom_score_adj",
            "/proc/sys/kernel/modules_disabled",
            "/proc/sys/kernel/printk_devkmsg",
            "/proc/sys/net/core/rmem_default",
            "/proc/sys/net/core/wmem_default",
            "/proc/sys/net/core/rmem_max",
            "/proc/sys/net/core/wmem_max",
        ];

        for path in production_paths {
            let result = write(path, content);
            // May fail due to permissions, but should NOT be PathNotAllowed
            assert!(
                !matches!(result, Err(Error::PathNotAllowed)),
                "Production path {} should be in whitelist",
                path
            );
        }

        // Test paths with correct prefix - should succeed
        let test_path = test_path("whitelist_test");
        let result = write(&test_path, content);
        assert!(result.is_ok(), "Test path should be allowed");

        // Disallowed paths should fail with PathNotAllowed
        let disallowed_paths = vec![
            "/etc/passwd",
            "/root/.ssh/authorized_keys",
            "/home/user/file",
            "/bin/bash",
            "relative/path",
            "/proc/sys/kernel/other_file", // Similar but not exact match
            "/proc/self/other",            // Similar but not exact match
            "/tmp/other_test",             // Wrong prefix for tests
        ];

        for path in disallowed_paths {
            let result = write(path, content);
            assert!(
                matches!(result, Err(Error::PathNotAllowed)),
                "Path {} should be rejected",
                path
            );
        }
    }

    // ==================== Compatibility Tests with std::fs ====================

    #[test]
    fn test_write_basic_compatibility() {
        let path = test_path("basic_compat");
        let content = b"hello";

        // Write with hardened_std
        write(&path, content).expect("hardened write failed");

        // Read with std::fs to verify
        let read_content = std::fs::read_to_string(&path).expect("std read failed");
        assert_eq!(read_content, "hello");

        // Clean up
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_write_truncates_existing_file() {
        let path = test_path("truncate");

        // Write long content with std::fs
        std::fs::write(&path, b"long initial content").unwrap();

        // Write short content with hardened_std (should truncate)
        write(&path, b"short").expect("hardened write failed");

        // Verify truncation
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "short", "File should be truncated");

        // Clean up
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_write_creates_nonexistent_file() {
        let path = test_path("create_new");

        // Ensure file doesn't exist
        let _ = std::fs::remove_file(&path);

        // Write with hardened_std
        write(&path, b"new").expect("hardened write failed");

        // Verify with std::fs
        assert!(std::path::Path::new(&path).exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "new");

        // Clean up
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_write_empty_content() {
        let path = test_path("empty");

        // Write empty content
        write(&path, b"").expect("empty write failed");

        // Verify
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "");

        // Clean up
        let _ = std::fs::remove_file(&path);
    }

    // ==================== Real Usage Pattern Tests ====================

    #[test]
    fn test_write_production_patterns() {
        // Test actual nvrc usage patterns
        let test_cases = vec![
            (b"1\n" as &[u8], "modules_disabled"), // lockdown.rs
            (b"on\n", "printk_devkmsg"),           // kernel_params.rs
            (b"16777216", "socket_buffer"),        // kmsg.rs
            (b"-997", "oom_score_adj"),            // kata_agent.rs
        ];

        for (content, name) in test_cases {
            let path = test_path(name);

            // Write with hardened_std
            write(&path, content).unwrap_or_else(|e| panic!("Failed to write {}: {:?}", name, e));

            // Verify with std::fs
            let read_content =
                std::fs::read(&path).unwrap_or_else(|e| panic!("Failed to read {}: {:?}", name, e));
            assert_eq!(&read_content[..], content, "Content mismatch for {}", name);

            // Clean up
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn test_write_permissions_0644() {
        use std::os::unix::fs::PermissionsExt;

        let path = test_path("permissions");

        // Write file
        write(&path, b"test").expect("write failed");

        // Check permissions
        let metadata = std::fs::metadata(&path).expect("metadata failed");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o644, "File should have 0644 permissions");

        // Clean up
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_write_error_on_invalid_path() {
        // Path doesn't exist and can't be created (no parent directory)
        let result = write("/tmp/nonexistent_dir_xyz/file", b"test");
        assert!(result.is_err(), "Should fail on invalid path");
    }
}

#[cfg(test)]
mod bench {
    // Future: Add criterion benchmarks comparing std::fs vs hardened_std
}
