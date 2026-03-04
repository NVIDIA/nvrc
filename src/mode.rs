// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Auto-detect NVRC mode from PCI hardware topology.
//!
//! Scans `/sys/bus/pci/devices` for NVIDIA GPUs, NVSwitches, and
//! Mellanox CX7 bridge LPFs (SW_MNG marker in PCI VPD), to determine
//! the correct operating mode and fabric manager configuration.
//!
//! NVL5 CX7 bridges expose 2 LPF (SW_MNG) + 2 FC PF per baseboard.
//! VPD is read directly from PCI sysfs to avoid dependency on IB drivers.

use log::debug;
use std::fs;

const PCI_DEVICES: &str = "/sys/bus/pci/devices";

/// Result of hardware topology detection.
pub struct Detection {
    /// Operating mode: "cpu", "gpu", "servicevm-nvl4", or "servicevm-nvl5"
    pub mode: &'static str,
    /// NVSwitch generation when present: "nvl4" or "nvl5"
    pub nvswitch: Option<&'static str>,
}

/// Detect NVRC mode from real sysfs paths.
pub fn detect() -> Detection {
    detect_from(PCI_DEVICES)
}

fn detect_from(pci_path: &str) -> Detection {
    let nvswitches = count_nvswitches_from(pci_path);
    let gpus = count_gpus_from(pci_path);
    let sw_mng = count_sw_mng_from(pci_path);

    debug!(
        "topology: {} GPU, {} NVSWITCH, {} PCI_SW_MNG",
        gpus, nvswitches, sw_mng
    );

    match (nvswitches, gpus, sw_mng) {
        (0, 0, 0) => {
            debug!("mode: cpu");
            Detection {
                mode: "cpu",
                nvswitch: None,
            }
        }
        (0, _, 0) => {
            debug!("mode: gpu {} GPU", gpus);
            Detection {
                mode: "gpu",
                nvswitch: None,
            }
        }
        (4, 8, 0) => {
            debug!(
                "mode: gpu FABRIC_MODE=0, {} GPU + {} NVSWITCH",
                gpus, nvswitches
            );
            Detection {
                mode: "gpu",
                nvswitch: Some("nvl4"),
            }
        }
        (4, 0, 0) => {
            debug!("mode: servicevm-nvl4 FABRIC_MODE=1");
            Detection {
                mode: "servicevm-nvl4",
                nvswitch: Some("nvl4"),
            }
        }
        (0, 8, 4) => {
            debug!(
                "mode: gpu FABRIC_MODE=0, {} GPU + {} PCI_SW_MNG",
                gpus, sw_mng
            );
            Detection {
                mode: "gpu",
                nvswitch: Some("nvl5"),
            }
        }
        (0, 0, 4) => {
            debug!("mode: servicevm-nvl5 FABRIC_MODE=1");
            Detection {
                mode: "servicevm-nvl5",
                nvswitch: Some("nvl5"),
            }
        }
        _ => {
            panic!(
                "unexpected topology: {} NVSWITCH, {} GPU, {} PCI_SW_MNG — cannot determine mode",
                nvswitches, gpus, sw_mng
            );
        }
    }
}

fn count_nvswitches_from(pci_path: &str) -> usize {
    let Ok(entries) = fs::read_dir(pci_path) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            let vendor = fs::read_to_string(e.path().join("vendor")).unwrap_or_default();
            let class = fs::read_to_string(e.path().join("class")).unwrap_or_default();
            vendor.trim() == "0x10de" && class.trim().starts_with("0x0680")
        })
        .count()
}

fn count_gpus_from(pci_path: &str) -> usize {
    let Ok(entries) = fs::read_dir(pci_path) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            let vendor = fs::read_to_string(e.path().join("vendor")).unwrap_or_default();
            let class = fs::read_to_string(e.path().join("class")).unwrap_or_default();
            vendor.trim() == "0x10de" && class.trim().starts_with("0x03")
        })
        .count()
}

