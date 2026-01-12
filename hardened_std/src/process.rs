// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Process execution with security-hardened restrictions
//!
//! **Security Model:**
//! - Only whitelisted binaries can be executed (runtime enforcement at spawn/status/exec time)
//! - Binary paths must be &'static str (compile-time constants) - no dynamic paths
//! - Arguments can be dynamic &str - validated but not restricted to static strings
//! - Maximum security for ephemeral VM init process
//!
//! **Allowed binaries (production):**
//! - /bin/nvidia-smi - GPU configuration
//! - /bin/nvidia-ctk - Container toolkit
//! - /sbin/modprobe - Kernel module loading
//! - /bin/nvidia-persistenced - GPU persistence daemon
//! - /bin/nv-hostengine - DCGM host engine
//! - /bin/dcgm-exporter - DCGM metrics exporter
//! - /bin/nv-fabricmanager - NVLink fabric manager
//! - /bin/kata-agent - Kata runtime agent
//!
//! **Test binaries (debug builds only):**
//! - /bin/true, /bin/false, /bin/sleep, /bin/sh - For unit tests

use crate::{last_os_error, Error, Result};
use core::ffi::{c_char, c_int};

/// Terminate the process with the given exit code.
/// This is a thin wrapper around libc::_exit() - it never returns.
/// Use this instead of std::process::exit() for no_std compatibility.
pub fn exit(code: i32) -> ! {
    // SAFETY: _exit() is always safe and never returns
    unsafe { libc::_exit(code) }
}

/// Check if binary is in the allowed list
fn is_binary_allowed(path: &str) -> bool {
    // Production binaries - always allowed
    let production_allowed = matches!(
        path,
        "/bin/nvidia-smi"
            | "/bin/nvidia-ctk"
            | "/sbin/modprobe"
            | "/bin/nvidia-persistenced"
            | "/bin/nv-hostengine"
            | "/bin/dcgm-exporter"
            | "/bin/nv-fabricmanager"
            | "/bin/kata-agent"
    );

    if production_allowed {
        return true;
    }

    // Test binaries - only allowed in debug builds (never in release)
    #[cfg(debug_assertions)]
    {
        matches!(path, "/bin/true" | "/bin/false" | "/bin/sleep" | "/bin/sh")
    }
    #[cfg(not(debug_assertions))]
    {
        false
    }
}

/// Maximum number of arguments allowed
const MAX_ARGS: usize = 32;

/// Command builder with security restrictions
pub struct Command {
    path: &'static str,
    args: alloc::vec::Vec<alloc::string::String>,
    stdout_fd: Option<c_int>,
    stderr_fd: Option<c_int>,
}

impl Command {
    /// Create a new Command for the given binary path.
    /// Binary whitelist is checked at spawn/status/exec time, not here.
    pub fn new(path: &'static str) -> Self {
        Self {
            path,
            args: alloc::vec::Vec::new(),
            stdout_fd: None,
            stderr_fd: None,
        }
    }

    /// Check if the binary is allowed before execution.
    fn check_allowed(&self) -> Result<()> {
        if !is_binary_allowed(self.path) {
            return Err(Error::BinaryNotAllowed);
        }
        Ok(())
    }

    /// Add arguments to the command.
    /// Maximum 32 arguments supported.
    pub fn args(&mut self, args: &[&str]) -> Result<&mut Self> {
        if self.args.len() + args.len() > MAX_ARGS {
            return Err(Error::InvalidInput(alloc::string::String::from(
                "Too many arguments (max 32)",
            )));
        }
        for &arg in args {
            self.args.push(alloc::string::String::from(arg));
        }
        Ok(self)
    }

    /// Configure stdout redirection.
    pub fn stdout(&mut self, cfg: Stdio) -> &mut Self {
        self.stdout_fd = cfg.as_fd();
        self
    }

    /// Configure stderr redirection.
    pub fn stderr(&mut self, cfg: Stdio) -> &mut Self {
        self.stderr_fd = cfg.as_fd();
        self
    }

    /// Spawn the command as a child process.
    pub fn spawn(&mut self) -> Result<Child> {
        // Check whitelist before forking
        self.check_allowed()?;

        // SAFETY: fork() is safe here because we're in a controlled init environment
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(last_os_error());
        }

        if pid == 0 {
            // Child process - setup stdio and exec
            self.setup_stdio();
            let _ = self.do_exec();
            // If exec fails, exit with 127 (standard "command not found" exit code)
            // This distinguishes exec failures from the spawned command returning 1
            unsafe { libc::_exit(127) };
        }

