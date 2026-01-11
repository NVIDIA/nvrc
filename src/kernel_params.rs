use anyhow::{anyhow, Context, Result};
use hardened_std::fs;
use log::{debug, warn};

use crate::nvrc::NVRC;

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

impl NVRC {
    /// Parse kernel command line parameters to configure NVRC behavior.
    /// Using kernel params allows configuration without userspace tools—critical
    /// for a minimal init where no config files or environment variables exist.
    pub fn process_kernel_params(&mut self, cmdline: Option<&str>) -> Result<()> {
        let content = match cmdline {
            Some(c) => c.to_owned(),
            None => fs::read_to_string("/proc/cmdline")
                .map_err(|e| anyhow!("read /proc/cmdline: {}", e))?,
        };

        for (k, v) in content.split_whitespace().filter_map(|p| p.split_once('=')) {
            match k {
                "nvrc.mode" => nvrc_mode(v, self)?,
                "nvrc.log" => nvrc_log(v, self)?,
                "nvrc.uvm.persistence.mode" => uvm_persistenced_mode(v, self)?,
                "nvrc.dcgm" => nvrc_dcgm(v, self)?,
                "nvrc.fabricmanager" => nvrc_fabricmanager(v, self)?,
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

/// Operation mode: "gpu" (default) or "cpu" (skip GPU management).
/// Use nvrc.mode=cpu for CPU-only workloads that don't need GPU initialization.
fn nvrc_mode(value: &str, ctx: &mut NVRC) -> Result<()> {
    ctx.mode = Some(value.to_lowercase());
    debug!("nvrc.mode: {}", value);
    Ok(())
}

/// DCGM (Data Center GPU Manager) provides telemetry and health monitoring.
/// Off by default—only enable when observability infrastructure expects it.
fn nvrc_dcgm(value: &str, ctx: &mut NVRC) -> Result<()> {
    let dcgm = parse_boolean(value);
    ctx.dcgm_enabled = Some(dcgm);
    debug!("nvrc.dcgm: {dcgm}");
    Ok(())
}

/// Fabric Manager enables NVLink/NVSwitch multi-GPU communication.
/// Only needed for multi-GPU systems with NVLink topology.
fn nvrc_fabricmanager(value: &str, ctx: &mut NVRC) -> Result<()> {
    let fabricmanager = parse_boolean(value);
    ctx.fabricmanager_enabled = Some(fabricmanager);
    debug!("nvrc.fabricmanager: {fabricmanager}");
    Ok(())
}

/// Control log verbosity at runtime. Defaults to off to minimize noise.
/// Enabling devkmsg allows kernel log output even in minimal init environments.
fn nvrc_log(value: &str, _ctx: &mut NVRC) -> Result<()> {
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
        .map_err(|e| anyhow!("printk_devkmsg: {}", e))?;

    Ok(())
}

/// Secure Randomization Seed for GPU memory. Passed directly to nvidia-smi.
fn nvidia_smi_srs(value: &str, ctx: &mut NVRC) -> Result<()> {
    ctx.nvidia_smi_srs = Some(value.to_owned());
    debug!("nvidia_smi_srs: {value}");
    Ok(())
}

/// Lock GPU core clocks to a fixed frequency (MHz) for consistent performance.
/// Eliminates thermal/power throttling variance in benchmarks and latency-sensitive workloads.
fn nvidia_smi_lgc(value: &str, ctx: &mut NVRC) -> Result<()> {
    let mhz: u32 = value.parse().context("nvrc.smi.lgc: invalid frequency")?;
    debug!("nvrc.smi.lgc: {} MHz (all GPUs)", mhz);
    ctx.nvidia_smi_lgc = Some(mhz);
    Ok(())
}

/// Lock memory clocks to a fixed frequency (MHz).
/// Used alongside lgc for fully deterministic GPU behavior.
fn nvidia_smi_lmc(value: &str, ctx: &mut NVRC) -> Result<()> {
    let mhz: u32 = value.parse().context("nvrc.smi.lmc: invalid frequency")?;
    debug!("nvrc.smi.lmc: {} MHz (all GPUs)", mhz);
    ctx.nvidia_smi_lmc = Some(mhz);
    Ok(())
}

/// Set GPU power limit (Watts). Lower limits reduce heat/power, higher allows peak perf.
/// Useful for power-constrained environments or thermal management.
fn nvidia_smi_pl(value: &str, ctx: &mut NVRC) -> Result<()> {
    let watts: u32 = value.parse().context("nvrc.smi.pl: invalid wattage")?;
    debug!("nvrc.smi.pl: {} W (all GPUs)", watts);
    ctx.nvidia_smi_pl = Some(watts);
    Ok(())
}

/// UVM persistence mode keeps unified memory state across CUDA context teardowns.
/// Reduces initialization overhead for short-lived CUDA applications.
fn uvm_persistenced_mode(value: &str, ctx: &mut NVRC) -> Result<()> {
    let enabled = parse_boolean(value);
    ctx.uvm_persistence_mode = Some(enabled);
    debug!("nvrc.uvm.persistence.mode: {enabled}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use serial_test::serial;
    use std::sync::{LazyLock, Once};

    static LOG: LazyLock<Once> = LazyLock::new(Once::new);

    fn log_setup() {
        LOG.call_once(|| {
            kernlog::init().unwrap();
        });
    }

    #[test]
    #[serial]
    fn test_nvrc_log_debug() {
        require_root();
        log_setup();
        let mut c = NVRC::default();

        nvrc_log("debug", &mut c).unwrap();
        assert!(log_enabled!(log::Level::Debug));
    }

    #[test]
    #[serial]
    fn test_process_kernel_params_nvrc_log_debug() {
        require_root();
        log_setup();
        let mut init = NVRC::default();

        init.process_kernel_params(Some(
            "nvidia.smi.lgc=1500 nvrc.log=debug nvidia.smi.lgc=1500",
        ))
        .unwrap();

        assert_eq!(log::max_level(), log::LevelFilter::Debug);
        assert!(!log_enabled!(log::Level::Trace));
    }

    #[test]
    #[serial]
    fn test_process_kernel_params_nvrc_log_info() {
        require_root();
        log_setup();
        let mut init = NVRC::default();

        init.process_kernel_params(Some(
            "nvidia.smi.lgc=1500 nvrc.log=info nvidia.smi.lgc=1500",
        ))
        .unwrap();

        assert_eq!(log::max_level(), log::LevelFilter::Info);
        assert!(!log_enabled!(log::Level::Debug));
    }

    #[test]
    #[serial]
    fn test_process_kernel_params_nvrc_log_0() {
        require_root();
        log_setup();
        let mut init = NVRC::default();

        init.process_kernel_params(Some("nvidia.smi.lgc=1500 nvrc.log=0 nvidia.smi.lgc=1500"))
            .unwrap();
        assert_eq!(log::max_level(), log::LevelFilter::Off);
    }

    #[test]
    #[serial]
    fn test_process_kernel_params_nvrc_log_none() {
        require_root();
        log_setup();
        let mut init = NVRC::default();

        init.process_kernel_params(Some("nvidia.smi.lgc=1500 nvrc.log= "))
            .unwrap();
        assert_eq!(log::max_level(), log::LevelFilter::Off);
    }

    #[test]
    #[serial]
    fn test_process_kernel_params_nvrc_log_trace() {
        require_root();
        log_setup();
        let mut init = NVRC::default();

        init.process_kernel_params(Some("nvrc.log=trace")).unwrap();
        assert_eq!(log::max_level(), log::LevelFilter::Trace);
    }

    #[test]
    #[serial]
    fn test_process_kernel_params_nvrc_log_unknown() {
        require_root();
        log_setup();
        let mut init = NVRC::default();

        // Unknown log level should default to Off
        init.process_kernel_params(Some("nvrc.log=garbage"))
            .unwrap();
        assert_eq!(log::max_level(), log::LevelFilter::Off);
    }

    #[test]
    fn test_nvrc_dcgm_parameter_handling() {
        let mut c = NVRC::default();

        // Test various "on" values
        nvrc_dcgm("on", &mut c).unwrap();
        assert_eq!(c.dcgm_enabled, Some(true));

        nvrc_dcgm("true", &mut c).unwrap();
        assert_eq!(c.dcgm_enabled, Some(true));

        nvrc_dcgm("1", &mut c).unwrap();
        assert_eq!(c.dcgm_enabled, Some(true));

        nvrc_dcgm("yes", &mut c).unwrap();
        assert_eq!(c.dcgm_enabled, Some(true));

        // Test "off" values
        nvrc_dcgm("off", &mut c).unwrap();
        assert_eq!(c.dcgm_enabled, Some(false));

        nvrc_dcgm("false", &mut c).unwrap();
        assert_eq!(c.dcgm_enabled, Some(false));

        nvrc_dcgm("invalid", &mut c).unwrap();
        assert_eq!(c.dcgm_enabled, Some(false));
    }

    #[test]
    fn test_nvrc_fabricmanager() {
        let mut c = NVRC::default();

        nvrc_fabricmanager("on", &mut c).unwrap();
        assert_eq!(c.fabricmanager_enabled, Some(true));

        nvrc_fabricmanager("off", &mut c).unwrap();
        assert_eq!(c.fabricmanager_enabled, Some(false));
    }

    #[test]
    fn test_nvidia_smi_srs() {
        let mut c = NVRC::default();

        nvidia_smi_srs("enabled", &mut c).unwrap();
        assert_eq!(c.nvidia_smi_srs, Some("enabled".to_owned()));

        nvidia_smi_srs("disabled", &mut c).unwrap();
        assert_eq!(c.nvidia_smi_srs, Some("disabled".to_owned()));
    }

    #[test]
    fn test_uvm_persistenced_mode() {
        let mut c = NVRC::default();

        uvm_persistenced_mode("on", &mut c).unwrap();
        assert_eq!(c.uvm_persistence_mode, Some(true));

        uvm_persistenced_mode("OFF", &mut c).unwrap();
        assert_eq!(c.uvm_persistence_mode, Some(false));

        uvm_persistenced_mode("True", &mut c).unwrap();
        assert_eq!(c.uvm_persistence_mode, Some(true));
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

        nvidia_smi_lgc("1500", &mut c).unwrap();
        assert_eq!(c.nvidia_smi_lgc, Some(1500));

        nvidia_smi_lgc("2100", &mut c).unwrap();
        assert_eq!(c.nvidia_smi_lgc, Some(2100));

        // Invalid value should error
        assert!(nvidia_smi_lgc("invalid", &mut c).is_err());
    }

    #[test]
    fn test_nvidia_smi_lmc() {
        let mut c = NVRC::default();

        nvidia_smi_lmc("5001", &mut c).unwrap();
        assert_eq!(c.nvidia_smi_lmc, Some(5001));

        nvidia_smi_lmc("6000", &mut c).unwrap();
        assert_eq!(c.nvidia_smi_lmc, Some(6000));

        // Invalid value should error
        assert!(nvidia_smi_lmc("not_a_number", &mut c).is_err());
    }

    #[test]
    fn test_nvidia_smi_pl() {
        let mut c = NVRC::default();

        nvidia_smi_pl("300", &mut c).unwrap();
        assert_eq!(c.nvidia_smi_pl, Some(300));

        nvidia_smi_pl("450", &mut c).unwrap();
        assert_eq!(c.nvidia_smi_pl, Some(450));

        // Invalid value should error
        assert!(nvidia_smi_pl("abc", &mut c).is_err());
    }

    #[test]
    fn test_process_kernel_params_gpu_settings() {
        let mut c = NVRC::default();

        c.process_kernel_params(Some("nvrc.smi.lgc=1500 nvrc.smi.lmc=5001 nvrc.smi.pl=300"))
            .unwrap();

        assert_eq!(c.nvidia_smi_lgc, Some(1500));
        assert_eq!(c.nvidia_smi_lmc, Some(5001));
        assert_eq!(c.nvidia_smi_pl, Some(300));
    }

    #[test]
    fn test_process_kernel_params_combined() {
        let mut c = NVRC::default();

        c.process_kernel_params(Some(
            "nvrc.smi.lgc=2100 nvrc.uvm.options=opt1=1,opt2=2 nvrc.dcgm=on nvrc.smi.pl=400",
        ))
        .unwrap();

        assert_eq!(c.nvidia_smi_lgc, Some(2100));
        assert_eq!(c.nvidia_smi_pl, Some(400));
        assert_eq!(c.dcgm_enabled, Some(true));
    }

    #[test]
    fn test_process_kernel_params_from_proc_cmdline() {
        // Exercise the None path which reads /proc/cmdline.
        // We can't control the contents but can verify it doesn't error.
        let mut c = NVRC::default();
        let result = c.process_kernel_params(None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_kernel_params_with_fabricmanager_and_uvm() {
        let mut c = NVRC::default();

        c.process_kernel_params(Some(
            "nvrc.fabricmanager=on nvrc.uvm.persistence.mode=true nvrc.smi.srs=enabled",
        ))
        .unwrap();

        assert_eq!(c.fabricmanager_enabled, Some(true));
        assert_eq!(c.uvm_persistence_mode, Some(true));
        assert_eq!(c.nvidia_smi_srs, Some("enabled".to_owned()));
    }

    #[test]
    fn test_nvrc_mode() {
        let mut c = NVRC::default();

        nvrc_mode("cpu", &mut c).unwrap();
        assert_eq!(c.mode, Some("cpu".to_owned()));

        nvrc_mode("GPU", &mut c).unwrap();
        assert_eq!(c.mode, Some("gpu".to_owned())); // normalized to lowercase

        nvrc_mode("nvswitch-nvl4", &mut c).unwrap();
        assert_eq!(c.mode, Some("nvswitch-nvl4".to_owned()));

        nvrc_mode("NVSWITCH-NVL4", &mut c).unwrap();
        assert_eq!(c.mode, Some("nvswitch-nvl4".to_owned())); // normalized to lowercase

        nvrc_mode("nvswitch-nvl5", &mut c).unwrap();
        assert_eq!(c.mode, Some("nvswitch-nvl5".to_owned()));

        nvrc_mode("NVSWITCH-NVL5", &mut c).unwrap();
        assert_eq!(c.mode, Some("nvswitch-nvl5".to_owned())); // normalized to lowercase
    }

    #[test]
    fn test_process_kernel_params_with_mode() {
        let mut c = NVRC::default();

        c.process_kernel_params(Some("nvrc.mode=cpu nvrc.dcgm=on"))
            .unwrap();

        assert_eq!(c.mode, Some("cpu".to_owned()));
        assert_eq!(c.dcgm_enabled, Some(true));
    }

    #[test]
    fn test_process_kernel_params_nvswitch_nvl4_mode() {
        let mut c = NVRC::default();

        c.process_kernel_params(Some("nvrc.mode=nvswitch-nvl4"))
            .unwrap();

        assert_eq!(c.mode, Some("nvswitch-nvl4".to_owned()));
    }

    #[test]
    fn test_process_kernel_params_nvswitch_nvl5_mode() {
        let mut c = NVRC::default();

        c.process_kernel_params(Some("nvrc.mode=nvswitch-nvl5"))
            .unwrap();

        assert_eq!(c.mode, Some("nvswitch-nvl5".to_owned()));
    }
}