/// Count NVLink management NICs (SW_MNG marker in PCI VPD).
/// Scans Mellanox (0x15b3) PCI devices and checks VPD directly,
/// avoiding dependency on IB drivers being loaded.
fn count_sw_mng_from(pci_path: &str) -> usize {
    let Ok(entries) = fs::read_dir(pci_path) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            let vendor = fs::read_to_string(e.path().join("vendor")).unwrap_or_default();
            if vendor.trim() != "0x15b3" {
                return false;
            }
            fs::read(e.path().join("vpd"))
                .map(|data| data.windows(6).any(|w| w == b"SW_MNG"))
                .unwrap_or(false)
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::panic;
    use tempfile::TempDir;

    fn create_pci_device(dir: &TempDir, name: &str, vendor: &str, class: &str) {
        let dev = dir.path().join(name);
        fs::create_dir_all(&dev).unwrap();
        fs::write(dev.join("vendor"), vendor).unwrap();
        fs::write(dev.join("class"), class).unwrap();
    }

    fn create_mlx_pci_device(dir: &TempDir, name: &str, vpd_content: &[u8]) {
        let dev = dir.path().join(name);
        fs::create_dir_all(&dev).unwrap();
        fs::write(dev.join("vendor"), "0x15b3\n").unwrap();
        fs::write(dev.join("vpd"), vpd_content).unwrap();
    }

    // --- NVSwitch counting ---

    #[test]
    fn test_count_nvswitches_single() {
        let tmpdir = TempDir::new().unwrap();
        create_pci_device(&tmpdir, "0000:00:00.0", "0x10de\n", "0x068000\n");
        assert_eq!(count_nvswitches_from(tmpdir.path().to_str().unwrap()), 1);
    }

    #[test]
    fn test_count_nvswitches_four() {
        let tmpdir = TempDir::new().unwrap();
        for i in 0..4 {
            create_pci_device(
                &tmpdir,
                &format!("0000:0{}:00.0", i),
                "0x10de\n",
                "0x068000\n",
            );
        }
        assert_eq!(count_nvswitches_from(tmpdir.path().to_str().unwrap()), 4);
    }

    #[test]
    fn test_count_nvswitches_skips_gpus() {
        let tmpdir = TempDir::new().unwrap();
        create_pci_device(&tmpdir, "0000:00:00.0", "0x10de\n", "0x068000\n");
        create_pci_device(&tmpdir, "0000:41:00.0", "0x10de\n", "0x030200\n");
        assert_eq!(count_nvswitches_from(tmpdir.path().to_str().unwrap()), 1);
    }

    #[test]
    fn test_count_nvswitches_skips_non_nvidia() {
        let tmpdir = TempDir::new().unwrap();
        create_pci_device(&tmpdir, "0000:00:00.0", "0x10de\n", "0x068000\n");
        create_pci_device(&tmpdir, "0000:01:00.0", "0x8086\n", "0x068000\n");
        assert_eq!(count_nvswitches_from(tmpdir.path().to_str().unwrap()), 1);
    }

    #[test]
    fn test_count_nvswitches_empty() {
        let tmpdir = TempDir::new().unwrap();
        assert_eq!(count_nvswitches_from(tmpdir.path().to_str().unwrap()), 0);
    }

    #[test]
    fn test_count_nvswitches_nonexistent() {
        assert_eq!(count_nvswitches_from("/nonexistent/path"), 0);
    }

    // --- GPU counting ---

    #[test]
    fn test_count_gpus_single() {
        let tmpdir = TempDir::new().unwrap();
        create_pci_device(&tmpdir, "0000:41:00.0", "0x10de\n", "0x030200\n");
        assert_eq!(count_gpus_from(tmpdir.path().to_str().unwrap()), 1);
    }

    #[test]
    fn test_count_gpus_multiple() {
        let tmpdir = TempDir::new().unwrap();
        for i in 0..8 {
            create_pci_device(
                &tmpdir,
                &format!("0000:4{}:00.0", i),
                "0x10de\n",
                "0x030200\n",
            );
        }
        assert_eq!(count_gpus_from(tmpdir.path().to_str().unwrap()), 8);
    }

    #[test]
    fn test_count_gpus_skips_nvswitches() {
        let tmpdir = TempDir::new().unwrap();
        create_pci_device(&tmpdir, "0000:41:00.0", "0x10de\n", "0x030200\n");
        create_pci_device(&tmpdir, "0000:00:00.0", "0x10de\n", "0x068000\n");
        assert_eq!(count_gpus_from(tmpdir.path().to_str().unwrap()), 1);
    }

    // --- SW_MNG device counting (PCI-based) ---

    #[test]
    fn test_count_sw_mng_single() {
        let tmpdir = TempDir::new().unwrap();
        create_mlx_pci_device(&tmpdir, "0000:b1:00.0", b"some data SW_MNG more data");
        assert_eq!(count_sw_mng_from(tmpdir.path().to_str().unwrap()), 1);
    }

    #[test]
    fn test_count_sw_mng_four() {
        let tmpdir = TempDir::new().unwrap();
        for i in 0..4 {
            create_mlx_pci_device(&tmpdir, &format!("0000:b{}:00.0", i), b"SW_MNG");
        }
        assert_eq!(count_sw_mng_from(tmpdir.path().to_str().unwrap()), 4);
    }

    #[test]
    fn test_count_sw_mng_skips_non_sw_mng() {
        let tmpdir = TempDir::new().unwrap();
        create_mlx_pci_device(&tmpdir, "0000:b1:00.0", b"SW_MNG");
        create_mlx_pci_device(&tmpdir, "0000:b2:00.0", b"some other data");
        assert_eq!(count_sw_mng_from(tmpdir.path().to_str().unwrap()), 1);
    }

    #[test]
    fn test_count_sw_mng_skips_non_mellanox() {
        let tmpdir = TempDir::new().unwrap();
        create_mlx_pci_device(&tmpdir, "0000:b1:00.0", b"SW_MNG");
        // Non-Mellanox device with SW_MNG in VPD
        let dev = tmpdir.path().join("0000:b2:00.0");
        fs::create_dir_all(&dev).unwrap();
        fs::write(dev.join("vendor"), "0x10de\n").unwrap();
        fs::write(dev.join("vpd"), b"SW_MNG").unwrap();
        assert_eq!(count_sw_mng_from(tmpdir.path().to_str().unwrap()), 1);
    }

    #[test]
    fn test_count_sw_mng_no_vpd_file() {
        let tmpdir = TempDir::new().unwrap();
        let dev = tmpdir.path().join("0000:b1:00.0");
        fs::create_dir_all(&dev).unwrap();
        fs::write(dev.join("vendor"), "0x15b3\n").unwrap();
        // No vpd file
        assert_eq!(count_sw_mng_from(tmpdir.path().to_str().unwrap()), 0);
    }

    #[test]
    fn test_count_sw_mng_no_pci_dir() {
        assert_eq!(count_sw_mng_from("/nonexistent/path"), 0);
    }

    #[test]
    fn test_count_sw_mng_empty_dir() {
        let tmpdir = TempDir::new().unwrap();
        assert_eq!(count_sw_mng_from(tmpdir.path().to_str().unwrap()), 0);
    }

    // --- Mode detection ---

    #[test]
    fn test_detect_cpu_mode() {
        let pci = TempDir::new().unwrap();
        let d = detect_from(pci.path().to_str().unwrap());
        assert_eq!(d.mode, "cpu");
        assert!(d.nvswitch.is_none());
    }

    #[test]
    fn test_detect_gpu_mode() {
        let pci = TempDir::new().unwrap();
        create_pci_device(&pci, "0000:41:00.0", "0x10de\n", "0x030200\n");
        let d = detect_from(pci.path().to_str().unwrap());
        assert_eq!(d.mode, "gpu");
        assert!(d.nvswitch.is_none());
    }

    #[test]
    fn test_detect_gpu_bare_metal_nvl4() {
        let pci = TempDir::new().unwrap();
        for i in 0..4 {
            create_pci_device(&pci, &format!("0000:0{}:00.0", i), "0x10de\n", "0x068000\n");
        }
        for i in 0..8 {
            create_pci_device(&pci, &format!("0000:4{}:00.0", i), "0x10de\n", "0x030200\n");
        }
        let d = detect_from(pci.path().to_str().unwrap());
        assert_eq!(d.mode, "gpu");
        assert_eq!(d.nvswitch, Some("nvl4"));
    }

    #[test]
    fn test_detect_gpu_bare_metal_nvl5() {
        let pci = TempDir::new().unwrap();
        // 8 GPUs + 4 CX7 PFs (all SW_MNG) on PCIe, no NVSwitches
        for i in 0..8 {
            create_pci_device(&pci, &format!("0000:4{}:00.0", i), "0x10de\n", "0x030200\n");
        }
        for i in 0..4 {
            create_mlx_pci_device(&pci, &format!("0000:ab:00.{}", i), b"SW_MNG");
        }
        let d = detect_from(pci.path().to_str().unwrap());
        assert_eq!(d.mode, "gpu");
        assert_eq!(d.nvswitch, Some("nvl5"));
    }

    #[test]
    fn test_detect_servicevm_nvl4() {
        let pci = TempDir::new().unwrap();
        for i in 0..4 {
            create_pci_device(&pci, &format!("0000:0{}:00.0", i), "0x10de\n", "0x068000\n");
        }
        let d = detect_from(pci.path().to_str().unwrap());
        assert_eq!(d.mode, "servicevm-nvl4");
        assert_eq!(d.nvswitch, Some("nvl4"));
    }

    #[test]
    fn test_detect_servicevm_nvl5() {
        let pci = TempDir::new().unwrap();
        // NVL5: no NVSwitches or GPUs on PCIe, only 4 CX7 PFs (all SW_MNG)
        for i in 0..4 {
            create_mlx_pci_device(&pci, &format!("0000:ab:00.{}", i), b"SW_MNG");
        }
        let d = detect_from(pci.path().to_str().unwrap());
        assert_eq!(d.mode, "servicevm-nvl5");
        assert_eq!(d.nvswitch, Some("nvl5"));
    }

    #[test]
    fn test_detect_unexpected_topology_panics() {
        let pci = TempDir::new().unwrap();
        // 2 NVSwitches + 3 GPUs — not a known topology
        for i in 0..2 {
            create_pci_device(&pci, &format!("0000:0{}:00.0", i), "0x10de\n", "0x068000\n");
        }
        for i in 0..3 {
            create_pci_device(&pci, &format!("0000:4{}:00.0", i), "0x10de\n", "0x030200\n");
        }
        let result = panic::catch_unwind(|| {
            detect_from(pci.path().to_str().unwrap());
        });
        assert!(result.is_err());
    }
}
