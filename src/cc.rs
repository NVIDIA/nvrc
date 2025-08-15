pub mod confidential {
    #![no_std]

    use crate::cpu::Cpu;
    use cfg_if::cfg_if;
    use log::debug;
    use sc::{nr, syscall};

    const AT_FDCWD: i32 = -100;
    const F_OK: i32 = 0;

    #[derive(Debug)]
    pub enum Error {
        Syscall(isize),
        InvalidPath,
    }

    pub type Result<T> = core::result::Result<T, Error>;

    pub enum CC {
        On,
        Off,
        Devtools,
    }

    cfg_if! {
        if #[cfg(any(target_arch = "x86_64", target_arch = "x86"))] {
            use core::arch::x86_64::__cpuid_count;
        }
    }

    /// Copies a rust string slice into a stack-allocated buffer and null-terminates it.
    fn str_to_cstring(s: &str, buf: &mut [u8]) -> Result<*const u8> {
        if s.len() >= buf.len() {
            return Err(Error::InvalidPath);
        }
        buf[..s.len()].copy_from_slice(s.as_bytes());
        buf[s.len()] = 0;
        Ok(buf.as_ptr())
    }

    /// Checks if a path exists using the faccessat syscall.
    fn path_exists(path: &str) -> bool {
        let mut path_buf = [0u8; 256];
        if let Ok(path_ptr) = str_to_cstring(path, &mut path_buf) {
            let result = unsafe {
                syscall!(
                    nr::FACCESSAT,
                    AT_FDCWD as isize,
                    path_ptr as usize,
                    F_OK as isize,
                    0
                )
            } as isize;
            result == 0
        } else {
            false
        }
    }

    // Per‑vendor small helpers -------------------------------------------------
    #[inline]
    fn amd_enabled() -> bool {
        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        let cpuid = unsafe { (__cpuid_count(0x8000_001f, 0).eax & (1 << 4)) != 0 }; // SEV‑SNP bit
        #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
        let cpuid = false;
        let devnode = path_exists("/dev/sev-guest");
        debug!("AMD SNP: cpuid={}, devnode={}", cpuid, devnode);
        cpuid && devnode
    }

    #[inline]
    fn intel_enabled() -> bool {
        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        let cpuid = unsafe { __cpuid_count(0x21, 0).eax != 0 }; // TDX leaf present
        #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
        let cpuid = false;
        let devnode = path_exists("/dev/tdx-guest");
        debug!("Intel TDX: cpuid={}, devnode={}", cpuid, devnode);
        cpuid && devnode
    }

    #[inline]
    fn arm_enabled() -> bool {
        #[cfg(target_arch = "aarch64")]
        {
            // NOTE: In a no_std environment, libc::getauxval is not available.
            // A proper implementation requires parsing the auxiliary vector passed
            // by the kernel at startup, which is beyond the scope of this conversion.
            // We default to false for the CPUID check.
            let cpuid = false;
            let devnode = path_exists("/dev/cca-guest");
            debug!("Arm CCA: cpuid={}, devnode={}", cpuid, devnode);
            return cpuid && devnode;
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            debug!("Arm CCA: unsupported architecture");
            false
        }
    }

    pub fn detect(cpu: &Cpu) -> Result<CC> {
        let on = match cpu {
            Cpu::Amd => amd_enabled(),
            Cpu::Intel => intel_enabled(),
            Cpu::Arm => arm_enabled(),
        };
        let mode = if on { CC::On } else { CC::Off };
        // The original code returned a std::io::Result, but since no IO is performed
        // that can fail here, we can return our own Result type.
        // For simplicity, we just return Ok.
        debug!("CPU CC mode: {:?}", mode as u8); // Use as u8 for debug formatting
        Ok(mode)
    }
}
