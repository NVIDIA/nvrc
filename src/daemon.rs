// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use crate::config::update_config_file;
use crate::gpu_extension;
use crate::kmsg;
use crate::macros::ResultExt;
use crate::nvrc::NVRC;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::OnceLock;

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

/// Kubernetes mode disables standalone HTTP server (we're behind kata-agent).
/// The collectors file ships with the exporter, so it follows the gpu extension.
fn dcgm_exporter_args() -> &'static [&'static str] {
    static COUNTERS: OnceLock<String> = OnceLock::new();
    static ARGS: OnceLock<Vec<&'static str>> = OnceLock::new();
    ARGS.get_or_init(|| {
        let counters = COUNTERS.get_or_init(|| gpu_extension::path(DCGM_EXPORTER_COUNTERS));
        vec!["-k", "-f", counters.as_str()]
    })
}

const FM_CONFIG: &str = "/usr/share/nvidia/nvswitch/fabricmanager.cfg";
const FM_RUNTIME_CONFIG: &str = "/run/fabricmanager.cfg";
const NVLSM_CONFIG: &str = "/usr/share/nvidia/nvlsm/nvlsm.conf";
const DCGM_EXPORTER_COUNTERS: &str = "/etc/dcgm-exporter/default-counters.csv";

/// FABRIC_MODE=0: full GPU passthrough, FM manages NVSwitches directly.
pub const FABRIC_MODE_FULL: u8 = 0;
/// FABRIC_MODE=1: shared NVSwitch virtualization, GPUs in tenant VMs.
pub const FABRIC_MODE_SHARED: u8 = 1;

/// Configurable path parameters allow testing with /bin/true instead of real
/// NVIDIA binaries that don't exist in the test environment.
impl NVRC {
    /// nvidia-persistenced keeps GPU state warm between container invocations,
    /// reducing cold-start latency. UVM persistence mode enables unified memory
    /// optimizations. Enabled by default since most workloads benefit from it.
    pub fn nvidia_persistenced(&mut self) {
        kmsg::wait_for_marker_after("Local RPC services initialized", 600, || {
            self.spawn_persistenced(
                "/var/run/nvidia-persistenced",
                &gpu_extension::path("/bin/nvidia-persistenced"),
            )
        });
    }

    fn spawn_persistenced(&mut self, run_dir: &str, bin: &str) {
        fs::create_dir_all(run_dir).or_panic(format_args!("create_dir_all {run_dir}"));
        let args = persistenced_args(self.uvm_persistence_mode.unwrap_or(true));
        self.spawn_daemon("nvidia-persistenced", bin, &args);
    }

    /// nv-hostengine is the DCGM backend daemon. Only started when DCGM monitoring
    /// is explicitly requested - not needed for basic GPU workloads.
    pub fn nv_hostengine(&mut self) {
        self.spawn_hostengine(&gpu_extension::path("/bin/nv-hostengine"))
    }

    fn spawn_hostengine(&mut self, bin: &str) {
        self.spawn_if_dcgm("nv-hostengine", bin, hostengine_args());
    }

    /// dcgm-exporter exposes GPU metrics for Prometheus. Only started when DCGM
    /// is enabled - adds overhead so disabled by default.
    pub fn dcgm_exporter(&mut self) {
        self.spawn_dcgm_exporter(&gpu_extension::path("/bin/dcgm-exporter"))
    }

    fn spawn_dcgm_exporter(&mut self, bin: &str) {
        self.spawn_if_dcgm("dcgm-exporter", bin, dcgm_exporter_args());
    }

    fn spawn_if_dcgm(&mut self, name: &str, bin: &str, args: &[&str]) {
        if self.dcgm_enabled.unwrap_or(false) {
            self.spawn_daemon(name, bin, args);
        }
    }

