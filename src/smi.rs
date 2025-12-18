use crate::execute::foreground;
use crate::modprobe;
use crate::nvrc::NVRC;
use anyhow::Result;

impl NVRC {
    pub fn nvidia_smi_lmcd(&self) -> Result<()> {
        let Some(mhz) = self.nvidia_smi_lmcd else {
            return Ok(());
        };

        let mhz_str = mhz.to_string();
        foreground("/bin/nvidia-smi", &["-lmcd", &mhz_str])?;

        // Memory clock lock requires driver reload
        modprobe::reload_nvidia_modules()
    }

    /// Lock GPU clocks for all GPUs
    pub fn nvidia_smi_lgc(&self) -> Result<()> {
        let Some(mhz) = self.nvidia_smi_lgc else {
            return Ok(());
        };

        let mhz_str = mhz.to_string();
        foreground("/bin/nvidia-smi", &["-lgc", &mhz_str])
    }

    /// Set power limit for all GPUs
    pub fn nvidia_smi_pl(&self) -> Result<()> {
        let Some(watts) = self.nvidia_smi_pl else {
            return Ok(());
        };

        let watts_str = watts.to_string();
        foreground("/bin/nvidia-smi", &["-pl", &watts_str])
    }

    /// Set Ready State for confidential compute (SRS)
    pub fn nvidia_smi_srs(&self) -> Result<()> {
        if self.nvidia_smi_srs.is_none() {
            return Ok(());
        }
        foreground(
            "/bin/nvidia-smi",
            &[
                "conf-compute",
                "-srs",
                self.nvidia_smi_srs.as_deref().unwrap_or("0"),
            ],
        )
    }
}
