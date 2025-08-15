use sc::syscall;
/// Custom error type for no_std environment
#[derive(Debug)]
pub enum CoreUtilsError {
    /// Contains the raw error code from the syscall.
    Syscall(isize),
    /// Path was too long for the internal buffer.
    InvalidPath,
    /// A string was not valid UTF-8.
    Utf8Error(core::str::Utf8Error),
}

pub type Result<T> = core::result::Result<T, CoreUtilsError>;

const AT_FDCWD: i32 = -100;

// File type constants
pub const S_IFCHR: u32 = 0o020000;

// From <fcntl.h>
const O_WRONLY: i32 = 1;
const O_RDONLY: i32 = 0;
const O_CREAT: i32 = 64;
const O_TRUNC: i32 = 512;
const O_APPEND: i32 = 1024;

/// Copies a rust string slice into a stack-allocated buffer and null-terminates it.
pub fn str_to_cstring(s: &str, buf: &mut [u8]) -> Result<*const u8> {
    if s.len() >= buf.len() {
        return Err(CoreUtilsError::InvalidPath);
    }
    buf[..s.len()].copy_from_slice(s.as_bytes());
    buf[s.len()] = 0;
    Ok(buf.as_ptr())
}

/// Converts a null-padded byte array to a &str slice.
pub fn cstr_as_str(bytes: &[u8]) -> Result<&str> {
    let len = bytes.iter().position(|&c| c == 0).unwrap_or(bytes.len());
    core::str::from_utf8(&bytes[..len]).map_err(CoreUtilsError::Utf8Error)
}

/// Replicates the `makedev` macro to create a device number.
fn makedev(major: u64, minor: u64) -> u64 {
    ((major & 0xfffff000) << 32)
        | ((major & 0xfff) << 8)
        | ((minor & 0xffffff00) << 12)
        | (minor & 0xff)
}

/// A helper to write a u32 to a buffer and return the written slice.
fn u32_to_str<'a>(mut n: u32, buf: &'a mut [u8; 10]) -> &'a [u8] {
    if n == 0 {
        buf[0] = b'0';
        return &buf[..1];
    }
    let mut i = buf.len();
    while n > 0 {
        i -= 1;
        buf[i] = (n % 10) as u8 + b'0';
        n /= 10;
    }
    &buf[i..]
}

/// A simple helper to write data into a byte slice buffer.
pub struct BufferWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> BufferWriter<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn write(&mut self, data: &[u8]) {
        let len = data.len();
        self.buf[self.pos..self.pos + len].copy_from_slice(data);
        self.pos += len;
    }

    pub fn write_u32(&mut self, n: u32) {
        let mut num_buf = [0u8; 10];
        let s = u32_to_str(n, &mut num_buf);
        self.write(s);
    }

    pub fn into_slice(self) -> &'a [u8] {
        &self.buf[..self.pos]
    }
}

// From <dirent.h>
#[repr(C)]
struct linux_dirent64 {
    d_ino: u64,
    d_off: i64,
    d_reclen: u16,
    d_type: u8,
    // d_name is a flexible array member.
}

/// A helper to parse a string slice into a u32.
fn str_to_u32(s: &str) -> Option<u32> {
    s.parse().ok()
}

/// Kills all processes with a given communication (comm) name.
pub fn kill_processes_by_comm(target_name: &str) -> Result<()> {
    let mut proc_path_buf = [0u8; 32];
    let proc_path_ptr = str_to_cstring("/proc", &mut proc_path_buf)?;

    let fd = unsafe { syscall!(OPENAT, AT_FDCWD as isize, proc_path_ptr as usize, O_RDONLY as isize) } as isize;
    if fd < 0 {
        return Err(CoreUtilsError::Syscall(fd));
    }

    let mut dir_buf = [0u8; 1024];
    loop {
        let nread = unsafe { syscall!(GETDENTS64, fd as usize, dir_buf.as_mut_ptr() as usize, dir_buf.len()) } as isize;
        if nread <= 0 {
            break; // Error or end of directory
        }

        let mut bpos = 0;
        while bpos < nread as usize {
            let dirent = unsafe {
                let dirent_ptr = dir_buf.as_ptr().add(bpos) as *const linux_dirent64;
                &*dirent_ptr
            };

            let name_ptr = dirent as *const _ as *const core::ffi::c_char;
            let name_offset = core::mem::size_of::<linux_dirent64>();
            let name_c = unsafe { core::ffi::CStr::from_ptr(name_ptr.add(name_offset)) };

            if let Ok(name_str) = name_c.to_str() {
                if let Some(pid) = str_to_u32(name_str) {
                    let mut comm_path_buf = [0u8; 256];
                    let mut writer = BufferWriter::new(&mut comm_path_buf);
                    writer.write(b"/proc/");
                    writer.write_u32(pid);
                    writer.write(b"/comm");

                    let mut comm_buf = [0u8; 64];
                    if let Ok(len) = fs_read(writer.into_slice(), &mut comm_buf) {
                        // The comm name can have a trailing newline.
                        let comm_name = cstr_as_str(&comm_buf[..len])?.trim_end();
                        if comm_name == target_name {
                            const SIGTERM: usize = 15;
                            let _ = unsafe { syscall!(KILL, pid as usize, SIGTERM) };
                            let _ = unsafe { syscall!(WAIT4, pid as usize, 0, 0, 0) };
                        }
                    }
                }
            }
            bpos += dirent.d_reclen as usize;
        }
    }

    let _ = unsafe { syscall!(CLOSE, fd as usize) };
    Ok(())
}

