// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{Context, Result};
use std::fs;
use std::io::BufRead;
use std::io::Cursor;

use crate::nvrc::NVRC;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cpu {
    Amd,
    Intel,
    Arm,
}

impl NVRC {
    pub fn query_cpu_vendor(&mut self) -> Result<()> {
        // Read whole file then iterate lines (avoids layered readers)
        let data =
            fs::read_to_string("/proc/cpuinfo").with_context(|| "Failed to open /proc/cpuinfo")?;
        let mut vendor = None;
        for line in Cursor::new(data).lines().map_while(Result::ok) {
            if let Some(v) = self.detect_vendor_from_line(&line) {
                vendor = Some(v);
                break;
            }
        }
        let v = vendor.ok_or_else(|| anyhow::anyhow!("CPU vendor not found"))?;
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
    use super::{Cpu, NVRC};
    use log::debug;
    use std::path::Path;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CC {
        On,
        Off,
    }

    // CPUID / HWCAP helpers -------------------------------------------------
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    fn amd_snp_cpuid() -> bool {
        unsafe {
            use core::arch::x86_64::__cpuid_count;
            (__cpuid_count(0x8000_001f, 0).eax & (1 << 4)) != 0
        }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
    fn amd_snp_cpuid() -> bool {
        false
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    fn intel_tdx_cpuid() -> bool {
        unsafe {
            use core::arch::x86_64::__cpuid_count;
            __cpuid_count(0x21, 0).eax != 0
        }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
    fn intel_tdx_cpuid() -> bool {
        false
    }

    #[cfg(target_arch = "aarch64")]
    fn arm_cca_hwcap() -> bool {
        const AT_HWCAP2: libc::c_ulong = 26;
        const HWCAP2_RME: u64 = 1 << 28;
        unsafe { (libc::getauxval(AT_HWCAP2) & HWCAP2_RME) != 0 }
    }
    #[cfg(not(target_arch = "aarch64"))]
    fn arm_cca_hwcap() -> bool {
        false
    }

    fn amd_enabled() -> bool {
        let cpuid = amd_snp_cpuid();
        let devnode = Path::new("/dev/sev-guest").exists();
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
        let devnode = Path::new("/dev/tdx-guest").exists();
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
        let devnode = Path::new("/dev/cca-guest").exists();
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
        pub fn query_cpu_cc_mode(&self) -> std::io::Result<CC> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nvrc::NVRC;

    #[test]
    fn test_query_cpu_vendor() {
        let mut nvrc = NVRC::default();
        nvrc.query_cpu_vendor().expect("Failed to query CPU vendor");
        let vendor = nvrc.cpu_vendor.expect("CPU vendor should be detected");
        assert!(matches!(vendor, Cpu::Amd | Cpu::Intel | Cpu::Arm));
    }

    #[test]
    fn test_detect_vendor_from_line() {
        let nvrc = NVRC::default();
        assert_eq!(
            nvrc.detect_vendor_from_line("vendor_id\t: AuthenticAMD"),
            Some(Cpu::Amd)
        );
        assert_eq!(
            nvrc.detect_vendor_from_line("vendor_id\t: GenuineIntel"),
            Some(Cpu::Intel)
        );
        assert_eq!(
            nvrc.detect_vendor_from_line("CPU implementer\t: 0x41"),
            Some(Cpu::Arm)
        );
        assert_eq!(
            nvrc.detect_vendor_from_line("vendor_id\t: UnknownVendor"),
            None
        );
    }
}
