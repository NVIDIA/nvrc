use anyhow::{anyhow, Context, Result};
use nix::sys::stat::Mode;
use nix::unistd::{chown, mkdir};
use std::ffi::OsStr;
use std::fmt;
use std::path::Path;
use std::process::{Command, Stdio};
use sysinfo::System;

use crate::kmsg::kmsg;
use crate::nvrc::NVRC;

/// Directory for nvidia-persistenced runtime files
const NVIDIA_PERSISTENCED_DIR: &str = "/var/run/nvidia-persistenced";

/// Command paths
const NVIDIA_PERSISTENCED_CMD: &str = "/bin/nvidia-persistenced";
const NV_HOSTENGINE_CMD: &str = "/bin/nv-hostengine";
const DCGM_EXPORTER_CMD: &str = "/bin/dcgm-exporter";
const NVIDIA_SMI_CMD: &str = "/bin/nvidia-smi";

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub enum Action {
    Start,
    Stop,
    Restart,
}

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub enum Name {
    Persistenced,
    NVHostengine,
    DCGMExporter,
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // First, pick the full string
        let full_str = match self {
            Name::Persistenced => "nvidia-persistenced",
            Name::NVHostengine => "nv-hostengine",
            Name::DCGMExporter => "dcgm-exporter",
        };
        // Truncate to 15 characters if longer; since /proc/<pid>/comm is
        // limited to 16 - 15 characters plus \0
        let truncated = &full_str[..std::cmp::min(full_str.len(), 15)];
        write!(f, "{truncated}")
    }
}

pub fn foreground(command: &str, args: &[&str]) -> Result<()> {
    debug!("{} {}", command, args.join(" "));

    let kmsg_file = kmsg().context("Failed to open kmsg device")?;
    let output = Command::new(command)
        .stdout(Stdio::from(kmsg_file.try_clone().unwrap()))
        .stderr(Stdio::from(kmsg_file))
        .args(args)
        .output()
        .context(format!("failed to execute {command}"))?;

    if !output.status.success() {
        return Err(anyhow!("{} failed with status: {}", command, output.status,));
    }
    Ok(())
}

fn background(command: &str, args: &[&str]) -> Result<std::process::Child> {
    let kmsg_file = kmsg().context("Failed to open kmsg device")?;
    let mut child = Command::new(command)
        .args(args)
        .stdout(Stdio::from(kmsg_file.try_clone().unwrap()))
        .stderr(Stdio::from(kmsg_file))
        .spawn()
        .with_context(|| format!("Failed to start {}", command))?;

    match child.try_wait() {
        Ok(Some(status)) => Err(anyhow!("{} exited with status: {}", command, status)),
        Ok(None) => Ok(child),
        Err(e) => Err(anyhow!("Error attempting to wait: {}", e)),
    }
}

fn kill_processes_by_comm(target_name: &str) {
    let mut system = System::new_all();
    // Refresh process info so `system.processes_by_name()` is up‐to‐date
    system.refresh_all();
    let os_str_name = OsStr::new(target_name);
    let processes = system.processes_by_name(os_str_name);

    for process in processes {
        debug!(
            "found PID {} matching name: '{}'",
            process.pid(),
            target_name
        );
        if !process.kill() {
            debug!("failed to send SIGTERM to PID {}", process.pid());
        }
        process.wait();
    }
}

impl NVRC {
    fn start(&mut self, daemon: &Name, command: &str, args: &[&str]) -> Result<()> {
        debug!("start {} {}", command, args.join(" "));
        let child = background(command, args)?;
        self.daemons.insert(daemon.clone(), child);
        Ok(())
    }

    fn stop(&mut self, daemon: &Name) -> Result<()> {
        debug!("stop {}", daemon);
        if let Some(mut child) = self.daemons.remove(daemon) {
            child.kill().context("Failed to kill daemon process")?;
            child.wait().context("Failed to wait for daemon process")?;

            let comm_name = daemon.to_string();
            debug!("killing all processes named '{}'", comm_name);
            kill_processes_by_comm(&comm_name);
        } else {
            debug!("daemon not running: {:?}", daemon);
        }
        Ok(())
    }