/// A handle to a child process.
#[derive(Debug, Clone, Copy)]
pub struct Child {
    pub pid: isize,
}

impl Child {
    /// Sends a SIGTERM signal to the child process.
    pub fn kill(&mut self) -> Result<()> {
        const SIGTERM: usize = 15;
        let res = unsafe { syscall!(KILL, self.pid as usize, SIGTERM) } as isize;
        if res < 0 {
            Err(CoreUtilsError::Syscall(res))
        } else {
            Ok(())
        }
    }

    /// Waits for the child process to exit and returns its status.
    pub fn wait(&mut self) -> Result<i32> {
        let mut status: i32 = 0;
        let wait_res =
            unsafe { syscall!(WAIT4, self.pid as usize, &mut status as *mut i32 as usize, 0, 0) }
                as isize;
        if wait_res < 0 {
            Err(CoreUtilsError::Syscall(wait_res))
        } else {
            Ok(status)
        }
    }
}

/// Reads the contents of a file into a buffer.
pub fn fs_read(path: &[u8], buf: &mut [u8]) -> Result<usize> {
    let mut path_buf = [0u8; 256];
    path_buf[..path.len()].copy_from_slice(path);
    let path_ptr = path_buf.as_ptr();

    let fd = unsafe { syscall!(OPENAT, AT_FDCWD as isize, path_ptr as usize, O_RDONLY as isize) } as isize;
    if fd < 0 {
        return Err(CoreUtilsError::Syscall(fd));
    }

    let res = unsafe { syscall!(READ, fd as usize, buf.as_mut_ptr() as usize, buf.len()) } as isize;
    let _ = unsafe { syscall!(CLOSE, fd as usize) };

    if res < 0 {
        Err(CoreUtilsError::Syscall(res))
    } else {
        Ok(res as usize)
    }
}

/// Spawns a command in the background.
pub fn background(command: &str, args: &[&str]) -> Result<Child> {
    // 1. Prepare argv for execve.
    let mut argv_storage = [[0u8; 256]; 16];
    let mut argv_ptrs = [core::ptr::null::<u8>(); 17];

    str_to_cstring(command, &mut argv_storage[0])?;
    argv_ptrs[0] = argv_storage[0].as_ptr();

    for (i, arg) in args.iter().enumerate() {
        if i + 1 >= argv_storage.len() {
            return Err(CoreUtilsError::InvalidPath);
        }
        str_to_cstring(arg, &mut argv_storage[i + 1])?;
        argv_ptrs[i + 1] = argv_storage[i + 1].as_ptr();
    }

    // 2. Fork the process.
    #[cfg(target_arch = "x86_64")]
    let pid = unsafe { syscall!(FORK) } as isize;
    #[cfg(target_arch = "aarch64")]
    let pid = {
        const SIGCHLD: usize = 17;
        // fork() is clone(SIGCHLD, 0, 0, 0, 0)
        (unsafe { syscall!(CLONE, SIGCHLD, 0, 0, 0, 0) }) as isize
    };
    if pid < 0 {
        return Err(CoreUtilsError::Syscall(pid));
    }

    if pid == 0 {
        // Child process: redirect output and execute the command.
        let mut kmsg_path_buf = [0u8; 32];
        if let Ok(kmsg_path_ptr) = str_to_cstring("/dev/kmsg", &mut kmsg_path_buf) {
            let kmsg_fd = unsafe {
                syscall!(OPENAT, AT_FDCWD as isize, kmsg_path_ptr as usize, O_WRONLY as isize)
            } as isize;
            if kmsg_fd >= 0 {
                unsafe {
                    #[cfg(target_arch = "x86_64")]
                    {
                        syscall!(DUP2, kmsg_fd as usize, 1);
                        syscall!(DUP2, kmsg_fd as usize, 2);
                    }
                    #[cfg(target_arch = "aarch64")]
                    {
                        syscall!(DUP3, kmsg_fd as usize, 1, 0);
                        syscall!(DUP3, kmsg_fd as usize, 2, 0);
                    }
                    syscall!(CLOSE, kmsg_fd as usize);
                }
            }
        }

        // Execute the new program.
        unsafe {
            syscall!(
                EXECVE,
                argv_ptrs[0] as usize,
                argv_ptrs.as_ptr() as usize,
                core::ptr::null::<*const u8>() as usize
            );
        }
        // execve only returns on error.
        unsafe { syscall!(EXIT, -1isize as usize) };
        unreachable!();
    } else {
        // Parent process: return the child handle.
        Ok(Child { pid })
    }
}

