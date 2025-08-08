#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cpu {
    Amd,
    Intel,
    Arm,
}
mod vendor {
    use anyhow::{Context, Result};
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    use super::super::NVRC;
    use super::*;

    const AMD_VENDOR_ID: &str = "AuthenticAMD";
    const INTEL_VENDOR_ID: &str = "GenuineIntel";
    const ARM_VENDOR_ID: &str = "0x41";

    const CPUINFO_PATH: &str = "/proc/cpuinfo";

    impl NVRC {
        pub fn query_cpu_vendor(&mut self) -> Result<()> {
            let file = File::open(CPUINFO_PATH)
                .with_context(|| format!("Failed to open {}", CPUINFO_PATH))?;
            let cpu = BufReader::new(file)
                .lines()
                .map_while(Result::ok)
                .find_map(|line| self.detect_vendor_from_line(&line))
                .ok_or_else(|| anyhow::anyhow!("CPU vendor not found"))?;
            debug!("{:?}", cpu);
            self.cpu_vendor = Some(cpu);
            Ok(())
        }

        pub fn detect_vendor_from_line(&self, line: &str) -> Option<Cpu> {
            match line {
                l if l.contains(AMD_VENDOR_ID) => Some(Cpu::Amd),
                l if l.contains(INTEL_VENDOR_ID) => Some(Cpu::Intel),
                l if l.contains("CPU implementer") && l.contains(ARM_VENDOR_ID) => Some(Cpu::Arm),
                _ => None,
            }
        }
    }
}

//#[cfg(feature = "confidential")]
pub mod confidential {
    use super::super::NVRC;
    use crate::cpu::Cpu;
    use log::debug;
    use std::path::Path;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CC {
        On,
        Off,
    }

    // ---- per‑architecture helpers (return false on unsupported targets) ----
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    #[inline]
    fn amd_snp_supported() -> bool {
        use core::arch::x86_64::__cpuid_count;
        unsafe { (__cpuid_count(0x8000_001f, 0).eax & (1 << 4)) != 0 }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
    #[inline]
    fn amd_snp_supported() -> bool {
        false
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    #[inline]
    fn intel_tdx_supported() -> bool {
        use core::arch::x86_64::__cpuid_count;
        unsafe { __cpuid_count(0x21, 0).eax != 0 }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
    #[inline]
    fn intel_tdx_supported() -> bool {
        false
    }

    #[cfg(target_arch = "aarch64")]
    #[inline]
    fn arm_cca_supported() -> bool {
        const AT_HWCAP2: libc::c_ulong = 26;
        const HWCAP2_RME: u64 = 1 << 28;
        unsafe { (libc::getauxval(AT_HWCAP2) & HWCAP2_RME) != 0 }
    }
    #[cfg(not(target_arch = "aarch64"))]
    #[inline]
    fn arm_cca_supported() -> bool {
        false
    }

    impl NVRC {
        pub fn query_cpu_cc_mode(&self) -> std::io::Result<CC> {
            let Some(vendor) = self.cpu_vendor.as_ref() else {
                debug!("CPU vendor not detected; CC mode = Off");
                return Ok(CC::Off);
            };
            let enabled = match *vendor {
                Cpu::Amd => {
                    let sev_snp = amd_snp_supported();
                    let devnode = Path::new("/dev/sev-guest").exists();
                    if sev_snp && devnode {
                        debug!(
                            "AMD SEV-SNP detected (cpuid={}, devnode={})",
                            sev_snp, devnode
                        );
                    }
                    sev_snp || devnode
                }
                Cpu::Intel => {
                    let tdx = intel_tdx_supported();
                    let devnode = Path::new("/dev/tdx-guest").exists();
                    if tdx && devnode {
                        debug!("Intel TDX detected (cpuid={}, devnode={})", tdx, devnode);
                    }
                    tdx || devnode
                }
                Cpu::Arm => {
                    let cca = arm_cca_supported();
                    let devnode = Path::new("/dev/todo-abi-guest").exists();
                    if cca && devnode {
                        debug!("Arm CCA detected (hwcap={}, devnode={})", cca, devnode);
                    }
                    cca || devnode
                }
            };
            Ok(if enabled { CC::On } else { CC::Off })
        }
    }
}
#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;

    #[test]
    fn test_query_cpu_vendor() {
        let mut nvrc = NVRC::default();
        nvrc.query_cpu_vendor().expect("Failed to query CPU vendor");
        let vendor = nvrc.cpu_vendor.expect("CPU vendor should be detected");
        assert!(
            matches!(vendor, Cpu::Amd | Cpu::Intel | Cpu::Arm),
            "Unknown CPU vendor: {:?}",
            vendor
        );
    }

    #[test]
    fn test_detect_vendor_from_line() {
        let nvrc = NVRC::default();
        assert_eq!(
            nvrc.detect_vendor_from_line("vendor_id	: AuthenticAMD"),
            Some(Cpu::Amd)
        );
        assert_eq!(
            nvrc.detect_vendor_from_line("vendor_id	: GenuineIntel"),
            Some(Cpu::Intel)
        );
        assert_eq!(
            nvrc.detect_vendor_from_line("CPU implementer	: 0x41"),
            Some(Cpu::Arm)
        );
        assert_eq!(
            nvrc.detect_vendor_from_line("vendor_id	: UnknownVendor"),
            None
        );
    }
}
