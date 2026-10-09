// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use log::{debug, warn};
use once_cell::sync::OnceCell;
use std::fs;

use crate::nvrc::NVRC;

const CMDLINE: &str = "/proc/cmdline";

static CMDLINE_CONTENT: OnceCell<String> = OnceCell::new();

/// The kernel command line, read once.
pub fn kernel_cmdline() -> std::io::Result<&'static str> {
    CMDLINE_CONTENT
        .get_or_try_init(|| fs::read_to_string(CMDLINE))
        .map(String::as_str)
}

/// Kernel parameters use various boolean representations (on/off, true/false, 1/0, yes/no).
/// Normalize them to a single bool to simplify downstream logic.
fn parse_boolean(s: &str) -> bool {
    match s.to_ascii_lowercase().as_str() {
        "on" | "true" | "1" | "yes" => true,
        "off" | "false" | "0" | "no" => false,
        _ => {
            warn!("unrecognized boolean '{}', defaulting to false", s);
            false
        }
    }
}

/// Numeric nvidia-smi setting (MHz or Watts) applied to all GPUs.
fn parse_u32(key: &str, what: &str, value: &str) -> Result<u32, String> {
    let n = value
        .parse()
        .map_err(|e| format!("{key}: invalid {what}: {e}"))?;
    debug!("{key}: {n} (all GPUs)");
    Ok(n)
}

