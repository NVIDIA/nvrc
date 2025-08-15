use crate::coreutils::{fs_write, str_to_cstring, CoreUtilsError as Error, Result};
use core::ptr;
use log::debug;
use sc::syscall;

// From <sys/resource.h>
const RLIMIT_NOFILE: i32 = 7;

// From <fcntl.h>
const AT_FDCWD: i32 = -100;

#[repr(C)]
#[derive(Debug, Default)]
struct Rlimit {
    rlim_cur: u64,
    rlim_max: u64,
}

pub fn kata_agent() -> Result<()> {
    let nofile = 1024 * 1024;
    let rlim = Rlimit {
        rlim_cur: nofile,
        rlim_max: nofile,
    };

    // setrlimit(Resource::NOFILE, nofile, nofile)
    let res = unsafe { syscall!(SETRLIMIT, RLIMIT_NOFILE as usize, &rlim as *const Rlimit as usize) } as isize;
    if res < 0 {
        return Err(Error::Syscall(res));
    }

    // fs::write("/proc/self/oom_score_adj", b"-997")
    fs_write("/proc/self/oom_score_adj", b"-997")?;

    // rlimit::getrlimit(Resource::NOFILE)
    let mut current_rlim = Rlimit::default();
    let res = unsafe { syscall!(GETRLIMIT, RLIMIT_NOFILE as usize, &mut current_rlim as *mut Rlimit as usize) } as isize;
    if res == 0 {
        debug!("kata-agent NOFILE: {:?}", current_rlim);
    }

    // Command::new("/usr/bin/kata-agent").exec()
    let mut agent_path_buf = [0u8; 256];
    let agent_path_ptr = str_to_cstring("/usr/bin/kata-agent", &mut agent_path_buf)?;
    let argv = [agent_path_ptr, ptr::null()];
    let envp = [ptr::null::<u8>()];

    let res = unsafe {
        syscall!(
            EXECVE,
            agent_path_ptr as usize,
            argv.as_ptr() as usize,
            envp.as_ptr() as usize
        )
    } as isize;

    // execve only returns on error
    Err(Error::Syscall(res))
}