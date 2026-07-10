use std::fs;

use crate::execute::foreground;
use crate::gpu_extension;

const MODPROBE: &str = "/sbin/modprobe";

/// Load a kernel module. Disables NVLink for single-GPU nvidia; NVIDIA modules
/// come from the `gpu` extension (`--dirname`) when present.
pub fn load(module: &str) {
    let single_gpu = module == "nvidia" && count_nvidia_gpus_from("/sys/bus/pci/devices") == 1;
    let dirname = gpu_extension::modprobe_dirname(module);
    let args = build_args(module, dirname.as_deref(), single_gpu);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    foreground(MODPROBE, &arg_refs);
}

fn build_args(module: &str, dirname: Option<&str>, single_gpu: bool) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(dir) = dirname {
        args.push("--dirname".to_owned());
        args.push(dir.to_owned());
    }
    args.push(module.to_owned());
    if single_gpu {
        args.push("NVreg_NvLinkDisable=1".to_owned());
    }
    args
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
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    #[cfg_attr(not(miri), serial)]
    fn test_load_loop() {
        require_root();
        load("loop");
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    #[cfg_attr(not(miri), serial)]
    fn test_load_nonexistent() {
        require_root();
        let result = panic::catch_unwind(|| {
            load("nonexistent_module_xyz123");
        });
        assert!(result.is_err());
    }

    // === build_args ===

    #[test]
    fn test_build_args_plain() {
        assert_eq!(build_args("erofs", None, false), vec!["erofs"]);
    }

    #[test]
    fn test_build_args_single_gpu() {
        assert_eq!(
            build_args("nvidia", None, true),
            vec!["nvidia", "NVreg_NvLinkDisable=1"]
        );
    }

    #[test]
    fn test_build_args_extension_dirname() {
        assert_eq!(
            build_args("nvidia", Some(gpu_extension::ROOT), false),
            vec!["--dirname", gpu_extension::ROOT, "nvidia"]
        );
    }

    #[test]
    fn test_build_args_extension_dirname_single_gpu() {
        assert_eq!(
            build_args("nvidia", Some(gpu_extension::ROOT), true),
            vec![
                "--dirname",
                gpu_extension::ROOT,
                "nvidia",
                "NVreg_NvLinkDisable=1"
            ]
        );
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