        // Parent process
        Ok(Child { pid })
    }

    /// Execute the command, blocking until completion.
    pub fn status(&mut self) -> Result<ExitStatus> {
        let mut child = self.spawn()?;
        child.wait()
    }

    /// Replace current process with the command (exec).
    /// Never returns on success - only returns Error on failure.
    pub fn exec(&mut self) -> Error {
        // Check whitelist before exec
        if let Err(e) = self.check_allowed() {
            return e;
        }

        self.setup_stdio();
        match self.do_exec() {
            Ok(_) => unreachable!("exec should never return Ok"),
            Err(e) => e,
        }
    }

    /// Setup stdio redirections for child process.
    /// Closes original fds after dup2 to prevent leaks.
    fn setup_stdio(&self) {
        unsafe {
            if let Some(fd) = self.stdout_fd {
                if libc::dup2(fd, libc::STDOUT_FILENO) == -1 {
                    libc::_exit(1);
                }
                // Close original fd after dup2 (unless it's a standard fd)
                if fd > libc::STDERR_FILENO {
                    libc::close(fd);
                }
            }
            if let Some(fd) = self.stderr_fd {
                if libc::dup2(fd, libc::STDERR_FILENO) == -1 {
                    libc::_exit(1);
                }
                // Close original fd after dup2 (unless it's a standard fd)
                if fd > libc::STDERR_FILENO {
                    libc::close(fd);
                }
            }
        }
    }

    /// Execute the command with execv.
    /// Uses absolute paths (no PATH search) for security - all binaries are whitelisted
    /// with full paths. Converts Rust strings to null-terminated C strings for execv.
    fn do_exec(&self) -> Result<()> {
        use alloc::ffi::CString;
        use alloc::vec::Vec;

        let c_path = CString::new(self.path).map_err(|_| {
            Error::InvalidInput(alloc::string::String::from("Path contains null byte"))
        })?;

        let mut c_args: Vec<CString> = Vec::new();
        for arg in &self.args {
            let c_arg = CString::new(arg.as_str()).map_err(|_| {
                Error::InvalidInput(alloc::string::String::from("Arg contains null byte"))
            })?;
            c_args.push(c_arg);
        }

        // Build argv: [path, args..., NULL]
        let mut argv: Vec<*const c_char> = Vec::new();
        argv.push(c_path.as_ptr());
        for c_arg in &c_args {
            argv.push(c_arg.as_ptr());
        }
        argv.push(core::ptr::null());

        // SAFETY: execv is safe here - we're replacing the process
        unsafe {
            libc::execv(c_path.as_ptr(), argv.as_ptr());
        }

        // If we get here, exec failed
        Err(last_os_error())
    }
}

/// Child process handle
#[derive(Debug)]
pub struct Child {
    pid: c_int,
}

impl Child {
    /// Check if child has exited without blocking.
    /// Returns Some(ExitStatus) if exited, None if still running.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        let mut status: c_int = 0;
        // SAFETY: waitpid with WNOHANG is safe
        let ret = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };

        if ret < 0 {
            return Err(last_os_error());
        }

        if ret == 0 {
            // Still running
            return Ok(None);
        }

        Ok(Some(ExitStatus { status }))
    }

    /// Wait for child to exit, blocking until it does.
    pub fn wait(&mut self) -> Result<ExitStatus> {
        let mut status: c_int = 0;
        // SAFETY: waitpid is safe
        let ret = unsafe { libc::waitpid(self.pid, &mut status, 0) };

        if ret < 0 {
            return Err(last_os_error());
        }

        Ok(ExitStatus { status })
    }

    /// Send SIGKILL to the child process.
    pub fn kill(&mut self) -> Result<()> {
        // SAFETY: kill syscall is safe
        let ret = unsafe { libc::kill(self.pid, libc::SIGKILL) };
        if ret < 0 {
            return Err(last_os_error());
        }
        Ok(())
    }
}

/// Process exit status
pub struct ExitStatus {
    status: c_int,
}

impl ExitStatus {
    /// Returns true if the process exited successfully (code 0).
    pub fn success(&self) -> bool {
        libc::WIFEXITED(self.status) && libc::WEXITSTATUS(self.status) == 0
    }

    /// Get the exit code if the process exited normally.
    pub fn code(&self) -> Option<i32> {
        if libc::WIFEXITED(self.status) {
            Some(libc::WEXITSTATUS(self.status))
        } else {
            None
        }
    }
}

impl core::fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if libc::WIFEXITED(self.status) {
            write!(f, "exit status: {}", libc::WEXITSTATUS(self.status))
        } else if libc::WIFSIGNALED(self.status) {
            write!(f, "signal: {}", libc::WTERMSIG(self.status))
        } else {
            write!(f, "unknown status: {}", self.status)
        }
    }
}

/// Standard I/O configuration
pub enum Stdio {
    /// Redirect to /dev/null
    Null,
    /// Inherit from parent
    Inherit,
    /// Create a pipe (not implemented - requires pipe2 syscall)
    Piped,
    /// Use specific file descriptor
    Fd(c_int),
}

impl Stdio {
    /// Convert to file descriptor option
    fn as_fd(&self) -> Option<c_int> {
        match self {
            Stdio::Fd(fd) => Some(*fd),
            Stdio::Null => {
                // Open /dev/null with O_CLOEXEC to prevent fd leak to child processes
                let null = b"/dev/null\0";
                // SAFETY: open is safe, path is null-terminated
                let fd = unsafe {
                    libc::open(
                        null.as_ptr() as *const c_char,
                        libc::O_RDWR | libc::O_CLOEXEC,
                    )
                };
                if fd >= 0 {
                    Some(fd)
                } else {
                    None
                }
            }
            Stdio::Inherit => None,
            Stdio::Piped => None, // TODO: implement if needed
        }
    }