    /// NVSwitch fabric manager is only needed for multi-GPU NVLink topologies.
    /// Disabled by default since most VMs have single GPUs.
    pub fn nv_fabricmanager(&mut self, fabric_mode: u8, rail_policy: &str) {
        // The stock config ships in the gpu extension; the editable runtime copy
        // stays on the writable /run tmpfs.
        let fm_config = gpu_extension::path(FM_CONFIG);
        fs::copy(&fm_config, FM_RUNTIME_CONFIG)
            .or_panic(format_args!("copy {fm_config} to {FM_RUNTIME_CONFIG}"));
        self.configure_fabricmanager(FM_RUNTIME_CONFIG, fabric_mode, rail_policy);
        fs::set_permissions(FM_RUNTIME_CONFIG, fs::Permissions::from_mode(0o400))
            .or_panic(format_args!("set permissions {FM_RUNTIME_CONFIG}"));
        kmsg::wait_for_marker_after("FM starting NvLink Inband", 120, || {
            self.spawn_fabricmanager(&gpu_extension::path("/bin/nv-fabricmanager"))
        });
    }

    fn spawn_fabricmanager(&mut self, bin: &str) {
        let guid = self.port_guid.clone();
        let args: Vec<&str> = ["-c", FM_RUNTIME_CONFIG]
            .into_iter()
            .chain(guid.iter().flat_map(|g| ["-g", g.as_str()]))
            .collect();
        self.spawn_daemon("nv-fabricmanager", bin, &args);
    }

    /// CX7 bridges require NVLSM to manage NVLink subnet before FM can initialize the fabric.
    pub fn nv_nvlsm(&mut self) {
        self.spawn_nvlsm(&gpu_extension::path("/sbin/nvlsm"))
    }

    fn spawn_nvlsm(&mut self, bin: &str) {
        let Some(guid) = self.port_guid.clone() else {
            return;
        };
        let nvlsm_config = gpu_extension::path(NVLSM_CONFIG);
        let args = ["-F", &nvlsm_config, "-g", &guid, "-f", "stdout"];
        self.spawn_daemon("nvlsm", bin, &args);
    }

    /// Write FABRIC_MODE and PARTITION_RAIL_POLICY to fabricmanager.cfg.
    /// FABRIC_MODE: 0 = bare metal (GPUs local), 1 = service VM (GPUs in tenant VMs)
    /// PARTITION_RAIL_POLICY: "greedy" (NVL4) or "symmetric" (NVL5, required for CC on Blackwell)
    fn configure_fabricmanager(&self, cfg_path: &str, fabric_mode: u8, rail_policy: &str) {
        let fm = &fabric_mode.to_string();
        let updates = &[
            ("FABRIC_MODE", fm.as_str()),
            ("PARTITION_RAIL_POLICY", rail_policy),
        ];
        update_config_file(cfg_path, updates);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
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
        let counters = gpu_extension::path(DCGM_EXPORTER_COUNTERS);
        assert_eq!(dcgm_exporter_args(), &["-k", "-f", counters.as_str()]);
    }

    // === Skip path tests ===

    #[test]
    fn test_nv_hostengine_skipped_by_default() {
        // DCGM disabled by default - should be a no-op, no daemon spawned
        let mut nvrc = NVRC::default();
        nvrc.nv_hostengine();
        nvrc.health_checks();
    }