/// Spawns a command, waits for it to finish, and returns its exit code.
/// stdout and stderr are redirected to /dev/kmsg if possible.
pub fn foreground(command: &str, args: &[&str]) -> Result<i32> {
    // 1. Prepare argv for execve: a null-terminated array of pointers to null-terminated strings.
    let mut argv_storage = [[0u8; 256]; 16]; // Max 15 args + command name
    let mut argv_ptrs = [core::ptr::null::<u8>(); 17]; // Pointers + null terminator

    // First argument is the command itself.
    str_to_cstring(command, &mut argv_storage[0])?;
    argv_ptrs[0] = argv_storage[0].as_ptr();

    // Subsequent arguments.
    for (i, arg) in args.iter().enumerate() {
        if i + 1 >= argv_storage.len() {
            // Too many arguments for our buffer.
            return Err(CoreUtilsError::InvalidPath);
        }
        str_to_cstring(arg, &mut argv_storage[i + 1])?;
        argv_ptrs[i + 1] = argv_storage[i + 1].as_ptr();
    }

    // 2. Fork the process.
    #[cfg(target_arch = "x86_64")]
    let pid = unsafe { syscall!(FORK) } as isize;
    #[cfg(target_arch = "aarch64")]
    let pid = {
        const SIGCHLD: usize = 17;
        // fork() is clone(SIGCHLD, 0, 0, 0, 0)
        (unsafe { syscall!(CLONE, SIGCHLD, 0, 0, 0, 0) }) as isize
    };
    if pid < 0 {
        return Err(CoreUtilsError::Syscall(pid));
    }

    if pid == 0 {
        // Child process: redirect output and execute the command.
        let mut kmsg_path_buf = [0u8; 32];
        if let Ok(kmsg_path_ptr) = str_to_cstring("/dev/kmsg", &mut kmsg_path_buf) {
            let kmsg_fd = unsafe {
                syscall!(OPENAT, AT_FDCWD as isize, kmsg_path_ptr as usize, O_WRONLY as isize)
            } as isize;
            if kmsg_fd >= 0 {
                unsafe {
                    #[cfg(target_arch = "x86_64")]
                    {
                        syscall!(DUP2, kmsg_fd as usize, 1);
                        syscall!(DUP2, kmsg_fd as usize, 2);
                    }
                    #[cfg(target_arch = "aarch64")]
                    {
                        syscall!(DUP3, kmsg_fd as usize, 1, 0);
                        syscall!(DUP3, kmsg_fd as usize, 2, 0);
                    }
                    syscall!(CLOSE, kmsg_fd as usize);
                }
            }
        }

        // Execute the new program.
        unsafe {
            syscall!(
                EXECVE,
                argv_ptrs[0] as usize,
                argv_ptrs.as_ptr() as usize,
                core::ptr::null::<*const u8>() as usize // No environment variables
            );
        }
        // execve only returns on error, so we exit immediately.
        unsafe { syscall!(EXIT, -1isize as usize) };
        unreachable!();
    } else {
        // Parent process: wait for the child to complete.
        let mut status: i32 = 0;
        let wait_res =
            unsafe { syscall!(WAIT4, pid as usize, &mut status as *mut i32 as usize, 0, 0) }
                as isize;
        if wait_res < 0 {
            return Err(CoreUtilsError::Syscall(wait_res));
        }
        Ok(status)
    }
}

