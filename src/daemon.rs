// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use crate::config::update_config_file;
use crate::execute::background;
use crate::macros::ResultExt;
use crate::nvrc::NVRC;
use std::fs;

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

const FM_CONFIG: &str = "/usr/share/nvidia/nvswitch/fabricmanager.cfg";
const NVLSM_CONFIG: &str = "/usr/share/nvidia/nvlsm/nvlsm.conf";

/// Configurable path parameters allow testing with /bin/true instead of real
/// NVIDIA binaries that don't exist in the test environment.
impl NVRC {
    /// nvidia-persistenced keeps GPU state warm between container invocations,
    /// reducing cold-start latency. UVM persistence mode enables unified memory
    /// optimizations. Enabled by default since most workloads benefit from it.
    pub fn nvidia_persistenced(&mut self) {
        self.spawn_persistenced("/var/run/nvidia-persistenced", "/bin/nvidia-persistenced")
    }

    fn spawn_persistenced(&mut self, run_dir: &str, bin: &str) {
        fs::create_dir_all(run_dir).or_panic(format_args!("create_dir_all {run_dir}"));
        let uvm_enabled = self.uvm_persistence_mode.unwrap_or(true);
        let args = persistenced_args(uvm_enabled);
        let child = background(bin, &args);
        self.track_daemon("nvidia-persistenced", child);
    }

    /// nv-hostengine is the DCGM backend daemon. Only started when DCGM monitoring
    /// is explicitly requested - not needed for basic GPU workloads.
    pub fn nv_hostengine(&mut self) {
        self.spawn_hostengine("/bin/nv-hostengine")
    }

    fn spawn_hostengine(&mut self, bin: &str) {
        if !self.dcgm_enabled.unwrap_or(false) {
            return;
        }
        let child = background(bin, hostengine_args());
        self.track_daemon("nv-hostengine", child);
    }

    /// dcgm-exporter exposes GPU metrics for Prometheus. Only started when DCGM
    /// is enabled - adds overhead so disabled by default.
    pub fn dcgm_exporter(&mut self) {
        self.spawn_dcgm_exporter("/bin/dcgm-exporter")
    }

    fn spawn_dcgm_exporter(&mut self, bin: &str) {
        if !self.dcgm_enabled.unwrap_or(false) {
            return;
        }
        let child = background(bin, dcgm_exporter_args());
        self.track_daemon("dcgm-exporter", child);
    }

    /// NVSwitch fabric manager is only needed for multi-GPU NVLink topologies.
    /// Disabled by default since most VMs have single GPUs.
    pub fn nv_fabricmanager(&mut self) {
        self.configure_fabricmanager(FM_CONFIG);
        self.spawn_fabricmanager("/bin/nv-fabricmanager")
    }

    fn spawn_fabricmanager(&mut self, bin: &str) {
        if self.fabric_mode.is_none() {
            return;
        }
        let mut args = vec!["-c", FM_CONFIG];
        let guid_owned: String;
        if let Some(ref guid) = self.port_guid {
            guid_owned = guid.clone();
            args.push("-g");
            args.push(&guid_owned);
        }
        let child = background(bin, &args);
        self.track_daemon("nv-fabricmanager", child);
    }

    /// CX7 bridges require NVLSM to manage NVLink subnet before FM can initialize the fabric.
    pub fn nv_nvlsm(&mut self) {
        self.spawn_nvlsm("/opt/nvidia/nvlsm/sbin/nvlsm")
    }

    fn spawn_nvlsm(&mut self, bin: &str) {
        let Some(ref guid) = self.port_guid else {
            return;
        };
        let guid_owned = guid.clone();
        let args = vec!["-F", NVLSM_CONFIG, "-g", &guid_owned];
        let child = background(bin, &args);
        self.track_daemon("nvlsm", child);
    }

