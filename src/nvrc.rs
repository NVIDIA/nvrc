// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{Context, Result};
use log::debug;
use std::fs;

use crate::user_group::UserGroup;

fn parse_boolean(s: &str) -> bool {
    matches!(s.to_ascii_lowercase().as_str(), "on" | "true" | "1" | "yes")
}

#[derive(Debug, Default)]
#[allow(clippy::upper_case_acronyms)]
pub struct NVRC {
    pub nvidia_smi_srs: Option<String>,
    pub nvidia_smi_lgc: Option<u32>,  // lock gpu clocks (MHz) - all GPUs
    pub nvidia_smi_lmcd: Option<u32>, // lock memory clocks (MHz) - all GPUs
    pub nvidia_smi_pl: Option<u32>,   // power limit (Watts) - all GPUs
    pub uvm_persistence_mode: Option<String>,
    pub uvm_options: Vec<String>,     // UVM module options (comma-separated)
    pub dcgm_enabled: Option<bool>,
    pub fabricmanager_enabled: Option<bool>,
    pub identity: UserGroup,
}

impl NVRC {
    pub fn process_kernel_params(&mut self, cmdline: Option<&str>) -> Result<()> {
        let content = match cmdline {
            Some(c) => c.to_owned(),
            None => fs::read_to_string("/proc/cmdline").context("read /proc/cmdline")?,
        };

        for (k, v) in content.split_whitespace().filter_map(|p| p.split_once('=')) {
            match k {
                "nvrc.log" => nvrc_log(v, self)?,
                "nvrc.uvm.persistence.mode" => uvm_persistenced_mode(v, self)?,
                "nvrc.uvm.options" => nvrc_uvm_options(v, self)?,
                "nvrc.dcgm" => nvrc_dcgm(v, self)?,
                "nvrc.fabricmanager" => nvrc_fabricmanager(v, self)?,
                "nvrc.smi.srs" => nvidia_smi_srs(v, self)?,
                "nvrc.smi.lgc" => nvidia_smi_lgc(v, self)?,
                "nvrc.smi.lmcd" => nvidia_smi_lmcd(v, self)?,
                "nvrc.smi.pl" => nvidia_smi_pl(v, self)?,
                _ => {}
            }
        }
        Ok(())
    }

    pub fn set_random_identity(&mut self) -> anyhow::Result<()> {
        self.identity = crate::user_group::random_user_group()?;
        Ok(())
    }
}

pub fn nvrc_dcgm(value: &str, ctx: &mut NVRC) -> Result<()> {
    let dcgm = parse_boolean(value);
    ctx.dcgm_enabled = Some(dcgm);
    debug!("nvrc.dcgm: {dcgm}");
    Ok(())
}

pub fn nvrc_fabricmanager(value: &str, ctx: &mut NVRC) -> Result<()> {
    let fabricmanager = parse_boolean(value);
    ctx.fabricmanager_enabled = Some(fabricmanager);
    debug!("nvrc.fabricmanager: {fabricmanager}");
    Ok(())
}

pub fn nvrc_log(value: &str, _ctx: &mut NVRC) -> Result<()> {
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

    fs::write("/proc/sys/kernel/printk_devkmsg", b"on\n").context("printk_devkmsg")?;

    Ok(())
}

pub fn nvidia_smi_srs(value: &str, ctx: &mut NVRC) -> Result<()> {
    ctx.nvidia_smi_srs = Some(value.to_owned());
    debug!("nvidia_smi_srs: {value}");
    Ok(())
}

/// Lock GPU clocks for all GPUs (value in MHz)
pub fn nvidia_smi_lgc(value: &str, ctx: &mut NVRC) -> Result<()> {
    let mhz: u32 = value.parse().context("nvrc.smi.lgc: invalid frequency")?;
    debug!("nvrc.smi.lgc: {} MHz (all GPUs)", mhz);
    ctx.nvidia_smi_lgc = Some(mhz);
    Ok(())
}

/// Lock memory clocks for all GPUs (value in MHz)
pub fn nvidia_smi_lmcd(value: &str, ctx: &mut NVRC) -> Result<()> {
    let mhz: u32 = value.parse().context("nvrc.smi.lmcd: invalid frequency")?;
    debug!("nvrc.smi.lmcd: {} MHz (all GPUs)", mhz);
    ctx.nvidia_smi_lmcd = Some(mhz);
    Ok(())
}

/// Set power limit for all GPUs (value in Watts)
pub fn nvidia_smi_pl(value: &str, ctx: &mut NVRC) -> Result<()> {
    let watts: u32 = value.parse().context("nvrc.smi.pl: invalid wattage")?;
    debug!("nvrc.smi.pl: {} W (all GPUs)", watts);
    ctx.nvidia_smi_pl = Some(watts);
    Ok(())
}

pub fn uvm_persistenced_mode(value: &str, ctx: &mut NVRC) -> Result<()> {
    ctx.uvm_persistence_mode = Some(value.to_owned());
    debug!("nvrc.uvm.persistence.mode: {value}");
    Ok(())
}

