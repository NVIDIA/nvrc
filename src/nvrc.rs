use anyhow::{Context, Result};
use log::debug;
use std::collections::HashMap;
use std::fs;
use std::os::unix::net::UnixDatagram;

use crate::cpu::Cpu;
use crate::daemon::Name;
use crate::devices::NvidiaDevice;
#[cfg(feature = "confidential")]
use crate::gpu::confidential::CC;
use crate::user_group::UserGroup;

fn parse_boolean(s: &str) -> bool {
    matches!(s.to_ascii_lowercase().as_str(), "on" | "true" | "1" | "yes")
}

#[derive(Debug)]
#[allow(clippy::upper_case_acronyms)]
pub struct NVRC {
    pub nvidia_smi_srs: Option<String>,
    pub nvidia_smi_lgc: Option<String>,
    pub uvm_persistence_mode: Option<String>,
    pub cpu_vendor: Option<Cpu>,
    pub nvidia_devices: Vec<NvidiaDevice>,
    pub gpu_supported: bool,
    #[cfg(feature = "confidential")]
    pub gpu_cc_mode: Option<CC>,
    pub cold_plug: bool,
    pub hot_or_cold_plug: HashMap<bool, fn(&mut NVRC)>,
    pub dcgm_enabled: Option<bool>,
    pub identity: UserGroup,
    pub daemons: HashMap<Name, std::process::Child>,
    pub syslog_socket: Option<UnixDatagram>,
}

impl Default for NVRC {
    fn default() -> Self {
        Self {
            nvidia_smi_srs: None,
            nvidia_smi_lgc: None,
            uvm_persistence_mode: None,
            cpu_vendor: None,
            nvidia_devices: Vec::new(),
            gpu_supported: false,
            #[cfg(feature = "confidential")]
            gpu_cc_mode: None,
            cold_plug: false,
            hot_or_cold_plug: HashMap::from([
                (true, NVRC::cold_plug as fn(&mut NVRC)),
                (false, NVRC::hot_plug as fn(&mut NVRC)),
            ]),
            dcgm_enabled: None,
            identity: UserGroup::new(),
            daemons: HashMap::new(),
            syslog_socket: None,
        }
    }
}

impl NVRC {
    pub fn setup_syslog(&mut self) -> Result<()> {
        let socket = crate::syslog::dev_log_setup().context("syslog socket")?;
        self.syslog_socket = Some(socket);
        Ok(())
    }

    pub fn poll_syslog(&self) -> Result<()> {
        if let Some(socket) = &self.syslog_socket {
            crate::syslog::poll_dev_log(socket).context("poll syslog")?;
        }
        Ok(())
    }

    pub fn process_kernel_params(&mut self, cmdline: Option<&str>) -> Result<()> {
        let content = match cmdline {
            Some(c) => c.to_owned(),
            None => fs::read_to_string("/proc/cmdline").context("read /proc/cmdline")?,
        };

        for (k, v) in content.split_whitespace().filter_map(|p| p.split_once('=')) {
            match k {
                "nvrc.log" => nvrc_log(v, self)?,
                "nvrc.uvm_persistence_mode" => uvm_persistenced_mode(v, self)?,
                "nvrc.dcgm" => nvrc_dcgm(v, self)?,
                "nvrc.smi.srs" => nvidia_smi_srs(v, self)?,
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

#[allow(dead_code)]
pub fn nvidia_smi_lgc(value: &str, ctx: &mut NVRC) -> Result<()> {
    ctx.nvidia_smi_lgc = Some(value.to_owned());
    debug!("nvidia_smi_lgc: {value}");
    Ok(())
}

pub fn uvm_persistenced_mode(value: &str, ctx: &mut NVRC) -> Result<()> {
    ctx.uvm_persistence_mode = Some(value.to_owned());
    debug!("nvrc.uvm_persistence_mode: {value}");
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
}
