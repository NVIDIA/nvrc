// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

pub mod confidential {
    use crate::cpu::Cpu;
    use cfg_if::cfg_if;
    use log::debug;
    use std::path::Path;

    pub enum CC { On, Off, Devtools }

    cfg_if! {
        if #[cfg(any(target_arch = "x86_64", target_arch = "x86"))] {
            use core::arch::x86_64::__cpuid_count;
        }
    }

    // Per‑vendor small helpers -------------------------------------------------
    #[inline]
    fn amd_enabled() -> bool {
        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        let cpuid = unsafe { (__cpuid_count(0x8000_001f, 0).eax & (1 << 4)) != 0 }; // SEV‑SNP bit
        #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
        let cpuid = false;
        let devnode = Path::new("/dev/sev-guest").exists();
        debug!("AMD SNP: cpuid={}, devnode={}", cpuid, devnode);
        cpuid && devnode
    }

    #[inline]
    fn intel_enabled() -> bool {
        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        let cpuid = unsafe { __cpuid_count(0x21, 0).eax != 0 }; // TDX leaf present
        #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
        let cpuid = false;
        let devnode = Path::new("/dev/tdx-guest").exists();
        debug!("Intel TDX: cpuid={}, devnode={}", cpuid, devnode);
        cpuid && devnode
    }

    #[inline]
    fn arm_enabled() -> bool {
        #[cfg(target_arch = "aarch64")] {
            const AT_HWCAP2: libc::c_ulong = 26;
            const HWCAP2_RME: u64 = 1 << 28; // Realm Management Extension
            let hw2 = unsafe { libc::getauxval(AT_HWCAP2) };
            let cpuid = (hw2 & HWCAP2_RME) != 0;
            let devnode = Path::new("/dev/cca-guest").exists();
            debug!("Arm CCA: cpuid={}, devnode={}", cpuid, devnode);
            return cpuid && devnode;
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            debug!("Arm CCA: unsupported architecture");
            false
        }
    }

    pub fn detect(cpu: &Cpu) -> std::io::Result<CC> {
        let on = match cpu {
            Cpu::Amd => amd_enabled(),
            Cpu::Intel => intel_enabled(),
            Cpu::Arm => arm_enabled(),
        };
        let mode = if on { CC::On } else { CC::Off };
        debug!("CPU CC mode: {:?}", mode);
        Ok(mode)
    }
}