/// Creates a directory. Ignores errors if the directory already exists.
pub fn mkdir(path: &str, mode: u32) -> Result<()> {
    let mut path_buf = [0u8; 256];
    let path_ptr = str_to_cstring(path, &mut path_buf)?;
    let res = unsafe { syscall!(MKDIRAT, AT_FDCWD as isize, path_ptr as usize, mode as usize) } as isize;
    if res < 0 {
        // -17 is EEXIST, which we can ignore.
        if res != -17 {
            return Err(CoreUtilsError::Syscall(res));
        }
    }
    Ok(())
}

/// Changes the ownership of a file or directory.
pub fn chown(path: &str, uid: Option<u32>, gid: Option<u32>) -> Result<()> {
    let mut path_buf = [0u8; 256];
    let path_ptr = str_to_cstring(path, &mut path_buf)?;
    let uid_val = uid.map(|u| u as isize).unwrap_or(-1);
    let gid_val = gid.map(|g| g as isize).unwrap_or(-1);
    let res = unsafe {
        syscall!(
            FCHOWNAT,
            AT_FDCWD as isize,
            path_ptr as usize,
            uid_val as usize,
            gid_val as usize,
            0
        )
    } as isize;
    if res < 0 {
        return Err(CoreUtilsError::Syscall(res));
    }
    Ok(())
}

/// Checks if a path exists.
pub fn path_exists(path: &str) -> bool {
    let mut path_buf = [0u8; 256];
    if let Ok(path_ptr) = str_to_cstring(path, &mut path_buf) {
        let res = unsafe { syscall!(FACCESSAT, AT_FDCWD as isize, path_ptr as usize, 0, 0) } as isize;
        res == 0
    } else {
        false
    }
}

/// Appends a byte slice to a file. Creates the file if it doesn't exist.
pub fn fs_append(path: &str, data: &[u8]) -> Result<()> {
    let mut path_buf = [0u8; 256];
    let path_ptr = str_to_cstring(path, &mut path_buf)?;

    let flags = O_WRONLY | O_CREAT | O_APPEND;
    let mode = 0o666;

    let fd = unsafe {
        syscall!(
            OPENAT,
            AT_FDCWD as isize,
            path_ptr as usize,
            flags as isize,
            mode as isize
        )
    } as isize;

    if fd < 0 {
        return Err(CoreUtilsError::Syscall(fd));
    }

    let mut written = 0;
    while written < data.len() {
        let res = unsafe {
            syscall!(
                WRITE,
                fd as usize,
                data.as_ptr().add(written) as usize,
                data.len() - written
            )
        } as isize;

        if res < 0 {
            // Ensure we attempt to close the file descriptor on write error.
            let _ = unsafe { syscall!(CLOSE, fd as usize) };
            return Err(CoreUtilsError::Syscall(res));
        }
        written += res as usize;
    }

    let res = unsafe { syscall!(CLOSE, fd as usize) } as isize;
    if res < 0 {
        return Err(CoreUtilsError::Syscall(res));
    }

    Ok(())
}

/// Writes a byte slice to a file. Creates the file if it doesn't exist,
/// and truncates it if it does.
pub fn fs_write(path: &str, data: &[u8]) -> Result<()> {
    let mut path_buf = [0u8; 256];
    let path_ptr = str_to_cstring(path, &mut path_buf)?;

    let flags = O_WRONLY | O_CREAT | O_TRUNC;
    let mode = 0o666;

    let fd = unsafe {
        syscall!(
            OPENAT,
            AT_FDCWD as isize,
            path_ptr as usize,
            flags as isize,
            mode as isize
        )
    } as isize;

    if fd < 0 {
        return Err(CoreUtilsError::Syscall(fd));
    }

    let mut written = 0;
    while written < data.len() {
        let res = unsafe {
            syscall!(
                WRITE,
                fd as usize,
                data.as_ptr().add(written) as usize,
                data.len() - written
            )
        } as isize;

        if res < 0 {
            // Ensure we attempt to close the file descriptor on write error.
            let _ = unsafe { syscall!(CLOSE, fd as usize) };
            return Err(CoreUtilsError::Syscall(res));
        }
        written += res as usize;
    }

    let res = unsafe { syscall!(CLOSE, fd as usize) } as isize;
    if res < 0 {
        return Err(CoreUtilsError::Syscall(res));
    }

    Ok(())
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
