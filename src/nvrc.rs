// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! NVRC configuration state and daemon lifecycle management.

use std::process::Child;

/// Central configuration state for the NVIDIA Runtime Container init.
/// Fields are populated from kernel command-line parameters and control
/// GPU configuration (clocks, power limits) and optional daemons.
#[derive(Default)]
#[allow(clippy::upper_case_acronyms)]
pub struct NVRC {
    /// Operation mode: "gpu" (default) or "cpu" (skip GPU management)
    pub mode: Option<String>,
    /// Set/unset ready state
    pub nvidia_smi_srs: Option<String>,
    /// Lock GPU clocks to specific frequency
    pub nvidia_smi_lgc: Option<u32>,
    /// Lock memory clocks to specific frequency
    pub nvidia_smi_lmc: Option<u32>,
    /// Set power limit in watts
    pub nvidia_smi_pl: Option<u32>,
    /// Enable UVM persistence mode for unified memory optimization
    pub uvm_persistence_mode: Option<bool>,
    /// Enable DCGM exporter for GPU metrics
    pub dcgm_enabled: Option<bool>,
    /// Fabric Manager mode: 0=bare metal, 1=servicevm
    pub fabric_mode: Option<u8>,
    /// Fabric Manager rail policy: "greedy" (default) or "symmetric"
    pub rail_policy: Option<String>,
    /// Port GUID for NVL5+ systems (0x-prefixed hex string)
    pub port_guid: Option<String>,
    /// Tracked background daemons for health monitoring
    children: Vec<(String, Child)>,
}

impl NVRC {
    /// Track a background daemon for later health check.
    /// Critical daemons (persistenced, hostengine, etc.) are tracked here
    /// so we can detect early failures before handing off to kata-agent.
    pub fn track_daemon(&mut self, name: &str, child: Child) {
        self.children.push((name.into(), child));
    }

    /// Check all background daemons haven't failed.
    /// Exit status 0 is OK (daemon may fork and parent exits successfully).
    /// Non-zero exit means the daemon crashed—fail init before kata-agent starts.
    pub fn health_checks(&mut self) {
        for (name, child) in &mut self.children {
            if let Ok(Some(status)) = child.try_wait() {
                if !status.success() {
                    panic!("{} exited with status: {}", name, status);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic;
    use std::process::Command;

    #[test]
    fn test_default() {
        let nvrc = NVRC::default();
        assert!(nvrc.mode.is_none());
        assert!(nvrc.nvidia_smi_srs.is_none());
        assert!(nvrc.nvidia_smi_lgc.is_none());
        assert!(nvrc.children.is_empty());
    }

    #[test]
    fn test_track_daemon() {
        let mut nvrc = NVRC::default();
        let child = Command::new("/bin/true").spawn().unwrap();
        nvrc.track_daemon("test-daemon", child);
        assert_eq!(nvrc.children.len(), 1);
        assert_eq!(nvrc.children[0].0, "test-daemon");
    }

    #[test]
    fn test_health_checks_success() {
        let mut nvrc = NVRC::default();
        // /bin/true exits with 0
        let child = Command::new("/bin/true").spawn().unwrap();
        nvrc.track_daemon("good-daemon", child);
        std::thread::sleep(std::time::Duration::from_millis(50));
        nvrc.health_checks();
    }

    #[test]
    fn test_health_checks_failure() {
        let mut nvrc = NVRC::default();
        // /bin/false exits with 1
        let child = Command::new("/bin/false").spawn().unwrap();
        nvrc.track_daemon("bad-daemon", child);
        std::thread::sleep(std::time::Duration::from_millis(50));
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            nvrc.health_checks();
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_health_checks_still_running() {
        let mut nvrc = NVRC::default();
        // sleep 1 will still be running when we check immediately
        let child = Command::new("/bin/sleep").arg("1").spawn().unwrap();
        nvrc.track_daemon("slow-daemon", child);
        // Check immediately while still running
        nvrc.health_checks();
        // Clean up: kill the child to avoid orphaned process
        if let Some((_, ref mut child)) = nvrc.children.last_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    #[test]
    fn test_health_checks_multiple() {
        let mut nvrc = NVRC::default();
        nvrc.track_daemon("d1", Command::new("/bin/true").spawn().unwrap());
        nvrc.track_daemon("d2", Command::new("/bin/true").spawn().unwrap());
        std::thread::sleep(std::time::Duration::from_millis(50));
        nvrc.health_checks();
    }
}
