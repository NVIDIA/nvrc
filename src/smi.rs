//! nvidia-smi GPU configuration commands.
//!
//! These functions apply GPU settings via nvidia-smi before workloads run.
//! All are optional—if the kernel param isn't set, they return immediately.

use crate::execute::foreground;
use crate::nvrc::NVRC;

const NVIDIA_SMI: &str = "/bin/nvidia-smi";

impl NVRC {
    /// Lock memory clocks to a specific frequency (MHz).
    /// Reduces memory clock jitter for latency-sensitive workloads.
    pub fn nvidia_smi_lmc(&self) {
        let Some(mhz) = self.nvidia_smi_lmc else {
            return;
        };
        foreground(NVIDIA_SMI, &["-lmc", &mhz.to_string()]);
    }

    /// Lock GPU core clocks to a specific frequency (MHz).
    /// Provides consistent performance by preventing dynamic frequency scaling.
    pub fn nvidia_smi_lgc(&self) {
        let Some(mhz) = self.nvidia_smi_lgc else {
            return;
        };
        foreground(NVIDIA_SMI, &["-lgc", &mhz.to_string()]);
    }

    /// Set GPU power limit in watts.
    /// Caps power consumption for thermal/power budget compliance.
    pub fn nvidia_smi_pl(&self) {
        let Some(watts) = self.nvidia_smi_pl else {
            return;
        };
        foreground(NVIDIA_SMI, &["-pl", &watts.to_string()]);
    }

    /// Set GPU Ready State after successful attestation.
    /// In Confidential Computing mode, GPUs default to NotReady and refuse
    /// workloads. After attestation verifies the GPU's integrity, we set
    /// the state to Ready so it can execute compute jobs.
    pub fn nvidia_smi_srs(&self) {
        let Some(ref state) = self.nvidia_smi_srs else {
            return;
        };
        foreground(NVIDIA_SMI, &["conf-compute", "-srs", state]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic;

    // When fields are None, functions return immediately (no nvidia-smi call)

    #[test]
    fn test_lmc_none() {
        let nvrc = NVRC::default();
        nvrc.nvidia_smi_lmc();
    }

    #[test]
    fn test_lgc_none() {
        let nvrc = NVRC::default();
        nvrc.nvidia_smi_lgc();
    }

    #[test]
    fn test_pl_none() {
        let nvrc = NVRC::default();
        nvrc.nvidia_smi_pl();
    }

    #[test]
    fn test_srs_none() {
        let nvrc = NVRC::default();
        nvrc.nvidia_smi_srs();
    }

    // When fields are Some, nvidia-smi is called (panics without NVIDIA hardware)

    #[test]
    fn test_lmc_some_fails_without_nvidia_smi() {
        let mut nvrc = NVRC::default();
        nvrc.nvidia_smi_lmc = Some(1000);
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            nvrc.nvidia_smi_lmc();
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_lgc_some_fails_without_nvidia_smi() {
        let mut nvrc = NVRC::default();
        nvrc.nvidia_smi_lgc = Some(1500);
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            nvrc.nvidia_smi_lgc();
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_pl_some_fails_without_nvidia_smi() {
        let mut nvrc = NVRC::default();
        nvrc.nvidia_smi_pl = Some(300);
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            nvrc.nvidia_smi_pl();
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_srs_some_fails_without_nvidia_smi() {
        let mut nvrc = NVRC::default();
        nvrc.nvidia_smi_srs = Some("1".into());
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            nvrc.nvidia_smi_srs();
        }));
        assert!(result.is_err());
    }
}
