// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{Context, Result};
use log::debug;
use std::collections::HashMap;
use std::os::unix::net::UnixDatagram;
use std::sync::Arc;
use std::thread;
use std::thread::sleep;
use std::time::Duration;

use crate::core::traits::{CCProvider, CpuVendor, PlatformInfo};
use crate::core::PlugMode;
use crate::daemon::{ManagedChild, Name};
use crate::devices::NvidiaDevice;
use crate::user_group::UserGroup;

#[derive(Debug)]
#[allow(clippy::upper_case_acronyms)]
pub struct NVRC {
    // Configuration
    /// nvidia-smi SRS (Secure Remote Services) value
    /// Used by daemon::nvidia_smi_srs() which delegates to the CC provider
    pub nvidia_smi_srs: Option<String>,
    /// nvidia-smi LGC (Low GPU Clock) value - reserved for future use
    #[allow(dead_code)]
    pub nvidia_smi_lgc: Option<String>,
    pub uvm_persistence_mode: Option<String>,
    pub dcgm_enabled: bool,
    pub fabricmanager_enabled: bool,

    // Hardware detection
    pub cpu_vendor: Option<CpuVendor>,
    #[allow(dead_code)]
    pub platform_info: Option<PlatformInfo>,
    pub nvidia_devices: Vec<NvidiaDevice>,
    pub gpu_supported: bool,

    // CC provider (replaces scattered cfg attributes)
    pub cc_provider: Arc<dyn CCProvider>,

    // Plug mode (replaces HashMap dispatch)
    pub plug_mode: PlugMode,

    // Runtime state
    pub identity: UserGroup,
    pub daemons: HashMap<Name, ManagedChild>,
    pub syslog_socket: Option<UnixDatagram>,
}

impl Default for NVRC {
    fn default() -> Self {
        // Note: Default uses StandardProvider. For real usage, use NVRCBuilder
        // which properly detects platform and creates the correct provider.
        Self {
            nvidia_smi_srs: None,
            nvidia_smi_lgc: None,
            uvm_persistence_mode: None,
            dcgm_enabled: false,
            fabricmanager_enabled: false,
            cpu_vendor: None,
            platform_info: None,
            nvidia_devices: Vec::new(),
            gpu_supported: false,
            cc_provider: Arc::new(crate::providers::StandardProvider::new()),
            plug_mode: PlugMode::default(),
            identity: UserGroup::new(),
            daemons: HashMap::new(),
            syslog_socket: None,
        }
    }
}

impl NVRC {
    pub fn setup_syslog(&mut self) -> Result<()> {
        let socket = crate::syslog::dev_log_setup().context("syslog socket")?;
        self.syslog_socket = Some(socket);
        Ok(())
    }

    pub fn poll_syslog(&self) -> Result<()> {
        if let Some(socket) = &self.syslog_socket {
            crate::syslog::poll_dev_log(socket).context("poll syslog")?;
        }
        Ok(())
    }

    pub fn watch_poll_syslog(&self) -> Result<()> {
        if let Some(socket) = &self.syslog_socket {
            thread::spawn({
                let socket = socket.try_clone().context("clone syslog socket")?;
                move || loop {
                    if let Err(e) = crate::syslog::poll_dev_log(&socket) {
                        error!("poll syslog: {e}");
                        break;
                    }
                    sleep(Duration::from_secs(1));
                }
            });
        }
        Ok(())
    }

    pub fn set_random_identity(&mut self) -> anyhow::Result<()> {
        self.identity = crate::user_group::random_user_group()?;
        Ok(())
    }

    /// Query CPU vendor using platform detector
    ///
    /// This is a convenience method that uses the platform module's
    /// vendor detection and stores the result in the NVRC struct.
    pub fn query_cpu_vendor(&mut self) -> anyhow::Result<()> {
        let vendor =
            crate::platform::detector::detect_cpu_vendor().map_err(|e| anyhow::anyhow!(e))?;
        debug!("CPU vendor: {:?}", vendor);
        self.cpu_vendor = Some(vendor);
        Ok(())
    }
}

// Old parameter handlers removed - all functionality now in config module
// See: src/config/mod.rs for KernelParams parsing
// See: src/config/parser.rs for parse_boolean() and other utilities

// Old tests removed - all parameter parsing now tested in config module
// See: src/config/mod.rs tests for KernelParams parsing tests
// See: src/config/parser.rs tests for parse_boolean() tests
