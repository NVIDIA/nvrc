//! nvidia-smi GPU configuration commands.
//!
//! These functions apply GPU settings via nvidia-smi before workloads run.
//! All are optional—if the kernel param isn't set, they return immediately.

use crate::execute::foreground;
use crate::gpu_extension;
use crate::nvrc::NVRC;

const NVIDIA_SMI: &str = "/bin/nvidia-smi";

fn nvidia_smi() -> String {
    gpu_extension::path(NVIDIA_SMI)
}

/// Apply a numeric GPU setting; no-op if its kernel parameter is unset.
fn set_gpu(flag: &str, value: Option<u32>) {
    value.into_iter().for_each(|n| {
        foreground(&nvidia_smi(), &[flag, &n.to_string()]);
    });
}

impl NVRC {
    /// Lock memory clocks to a specific frequency (MHz).
    /// Reduces memory clock jitter for latency-sensitive workloads.
    pub fn nvidia_smi_lmc(&self) {
        set_gpu("-lmc", self.nvidia_smi_lmc);
    }

    /// Lock GPU core clocks to a specific frequency (MHz).
    /// Provides consistent performance by preventing dynamic frequency scaling.
    pub fn nvidia_smi_lgc(&self) {
        set_gpu("-lgc", self.nvidia_smi_lgc);
    }

    /// Set GPU power limit in watts.
    /// Caps power consumption for thermal/power budget compliance.
    pub fn nvidia_smi_pl(&self) {
        set_gpu("-pl", self.nvidia_smi_pl);
    }

    /// Set GPU Ready State after successful attestation.
    /// In Confidential Computing mode, GPUs default to NotReady and refuse
    /// workloads. After attestation verifies the GPU's integrity, we set
    /// the state to Ready so it can execute compute jobs.
    pub fn nvidia_smi_srs(&self) {
        self.nvidia_smi_srs.iter().for_each(|state| {
            foreground(&nvidia_smi(), &["conf-compute", "-srs", state]);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::panic;

    #[test]
    fn test_unset_params_run_nothing() {
        let nvrc = NVRC::default();
        nvrc.nvidia_smi_lmc();
        nvrc.nvidia_smi_lgc();
        nvrc.nvidia_smi_pl();
        nvrc.nvidia_smi_srs();
    }

    // Set params call nvidia-smi, which panics without NVIDIA hardware.
    #[rstest]
    #[case::lmc(|n: &mut NVRC| n.nvidia_smi_lmc = Some(1000), NVRC::nvidia_smi_lmc)]
    #[case::lgc(|n: &mut NVRC| n.nvidia_smi_lgc = Some(1500), NVRC::nvidia_smi_lgc)]
    #[case::pl(|n: &mut NVRC| n.nvidia_smi_pl = Some(300), NVRC::nvidia_smi_pl)]
    #[case::srs(|n: &mut NVRC| n.nvidia_smi_srs = Some("1".into()), NVRC::nvidia_smi_srs)]
    #[cfg_attr(miri, ignore = "spawns a process, which miri cannot emulate")]
    fn test_set_param_fails_without_nvidia_smi(
        #[case] set: fn(&mut NVRC),
        #[case] apply: fn(&NVRC),
    ) {
        let mut nvrc = NVRC::default();
        set(&mut nvrc);
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| apply(&nvrc)));
        assert!(result.is_err());
    }
}
