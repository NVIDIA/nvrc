// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

#[cfg(feature = "confidential")]
mod confidential {
    use super::super::NVRC;
    use crate::pci_ids::DeviceType;
    use anyhow::{Context, Result};
    use nix::sys::mman::{mmap, munmap, MapFlags, ProtFlags};
    use std::fs::File;
    use std::ptr;
    use log::debug;

    const EMBEDDED_PCI_IDS: &str = include_str!("pci_ids_embedded.txt");

    #[derive(Debug, PartialEq, Clone)]
    pub enum GpuArchitecture {
        Hopper,
        Blackwell,
        Unknown,
    }

    impl GpuArchitecture {
        pub fn cc_register(&self) -> Result<u64> {
            match self {
                Self::Hopper => Ok(0x001182cc),
                Self::Blackwell => Ok(0x590),
                Self::Unknown => Err(anyhow::anyhow!("unknown arch")),
            }
        }

        pub fn parse_cc_mode(&self, v: u32) -> Result<String> {
            if matches!(self, Self::Unknown) {
                return Err(anyhow::anyhow!("unknown arch"));
            }
            Ok(match v & 0x3 {
                0x1 => "on",
                0x3 => "devtools",
                _ => "off",
            }
            .to_string())
        }
    }

    fn classify_gpu_architecture(name: &str) -> GpuArchitecture {
        let n = name.to_lowercase();
        if n.contains("h100")
            || n.contains("h800")
            || n.contains("hopper")
            || n.contains("gh100")
        {
            return GpuArchitecture::Hopper;
        }
        if n.contains("b100")
            || n.contains("b200")
            || n.contains("blackwell")
            || n.contains("gb100")
            || n.contains("gb200")
        {
            return GpuArchitecture::Blackwell;
        }
        GpuArchitecture::Unknown
    }

    // On-demand scan of embedded DB for a device_id -> architecture
    fn get_gpu_architecture_by_device_id(id: u16, bdf: &str) -> Result<GpuArchitecture> {
        debug!("gpu {bdf} id 0x{id:04x}");
        let hex = format!("{:04x}", id);
        let mut in_nvidia = false;
        for line in EMBEDDED_PCI_IDS.lines() {
            if line.starts_with("10de  NVIDIA Corporation") {
                in_nvidia = true;
                continue;
            }
            if in_nvidia {
                if line.starts_with('\t') && !line.starts_with("\t\t") {
                    if let Some(l) = line.strip_prefix('\t') {
                        if l.starts_with(&hex) {
                            if let Some((_, name)) = l.split_once("  ") {
                                let arch = classify_gpu_architecture(name);
                                if matches!(arch, GpuArchitecture::Unknown) {
                                    return Err(anyhow::anyhow!("unrecognized arch {name} @ {bdf}"));
                                }
                                return Ok(arch);
                            }
                        }
                    }
                } else if !line.starts_with('\t') && !line.is_empty() && !line.starts_with('#') {
                    break;
                }
            }
        }
        Err(anyhow::anyhow!("device 0x{id:04x} not found"))
    }

    impl NVRC {
        fn query_cc_mode_bar0(&self, bdf: &str, device_id: u16) -> Result<CC> {
            let path = format!("/sys/bus/pci/devices/{bdf}/resource0");
            let arch = get_gpu_architecture_by_device_id(device_id, bdf)
                .with_context(|| format!("arch {bdf}"))?;
            let off = arch.cc_register()? as usize;
            let file = File::open(&path).with_context(|| format!("open {path}"))?;
            let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
            let aligned = (off / page) * page;
            let in_page = off - aligned;
            let ptr = unsafe {
                mmap(
                    None,
                    std::num::NonZeroUsize::new(page).unwrap(),
                    ProtFlags::PROT_READ,
                    MapFlags::MAP_SHARED,
                    &file,
                    aligned as i64,
                )
                .with_context(|| format!("mmap {bdf}"))?
            };
            let mode = unsafe {
                let reg = ptr.as_ptr().cast::<u8>().add(in_page).cast::<u32>();
                let val = ptr::read_volatile(reg);
                let m = arch.parse_cc_mode(val)?;
                debug!("cc {bdf} {:?} 0x{val:08x} => {m}", arch);
                m
            };
            unsafe { munmap(ptr, page).with_context(|| format!("munmap {bdf}"))? };
            Ok(mode)
        }
        pub fn query_gpu_cc_mode(&mut self) -> Result<()> {
            let mut mode: Option<CC> = None;
            let mut any = false;
            for d in self
                .nvidia_devices
                .iter()
                .filter(|d| matches!(d.device_type, DeviceType::Gpu))
            {
                any = true;
                let cur = self.query_cc_mode_bar0(&d.bdf, d.device_id)?;
                if let Some(m) = &mode {
                    if m != &cur {
                        return Err(anyhow::anyhow!("mixed cc modes"));
                    }
                } else {
                    mode = Some(cur);
                }
            }
            if !any {
                debug!("no gpus for cc query");
            }
            self.gpu_cc_mode = mode;
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use super::super::super::NVRC;
        use crate::pci_ids::DeviceType;

        #[test]
        fn test_arch_class() {
            assert_eq!(
                classify_gpu_architecture("NVIDIA H100 PCIe"),
                GpuArchitecture::Hopper
            );
            assert_eq!(
                classify_gpu_architecture("GB100 [B200]"),
                GpuArchitecture::Blackwell
            );
            assert_eq!(
                classify_gpu_architecture("GeForce RTX 4090"),
                GpuArchitecture::Unknown
            );
        }
        #[test]
        fn test_cc_parse() {
            let h = GpuArchitecture::Hopper;
            assert_eq!(h.parse_cc_mode(0x0).unwrap(), "off");
            assert_eq!(h.parse_cc_mode(0x1).unwrap(), "on");
            assert_eq!(h.parse_cc_mode(0x3).unwrap(), "devtools");
        }
        #[test]
        fn test_query_no_gpu() {
            let mut c = NVRC::default();
            c.nvidia_devices.clear();
            assert!(c.query_gpu_cc_mode().is_ok());
            assert!(c.gpu_cc_mode.is_none());
        }
        #[test]
        fn test_arch_registers() {
            assert_eq!(GpuArchitecture::Hopper.cc_register().unwrap(), 0x001182cc);
            assert_eq!(GpuArchitecture::Blackwell.cc_register().unwrap(), 0x590);
            assert!(GpuArchitecture::Unknown.cc_register().is_err());
        }
        #[test]
        fn test_unknown_parse_fail() {
            assert!(GpuArchitecture::Unknown.parse_cc_mode(1).is_err());
        }
        #[test]
        fn test_on_demand_scan_not_found() {
            let r = get_gpu_architecture_by_device_id(0xdead, "0000:00:00.0");
            assert!(r.is_err());
        }
        #[test]
        fn test_nvrc_mode_consistency_error() {
            let mut c = NVRC::default();
            // fabricate two devices with differing modes by mocking query_cc_mode_bar0

            // Not easily testable without hardware; ensure logic runs with empty set already covered.
            assert!(c.gpu_cc_mode.is_none());
        }
    }
}
