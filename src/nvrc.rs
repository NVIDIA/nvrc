use anyhow::{Context, Result};
use log::debug;
use std::collections::HashMap;
use std::fs;
use std::os::unix::net::UnixDatagram;
use std::sync::LazyLock;

use crate::cpu::Cpu;
use crate::daemon::Name;
use crate::devices::NvidiaDevice;
#[cfg(feature = "confidential")]
use crate::gpu::confidential::CC;
use crate::user_group::UserGroup;

/// Trait for parsing boolean-like values from strings
trait BooleanLike {
    fn parse_boolean(&self) -> bool;
}

impl BooleanLike for str {
    fn parse_boolean(&self) -> bool {
        matches!(
            self.to_ascii_lowercase().as_str(),
            "on" | "true" | "1" | "yes"
        )
    }
}

pub const NVRC_LOG: &str = "nvrc.log";
pub const NVRC_UVM_PERISTENCE_MODE: &str = "nvrc.uvm_persistence_mode";
pub const NVRC_DCGM: &str = "nvrc.dcgm";
pub const NVRC_SMI_SRS: &str = "nvrc.smi.srs";

const PROC_CMDLINE: &str = "/proc/cmdline";
const PROC_PRINTK_DEVKMSG: &str = "/proc/sys/kernel/printk_devkmsg";

pub type ParamHandler = fn(&str, &mut NVRC) -> Result<()>;

// Use const array for better compile-time initialization
const PARAM_HANDLERS: &[(&str, ParamHandler)] = &[
    (NVRC_LOG, nvrc_log),
    (NVRC_UVM_PERISTENCE_MODE, uvm_persistenced_mode),
    (NVRC_DCGM, nvrc_dcgm),
    (NVRC_SMI_SRS, nvidia_smi_srs),
];

static PARAM_HANDLER: LazyLock<HashMap<&'static str, ParamHandler>> =
    LazyLock::new(|| HashMap::from_iter(PARAM_HANDLERS.iter().copied()));
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
        let socket = crate::syslog::dev_log_setup().context("Failed to setup syslog socket")?;
        self.syslog_socket = Some(socket);
        Ok(())
    }

    pub fn poll_syslog(&self) -> Result<()> {
        if let Some(socket) = &self.syslog_socket {
            crate::syslog::poll_dev_log(socket).context("Failed to poll syslog")?;
        }
        Ok(())
    }

    pub fn process_kernel_params(&mut self, cmdline: Option<&str>) -> Result<()> {
        let content = match cmdline {
            Some(custom) => custom.to_owned(),
            None => fs::read_to_string(PROC_CMDLINE).context("Failed to read /proc/cmdline")?,
        };

        content
            .split_whitespace()
            .filter_map(|param| param.split_once('='))
            .try_for_each(|(key, value)| {
                if let Some(handler) = PARAM_HANDLER.get(key) {
                    handler(value, self)
                } else {
                    Ok(())
                }
            })
    }
}

pub fn nvrc_dcgm(value: &str, context: &mut NVRC) -> Result<()> {
    let dcgm = value.parse_boolean();
    context.dcgm_enabled = Some(dcgm);
    debug!("nvrc.dcgm: {}", dcgm);
    Ok(())
}

pub fn nvrc_log(value: &str, _context: &mut NVRC) -> Result<()> {
    let level = match value.to_ascii_lowercase().as_str() {
        "off" | "0" | "" => log::LevelFilter::Off,
        "error" => log::LevelFilter::Error,
        "warn" => log::LevelFilter::Warn,
        "info" => log::LevelFilter::Info,
        "debug" => log::LevelFilter::Debug,
        "trace" => log::LevelFilter::Trace,
        _ => log::LevelFilter::Off,
    };

    log::set_max_level(level);
    debug!("nvrc.log: {}", log::max_level());

    // Do not ratelimit userspace to /dev/kmsg if we have debug enabled
    fs::write(PROC_PRINTK_DEVKMSG, b"on\n")
        .context("Failed to write to /proc/sys/kernel/printk_devkmsg")?;

    Ok(())
}

pub fn nvidia_smi_srs(value: &str, context: &mut NVRC) -> Result<()> {
    context.nvidia_smi_srs = Some(value.to_owned());
    debug!("nvidia_smi_srs: {}", value);
    Ok(())
}

#[allow(dead_code)]
pub fn nvidia_smi_lgc(value: &str, context: &mut NVRC) -> Result<()> {
    context.nvidia_smi_lgc = Some(value.to_owned());
    debug!("nvidia_smi_lgc: {}", value);
    Ok(())
}

pub fn uvm_persistenced_mode(value: &str, context: &mut NVRC) -> Result<()> {
    context.uvm_persistence_mode = Some(value.to_owned());
    debug!("nvrc.uvm_persistence_mode: {}", value);
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
            Ok(output) => {
                if output.success() {
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
        let mut context = NVRC::default();

        nvrc_log("debug", &mut context).unwrap();
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
            format!("nvidia.smi.lgc=1500 {NVRC_LOG}=debug nvidia.smi.lgc=1500").as_str(),
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
            format!("nvidia.smi.lgc=1500 {NVRC_LOG}=info nvidia.smi.lgc=1500").as_str(),
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

        init.process_kernel_params(Some(
            format!("nvidia.smi.lgc=1500 {NVRC_LOG}=0 nvidia.smi.lgc=1500").as_str(),
        ))
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

        init.process_kernel_params(Some(format!("nvidia.smi.lgc=1500 {NVRC_LOG}= ").as_str()))
            .unwrap();
        assert_eq!(log::max_level(), log::LevelFilter::Off);
    }

    #[test]
    fn test_nvrc_dcgm_parameter_handling() {
        let mut context = NVRC::default();

        // Test various "on" values
        nvrc_dcgm("on", &mut context).unwrap();
        assert_eq!(context.dcgm_enabled, Some(true));

        nvrc_dcgm("true", &mut context).unwrap();
        assert_eq!(context.dcgm_enabled, Some(true));

        nvrc_dcgm("1", &mut context).unwrap();
        assert_eq!(context.dcgm_enabled, Some(true));

        nvrc_dcgm("yes", &mut context).unwrap();
        assert_eq!(context.dcgm_enabled, Some(true));

        // Test "off" values
        nvrc_dcgm("off", &mut context).unwrap();
        assert_eq!(context.dcgm_enabled, Some(false));

        nvrc_dcgm("false", &mut context).unwrap();
        assert_eq!(context.dcgm_enabled, Some(false));

        nvrc_dcgm("invalid", &mut context).unwrap();
        assert_eq!(context.dcgm_enabled, Some(false));
    }

    #[test]
    fn test_boolean_like_trait() {
        assert!("on".parse_boolean());
        assert!("true".parse_boolean());
        assert!("1".parse_boolean());
        assert!("yes".parse_boolean());
        assert!("ON".parse_boolean()); // Test case insensitive
        assert!("True".parse_boolean());
        assert!("YES".parse_boolean());

        assert!(!"off".parse_boolean());
        assert!(!"false".parse_boolean());
        assert!(!"0".parse_boolean());
        assert!(!"no".parse_boolean());
        assert!(!"invalid".parse_boolean());
        assert!(!"".parse_boolean());
    }
}