    /// Write FABRIC_MODE, FABRIC_MODE_RESTART, and PARTITION_RAIL_POLICY to fabricmanager.cfg.
    /// ServiceVM (mode 1) requires FABRIC_MODE_RESTART=1 for resiliency.
    fn configure_fabricmanager(&self, cfg_path: &str) {
        let Some(mode) = self.fabric_mode else {
            return;
        };

        let mode_str = mode.to_string();
        let restart = if mode == 1 { "1" } else { "0" };
        let policy = self.rail_policy.as_deref().unwrap_or("greedy");

        let updates = &[
            ("FABRIC_MODE", mode_str.as_str()),
            ("FABRIC_MODE_RESTART", restart),
            ("PARTITION_RAIL_POLICY", policy),
        ];

        update_config_file(cfg_path, updates);
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

    // === Skip path tests ===

    #[test]
    fn test_nv_hostengine_skipped_by_default() {
        // DCGM disabled by default - should be a no-op, no daemon spawned
        let mut nvrc = NVRC::default();
        nvrc.nv_hostengine();
        nvrc.check_daemons();
    }

    #[test]
    fn test_dcgm_exporter_skipped_by_default() {
        let mut nvrc = NVRC::default();
        nvrc.dcgm_exporter();
    }

    #[test]
    fn test_nv_fabricmanager_skipped_by_default() {
        let mut nvrc = NVRC::default();
        nvrc.nv_fabricmanager();
    }

    #[test]
    fn test_spawn_persistenced_success() {
        let tmpdir = TempDir::new().unwrap();
        let run_dir = tmpdir.path().join("nvidia-persistenced");

        let mut nvrc = NVRC::default();
        nvrc.spawn_persistenced(run_dir.to_str().unwrap(), "/bin/true");

        // Directory should be created
        assert!(run_dir.exists());

        // Daemon should be tracked and exit cleanly
        nvrc.check_daemons();
    }

    #[test]
    fn test_spawn_persistenced_uvm_disabled() {
        let tmpdir = TempDir::new().unwrap();
        let run_dir = tmpdir.path().join("nvidia-persistenced");

        let mut nvrc = NVRC::default();
        nvrc.uvm_persistence_mode = Some(false); // Tests the else branch for args
        nvrc.spawn_persistenced(run_dir.to_str().unwrap(), "/bin/true");
    }

    #[test]
    fn test_spawn_hostengine_success() {
        let mut nvrc = NVRC::default();
        nvrc.dcgm_enabled = Some(true);
        nvrc.spawn_hostengine("/bin/true");
        nvrc.check_daemons();
    }

    #[test]
    fn test_spawn_dcgm_exporter_success() {
        let mut nvrc = NVRC::default();
        nvrc.dcgm_enabled = Some(true);
        nvrc.spawn_dcgm_exporter("/bin/true");
    }

    #[test]
    fn test_spawn_fabricmanager_success() {
        let mut nvrc = NVRC::default();
        nvrc.fabric_mode = Some(1);
        nvrc.spawn_fabricmanager("/bin/true");
    }

    #[test]
    fn test_spawn_fabricmanager_with_port_guid() {
        let mut nvrc = NVRC::default();
        nvrc.fabric_mode = Some(1);
        nvrc.port_guid = Some("0xdeadbeef".to_string());
        nvrc.spawn_fabricmanager("/bin/true");
        nvrc.health_checks();
    }

    #[test]
    fn test_spawn_nvlsm_success() {
        let mut nvrc = NVRC::default();
        nvrc.port_guid = Some("0xdeadbeef".to_string());
        nvrc.spawn_nvlsm("/bin/true");
        nvrc.health_checks();
    }

    #[test]
    fn test_spawn_nvlsm_skipped_without_guid() {
        let mut nvrc = NVRC::default();
        // port_guid is None, should be a no-op
        nvrc.spawn_nvlsm("/bin/true");
    }

    #[test]
    fn test_spawn_persistenced_binary_not_found() {
        use std::panic;

        let tmpdir = TempDir::new().unwrap();
        let run_dir = tmpdir.path().join("nvidia-persistenced");

        let result = panic::catch_unwind(|| {
            let mut nvrc = NVRC::default();
            nvrc.spawn_persistenced(run_dir.to_str().unwrap(), "/nonexistent/binary");
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_check_daemons_empty() {
        let mut nvrc = NVRC::default();
        nvrc.check_daemons();
    }

    // === Fabricmanager configuration tests ===

    #[test]
    fn test_configure_fabricmanager_mode_0_bare_metal() {
        use tempfile::NamedTempFile;

        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();
        fs::write(path, "").unwrap();

        let mut nvrc = NVRC::default();
        nvrc.fabric_mode = Some(0);
        nvrc.configure_fabricmanager(path);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("FABRIC_MODE=0"));
        assert!(content.contains("FABRIC_MODE_RESTART=0"));
    }

    #[test]
    fn test_configure_fabricmanager_mode_1_servicevm() {
        use tempfile::NamedTempFile;

        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();
        fs::write(path, "").unwrap();

        let mut nvrc = NVRC::default();
        nvrc.fabric_mode = Some(1);
        nvrc.configure_fabricmanager(path);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("FABRIC_MODE=1"));
        assert!(content.contains("FABRIC_MODE_RESTART=1"));
    }

    #[test]
    fn test_configure_fabricmanager_updates_existing() {
        use tempfile::NamedTempFile;

        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();
        fs::write(path, "FABRIC_MODE=0\nFABRIC_MODE_RESTART=0\n").unwrap();

        let mut nvrc = NVRC::default();
        nvrc.fabric_mode = Some(1);
        nvrc.configure_fabricmanager(path);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("FABRIC_MODE=1"));
        assert!(content.contains("FABRIC_MODE_RESTART=1"));
        // Should not have old values
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(
            lines
                .iter()
                .filter(|l| l.starts_with("FABRIC_MODE="))
                .count(),
            1
        );
        assert_eq!(
            lines
                .iter()
                .filter(|l| l.starts_with("FABRIC_MODE_RESTART="))
                .count(),
            1
        );
    }

    #[test]
    fn test_configure_fabricmanager_preserves_other_config() {
        use tempfile::NamedTempFile;

        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();
        fs::write(path, "# Comment\nOTHER_SETTING=value\nFABRIC_MODE=0\n").unwrap();

        let mut nvrc = NVRC::default();
        nvrc.fabric_mode = Some(1);
        nvrc.configure_fabricmanager(path);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("# Comment"));
        assert!(content.contains("OTHER_SETTING=value"));
        assert!(content.contains("FABRIC_MODE=1"));
    }

