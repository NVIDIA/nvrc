use crate::execute::foreground;
use crate::gpu_extension;

const MODPROBE: &str = "/sbin/modprobe";

pub fn load(module: &str) {
    run(module, false)
}

/// Mode detection already knows whether the GPU has an NVLink peer.
pub fn load_nvidia(isolated_gpu: bool) {
    run("nvidia", isolated_gpu)
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