    fn restart(&mut self, command: &str, args: &[&str], daemon: &Name) -> Result<()> {
        debug!("restart {} {}", command, args.join(" "));
        self.stop(daemon)?;
        self.start(daemon, command, args)?;
        Ok(())
    }

    /// Configure and manage nvidia-persistenced daemon
    pub fn nvidia_persistenced(&mut self, mode: Action) -> Result<()> {
        let uvm_persistence_mode = match self.uvm_persistence_mode.as_deref() {
            Some("on") => "--uvm-persistence-mode",
            Some("off") => "",
            None => "--uvm-persistence-mode",
            Some(other) => {
                warn!(
                    "Unknown UVM persistence mode '{}', defaulting to 'on'",
                    other
                );
                "--uvm-persistence-mode"
            }
        };

        let user_name = self.identity.user_name.clone();
        let group_name = self.identity.group_name.clone();
        let uid = self.identity.user_id;
        let gid = self.identity.group_id;

        // Create runtime directory if it doesn't exist
        if !Path::new(NVIDIA_PERSISTENCED_DIR).exists() {
            mkdir(NVIDIA_PERSISTENCED_DIR, Mode::S_IRWXU).with_context(|| {
                format!("Failed to create directory {}", NVIDIA_PERSISTENCED_DIR)
            })?;
        }

        chown(NVIDIA_PERSISTENCED_DIR, Some(uid), Some(gid))
            .with_context(|| format!("Failed to chown {}", NVIDIA_PERSISTENCED_DIR))?;

        let mut args = vec!["--verbose"];
        if !uvm_persistence_mode.is_empty() {
            args.push(uvm_persistence_mode);
        }

        let gpu_cc_mode = self.gpu_cc_mode.clone();
        match gpu_cc_mode.as_deref() {
            Some("on") => {
                warn!("TODO: Running in GPU Confidential Computing mode, not setting user/group for nvidia-persistenced");
            }
            _ => {
                args.extend_from_slice(&["-u", &user_name, "-g", &group_name]);
            }
        }

        match mode {
            Action::Start => self.start(&Name::Persistenced, NVIDIA_PERSISTENCED_CMD, &args)?,
            Action::Stop => self.stop(&Name::Persistenced)?,
            Action::Restart => self.restart(NVIDIA_PERSISTENCED_CMD, &args, &Name::Persistenced)?,
        }

        Ok(())
    }

    /// Manage nv-hostengine daemon for DCGM
    pub fn nv_hostengine(&mut self, mode: Action) -> Result<()> {
        if !self.dcgm_enabled.unwrap_or(false) {
            return Ok(());
        }

        let args = ["--service-account", "nvidia-dcgm", "--home-dir", "/tmp"];

        match mode {
            Action::Start => self.start(&Name::NVHostengine, NV_HOSTENGINE_CMD, &args)?,
            Action::Stop => self.stop(&Name::NVHostengine)?,
            Action::Restart => self.restart(NV_HOSTENGINE_CMD, &args, &Name::NVHostengine)?,
        }
        Ok(())
    }

    /// Manage dcgm-exporter daemon
    pub fn dcgm_exporter(&mut self, mode: Action) -> Result<()> {
        if !self.dcgm_enabled.unwrap_or(false) {
            return Ok(());
        }

        let args = ["-k"];

        match mode {
            Action::Start => self.start(&Name::DCGMExporter, DCGM_EXPORTER_CMD, &args)?,
            Action::Stop => self.stop(&Name::DCGMExporter)?,
            Action::Restart => self.restart(DCGM_EXPORTER_CMD, &args, &Name::DCGMExporter)?,
        }
        Ok(())
    }

    /// Configure nvidia-smi secure reset sequence for Confidential Computing
    pub fn nvidia_smi_srs(&self) -> Result<()> {
        if self.gpu_cc_mode != Some("on".to_string()) {
            debug!("CC mode is off, skipping nvidia-smi conf-compute -srs");
            return Ok(());
        }

        let args = [
            "conf-compute",
            "-srs",
            self.nvidia_smi_srs.as_deref().unwrap_or("0"),
        ];

        foreground(NVIDIA_SMI_CMD, &args)
    }
}
