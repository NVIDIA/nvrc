// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

#[cfg(feature = "confidential")]
pub mod confidential {
    use super::super::NVRC;
    use crate::pci_ids::DeviceType;
    use anyhow::{Context, Result};
    use nix::sys::mman::{mmap, munmap, MapFlags, ProtFlags};
    use std::fs::File;
    use std::ptr;

    const EMBEDDED_PCI_IDS: &str = include_str!("pci_ids_embedded.txt");

    #[derive(Debug, PartialEq, Clone, Copy)]
    pub enum GpuArchitecture {
        Hopper,
        Blackwell,
        Unknown,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CC {
        On,
        Off,
        Devtools,
    }

    impl GpuArchitecture {
        pub const CC_STATE_MASK: u32 = 0x3;
        pub fn cc_register(&self) -> Result<u64> {
            match self {
                GpuArchitecture::Hopper => Ok(0x001182cc),
                GpuArchitecture::Blackwell => Ok(0x590),
                GpuArchitecture::Unknown => Err(anyhow::anyhow!(
                    "Cannot determine CC register for unknown GPU architecture."
                )),
            }
        }

        pub fn parse_cc_mode(&self, reg_value: u32) -> Result<CC> {
            if matches!(self, GpuArchitecture::Unknown) {
                return Err(anyhow::anyhow!(
                    "Cannot parse CC mode for unknown GPU architecture."
                ));
            }
            let cc_state = reg_value & Self::CC_STATE_MASK;
            let mode = match cc_state {
                0x1 => CC::On,
                0x3 => CC::Devtools,
                _ => CC::Off,
            };
            Ok(mode)
        }
    }

    fn classify_gpu_architecture(device_name: &str) -> GpuArchitecture {
        let name_lower = device_name.to_lowercase();
        if name_lower.contains("h100")
            || name_lower.contains("h800")
            || name_lower.contains("hopper")
            || name_lower.contains("gh100")
        {
            return GpuArchitecture::Hopper;
        }
        if name_lower.contains("b100")
            || name_lower.contains("b200")
            || name_lower.contains("blackwell")
            || name_lower.contains("gb100")
            || name_lower.contains("gb200")
        {
            return GpuArchitecture::Blackwell;
        }
        GpuArchitecture::Unknown
    }

    fn get_gpu_architecture_by_device_id(device_id: u16, bdf: &str) -> Result<GpuArchitecture> {
        // Single-pass scan of embedded DB (avoid allocating HashMap per call)
        let needle = format!("\t{:04x} ", device_id).to_lowercase();
        for line in EMBEDDED_PCI_IDS.lines() {
            // Start scanning only inside NVIDIA vendor section
            if line.starts_with("10de  NVIDIA") || line.starts_with("10de  Nvidia") {
                // from here until a non-indented or new vendor line we search device entries
                continue; // device lines are indented
            }
            if line.starts_with('\t') && !line.starts_with("\t\t") {
                let lower = line.to_lowercase();
                if lower.contains(&needle) {
                    // Extract name after the id token
                    if let Some(rest) = line
                        .trim_start()
                        .strip_prefix(&format!("{:04x}  ", device_id))
                    {
                        let arch = classify_gpu_architecture(rest);
                        if arch == GpuArchitecture::Unknown {
                            return Err(anyhow::anyhow!("Device 0x{:04x} ('{}') at BDF {} unsupported (need Hopper/Blackwell)", device_id, rest.trim(), bdf));
                        }
                        return Ok(arch);
                    }
                }
            } else if !line.starts_with('\t') && line.starts_with("10df") {
                // next vendor, stop early
                break;
            }
        }
        Err(anyhow::anyhow!(
            "Device ID 0x{:04x} not found in embedded PCI DB for BDF {}",
            device_id,
            bdf
        ))
    }

