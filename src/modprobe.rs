use crate::execute::foreground;
use crate::gpu_extension;
use log::debug;
use std::path::Path;

const MODPROBE: &str = "/sbin/modprobe";

pub fn load(module: &str) {
    run(module, false)
}

/// Mode detection already knows whether the GPU has an NVLink peer.
pub fn load_nvidia(nvlink_disabled: bool) {
    run("nvidia", nvlink_disabled)
}

/// In-tree drivers may be built into the kernel, where no module tree exists
/// for modprobe to resolve them against. `marker` is the sysfs node the driver
/// registers once it is present, built in or loaded.
pub fn load_unless_present(module: &str, marker: &str) {
    if Path::new(marker).exists() {
        debug!("{module}: {marker} exists, not loading");
        return;
    }
    run(module, false)
}

/// The GPU extension supplies modules matching its userspace driver stack.
fn run(module: &str, nvlink_disabled: bool) {
    let dirname = gpu_extension::modprobe_dirname(module);
    let args = build_args(module, dirname.as_deref(), nvlink_disabled);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    foreground(MODPROBE, &arg_refs);
}

fn build_args(module: &str, dirname: Option<&str>, nvlink_disabled: bool) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(dir) = dirname {
        args.push("--dirname".to_owned());
        args.push(dir.to_owned());
    }
    args.push(module.to_owned());
    if nvlink_disabled {
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
    fn present_marker_skips_modprobe() {
        let tmpdir = tempfile::tempdir().unwrap();
        load_unless_present("nvrc-no-such-module", tmpdir.path().to_str().unwrap());
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    #[cfg_attr(not(miri), serial)]
    #[should_panic(expected = "modprobe")]
    fn missing_marker_loads_the_module() {
        let tmpdir = tempfile::tempdir().unwrap();
        let marker = tmpdir.path().join("absent");
        load_unless_present("nvrc-no-such-module", marker.to_str().unwrap());
    }

    #[test]
    fn test_build_args_nvlink_disabled() {
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
    fn test_build_args_extension_dirname_nvlink_disabled() {
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
}