/// UVM module options (comma-separated, e.g., "opt1=1,opt2=1")
pub fn nvrc_uvm_options(value: &str, ctx: &mut NVRC) -> Result<()> {
    ctx.uvm_options = value
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    debug!("nvrc.uvm.options: {:?}", ctx.uvm_options);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::unistd::Uid;
    use serial_test::serial;
    use std::env;
    use std::process::Command;
    use std::sync::{LazyLock, Once};

    static LOG: LazyLock<Once> = LazyLock::new(Once::new);

    fn log_setup() {
        LOG.call_once(|| {
            kernlog::init().unwrap();
        });
    }

    fn rerun_with_sudo() {
        let args: Vec<String> = env::args().collect();
        let output = Command::new("sudo").args(&args).status();

        match output {
            Ok(o) => {
                if o.success() {
                    println!("running with sudo")
                } else {
                    panic!("not running with sudo")
                }
            }
            Err(e) => {
                panic!("Failed to escalate privileges: {e:?}")
            }
        }
    }

    #[test]
    #[serial]
    fn test_nvrc_log_debug() {
        if !Uid::effective().is_root() {
            return rerun_with_sudo();
        }

        log_setup();
        let mut c = NVRC::default();

        nvrc_log("debug", &mut c).unwrap();
        assert!(log_enabled!(log::Level::Debug));
    }

    #[test]
    #[serial]
    fn test_process_kernel_params_nvrc_log_debug() {
        if !Uid::effective().is_root() {
            return rerun_with_sudo();
        }

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
        if !Uid::effective().is_root() {
            return rerun_with_sudo();
        }

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
        if !Uid::effective().is_root() {
            return rerun_with_sudo();
        }

        log_setup();
        let mut init = NVRC::default();

        init.process_kernel_params(Some("nvidia.smi.lgc=1500 nvrc.log=0 nvidia.smi.lgc=1500"))
            .unwrap();
        assert_eq!(log::max_level(), log::LevelFilter::Off);
    }

    #[test]
    #[serial]
    fn test_process_kernel_params_nvrc_log_none() {
        if !Uid::effective().is_root() {
            return rerun_with_sudo();
        }

        log_setup();
        let mut init = NVRC::default();

        init.process_kernel_params(Some("nvidia.smi.lgc=1500 nvrc.log= "))
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
    fn test_nvidia_smi_lmcd() {
        let mut c = NVRC::default();

        nvidia_smi_lmcd("5001", &mut c).unwrap();
        assert_eq!(c.nvidia_smi_lmcd, Some(5001));

        nvidia_smi_lmcd("6000", &mut c).unwrap();
        assert_eq!(c.nvidia_smi_lmcd, Some(6000));

        // Invalid value should error
        assert!(nvidia_smi_lmcd("not_a_number", &mut c).is_err());
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
    fn test_nvrc_uvm_options_single() {
        let mut c = NVRC::default();

        nvrc_uvm_options("uvm_enable_builtin_tests=1", &mut c).unwrap();
        assert_eq!(c.uvm_options, vec!["uvm_enable_builtin_tests=1"]);
    }

    #[test]
    fn test_nvrc_uvm_options_multiple() {
        let mut c = NVRC::default();

        nvrc_uvm_options(
            "uvm_enable_builtin_tests=1,uvm_perf_access_counter_mimc_migration_enable=1",
            &mut c,
        )
        .unwrap();
        assert_eq!(
            c.uvm_options,
            vec![
                "uvm_enable_builtin_tests=1",
                "uvm_perf_access_counter_mimc_migration_enable=1"
            ]
        );
    }

    #[test]
    fn test_nvrc_uvm_options_with_spaces() {
        let mut c = NVRC::default();

        nvrc_uvm_options("opt1=1, opt2=2 , opt3=3", &mut c).unwrap();
        assert_eq!(c.uvm_options, vec!["opt1=1", "opt2=2", "opt3=3"]);
    }

    #[test]
    fn test_nvrc_uvm_options_empty() {
        let mut c = NVRC::default();

        nvrc_uvm_options("", &mut c).unwrap();
        assert!(c.uvm_options.is_empty());

        nvrc_uvm_options(",,", &mut c).unwrap();
        assert!(c.uvm_options.is_empty());
    }

    #[test]
    fn test_process_kernel_params_gpu_settings() {
        let mut c = NVRC::default();

        c.process_kernel_params(Some(
            "nvrc.smi.lgc=1500 nvrc.smi.lmcd=5001 nvrc.smi.pl=300",
        ))
        .unwrap();

        assert_eq!(c.nvidia_smi_lgc, Some(1500));
        assert_eq!(c.nvidia_smi_lmcd, Some(5001));
        assert_eq!(c.nvidia_smi_pl, Some(300));
    }

    #[test]
    fn test_process_kernel_params_uvm_options() {
        let mut c = NVRC::default();

        c.process_kernel_params(Some(
            "nvrc.uvm.options=uvm_enable_builtin_tests=1,uvm_perf_test=0",
        ))
        .unwrap();

        assert_eq!(
            c.uvm_options,
            vec!["uvm_enable_builtin_tests=1", "uvm_perf_test=0"]
        );
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
        assert_eq!(c.uvm_options, vec!["opt1=1", "opt2=2"]);
        assert_eq!(c.dcgm_enabled, Some(true));
    }
}
