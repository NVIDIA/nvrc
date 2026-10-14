// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use log::{debug, warn};
use std::fs;

use crate::nvrc::NVRC;

/// Kernel parameters use various boolean representations (on/off, true/false, 1/0, yes/no).
/// Normalize them to a single bool to simplify downstream logic.
fn parse_boolean(s: &str) -> bool {
    parse_boolean_strict(s).unwrap_or_else(|e| {
        warn!("{e}, defaulting to false");
        false
    })
}

/// eq_ignore_ascii_case over to_ascii_lowercase: no allocation per parameter
/// in PID 1, and the symbolic-length String does not converge under CBMC.
fn parse_boolean_strict(s: &str) -> Result<bool, String> {
    const TRUE: [&str; 4] = ["on", "true", "1", "yes"];
    const FALSE: [&str; 4] = ["off", "false", "0", "no"];
    if TRUE.iter().any(|t| s.eq_ignore_ascii_case(t)) {
        Ok(true)
    } else if FALSE.iter().any(|t| s.eq_ignore_ascii_case(t)) {
        Ok(false)
    } else {
        Err(format!("unrecognized boolean '{s}'"))
    }
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
            Some(c) => c.to_owned(),
            None => fs::read_to_string("/proc/cmdline")
                .map_err(|e| format!("read /proc/cmdline: {e}"))?,
        };

        for (k, v) in content.split_whitespace().filter_map(|p| p.split_once('=')) {
            match k {
                "nvrc.log" => nvrc_log(v, self)?,
                "nvrc.uvm.persistence.mode" => uvm_persistenced_mode(v, self),
                "nvrc.uvm.tools" => {
                    self.uvm_tools_enabled = parse_boolean(v);
                    debug!("nvrc.uvm.tools: {}", self.uvm_tools_enabled);
                }
                "nvrc.dcgm" => nvrc_dcgm(v, self),

                "nvrc.smi.srs" => nvidia_smi_srs(v, self)?,
                "nvrc.smi.lgc" => nvidia_smi_lgc(v, self)?,
                "nvrc.smi.lmc" => nvidia_smi_lmc(v, self)?,
                "nvrc.smi.pl" => nvidia_smi_pl(v, self)?,
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
fn nvrc_log(value: &str, _ctx: &mut NVRC) -> Result<(), String> {
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

/// nvidia-smi only accepts 0 or 1 for the ready state, so an unrecognized
/// value fails here instead of leaving every GPU silently NotReady.
fn nvidia_smi_srs(value: &str, ctx: &mut NVRC) -> Result<(), String> {
    let ready = parse_boolean_strict(value).map_err(|e| format!("nvrc.smi.srs: {e}"))?;
    debug!("nvrc.smi.srs: {ready}");
    ctx.nvidia_smi_srs = Some(ready);
    Ok(())
}

/// Lock GPU core clocks to a fixed frequency (MHz) for consistent performance.
/// Eliminates thermal/power throttling variance in benchmarks and latency-sensitive workloads.
fn nvidia_smi_lgc(value: &str, ctx: &mut NVRC) -> Result<(), String> {
    let mhz: u32 = value
        .parse()
        .map_err(|e| format!("nvrc.smi.lgc: invalid frequency: {e}"))?;
    debug!("nvrc.smi.lgc: {} MHz (all GPUs)", mhz);
    ctx.nvidia_smi_lgc = Some(mhz);
    Ok(())
}

/// Lock memory clocks to a fixed frequency (MHz).
/// Used alongside lgc for fully deterministic GPU behavior.
fn nvidia_smi_lmc(value: &str, ctx: &mut NVRC) -> Result<(), String> {
    let mhz: u32 = value
        .parse()
        .map_err(|e| format!("nvrc.smi.lmc: invalid frequency: {e}"))?;
    debug!("nvrc.smi.lmc: {} MHz (all GPUs)", mhz);
    ctx.nvidia_smi_lmc = Some(mhz);
    Ok(())
}

/// Set GPU power limit (Watts). Lower limits reduce heat/power, higher allows peak perf.
/// Useful for power-constrained environments or thermal management.
fn nvidia_smi_pl(value: &str, ctx: &mut NVRC) -> Result<(), String> {
    let watts: u32 = value
        .parse()
        .map_err(|e| format!("nvrc.smi.pl: invalid wattage: {e}"))?;
    debug!("nvrc.smi.pl: {} W (all GPUs)", watts);
    ctx.nvidia_smi_pl = Some(watts);
    Ok(())
}

/// UVM persistence mode keeps unified memory state across CUDA context teardowns.
/// Reduces initialization overhead for short-lived CUDA applications.
fn uvm_persistenced_mode(value: &str, ctx: &mut NVRC) {
    let enabled = parse_boolean(value);
    ctx.uvm_persistence_mode = Some(enabled);
    debug!("nvrc.uvm.persistence.mode: {enabled}");
}

/// Kani proof harnesses. NVRC panics by design on invalid state (panic ->
/// reboot), so the property is not "no panics" but "rejects exactly the
/// invalid values": a handler returns Err precisely for malformed input and
/// stores exactly the parsed value otherwise. `#[kani::should_panic]` cannot
/// express this (it passes if ANY input panics), hence assertions on the
/// Result twins. The stub set only drops error text and swaps core's memchr
/// for a byte loop, see kani_support.
#[cfg(kani)]
mod proofs {
    use super::*;
    use crate::kani_support::any_string;

    /// 8 bytes covers every accepted spelling (longest is "false") plus slack.
    const N: usize = 8;

    /// The eight documented spellings, pinned independently of the parser.
    /// Allocation-free like the parser: a symbolic-length String does not
    /// converge under CBMC.
    fn boolean_spec(s: &str) -> Option<bool> {
        const TRUE: [&str; 4] = ["on", "true", "1", "yes"];
        const FALSE: [&str; 4] = ["off", "false", "0", "no"];
        if TRUE.iter().any(|t| s.eq_ignore_ascii_case(t)) {
            Some(true)
        } else if FALSE.iter().any(|t| s.eq_ignore_ascii_case(t)) {
            Some(false)
        } else {
            None
        }
    }

    #[kani::proof]
    #[kani::unwind(9)]
    #[kani::use_stub_set(crate::kani_support::cbmc)]
    fn parse_boolean_strict_accepts_exactly_eight_spellings() {
        let s = any_string::<N>();
        let expected = boolean_spec(&s);
        assert_eq!(parse_boolean_strict(&s).ok(), expected);
        kani::cover!(expected == Some(true), "a true spelling is reachable");
        kani::cover!(expected == Some(false), "a false spelling is reachable");
        kani::cover!(expected.is_none(), "a rejected value is reachable");
    }

    /// Deny by default: an unrecognized value is false, never true, never an error.
    #[kani::proof]
    #[kani::unwind(9)]
    #[kani::use_stub_set(crate::kani_support::cbmc)]
    fn parse_boolean_denies_by_default() {
        let s = any_string::<N>();
        assert_eq!(parse_boolean(&s), boolean_spec(&s) == Some(true));
    }

    /// The lax handlers store the deny-by-default value and cannot fail boot.
    #[kani::proof]
    #[kani::unwind(9)]
    #[kani::use_stub_set(crate::kani_support::cbmc)]
    fn lax_boolean_handlers_store_parse_boolean() {
        let s = any_string::<N>();
        let mut c = NVRC::default();
        nvrc_dcgm(&s, &mut c);
        uvm_persistenced_mode(&s, &mut c);
        let expected = Some(parse_boolean(&s));
        assert_eq!(c.dcgm_enabled, expected);
        assert_eq!(c.uvm_persistence_mode, expected);
    }

    /// nvrc.smi.srs is strict: Err (a boot panic) outside the eight spellings,
    /// and a rejected value leaves the field untouched.
    #[kani::proof]
    #[kani::unwind(9)]
    #[kani::use_stub_set(crate::kani_support::cbmc)]
    fn smi_srs_rejects_exactly_non_booleans() {
        let s = any_string::<N>();
        let mut c = NVRC::default();
        let result = nvidia_smi_srs(&s, &mut c);
        let expected = boolean_spec(&s);
        assert_eq!(result.is_ok(), expected.is_some());
        assert_eq!(c.nvidia_smi_srs, expected);
    }

    /// Clock and power handlers are exactly `str::parse::<u32>`: Err (a boot
    /// panic) for every string u32 rejects, the parsed value stored otherwise.
    /// One harness per entry point so an edit to one parser cannot silently
    /// relax it.
    macro_rules! smi_u32_proof {
        ($name:ident, $handler:ident, $field:ident) => {
            #[kani::proof]
            #[kani::unwind(9)]
            #[kani::use_stub_set(crate::kani_support::cbmc)]
            fn $name() {
                let s = any_string::<N>();
                let mut c = NVRC::default();
                let result = $handler(&s, &mut c);
                let expected = s.parse::<u32>().ok();
                assert_eq!(result.is_ok(), expected.is_some());
                assert_eq!(c.$field, expected);
                kani::cover!(expected.is_some(), "an accepted value is reachable");
                kani::cover!(expected.is_none(), "a rejected value is reachable");
            }
        };
    }
    smi_u32_proof!(smi_lgc_is_exactly_u32_parse, nvidia_smi_lgc, nvidia_smi_lgc);
    smi_u32_proof!(smi_lmc_is_exactly_u32_parse, nvidia_smi_lmc, nvidia_smi_lmc);
    smi_u32_proof!(smi_pl_is_exactly_u32_parse, nvidia_smi_pl, nvidia_smi_pl);

    // process_kernel_params dispatch harnesses are out of reach: a symbolic
    // value through split_whitespace and split_once does not converge in
    // CI-viable time (12 min, ASCII-bounded, memchr stubbed). The handlers
    // are proven above; dispatch stays covered by the kernel_params fuzz target.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use serial_test::serial;
    use std::panic;
    use std::sync::{LazyLock, Once};

    static LOG: LazyLock<Once> = LazyLock::new(Once::new);

    fn log_setup() {
        LOG.call_once(|| {
            kernlog::init().unwrap();
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
        let mut c = NVRC::default();

        nvrc_log("debug", &mut c).unwrap();
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

        assert!(nvidia_smi_srs("1", &mut c).is_ok());
        assert_eq!(c.nvidia_smi_srs, Some(true));

        assert!(nvidia_smi_srs("off", &mut c).is_ok());
        assert_eq!(c.nvidia_smi_srs, Some(false));

        let err = nvidia_smi_srs("enabled", &mut c).unwrap_err();
        assert!(err.starts_with("nvrc.smi.srs:"), "{err}");
        assert_eq!(c.nvidia_smi_srs, Some(false));
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
    fn test_nvidia_smi_lgc() {
        let mut c = NVRC::default();

        assert!(nvidia_smi_lgc("1500", &mut c).is_ok());
        assert_eq!(c.nvidia_smi_lgc, Some(1500));

        assert!(nvidia_smi_lgc("2100", &mut c).is_ok());
        assert_eq!(c.nvidia_smi_lgc, Some(2100));

        assert!(nvidia_smi_lgc("invalid", &mut NVRC::default()).is_err());
    }

    #[test]
    fn test_nvidia_smi_lmc() {
        let mut c = NVRC::default();

        assert!(nvidia_smi_lmc("5001", &mut c).is_ok());
        assert_eq!(c.nvidia_smi_lmc, Some(5001));

        assert!(nvidia_smi_lmc("6000", &mut c).is_ok());
        assert_eq!(c.nvidia_smi_lmc, Some(6000));

        assert!(nvidia_smi_lmc("not_a_number", &mut NVRC::default()).is_err());
    }

    #[test]
    fn test_nvidia_smi_pl() {
        let mut c = NVRC::default();

        assert!(nvidia_smi_pl("300", &mut c).is_ok());
        assert_eq!(c.nvidia_smi_pl, Some(300));

        assert!(nvidia_smi_pl("450", &mut c).is_ok());
        assert_eq!(c.nvidia_smi_pl, Some(450));

        assert!(nvidia_smi_pl("abc", &mut NVRC::default()).is_err());
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

        c.process_kernel_params(Some("nvrc.uvm.persistence.mode=true nvrc.smi.srs=1"));

        assert_eq!(c.uvm_persistence_mode, Some(true));
        assert_eq!(c.nvidia_smi_srs, Some(true));
    }
}