    impl NVRC {
        fn query_cc_mode_bar0(&self, bdf: &str, device_id: u16) -> Result<CC> {
            let resource = format!("/sys/bus/pci/devices/{bdf}/resource0");
            let arch = get_gpu_architecture_by_device_id(device_id, bdf)
                .with_context(|| format!("arch lookup failed for BDF {bdf}"))?;
            let reg = arch.cc_register()?;
            debug!("BDF {bdf}: arch={:?} cc_reg=0x{:x}", arch, reg);
            let file =
                File::open(&resource).with_context(|| format!("open BAR0 failed for {bdf}"))?;
            let ps = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize };
            let page = (reg as usize / ps) * ps;
            let off = reg as usize - page;
            let map_len = ps;
            let map = unsafe {
                mmap(
                    None,
                    std::num::NonZeroUsize::new(map_len).unwrap(),
                    ProtFlags::PROT_READ,
                    MapFlags::MAP_SHARED,
                    &file,
                    page as i64,
                )
                .with_context(|| format!("mmap BAR0 failed for {bdf}"))?
            };
            let mode = unsafe {
                let reg_ptr = map.as_ptr().cast::<u8>().add(off).cast::<u32>();
                let val = ptr::read_volatile(reg_ptr);
                let m = arch
                    .parse_cc_mode(val)
                    .with_context(|| format!("parse CC mode failed (val=0x{val:x}) for {bdf}"))?;
                debug!("BDF {bdf}: CC mode {:?} (raw=0x{:x})", m, val);
                m
            };
            unsafe { munmap(map, map_len).with_context(|| format!("munmap failed for {bdf}"))? };
            Ok(mode)
        }
        pub fn query_gpu_cc_mode(&mut self) -> Result<()> {
            let mut aggregate: Option<CC> = None;
            for d in self
                .nvidia_devices
                .iter()
                .filter(|d| matches!(d.device_type, DeviceType::Gpu))
            {
                let m = self.query_cc_mode_bar0(&d.bdf, d.device_id)?;
                if let Some(prev) = aggregate {
                    if prev != m {
                        return Err(anyhow::anyhow!(
                            "Inconsistent CC mode: {} has {:?} expected {:?}",
                            d.bdf,
                            m,
                            prev
                        ));
                    }
                } else {
                    aggregate = Some(m);
                }
            }
            self.gpu_cc_mode = aggregate; // None if no GPUs
            if self.gpu_cc_mode.is_none() {
                debug!("No GPUs for CC mode query");
            }
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn classify_arch_names() {
            assert_eq!(
                classify_gpu_architecture("NVIDIA H100 PCIe"),
                GpuArchitecture::Hopper
            );
            assert_eq!(classify_gpu_architecture("GH100"), GpuArchitecture::Hopper);
            assert_eq!(
                classify_gpu_architecture("GB100 [B200]"),
                GpuArchitecture::Blackwell
            );
            assert_eq!(
                classify_gpu_architecture("Random Device"),
                GpuArchitecture::Unknown
            );
        }

        #[test]
        fn cc_register_values() {
            assert_eq!(GpuArchitecture::Hopper.cc_register().unwrap(), 0x001182cc);
            assert_eq!(GpuArchitecture::Blackwell.cc_register().unwrap(), 0x590);
            assert!(GpuArchitecture::Unknown.cc_register().is_err());
        }

        #[test]
        fn parse_cc_modes() {
            let h = GpuArchitecture::Hopper;
            assert_eq!(h.parse_cc_mode(0x0).unwrap(), CC::Off);
            assert_eq!(h.parse_cc_mode(0x1).unwrap(), CC::On);
            assert_eq!(h.parse_cc_mode(0x3).unwrap(), CC::Devtools);
            assert!(GpuArchitecture::Unknown.parse_cc_mode(0x1).is_err());
        }

        #[test]
        fn lookup_hopper() {
            // 2302  GH100
            let a = get_gpu_architecture_by_device_id(0x2302, "0000:01:00.0").unwrap();
            assert_eq!(a, GpuArchitecture::Hopper);
        }

        #[test]
        fn lookup_blackwell() {
            // 2901  GB100 [B200]
            let a = get_gpu_architecture_by_device_id(0x2901, "0000:02:00.0").unwrap();
            assert_eq!(a, GpuArchitecture::Blackwell);
        }

        #[test]
        fn lookup_unsupported_device_in_vendor_section() {
            // 1af1  GA100GL [A100 NVSwitch] -> does not match hopper/blackwell patterns
            let r = get_gpu_architecture_by_device_id(0x1af1, "0000:03:00.0");
            assert!(r.is_err());
            assert!(format!("{}", r.unwrap_err()).contains("unsupported"));
        }

        #[test]
        fn lookup_not_found() {
            let r = get_gpu_architecture_by_device_id(0xdead, "0000:04:00.0");
            assert!(r.is_err());
            assert!(format!("{}", r.unwrap_err()).contains("not found"));
        }
    }
}
