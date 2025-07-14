use anyhow::Context;
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::Read;

use lazy_static::lazy_static;

use crate::daemon::Name;
use crate::user_group::UserGroup;

pub const NVRC_LOG: &str = "nvrc.log";
pub const NVRC_UVM_PERISTENCE_MODE: &str = "nvrc.uvm_persistence_mode";
pub const NVRC_DCGM: &str = "nvrc.dcgm";
pub const NVIDIA_SMI_SRS: &str = "nvidia.smi.srs";

lazy_static! {
    static ref PARAM_HANDLER: HashMap<&'static str, ParamHandler> = {
        let mut m = HashMap::new();
        m.insert(NVRC_LOG, nvrc_log as ParamHandler);
        m.insert(
            NVRC_UVM_PERISTENCE_MODE,
            uvm_persistenced_mode as ParamHandler,
        );
        m.insert(NVRC_DCGM, nvrc_dcgm as ParamHandler);
        m.insert(NVIDIA_SMI_SRS, nvidia_smi_srs as ParamHandler);
        m
    };
}
#[derive(Debug)]
#[allow(clippy::upper_case_acronyms)]
pub struct NVRC {
    pub nvidia_smi_srs: Option<String>,
    pub nvidia_smi_lgc: Option<String>,
    pub uvm_persistence_mode: Option<String>,
    pub cpu_vendor: Option<String>,
    pub gpu_bdfs: Vec<String>,
    pub gpu_devids: Vec<String>,
    pub gpu_supported: bool,
    pub gpu_cc_mode: Option<String>,
    pub cold_plug: bool,
    pub hot_or_cold_plug: HashMap<bool, fn(&mut NVRC)>,
    pub dcgm_enabled: Option<bool>,
    pub identity: UserGroup,
    pub daemons: HashMap<Name, std::process::Child>,
}

pub type ParamHandler = fn(&str, &mut NVRC) -> Result<()>;

impl NVRC {
    pub fn default() -> Self {
        let mut init = NVRC {
            nvidia_smi_srs: None,
            nvidia_smi_lgc: None,
            uvm_persistence_mode: None,
            cpu_vendor: None,
            gpu_bdfs: Vec::new(),
            gpu_devids: Vec::new(),
            gpu_supported: false,
            gpu_cc_mode: None,
            cold_plug: false,
            hot_or_cold_plug: HashMap::new(),
            dcgm_enabled: None,
            identity: UserGroup::new(),
            daemons: HashMap::new(),
        };

        init.hot_or_cold_plug.insert(true, NVRC::cold_plug);
        init.hot_or_cold_plug.insert(false, NVRC::hot_plug);

        init
    }

    pub fn process_kernel_params(&mut self, cmdline: Option<&str>) -> Result<()> {
        let content = match cmdline {
            Some(custom) => custom.to_string(),
            None => {
                let mut file =
                    File::open("/proc/cmdline").context("Failed to open /proc/cmdline")?;
                let mut content = String::new();
                file.read_to_string(&mut content)
                    .context("Failed to read /proc/cmdline")?;
                content
            }
        };
        // Split the content into key-value pairs
        for param in content.split_whitespace() {
            if let Some((key, value)) = param.split_once('=') {
                if let Some(handler) = PARAM_HANDLER.get(key) {
                    handler(value, self)?;
                }
            }
        }

        Ok(())
    }
}

pub fn nvrc_dcgm(value: &str, context: &mut NVRC) -> Result<()> {
    let dcgm = match value.to_lowercase().as_str() {
        "on" => true,
        "off" => false,
        _ => false,
    };
    context.dcgm_enabled = Some(dcgm);
    debug!("nvrc.dcgm: {}", context.dcgm_enabled.unwrap());
    Ok(())
}

pub fn nvrc_log(value: &str, _context: &mut NVRC) -> Result<()> {
    let level = match value.to_lowercase().as_str() {
        "off" => log::LevelFilter::Off,
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
    fs::write("/proc/sys/kernel/printk_devkmsg", b"on\n").unwrap();
    Ok(())
}

pub fn nvidia_smi_srs(value: &str, context: &mut NVRC) -> Result<()> {
    context.nvidia_smi_srs = Some(value.to_string());
    Ok(())
}

#[allow(dead_code)]
pub fn nvidia_smi_lgc(value: &str, context: &mut NVRC) -> Result<()> {
    context.nvidia_smi_lgc = Some(value.to_string());
    Ok(())
}

pub fn uvm_persistenced_mode(value: &str, context: &mut NVRC) -> Result<()> {
    context.uvm_persistence_mode = Some(value.to_string());
    debug!(
        "nvrc.uvm_persistence_mode {}",
        context.uvm_persistence_mode.as_ref().unwrap()
    );
    Ok(())
}

#[cfg(test)]

mod tests {
    use super::*;
    use lazy_static::lazy_static;
    use nix::unistd::Uid;
    use serial_test::serial;
    use std::env;
    use std::process::Command;
    use std::sync::Once;

    lazy_static! {
        static ref LOG: Once = Once::new();
    }

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
        assert_eq!(log_enabled!(log::Level::Trace), false);
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
        assert_eq!(log_enabled!(log::Level::Debug), false);
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
}
