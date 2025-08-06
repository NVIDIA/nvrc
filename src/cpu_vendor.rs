use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};

use super::NVRC;

/// CPU vendor identification strings
const AMD_VENDOR_ID: &str = "AuthenticAMD";
const INTEL_VENDOR_ID: &str = "GenuineIntel";
const ARM_IMPLEMENTER_ID: &str = "0x41";

/// CPU vendor names
const AMD_VENDOR: &str = "amd";
const INTEL_VENDOR: &str = "intel";
const ARM_VENDOR: &str = "arm";

/// Path to CPU information file
const CPUINFO_PATH: &str = "/proc/cpuinfo";

impl NVRC {
    /// Query the CPU vendor from /proc/cpuinfo
    pub fn query_cpu_vendor(&mut self) -> Result<()> {
        let file =
            File::open(CPUINFO_PATH).with_context(|| format!("Failed to open {}", CPUINFO_PATH))?;
        let reader = BufReader::new(file);

        let cpu_vendor = reader
            .lines()
            .map(|line| line.context("Failed to read line from /proc/cpuinfo"))
            .find_map(|line_result| {
                let line = line_result.ok()?;
                self.detect_vendor_from_line(&line)
            })
            .ok_or_else(|| anyhow::anyhow!("CPU vendor not found"))?;

        debug!("cpu vendor: {}", cpu_vendor);
        self.cpu_vendor = Some(cpu_vendor);
        Ok(())
    }

    /// Detect CPU vendor from a single line of /proc/cpuinfo
    fn detect_vendor_from_line(&self, line: &str) -> Option<String> {
        if line.contains(AMD_VENDOR_ID) {
            Some(AMD_VENDOR.to_string())
        } else if line.contains(INTEL_VENDOR_ID) {
            Some(INTEL_VENDOR.to_string())
        } else if line.contains("CPU implementer") && line.contains(ARM_IMPLEMENTER_ID) {
            Some(ARM_VENDOR.to_string())
        } else {
            None
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_cpu_vendor() {
        let mut nvrc = NVRC::default();
        nvrc.query_cpu_vendor().expect("Failed to query CPU vendor");

        let vendor = nvrc.cpu_vendor.expect("CPU vendor should be detected");

        assert!(
            matches!(vendor.as_str(), "amd" | "intel" | "arm"),
            "Unknown CPU vendor: {}",
            vendor
        );
    }

    #[test]
    fn test_detect_vendor_from_line() {
        let nvrc = NVRC::default();

        // Test AMD detection
        assert_eq!(
            nvrc.detect_vendor_from_line("vendor_id	: AuthenticAMD"),
            Some("amd".to_string())
        );

        // Test Intel detection
        assert_eq!(
            nvrc.detect_vendor_from_line("vendor_id	: GenuineIntel"),
            Some("intel".to_string())
        );

        // Test ARM detection
        assert_eq!(
            nvrc.detect_vendor_from_line("CPU implementer	: 0x41"),
            Some("arm".to_string())
        );

        // Test unknown vendor
        assert_eq!(
            nvrc.detect_vendor_from_line("vendor_id	: UnknownVendor"),
            None
        );
    }
}
