use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};

use super::NVRC;

pub fn query_cpu_vendor(context: &mut NVRC) -> Result<()> {
    let cpu_vendor_file = "/proc/cpuinfo";
    let file = File::open(cpu_vendor_file).context("Failed to open /proc/cpuinfo")?;
    let reader = BufReader::new(file);

    let mut cpu_vendor = String::new();

    for line in reader.lines() {
        let line = line.context("Failed to read line from /proc/cpuinfo")?;
        if line.contains("AuthenticAMD") {
            cpu_vendor = "amd".to_string();
        } else if line.contains("GenuineIntel") {
            cpu_vendor = "intel".to_string();
        } else if line.contains("CPU implementer") && line.contains("0x41") {
            cpu_vendor = "arm".to_string();
        }
    }
    if cpu_vendor.is_empty() {
        return Err(anyhow::anyhow!("CPU vendor not found"));
    }

    debug!("cpu vendor: {}", cpu_vendor);
    context.cpu_vendor = Some(cpu_vendor);

    Ok(())
}

#[cfg(test)]

mod tests {
    use super::*;
    #[test]
    fn test_query_cpu_vendor() {
        let mut context = NVRC::default();
        query_cpu_vendor(&mut context).unwrap();
        let vendor = context.cpu_vendor.unwrap();

        assert!(vendor == "amd" || vendor == "intel" || vendor == "arm");
    }
}
