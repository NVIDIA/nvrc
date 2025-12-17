use crate::daemon::foreground;
use anyhow::Result;

pub fn nvidia() -> Result<()> {
    foreground("/sbin/modprobe", &["nvidia"])
}

pub fn nvidia_uvm() -> Result<()> {
    foreground("/sbin/modprobe", &["nvidia-uvm"])
}

pub fn nvidia_modeset() -> Result<()> {
    foreground("/sbin/modprobe", &["nvidia-modeset"])
}

pub fn reload_nvidia_modules() -> Result<()> {
    foreground(
        "/sbin/modprobe",
        &["-r", "nvidia_uvm", "nvidia_modeset", "nvidia"],
    )?;
    foreground("/sbin/modprobe", &["nvidia"])?;
    foreground("/sbin/modprobe", &["nvidia-uvm"])?;
    foreground("/sbin/modprobe", &["nvidia-modeset"])
}
