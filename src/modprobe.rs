use std::fs;

use crate::execute::foreground;

const MODPROBE: &str = "/sbin/modprobe";

/// Load a kernel module via modprobe.
/// For nvidia, automatically disables NVLink when only one GPU is present.
pub fn load(module: &str) {
    let mut args = vec![module];
    if module == "nvidia" && count_nvidia_gpus_from("/sys/bus/pci/devices") == 1 {
        args.push("NVreg_NvLinkDisable=1");
    }
    foreground(MODPROBE, &args);
}

fn count_nvidia_gpus_from(pci_path: &str) -> usize {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use serial_test::serial;
    use std::panic;
    use tempfile::TempDir;

    // Kernel module loading must be serialized - parallel modprobe
    // calls can race and cause spurious failures.

    #[test]
    #[serial]
    fn test_load_loop() {
        require_root();
        load("loop");
    }

    #[test]
    #[serial]
    fn test_load_nonexistent() {
        require_root();
        let result = panic::catch_unwind(|| {
            load("nonexistent_module_xyz123");
        });
        assert!(result.is_err());
    }

    fn create_pci_device(tmpdir: &TempDir, name: &str, vendor: &str, class: &str) {
        let dev = tmpdir.path().join(name);
        fs::create_dir_all(&dev).unwrap();
        fs::write(dev.join("vendor"), vendor).unwrap();
        fs::write(dev.join("class"), class).unwrap();
    }

    #[test]
    fn test_count_nvidia_gpus_single() {
        let tmpdir = TempDir::new().unwrap();
        create_pci_device(&tmpdir, "0000:41:00.0", "0x10de\n", "0x030200\n");
        assert_eq!(count_nvidia_gpus_from(tmpdir.path().to_str().unwrap()), 1);
    }

    #[test]
    fn test_count_nvidia_gpus_multiple() {
        let tmpdir = TempDir::new().unwrap();
        create_pci_device(&tmpdir, "0000:41:00.0", "0x10de\n", "0x030200\n");
        create_pci_device(&tmpdir, "0000:42:00.0", "0x10de\n", "0x030000\n");
        assert_eq!(count_nvidia_gpus_from(tmpdir.path().to_str().unwrap()), 2);
    }

    #[test]
    fn test_count_nvidia_gpus_skips_non_gpu() {
        let tmpdir = TempDir::new().unwrap();
        create_pci_device(&tmpdir, "0000:41:00.0", "0x10de\n", "0x030200\n");
        // NVIDIA audio device (class 0x0403)
        create_pci_device(&tmpdir, "0000:41:00.1", "0x10de\n", "0x040300\n");
        // Non-NVIDIA device
        create_pci_device(&tmpdir, "0000:00:02.0", "0x8086\n", "0x030000\n");
        assert_eq!(count_nvidia_gpus_from(tmpdir.path().to_str().unwrap()), 1);
    }

    #[test]
    fn test_count_nvidia_gpus_empty() {
        let tmpdir = TempDir::new().unwrap();
        assert_eq!(count_nvidia_gpus_from(tmpdir.path().to_str().unwrap()), 0);
    }

    #[test]
    fn test_count_nvidia_gpus_nonexistent() {
        assert_eq!(count_nvidia_gpus_from("/nonexistent/path"), 0);
    }
}
