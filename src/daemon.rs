// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{Context, Result};

use crate::execute::background;
use crate::nvrc::NVRC;
use std::fs;

/// Configurable path parameters allow testing with /bin/true instead of real
/// NVIDIA binaries that don't exist in the test environment.
impl NVRC {
    /// nvidia-persistenced keeps GPU state warm between container invocations,
    /// reducing cold-start latency. UVM persistence mode enables unified memory
    /// optimizations. Enabled by default since most workloads benefit from it.
    pub fn nvidia_persistenced(&mut self) -> Result<()> {
        self.spawn_persistenced("/var/run/nvidia-persistenced", "/bin/nvidia-persistenced")
    }

    fn spawn_persistenced(&mut self, run_dir: &str, bin: &str) -> Result<()> {
        fs::create_dir_all(run_dir).with_context(|| format!("create_dir_all {}", run_dir))?;

        let uvm_enabled = self.uvm_persistence_mode.unwrap_or(true);
        let args: &[&str] = if uvm_enabled {
            &["--verbose", "--uvm-persistence-mode"]
        } else {
            &["--verbose"]
        };

        let child = background(bin, args)?;
        self.track_daemon("nvidia-persistenced", child);
        Ok(())
    }

    /// nv-hostengine is the DCGM backend daemon. Only started when DCGM monitoring
    /// is explicitly requested - not needed for basic GPU workloads.
    pub fn nv_hostengine(&mut self) -> Result<()> {
        self.spawn_hostengine("/bin/nv-hostengine")
    }

    fn spawn_hostengine(&mut self, bin: &str) -> Result<()> {
        if !self.dcgm_enabled.unwrap_or(false) {
            return Ok(());
        }
        let child = background(bin, &[])?;
        self.track_daemon("nv-hostengine", child);
        Ok(())
    }

    /// dcgm-exporter exposes GPU metrics for Prometheus. Only started when DCGM
    /// is enabled - adds overhead so disabled by default.
    pub fn dcgm_exporter(&mut self) -> Result<()> {
        self.spawn_dcgm_exporter("/bin/dcgm-exporter")
    }

    fn spawn_dcgm_exporter(&mut self, bin: &str) -> Result<()> {
        if !self.dcgm_enabled.unwrap_or(false) {
            return Ok(());
        }
        let child = background(bin, &[])?;
        self.track_daemon("dcgm-exporter", child);
        Ok(())
    }

    /// NVSwitch fabric manager is only needed for multi-GPU NVLink topologies.
    /// Disabled by default since most VMs have single GPUs.
    pub fn nv_fabricmanager(&mut self) -> Result<()> {
        self.spawn_fabricmanager("/bin/nv-fabricmanager")
    }

    fn spawn_fabricmanager(&mut self, bin: &str) -> Result<()> {
        if !self.fabricmanager_enabled.unwrap_or(false) {
            return Ok(());
        }
        let child = background(bin, &[])?;
        self.track_daemon("nv-fabricmanager", child);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ==================== skip path tests ====================

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

    // ==================== success path tests with fake binaries ====================

    #[test]
    fn test_spawn_persistenced_success() {
        let tmpdir = TempDir::new().unwrap();
        let run_dir = tmpdir.path().join("nvidia-persistenced");

        let mut nvrc = NVRC::default();
        let result = nvrc.spawn_persistenced(run_dir.to_str().unwrap(), "/bin/true");
        assert!(result.is_ok());

        // Directory should be created
        assert!(run_dir.exists());

        // Daemon should be tracked and exit cleanly
        assert!(nvrc.check_daemons().is_ok());
    }

    #[test]
    fn test_spawn_persistenced_uvm_disabled() {
        let tmpdir = TempDir::new().unwrap();
        let run_dir = tmpdir.path().join("nvidia-persistenced");

        let mut nvrc = NVRC::default();
        nvrc.uvm_persistence_mode = Some(false); // Tests the else branch for args
        let result = nvrc.spawn_persistenced(run_dir.to_str().unwrap(), "/bin/true");
        assert!(result.is_ok());
    }

    #[test]
    fn test_spawn_hostengine_success() {
        let mut nvrc = NVRC::default();
        nvrc.dcgm_enabled = Some(true);
        let result = nvrc.spawn_hostengine("/bin/true");
        assert!(result.is_ok());
        assert!(nvrc.check_daemons().is_ok());
    }

    #[test]
    fn test_spawn_dcgm_exporter_success() {
        let mut nvrc = NVRC::default();
        nvrc.dcgm_enabled = Some(true);
        let result = nvrc.spawn_dcgm_exporter("/bin/true");
        assert!(result.is_ok());
    }

    #[test]
    fn test_spawn_fabricmanager_success() {
        let mut nvrc = NVRC::default();
        nvrc.fabricmanager_enabled = Some(true);
        let result = nvrc.spawn_fabricmanager("/bin/true");
        assert!(result.is_ok());
    }

    // ==================== error path tests ====================

    #[test]
    fn test_spawn_persistenced_binary_not_found() {
        let tmpdir = TempDir::new().unwrap();
        let run_dir = tmpdir.path().join("nvidia-persistenced");

        let mut nvrc = NVRC::default();
        let result = nvrc.spawn_persistenced(run_dir.to_str().unwrap(), "/nonexistent/binary");
        assert!(result.is_err());
    }

    #[test]
    fn test_check_daemons_empty() {
        let mut nvrc = NVRC::default();
        assert!(nvrc.check_daemons().is_ok());
    }
}
