use anyhow::Context;
use anyhow::Result;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

use lazy_static::lazy_static;

pub const NVRC_LOG: &str = "nvrc.log";
pub const NVRC_UVM_PERISTENCE_MODE: &str = "nvrc.uvm_persistence_mode";

lazy_static! {
    static ref PARAM_HANDLER: HashMap<&'static str, ParamHandler> = {
        let mut m = HashMap::new();
        m.insert(NVRC_LOG, nvrc_log as ParamHandler);
        m.insert(
            NVRC_UVM_PERISTENCE_MODE,
            uvm_persistenced_mode as ParamHandler,
        );
        m
    };
}
#[derive(Debug, Default)]
pub struct NVRC {
    pub nvidia_smi_lgc: Option<String>,
    pub uvm_persistence_mode: Option<String>,
    pub cpu_vendor: Option<String>,
    pub gpu_bdfs: Vec<String>,
    pub gpu_devids: Vec<String>,
    pub gpu_supported: bool,
    pub gpu_cc_mode: Option<String>,
    pub cold_plug: bool,
}

pub type ParamHandler = fn(&str, &mut NVRC) -> Result<()>;

pub fn process_kernel_params(context: &mut NVRC, cmdline: Option<&str>) -> Result<()> {
    let content = match cmdline {
        Some(custom) => custom.to_string(),
        None => {
            let mut file = File::open("/proc/cmdline").context("Failed to open /proc/cmdline")?;
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
                handler(value, context)?;
            }
        }
    }

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

    Ok(())
}

#[allow(dead_code)]
pub fn nvidia_smi_lgc(value: &str, context: &mut NVRC) -> Result<()> {
    context.nvidia_smi_lgc = Some(value.to_string());
    Ok(())
}

pub fn uvm_persistenced_mode(value: &str, context: &mut NVRC) -> Result<()> {
    context.uvm_persistence_mode = Some(value.to_string());
    Ok(())
}

#[cfg(test)]

mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_nvrc_log_debug() {
        let mut context = NVRC::default();

        nvrc_log("debug", &mut context).unwrap();
        let kernlog_level = env::var("KERNLOG_LEVEL").unwrap();
        assert_eq!(kernlog_level, "7".to_string());
    }

    #[test]
    fn test_process_kernel_params_nvrc_log_debug() {
        let mut context = NVRC::default();
        process_kernel_params(
            &mut context,
            Some(format!("nvidia.smi.lgc=1500 {}=debug nvidia.smi.lgc=1500", NVRC_LOG).as_str()),
        )
        .unwrap();
        let kernlog_level = env::var("KERNLOG_LEVEL").unwrap();
        assert_eq!(kernlog_level, "7".to_string());
    }
    #[test]
    fn test_process_kernel_params_nvrc_log_0() {
        let mut context = NVRC::default();

        process_kernel_params(
            &mut context,
            Some(format!("nvidia.smi.lgc=1500 {}=0 nvidia.smi.lgc=1500", NVRC_LOG).as_str()),
        )
        .unwrap();
        let kernlog_level = env::var("KERNLOG_LEVEL").unwrap();
        assert_eq!(kernlog_level, "1".to_string());
    }
    #[test]
    fn test_process_kernel_params_nvrc_log_none() {
        let mut context = NVRC::default();

        process_kernel_params(
            &mut context,
            Some(format!("nvidia.smi.lgc=1500 {}= ", NVRC_LOG).as_str()),
        )
        .unwrap();
        let kernlog_level = env::var("KERNLOG_LEVEL").unwrap();
        assert_eq!(kernlog_level, "1".to_string());
    }
}