    #[test]
    fn test_configure_fabricmanager_no_fabric_mode() {
        use tempfile::NamedTempFile;

        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();
        fs::write(path, "ORIGINAL=content\n").unwrap();

        let nvrc = NVRC::default();
        // fabric_mode is None
        nvrc.configure_fabricmanager(path);

        // File should be unchanged
        let content = fs::read_to_string(path).unwrap();
        assert_eq!(content, "ORIGINAL=content\n");
    }

    #[test]
    fn test_configure_fabricmanager_default_rail_policy() {
        use tempfile::NamedTempFile;

        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();
        fs::write(path, "").unwrap();

        let mut nvrc = NVRC::default();
        nvrc.fabric_mode = Some(1);
        // rail_policy is None, should default to greedy
        nvrc.configure_fabricmanager(path);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("PARTITION_RAIL_POLICY=greedy"));
    }

    #[test]
    fn test_configure_fabricmanager_symmetric_rail_policy() {
        use tempfile::NamedTempFile;

        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();
        fs::write(path, "").unwrap();

        let mut nvrc = NVRC::default();
        nvrc.fabric_mode = Some(1);
        nvrc.rail_policy = Some("symmetric".to_owned());
        nvrc.configure_fabricmanager(path);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("PARTITION_RAIL_POLICY=symmetric"));
    }

    #[test]
    fn test_configure_fabricmanager_all_settings() {
        use tempfile::NamedTempFile;

        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();
        fs::write(path, "").unwrap();

        let mut nvrc = NVRC::default();
        nvrc.fabric_mode = Some(1);
        nvrc.rail_policy = Some("symmetric".to_owned());
        nvrc.configure_fabricmanager(path);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("FABRIC_MODE=1"));
        assert!(content.contains("FABRIC_MODE_RESTART=1"));
        assert!(content.contains("PARTITION_RAIL_POLICY=symmetric"));
    }
}
