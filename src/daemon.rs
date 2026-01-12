// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{anyhow, Result};
use hardened_std::fs;

use crate::execute::background;
use crate::nvrc::NVRC;

/// UVM persistence mode keeps unified memory mappings alive between kernel launches,
/// avoiding expensive page migrations. Enabled by default for ML workloads.
fn persistenced_args(uvm_enabled: bool) -> Vec<&'static str> {
    if uvm_enabled {
        vec!["--verbose", "--uvm-persistence-mode"]
    } else {
        vec!["--verbose"]
    }
}

/// Hostengine needs a service account to avoid running as root, and /tmp as home
/// because the rootfs is read-only after init completes.
fn hostengine_args() -> &'static [&'static str] {
    &["--service-account", "nvidia-dcgm", "--home-dir", "/tmp"]
}

/// Kubernetes mode disables standalone HTTP server (we're behind kata-agent),
/// and we use the standard counters config shipped with the container image.
fn dcgm_exporter_args() -> &'static [&'static str] {
    &["-k", "-f", "/etc/dcgm-exporter/default-counters.csv"]
}

/// Fabricmanager needs explicit config path because it doesn't search standard
/// locations when running as a subprocess of init.
fn fabricmanager_args() -> &'static [&'static str] {
    &["-c", "/usr/share/nvidia/nvswitch/fabricmanager.cfg"]
}

const NVIDIA_PERSISTENCED: &str = "/bin/nvidia-persistenced";
const NV_HOSTENGINE: &str = "/bin/nv-hostengine";
const DCGM_EXPORTER: &str = "/bin/dcgm-exporter";
const NV_FABRICMANAGER: &str = "/bin/nv-fabricmanager";

impl NVRC {
    /// nvidia-persistenced keeps GPU state warm between container invocations,
    /// reducing cold-start latency. UVM persistence mode enables unified memory
    /// optimizations. Enabled by default since most workloads benefit from it.
    pub fn nvidia_persistenced(&mut self) -> Result<()> {
        self.spawn_persistenced("/var/run/nvidia-persistenced", NVIDIA_PERSISTENCED)
    }

    fn spawn_persistenced(&mut self, run_dir: &str, bin: &'static str) -> Result<()> {
        fs::create_dir_all(run_dir).map_err(|e| anyhow!("create_dir_all {}: {}", run_dir, e))?;
        let uvm_enabled = self.uvm_persistence_mode.unwrap_or(true);
        let args = persistenced_args(uvm_enabled);
        let child = background(bin, &args)?;
        self.track_daemon("nvidia-persistenced", child);
        Ok(())
    }

    /// nv-hostengine is the DCGM backend daemon. Only started when DCGM monitoring
    /// is explicitly requested - not needed for basic GPU workloads.
    pub fn nv_hostengine(&mut self) -> Result<()> {
        self.spawn_hostengine(NV_HOSTENGINE)
    }

    fn spawn_hostengine(&mut self, bin: &'static str) -> Result<()> {
        if !self.dcgm_enabled.unwrap_or(false) {
            return Ok(());
        }
        let child = background(bin, hostengine_args())?;
        self.track_daemon("nv-hostengine", child);
        Ok(())
    }

    /// dcgm-exporter exposes GPU metrics for Prometheus. Only started when DCGM
    /// is enabled - adds overhead so disabled by default.
    pub fn dcgm_exporter(&mut self) -> Result<()> {
        self.spawn_dcgm_exporter(DCGM_EXPORTER)
    }

    fn spawn_dcgm_exporter(&mut self, bin: &'static str) -> Result<()> {
        if !self.dcgm_enabled.unwrap_or(false) {
            return Ok(());
        }
        let child = background(bin, dcgm_exporter_args())?;
        self.track_daemon("dcgm-exporter", child);
        Ok(())
    }

    /// NVSwitch fabric manager is only needed for multi-GPU NVLink topologies.
    /// Disabled by default since most VMs have single GPUs.
    pub fn nv_fabricmanager(&mut self) -> Result<()> {
        self.spawn_fabricmanager(NV_FABRICMANAGER)
    }

