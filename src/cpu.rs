use crate::nvrc::NVRC;
use core::str;
use sc::{syscall};

const O_RDONLY: i32 = 0;
const AT_FDCWD: i32 = -100;

#[derive(Debug)]
pub enum Error {
    Syscall(isize),
    VendorNotFound,
    Utf8Error(str::Utf8Error),
    InvalidPath,
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cpu {
    Amd,
    Intel,
    Arm,
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

impl NVRC {
    pub fn query_cpu_vendor(&mut self) -> Result<()> {
        let mut path_buf = [0u8; 256];
        let path_ptr = str_to_cstring("/proc/cpuinfo", &mut path_buf)?;

        let fd = unsafe {
            syscall!(
                OPENAT,
                AT_FDCWD as isize,
                path_ptr as usize,
                O_RDONLY as isize
            )
        } as isize;

        if fd < 0 {
            return Err(Error::Syscall(fd));
        }

        let mut buf = [0u8; 4096];
        let bytes_read = unsafe { syscall!(READ, fd, buf.as_mut_ptr() as usize, buf.len()) } as isize;

        unsafe {
            syscall!(CLOSE, fd);
        }

        if bytes_read < 0 {
            return Err(Error::Syscall(bytes_read));
        }

        let content = str::from_utf8(&buf[..bytes_read as usize]).map_err(Error::Utf8Error)?;
        let mut vendor = None;
        for line in content.lines() {
            if let Some(v) = self.detect_vendor_from_line(line) {
                vendor = Some(v);
                break;
            }
        }
        let v = vendor.ok_or(Error::VendorNotFound)?;
        debug!("CPU vendor: {:?}", v);
        self.cpu_vendor = Some(v);
        Ok(())
    }

    pub fn detect_vendor_from_line(&self, line: &str) -> Option<Cpu> {
        if line.contains("AuthenticAMD") {
            return Some(Cpu::Amd);
        }
        if line.contains("GenuineIntel") {
            return Some(Cpu::Intel);
        }
        if line.contains("CPU implementer") && line.contains("0x41") {
            return Some(Cpu::Arm);
        }
        None
    }
}

#[cfg(feature = "confidential")]
pub mod confidential {
    use super::{Cpu, Error, NVRC};
    use cfg_if::cfg_if;
    use log::debug;
    use sc::{syscall, nr};

    const AT_FDCWD: i32 = -100;
    const F_OK: i32 = 0;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CC {
        On,
        Off,
    }

    /// Checks if a path exists using the faccessat syscall.
    fn path_exists(path: &str) -> bool {
        let mut path_buf = [0u8; 256];
        // This helper can't return a Result easily, so we treat path errors as "doesn't exist".
        if let Ok(path_ptr) = super::str_to_cstring(path, &mut path_buf) {
            let result = unsafe {
                syscall!(
                    FACCESSAT,
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

    // CPUID / HWCAP helpers -------------------------------------------------
    cfg_if! {
        if #[cfg(any(target_arch = "x86_64", target_arch = "x86"))] {
            use core::arch::x86_64::__cpuid_count;
        }
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    fn amd_snp_cpuid() -> bool {
        unsafe { (__cpuid_count(0x8000_001f, 0).eax & (1 << 4)) != 0 }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
    fn amd_snp_cpuid() -> bool {
        false
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    fn intel_tdx_cpuid() -> bool {
        unsafe { __cpuid_count(0x21, 0).eax != 0 }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
    fn intel_tdx_cpuid() -> bool {
        false
    }

    #[cfg(target_arch = "aarch64")]
    fn arm_cca_hwcap() -> bool {
        // NOTE: In a no_std environment, libc::getauxval is not available.
        // A proper implementation requires parsing the auxiliary vector passed
        // by the kernel at startup, which is beyond the scope of this conversion.
        false
    }
    #[cfg(not(target_arch = "aarch64"))]
    fn arm_cca_hwcap() -> bool {
        false
    }

    fn amd_enabled() -> bool {
        let cpuid = amd_snp_cpuid();
        let devnode = path_exists("/dev/sev-guest");
        debug!("AMD SEV-SNP: cpuid={}, devnode={}", cpuid, devnode);
        if cpuid && !devnode {
            debug!("AMD SEV-SNP devnode missing");
        }
        if devnode && !cpuid {
            debug!("AMD SEV-SNP cpuid bit missing");
        }
        cpuid && devnode
    }
    fn intel_enabled() -> bool {
        let cpuid = intel_tdx_cpuid();
        let devnode = path_exists("/dev/tdx-guest");
        debug!("Intel TDX: cpuid={}, devnode={}", cpuid, devnode);
        if cpuid && !devnode {
            debug!("Intel TDX devnode missing");
        }
        if devnode && !cpuid {
            debug!("Intel TDX cpuid leaf missing");
        }
        cpuid && devnode
    }
    fn arm_enabled() -> bool {
        let hw = arm_cca_hwcap();
        let devnode = path_exists("/dev/cca-guest");
        debug!("Arm CCA: hwcap_rme={}, devnode={}", hw, devnode);
        if hw && !devnode {
            debug!("Arm CCA devnode missing");
        }
        if devnode && !hw {
            debug!("Arm CCA HWCAP2_RME missing");
        }
        hw && devnode
    }

    impl NVRC {
        pub fn query_cpu_cc_mode(&self) -> super::Result<CC> {
            let Some(vendor) = self.cpu_vendor.as_ref() else {
                debug!("CPU vendor unknown; CC Off");
                return Ok(CC::Off);
            };
            let on = match vendor {
                Cpu::Amd => amd_enabled(),
                Cpu::Intel => intel_enabled(),
                Cpu::Arm => arm_enabled(),
            };
            let mode = if on { CC::On } else { CC::Off };
            debug!("CPU CC mode: {:?}", mode);
            Ok(mode)
        }
    }
}