    #[test]
    fn test_dcgm_exporter_skipped_by_default() {
        let mut nvrc = NVRC::default();
        nvrc.dcgm_exporter();
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    fn test_nv_fabricmanager_gpu_mode() {
        use tempfile::NamedTempFile;

        let tmpfile = NamedTempFile::new().unwrap();
        let cfg = tmpfile.path().to_str().unwrap();
        fs::write(cfg, "FABRIC_MODE=0\n").unwrap();

        let mut nvrc = NVRC::default();
        nvrc.configure_fabricmanager(cfg, FABRIC_MODE_FULL, "greedy");
        nvrc.spawn_fabricmanager("/bin/true");

        let content = fs::read_to_string(cfg).unwrap();
        assert!(content.contains("FABRIC_MODE=0"));
        nvrc.health_checks();
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    fn test_spawn_persistenced_success() {
        let tmpdir = TempDir::new().unwrap();
        let run_dir = tmpdir.path().join("nvidia-persistenced");

        let mut nvrc = NVRC::default();
        nvrc.spawn_persistenced(run_dir.to_str().unwrap(), "/bin/true");

        // Directory should be created
        assert!(run_dir.exists());

        // Daemon should be tracked and exit cleanly
        nvrc.health_checks();
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    fn test_spawn_persistenced_uvm_disabled() {
        let tmpdir = TempDir::new().unwrap();
        let run_dir = tmpdir.path().join("nvidia-persistenced");

        let mut nvrc = NVRC::default();
        nvrc.uvm_persistence_mode = Some(false); // Tests the else branch for args
        nvrc.spawn_persistenced(run_dir.to_str().unwrap(), "/bin/true");
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    fn test_spawn_hostengine_success() {
        let mut nvrc = NVRC::default();
        nvrc.dcgm_enabled = Some(true);
        nvrc.spawn_hostengine("/bin/true");
        nvrc.health_checks();
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    fn test_spawn_dcgm_exporter_success() {
        let mut nvrc = NVRC::default();
        nvrc.dcgm_enabled = Some(true);
        nvrc.spawn_dcgm_exporter("/bin/true");
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    fn test_spawn_fabricmanager_success() {
        let mut nvrc = NVRC::default();
        nvrc.spawn_fabricmanager("/bin/true");
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    fn test_spawn_fabricmanager_with_port_guid() {
        let mut nvrc = NVRC::default();
        nvrc.port_guid = Some("0xdeadbeef".to_string());
        nvrc.spawn_fabricmanager("/bin/true");
        nvrc.health_checks();
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    fn test_spawn_nvlsm_success() {
        let mut nvrc = NVRC::default();
        nvrc.port_guid = Some("0xdeadbeef".to_string());
        nvrc.spawn_nvlsm("/bin/true");
        nvrc.health_checks();
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
    fn test_spawn_nvlsm_skipped_without_guid() {
        let mut nvrc = NVRC::default();
        // port_guid is None, should be a no-op
        nvrc.spawn_nvlsm("/bin/true");
    }

    #[test]
    fn test_nv_nvlsm_skipped_without_guid() {
        // Resolves the nvlsm binary path, then no-ops without a port GUID.
        let mut nvrc = NVRC::default();
        nvrc.nv_nvlsm();
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri cannot emulate process spawn")]
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
    fn test_health_checks_empty() {
        let mut nvrc = NVRC::default();
        nvrc.health_checks();
    }

    // === Fabricmanager configuration tests ===

    /// Run configure_fabricmanager over a config seeded with `initial`.
    fn configured(initial: &str, fabric_mode: u8, rail_policy: &str) -> String {
        use tempfile::NamedTempFile;

        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();
        fs::write(path, initial).unwrap();

        NVRC::default().configure_fabricmanager(path, fabric_mode, rail_policy);

        fs::read_to_string(path).unwrap()
    }

    #[rstest]
    #[case::bare_metal_greedy(FABRIC_MODE_FULL, "greedy")]
    #[case::bare_metal_nvl5_symmetric(FABRIC_MODE_FULL, "symmetric")]
    #[case::servicevm_nvl4(FABRIC_MODE_SHARED, "greedy")]
    #[case::servicevm_nvl5(FABRIC_MODE_SHARED, "symmetric")]
    fn test_configure_fabricmanager_writes_mode_and_rail_policy(
        #[case] fabric_mode: u8,
        #[case] rail_policy: &str,
    ) {
        let content = configured("", fabric_mode, rail_policy);

        assert!(content.contains(&format!("FABRIC_MODE={fabric_mode}")));
        assert!(content.contains(&format!("PARTITION_RAIL_POLICY={rail_policy}")));
    }

    #[test]
    fn test_configure_fabricmanager_updates_existing() {
        let content = configured("FABRIC_MODE=0\n", FABRIC_MODE_SHARED, "greedy");

        assert!(content.contains("FABRIC_MODE=1"));
        assert_eq!(
            content
                .lines()
                .filter(|l| l.starts_with("FABRIC_MODE="))
                .count(),
            1
        );
    }

    #[test]
    fn test_configure_fabricmanager_preserves_other_config() {
        let content = configured(
            "# Comment\nOTHER_SETTING=value\nFABRIC_MODE=0\n",
            FABRIC_MODE_SHARED,
            "greedy",
        );

        assert!(content.contains("# Comment"));
        assert!(content.contains("OTHER_SETTING=value"));
        assert!(content.contains("FABRIC_MODE=1"));
    }
}