    fn spawn_fabricmanager(&mut self, bin: &'static str) -> Result<()> {
        if !self.fabricmanager_enabled.unwrap_or(false) {
            return Ok(());
        }
        let child = background(bin, fabricmanager_args())?;
        self.track_daemon("nv-fabricmanager", child);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // === Args builder tests ===

    #[test]
    fn test_persistenced_args_with_uvm() {
        let args = persistenced_args(true);
        assert_eq!(args, vec!["--verbose", "--uvm-persistence-mode"]);
    }

    #[test]
    fn test_persistenced_args_without_uvm() {
        let args = persistenced_args(false);
        assert_eq!(args, vec!["--verbose"]);
    }

    #[test]
    fn test_hostengine_args() {
        let args = hostengine_args();
        assert_eq!(
            args,
            &["--service-account", "nvidia-dcgm", "--home-dir", "/tmp"]
        );
    }

    #[test]
    fn test_dcgm_exporter_args() {
        let args = dcgm_exporter_args();
        assert_eq!(
            args,
            &["-k", "-f", "/etc/dcgm-exporter/default-counters.csv"]
        );
    }

    #[test]
    fn test_fabricmanager_args() {
        let args = fabricmanager_args();
        assert_eq!(
            args,
            &["-c", "/usr/share/nvidia/nvswitch/fabricmanager.cfg"]
        );
    }

    // === Skip path tests ===

    #[test]
    fn test_nv_hostengine_skipped_by_default() {
        // DCGM disabled by default - should be a no-op, no daemon spawned
        let mut nvrc = NVRC::default();
        assert!(nvrc.nv_hostengine().is_ok());
        assert!(nvrc.check_daemons().is_ok());
    }

    #[test]
    fn test_dcgm_exporter_skipped_by_default() {
        let mut nvrc = NVRC::default();
        assert!(nvrc.dcgm_exporter().is_ok());
    }

    #[test]
    fn test_nv_fabricmanager_skipped_by_default() {
        let mut nvrc = NVRC::default();
        assert!(nvrc.nv_fabricmanager().is_ok());
    }

    // Test binary - allowed in cfg(test) only
    const TEST_BIN_TRUE: &'static str = "/bin/true";

    #[test]
    fn test_spawn_persistenced_success() {
        let tmpdir = TempDir::new().unwrap();
        let run_dir = tmpdir.path().join("nvidia-persistenced");

        let mut nvrc = NVRC::default();
        nvrc.spawn_persistenced(run_dir.to_str().unwrap(), TEST_BIN_TRUE)
            .expect("spawn_persistenced failed - check: 1) temp dir creation, 2) process spawn, 3) daemon tracking");

        // Directory should be created
        assert!(run_dir.exists());

        // Daemon should be tracked and exit cleanly
        nvrc.check_daemons()
            .expect("persistenced daemon should exit cleanly");
    }

    #[test]
    fn test_spawn_persistenced_uvm_disabled() {
        let tmpdir = TempDir::new().unwrap();
        let run_dir = tmpdir.path().join("nvidia-persistenced");

        let mut nvrc = NVRC::default();
        nvrc.uvm_persistence_mode = Some(false); // Tests the else branch for args
        nvrc.spawn_persistenced(run_dir.to_str().unwrap(), TEST_BIN_TRUE)
            .expect("spawn_persistenced with UVM disabled should succeed");
    }

    #[test]
    fn test_spawn_hostengine_success() {
        let mut nvrc = NVRC::default();
        nvrc.dcgm_enabled = Some(true);
        nvrc.spawn_hostengine(TEST_BIN_TRUE)
            .expect("spawn_hostengine should succeed when DCGM enabled");
        nvrc.check_daemons()
            .expect("hostengine daemon should exit cleanly");
    }

    #[test]
    fn test_spawn_dcgm_exporter_success() {
        let mut nvrc = NVRC::default();
        nvrc.dcgm_enabled = Some(true);
        nvrc.spawn_dcgm_exporter(TEST_BIN_TRUE)
            .expect("spawn_dcgm_exporter should succeed when DCGM enabled");
    }

    #[test]
    fn test_spawn_fabricmanager_success() {
        let mut nvrc = NVRC::default();
        nvrc.fabricmanager_enabled = Some(true);
        nvrc.spawn_fabricmanager(TEST_BIN_TRUE)
            .expect("spawn_fabricmanager should succeed when enabled");
    }

    #[test]
    fn test_check_daemons_empty() {
        let mut nvrc = NVRC::default();
        assert!(nvrc.check_daemons().is_ok());
    }
}
