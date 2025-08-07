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
pub mod confidential {

    use crate::cpu::Cpu;
    use cfg_if::cfg_if;
    use log::debug;
    use std::path::Path;

    pub enum CC {
        On,
        Off,
    }

    cfg_if! {
        if #[cfg(any(target_arch = "x86_64", target_arch = "x86"))] {
            use core::arch::x86_64::__cpuid_count;
        }
    }

    pub fn detect(cpu: &Cpu) -> std::io::Result<CC> {
        let mut cc: bool = false;
        match cpu {
            Cpu::Amd => {
                cfg_if! {
                    if #[cfg(any(target_arch = "x86_64", target_arch = "x86"))] {
                        unsafe {
                            // AMD SEV‑SNP — leaf 0x8000001F, EAX[4]
                            let regs = __cpuid_count(0x8000_001f, 0);
                            if regs.eax & (1 << 4) != 0 {
                                cc = true;
                            }
                        }
                    }
                }
                if cc | Path::new("/dev/sev-guest").exists() {
                    debug!("AMD SEV-SNP");
                    return Ok(CC::On);
                }
            }
            Cpu::Intel => {
                cfg_if! {
                    if #[cfg(any(target_arch = "x86_64", target_arch = "x86"))] {
                        unsafe {
                            // Intel TDX — leaf 0x21 sub‑leaf 0, EAX != 0
                            let regs = __cpuid_count(0x21, 0);
                            if regs.eax != 0 {
                                cc = true;
                            }
                        }
                    }
                }
                if cc | Path::new("/dev/tdx-guest").exists() {
                    debug!("Intel TDX");
                    return Ok(CC::On);
                }
            }
            Cpu::Arm => {
                cfg_if! {
                        if #[cfg(target_arch = "aarch64")] {
                        // Arm CCA — HWCAP2_RME (bit 28)
                        const AT_HWCAP2: libc::c_ulong = 26;
                        let hw2 = unsafe { libc::getauxval(AT_HWCAP2) };
                        const HWCAP2_RME: u64 = 1 << 28;
                        if (hw2 & HWCAP2_RME) != 0 { cc = true; }
                    }
                }
                if cc | Path::new("/dev/todo-abi-guest").exists() {
                    debug!("Arm CCA");
                    return Ok(CC::On);
                }
            }
        }
        Ok(CC::Off)
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
