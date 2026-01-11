// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Process execution with security-hardened restrictions
//!
//! **Security Model:**
//! - Only whitelisted binaries can be executed (compile-time enforcement)
//! - All arguments must be &'static str (no dynamic strings) to prevent injection
//!   attacks from runtime data - only compile-time constants are accepted
//! - Maximum security for ephemeral VM init process
//!
//! **Allowed binaries (production):**
//! - /usr/bin/nvidia-smi - GPU configuration
//! - /usr/bin/nvidia-ctk - Container toolkit
//! - /usr/sbin/modprobe - Kernel module loading
//! - /usr/bin/nvidia-persistenced - GPU persistence daemon
//! - /usr/bin/nv-hostengine - DCGM host engine
//! - /usr/bin/dcgm-exporter - DCGM metrics exporter
//! - /usr/bin/nv-fabricmanager - NVLink fabric manager
//! - /usr/bin/kata-agent - Kata runtime agent
//!
//! **Test binaries (cfg(test) only):**
//! - /bin/true, /bin/false, /bin/sleep, /bin/sh - For unit tests

use crate::{last_os_error, Error, Result};
use core::ffi::{c_char, c_int};

/// Check if binary is in the allowed list
fn is_binary_allowed(path: &str) -> bool {
    // Production binaries - always allowed
    let production_allowed = matches!(
        path,
        "/usr/bin/nvidia-smi"
            | "/usr/bin/nvidia-ctk"
            | "/usr/sbin/modprobe"
            | "/usr/bin/nvidia-persistenced"
            | "/usr/bin/nv-hostengine"
            | "/usr/bin/dcgm-exporter"
            | "/usr/bin/nv-fabricmanager"
            | "/usr/bin/kata-agent"
    );

    if production_allowed {
        return true;
    }

    // Test binaries - only allowed in test builds
    #[cfg(test)]
    {
        matches!(path, "/bin/true" | "/bin/false" | "/bin/sleep" | "/bin/sh")
    }
    #[cfg(not(test))]
    {
        false
    }
}

/// Command builder with security restrictions
pub struct Command {
    path: &'static str,
    args: [Option<&'static str>; 16], // Max 16 args
    arg_count: usize,
    stdout_fd: Option<c_int>,
    stderr_fd: Option<c_int>,
}

impl Command {
    /// Create a new Command for the given binary path.
    /// Returns BinaryNotAllowed error if path is not in the whitelist.
    pub fn new(path: &'static str) -> Result<Self> {
        if !is_binary_allowed(path) {
            return Err(Error::BinaryNotAllowed);
        }
        Ok(Self {
            path,
            args: [None; 16],
            arg_count: 0,
            stdout_fd: None,
            stderr_fd: None,
        })
    }

    /// Add arguments to the command.
    /// Arguments must be &'static str for security (no dynamic strings).
    /// Maximum 16 arguments supported.
    pub fn args(&mut self, args: &[&'static str]) -> Result<&mut Self> {
        if self.arg_count + args.len() > 16 {
            return Err(Error::InvalidInput(alloc::string::String::from(
                "Too many arguments (max 16)",
            )));
        }
        for &arg in args {
            self.args[self.arg_count] = Some(arg);
            self.arg_count += 1;
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
        // SAFETY: fork() is safe here because we're in a controlled init environment
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(last_os_error());
        }

        if pid == 0 {
            // Child process - setup stdio and exec
            self.setup_stdio();
            let _ = self.do_exec();
            // If exec fails, exit child
            unsafe { libc::_exit(1) };
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
        for i in 0..self.arg_count {
            if let Some(arg) = self.args[i] {
                let c_arg = CString::new(arg).map_err(|_| {
                    Error::InvalidInput(alloc::string::String::from("Arg contains null byte"))
                })?;
                c_args.push(c_arg);
            }
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
    #[cfg(test)]
    pub fn code(&self) -> Option<i32> {
        if libc::WIFEXITED(self.status) {
            Some(libc::WEXITSTATUS(self.status))
        } else {
            None
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
        assert!(is_binary_allowed("/usr/bin/nvidia-smi"));
        assert!(is_binary_allowed("/usr/bin/nvidia-ctk"));
        assert!(is_binary_allowed("/usr/sbin/modprobe"));
        assert!(is_binary_allowed("/usr/bin/nvidia-persistenced"));
        assert!(is_binary_allowed("/usr/bin/nv-hostengine"));
        assert!(is_binary_allowed("/usr/bin/dcgm-exporter"));
        assert!(is_binary_allowed("/usr/bin/nv-fabricmanager"));
        assert!(is_binary_allowed("/usr/bin/kata-agent"));
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
        let cmd = Command::new("/bin/true");
        assert!(cmd.is_ok());
    }

    #[test]
    fn test_command_new_disallowed() {
        let cmd = Command::new("/bin/bash");
        assert!(matches!(cmd, Err(Error::BinaryNotAllowed)));
    }

    // ==================== Command execution tests ====================

    #[test]
    fn test_command_status_success() {
        let mut cmd = Command::new("/bin/true").unwrap();
        let status = cmd.status().unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_command_status_failure() {
        let mut cmd = Command::new("/bin/false").unwrap();
        let status = cmd.status().unwrap();
        assert!(!status.success());
    }

    #[test]
    fn test_command_with_args() {
        let mut cmd = Command::new("/bin/sh").unwrap();
        cmd.args(&["-c", "exit 0"]).unwrap();
        let status = cmd.status().unwrap();
        assert!(status.success());

        let mut cmd = Command::new("/bin/sh").unwrap();
        cmd.args(&["-c", "exit 42"]).unwrap();
        let status = cmd.status().unwrap();
        assert!(!status.success());
        assert_eq!(status.code(), Some(42));
    }

    // ==================== Child process tests ====================

    #[test]
    fn test_spawn_and_wait() {
        let mut cmd = Command::new("/bin/true").unwrap();
        let mut child = cmd.spawn().unwrap();
        let status = child.wait().unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_try_wait() {
        let mut cmd = Command::new("/bin/sleep").unwrap();
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
        let mut cmd = Command::new("/bin/sleep").unwrap();
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
        let mut cmd = Command::new("/bin/sh").unwrap();
        cmd.args(&["-c", "echo test"]).unwrap();
        cmd.stdout(Stdio::Null);
        let status = cmd.status().unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_max_args_exceeded() {
        let mut cmd = Command::new("/bin/true").unwrap();
        // Try to add 17 args (exceeds max of 16)
        let many_args: [&'static str; 17] = [
            "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
            "17",
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

        let mut cmd = Command::new("/bin/sh").unwrap();
        cmd.args(&["-c", "echo test"]).unwrap();
        cmd.stdout(stdio);
        let status = cmd.status().unwrap();
        assert!(status.success());
    }
}
