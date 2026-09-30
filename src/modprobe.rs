use crate::macros::ResultExt;
use pcilibs_rs::{platform, Sysfs};

use crate::execute::foreground;
use crate::gpu_extension;

const MODPROBE: &str = "/sbin/modprobe";

/// The GPU extension supplies modules matching its userspace driver stack.
pub fn load(module: &str) {
    let single_gpu = module == "nvidia" && isolated_gpu(&Sysfs::default());
    let dirname = gpu_extension::modprobe_dirname(module);
    let args = build_args(module, dirname.as_deref(), single_gpu);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    foreground(MODPROBE, &arg_refs);
}

fn isolated_gpu(sysfs: &Sysfs) -> bool {
    let topology =
        platform::discover_topology(sysfs).or_panic("discover GPUs before loading nvidia");
    // A single assigned GPU may still need its assigned fabric.
    topology.gpus.len() == 1
        && topology.switches.is_empty()
        && topology.management_functions.is_empty()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use serial_test::serial;
    use std::panic;

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

    #[rstest::rstest]
    #[case(0, false, false, false)]
    #[case(1, false, false, true)]
    #[case(2, false, false, false)]
    #[case(1, true, false, false)]
    #[case(1, false, true, false)]
    fn nvlink_is_disabled_only_for_an_isolated_gpu(
        #[case] gpus: usize,
        #[case] direct_switch: bool,
        #[case] management_pf: bool,
        #[case] expected: bool,
    ) {
        let f = pcilibs_rs::testfs::fake();
        for i in 0..gpus {
            f.add_pci_device(
                &format!("0000:01:{i:02x}.0"),
                0x10de,
                0x2330,
                0x030200,
                None,
            );
        }
        if direct_switch {
            f.add_pci_device("0000:02:00.0", 0x10de, 0x22a3, 0x068000, None);
        }
        if management_pf {
            crate::test_utils::add_management_pf(&f, "0000:03:00.0", true);
        }
        assert_eq!(isolated_gpu(&f.sysfs), expected);
    }

    #[test]
    #[should_panic(expected = "discover GPUs before loading nvidia")]
    fn incomplete_scan_cannot_choose_driver_options() {
        let f = pcilibs_rs::testfs::fake();
        f.add_device("0000:01:00.0", None);
        isolated_gpu(&f.sysfs);
    }
}
