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
