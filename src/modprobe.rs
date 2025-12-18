use crate::execute::foreground;
use anyhow::Result;

pub fn nvidia() -> Result<()> {
    foreground("/sbin/modprobe", &["nvidia"])
}

pub fn nvidia_uvm() -> Result<()> {
    foreground("/sbin/modprobe", &["nvidia-uvm"])
}

pub fn reload_nvidia_modules() -> Result<()> {
    foreground("/sbin/modprobe", &["-r", "nvidia-uvm", "nvidia"])?;
    foreground("/sbin/modprobe", &["nvidia"])?;
    foreground("/sbin/modprobe", &["nvidia-uvm"])
}