    /// Create Stdio from hardened_std::fs::File
    pub fn from(file: crate::fs::File) -> Self {
        Stdio::Fd(file.into_raw_fd())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Binary whitelist tests ====================

    #[test]
    fn test_allowed_production_binaries() {
        // All production binaries should be allowed
        assert!(is_binary_allowed("/bin/nvidia-smi"));
        assert!(is_binary_allowed("/bin/nvidia-ctk"));
        assert!(is_binary_allowed("/sbin/modprobe"));
        assert!(is_binary_allowed("/bin/nvidia-persistenced"));
        assert!(is_binary_allowed("/bin/nv-hostengine"));
        assert!(is_binary_allowed("/bin/dcgm-exporter"));
        assert!(is_binary_allowed("/bin/nv-fabricmanager"));
        assert!(is_binary_allowed("/bin/kata-agent"));
    }

    #[test]
    fn test_allowed_test_binaries() {
        // Test binaries only allowed in test builds
        assert!(is_binary_allowed("/bin/true"));
        assert!(is_binary_allowed("/bin/false"));
        assert!(is_binary_allowed("/bin/sleep"));
        assert!(is_binary_allowed("/bin/sh"));
    }

    #[test]
    fn test_disallowed_binaries() {
        assert!(!is_binary_allowed("/bin/bash"));
        assert!(!is_binary_allowed("/usr/bin/wget"));
        assert!(!is_binary_allowed("/usr/bin/curl"));
        assert!(!is_binary_allowed("nvidia-smi")); // Must be absolute path
        assert!(!is_binary_allowed(""));
    }

    // ==================== Command creation tests ====================

    #[test]
    fn test_command_new_allowed() {
        // new() is infallible, whitelist checked at spawn time
        let mut cmd = Command::new("/bin/true");
        assert!(cmd.spawn().is_ok());
    }

    #[test]
    fn test_command_new_disallowed() {
        // new() succeeds, but spawn() fails for disallowed binary
        let mut cmd = Command::new("/bin/bash");
        assert!(matches!(cmd.spawn(), Err(Error::BinaryNotAllowed)));
    }

    // ==================== Command execution tests ====================

    #[test]
    fn test_command_status_success() {
        let mut cmd = Command::new("/bin/true");
        let status = cmd.status().unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_command_status_failure() {
        let mut cmd = Command::new("/bin/false");
        let status = cmd.status().unwrap();
        assert!(!status.success());
    }

    #[test]
    fn test_command_with_args() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(&["-c", "exit 0"]).unwrap();
        let status = cmd.status().unwrap();
        assert!(status.success());

        let mut cmd = Command::new("/bin/sh");
        cmd.args(&["-c", "exit 42"]).unwrap();
        let status = cmd.status().unwrap();
        assert!(!status.success());
        assert_eq!(status.code(), Some(42));
    }

    // ==================== Child process tests ====================

    #[test]
    fn test_spawn_and_wait() {
        let mut cmd = Command::new("/bin/true");
        let mut child = cmd.spawn().unwrap();
        let status = child.wait().unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_try_wait() {
        let mut cmd = Command::new("/bin/sleep");
        cmd.args(&["1"]).unwrap();
        let mut child = cmd.spawn().unwrap();

        // Should be None initially (still running)
        let result = child.try_wait().unwrap();
        assert!(result.is_none());

        // Wait for it to finish
        let status = child.wait().unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_kill() {
        let mut cmd = Command::new("/bin/sleep");
        cmd.args(&["10"]).unwrap();
        let mut child = cmd.spawn().unwrap();

        // Kill it
        child.kill().unwrap();

        // Wait should return (killed status)
        let status = child.wait().unwrap();
        assert!(!status.success());
    }

    // ==================== Stdio tests ====================

    #[test]
    fn test_stdio_null() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(&["-c", "echo test"]).unwrap();
        cmd.stdout(Stdio::Null);
        let status = cmd.status().unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_max_args_exceeded() {
        let mut cmd = Command::new("/bin/true");
        // Try to add 33 args (exceeds max of 32)
        let many_args: [&str; 33] = [
            "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
            "17", "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30",
            "31", "32", "33",
        ];
        let result = cmd.args(&many_args);
        assert!(result.is_err());
    }

    #[test]
    fn test_stdio_from_file() {
        use crate::fs::OpenOptions;

        // Open /dev/null as a file and use it for stdio
        let file = OpenOptions::new().write(true).open("/dev/null").unwrap();
        let stdio = Stdio::from(file);

        let mut cmd = Command::new("/bin/sh");
        cmd.args(&["-c", "echo test"]).unwrap();
        cmd.stdout(stdio);
        let status = cmd.status().unwrap();
        assert!(status.success());
    }
}