impl NVRC {
    /// Parse kernel command line parameters to configure NVRC behavior.
    /// Using kernel params allows configuration without userspace tools—critical
    /// for a minimal init where no config files or environment variables exist.
    pub fn process_kernel_params(&mut self, cmdline: Option<&str>) {
        self.try_process_kernel_params(cmdline)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// Result twin of [`Self::process_kernel_params`] for the fuzz target:
    /// cargo-fuzz builds with panic=abort, so catch_unwind cannot filter the
    /// expected validation panics there.
    pub fn try_process_kernel_params(&mut self, cmdline: Option<&str>) -> Result<(), String> {
        let content = match cmdline {
            Some(c) => c,
            None => kernel_cmdline().map_err(|e| format!("read {CMDLINE}: {e}"))?,
        };

        for (k, v) in content.split_whitespace().filter_map(|p| p.split_once('=')) {
            match k {
                "nvrc.log" => nvrc_log(v)?,
                "nvrc.uvm.persistence.mode" => uvm_persistenced_mode(v, self),
                "nvrc.uvm.tools" => {
                    self.uvm_tools_enabled = parse_boolean(v);
                    debug!("nvrc.uvm.tools: {}", self.uvm_tools_enabled);
                }
                "nvrc.dcgm" => nvrc_dcgm(v, self),

                "nvrc.smi.srs" => nvidia_smi_srs(v, self),
                "nvrc.smi.lgc" => self.nvidia_smi_lgc = Some(parse_u32(k, "frequency", v)?),
                "nvrc.smi.lmc" => self.nvidia_smi_lmc = Some(parse_u32(k, "frequency", v)?),
                "nvrc.smi.pl" => self.nvidia_smi_pl = Some(parse_u32(k, "wattage", v)?),
                _ => {}
            }
        }
        Ok(())
    }
}

/// DCGM (Data Center GPU Manager) provides telemetry and health monitoring.
/// Off by default—only enable when observability infrastructure expects it.
fn nvrc_dcgm(value: &str, ctx: &mut NVRC) {
    let dcgm = parse_boolean(value);
    ctx.dcgm_enabled = Some(dcgm);
    debug!("nvrc.dcgm: {dcgm}");
}

/// Control log verbosity at runtime. Defaults to off to minimize noise.
/// Enabling devkmsg allows kernel log output even in minimal init environments.
fn nvrc_log(value: &str) -> Result<(), String> {
    let lvl = match value.to_ascii_lowercase().as_str() {
        "off" | "0" | "" => log::LevelFilter::Off,
        "error" => log::LevelFilter::Error,
        "warn" => log::LevelFilter::Warn,
        "info" => log::LevelFilter::Info,
        "debug" => log::LevelFilter::Debug,
        "trace" => log::LevelFilter::Trace,
        _ => log::LevelFilter::Off,
    };

    log::set_max_level(lvl);
    debug!("nvrc.log: {}", log::max_level());
    fs::write("/proc/sys/kernel/printk_devkmsg", b"on\n")
        .map_err(|e| format!("printk_devkmsg: {e}"))
}

/// Secure Randomization Seed for GPU memory. Passed directly to nvidia-smi.
fn nvidia_smi_srs(value: &str, ctx: &mut NVRC) {
    ctx.nvidia_smi_srs = Some(value.to_owned());
    debug!("nvidia_smi_srs: {value}");
}

/// UVM persistence mode keeps unified memory state across CUDA context teardowns.
/// Reduces initialization overhead for short-lived CUDA applications.
fn uvm_persistenced_mode(value: &str, ctx: &mut NVRC) {
    let enabled = parse_boolean(value);
    ctx.uvm_persistence_mode = Some(enabled);
    debug!("nvrc.uvm.persistence.mode: {enabled}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use serial_test::serial;
    use std::panic;
    use std::sync::Once;

    static LOG: Once = Once::new();

    fn log_setup() {
        // kmsg's tests install the logger too; whichever runs first wins.
        LOG.call_once(|| {
            let _ = kernlog::init();
        });
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    #[cfg_attr(not(miri), serial)]
    fn test_nvrc_log_debug() {
        require_root();
        log_setup();

        nvrc_log("debug").unwrap();
        assert!(log_enabled!(log::Level::Debug));
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    #[cfg_attr(not(miri), serial)]
    fn test_process_kernel_params_nvrc_log_debug() {
        require_root();
        log_setup();
        let mut init = NVRC::default();

        init.process_kernel_params(Some(
            "nvidia.smi.lgc=1500 nvrc.log=debug nvidia.smi.lgc=1500",
        ));

        assert_eq!(log::max_level(), log::LevelFilter::Debug);
        assert!(!log_enabled!(log::Level::Trace));
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    #[cfg_attr(not(miri), serial)]
    fn test_process_kernel_params_nvrc_log_info() {
        require_root();
        log_setup();
        let mut init = NVRC::default();

        init.process_kernel_params(Some(
            "nvidia.smi.lgc=1500 nvrc.log=info nvidia.smi.lgc=1500",
        ));

        assert_eq!(log::max_level(), log::LevelFilter::Info);
        assert!(!log_enabled!(log::Level::Debug));
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    #[cfg_attr(not(miri), serial)]
    fn test_process_kernel_params_nvrc_log_0() {
        require_root();
        log_setup();
        let mut init = NVRC::default();

        init.process_kernel_params(Some("nvidia.smi.lgc=1500 nvrc.log=0 nvidia.smi.lgc=1500"));
        assert_eq!(log::max_level(), log::LevelFilter::Off);
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    #[cfg_attr(not(miri), serial)]
    fn test_process_kernel_params_nvrc_log_none() {
        require_root();
        log_setup();
        let mut init = NVRC::default();

        init.process_kernel_params(Some("nvidia.smi.lgc=1500 nvrc.log= "));
        assert_eq!(log::max_level(), log::LevelFilter::Off);
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    #[cfg_attr(not(miri), serial)]
    fn test_process_kernel_params_nvrc_log_trace() {
        require_root();
        log_setup();
        let mut init = NVRC::default();

        init.process_kernel_params(Some("nvrc.log=trace"));
        assert_eq!(log::max_level(), log::LevelFilter::Trace);
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    #[cfg_attr(not(miri), serial)]
    fn test_process_kernel_params_nvrc_log_unknown() {
        require_root();
        log_setup();
        let mut init = NVRC::default();

        // Unknown log level should default to Off
        init.process_kernel_params(Some("nvrc.log=garbage"));
        assert_eq!(log::max_level(), log::LevelFilter::Off);
    }

    #[test]
    fn test_nvrc_dcgm_parameter_handling() {
        let mut c = NVRC::default();

        // Test various "on" values
        nvrc_dcgm("on", &mut c);
        assert_eq!(c.dcgm_enabled, Some(true));

        nvrc_dcgm("true", &mut c);
        assert_eq!(c.dcgm_enabled, Some(true));

        nvrc_dcgm("1", &mut c);
        assert_eq!(c.dcgm_enabled, Some(true));

        nvrc_dcgm("yes", &mut c);
        assert_eq!(c.dcgm_enabled, Some(true));

        // Test "off" values
        nvrc_dcgm("off", &mut c);
        assert_eq!(c.dcgm_enabled, Some(false));

        nvrc_dcgm("false", &mut c);
        assert_eq!(c.dcgm_enabled, Some(false));

        nvrc_dcgm("invalid", &mut c);
        assert_eq!(c.dcgm_enabled, Some(false));
    }

    #[test]
    fn test_nvidia_smi_srs() {
        let mut c = NVRC::default();

        nvidia_smi_srs("enabled", &mut c);
        assert_eq!(c.nvidia_smi_srs, Some("enabled".to_owned()));

        nvidia_smi_srs("disabled", &mut c);
        assert_eq!(c.nvidia_smi_srs, Some("disabled".to_owned()));
    }

    #[test]
    fn test_uvm_persistenced_mode() {
        let mut c = NVRC::default();

        uvm_persistenced_mode("on", &mut c);
        assert_eq!(c.uvm_persistence_mode, Some(true));

        uvm_persistenced_mode("OFF", &mut c);
        assert_eq!(c.uvm_persistence_mode, Some(false));

        uvm_persistenced_mode("True", &mut c);
        assert_eq!(c.uvm_persistence_mode, Some(true));
    }

    #[test]
    fn uvm_tools_defaults_to_disabled() {
        for cmdline in [
            "",
            "nvrc.uvm.tools",
            "nvrc.uvm.tools=",
            "nvrc.uvm.tools=0",
            "nvrc.uvm.tools=2",
            "nvrc.uvm.tools=01",
            "nvrc.uvm.tools=invalid",
            "other.nvrc.uvm.tools=1",
            "nvrc.uvm.persistence.mode=1 nvrc.dcgm=1",
        ] {
            let mut nvrc = NVRC::default();
            nvrc.process_kernel_params(Some(cmdline));
            assert!(!nvrc.uvm_tools_enabled, "{cmdline}");
        }
    }

    #[test]
    fn uvm_tools_accepts_boolean_aliases() {
        for (value, expected) in [
            ("1", true),
            ("true", true),
            ("on", true),
            ("yes", true),
            ("TRUE", true),
            ("On", true),
            ("YES", true),
            ("0", false),
            ("false", false),
            ("off", false),
            ("no", false),
            ("FALSE", false),
            ("Off", false),
            ("NO", false),
        ] {
            let mut nvrc = NVRC::default();
            nvrc.uvm_tools_enabled = !expected;
            nvrc.process_kernel_params(Some(&format!("nvrc.uvm.tools={value}")));
            assert_eq!(nvrc.uvm_tools_enabled, expected, "{value}");
        }
    }

    #[test]
    fn last_uvm_tools_parameter_wins() {
        for (cmdline, expected) in [
            ("nvrc.uvm.tools=0 nvrc.uvm.tools=1", true),
            ("nvrc.uvm.tools=1 nvrc.uvm.tools=0", false),
            ("nvrc.uvm.tools=0 nvrc.uvm.tools=true", true),
            ("nvrc.uvm.tools=yes nvrc.uvm.tools=OFF", false),
            ("nvrc.uvm.tools=1 nvrc.uvm.tools=invalid", false),
            ("nvrc.uvm.tools=1 nvrc.uvm.tools=", false),
        ] {
            let mut nvrc = NVRC::default();
            nvrc.process_kernel_params(Some(cmdline));
            assert_eq!(nvrc.uvm_tools_enabled, expected, "{cmdline}");
        }
    }

    #[test]
    fn test_parse_boolean() {
        assert!(parse_boolean("on"));
        assert!(parse_boolean("true"));
        assert!(parse_boolean("1"));
        assert!(parse_boolean("yes"));
        assert!(parse_boolean("ON"));
        assert!(parse_boolean("True"));
        assert!(parse_boolean("YES"));

        assert!(!parse_boolean("off"));
        assert!(!parse_boolean("false"));
        assert!(!parse_boolean("0"));
        assert!(!parse_boolean("no"));
        assert!(!parse_boolean("invalid"));
        assert!(!parse_boolean(""));
    }

    #[test]
    fn test_kernel_cmdline_is_read_once() {
        let first = kernel_cmdline().unwrap();
        let second = kernel_cmdline().unwrap();
        assert!(std::ptr::eq(first, second));
        assert_eq!(first, fs::read_to_string(CMDLINE).unwrap());
    }

    #[test]
    fn test_parse_u32() {
        assert_eq!(parse_u32("nvrc.smi.lgc", "frequency", "1500"), Ok(1500));
        assert_eq!(parse_u32("nvrc.smi.pl", "wattage", "0"), Ok(0));
    }

    #[test]
    fn test_parse_u32_error_names_key_and_quantity() {
        let err = parse_u32("nvrc.smi.pl", "wattage", "abc").unwrap_err();
        assert!(err.starts_with("nvrc.smi.pl: invalid wattage: "), "{err}");
        assert!(parse_u32("nvrc.smi.lmc", "frequency", "-1").is_err());
        assert!(parse_u32("nvrc.smi.lmc", "frequency", "").is_err());
    }

    #[test]
    fn test_process_kernel_params_gpu_settings() {
        let mut c = NVRC::default();

        c.process_kernel_params(Some("nvrc.smi.lgc=1500 nvrc.smi.lmc=5001 nvrc.smi.pl=300"));

        assert_eq!(c.nvidia_smi_lgc, Some(1500));
        assert_eq!(c.nvidia_smi_lmc, Some(5001));
        assert_eq!(c.nvidia_smi_pl, Some(300));
    }

    #[test]
    fn test_try_process_kernel_params_invalid_lgc_is_err() {
        assert!(NVRC::default()
            .try_process_kernel_params(Some("nvrc.smi.lgc=bad"))
            .is_err());
    }

    #[test]
    fn test_try_process_kernel_params_invalid_lmc_is_err() {
        assert!(NVRC::default()
            .try_process_kernel_params(Some("nvrc.smi.lmc=bad"))
            .is_err());
    }

    #[test]
    fn test_try_process_kernel_params_invalid_pl_is_err() {
        assert!(NVRC::default()
            .try_process_kernel_params(Some("nvrc.smi.pl=bad"))
            .is_err());
    }

    #[test]
    fn test_process_kernel_params_invalid_lgc_panics() {
        let result = panic::catch_unwind(|| {
            NVRC::default().process_kernel_params(Some("nvrc.smi.lgc=bad"));
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_process_kernel_params_combined() {
        let mut c = NVRC::default();

        c.process_kernel_params(Some(
            "nvrc.smi.lgc=2100 nvrc.uvm.options=opt1=1,opt2=2 nvrc.dcgm=on nvrc.smi.pl=400",
        ));

        assert_eq!(c.nvidia_smi_lgc, Some(2100));
        assert_eq!(c.nvidia_smi_pl, Some(400));
        assert_eq!(c.dcgm_enabled, Some(true));
    }

    #[test]
    fn test_process_kernel_params_from_proc_cmdline() {
        // Exercise the None path which reads /proc/cmdline.
        // We can't control the contents but can verify it doesn't panic.
        let mut c = NVRC::default();
        c.process_kernel_params(None);
    }

    #[test]
    fn test_process_kernel_params_with_uvm_and_srs() {
        let mut c = NVRC::default();

        c.process_kernel_params(Some("nvrc.uvm.persistence.mode=true nvrc.smi.srs=enabled"));

        assert_eq!(c.uvm_persistence_mode, Some(true));
        assert_eq!(c.nvidia_smi_srs, Some("enabled".to_owned()));
    }
}
