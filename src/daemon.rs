// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{anyhow, Context, Result};
use nix::sys::stat::Mode;
use nix::unistd::{chown, mkdir};
use std::ffi::OsStr;
use std::fmt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use sysinfo::System;

use crate::kmsg::kmsg;
use crate::nvrc::NVRC;

/// RAII wrapper for daemon Child processes to ensure cleanup on drop
#[derive(Debug)]
pub struct ManagedChild {
    child: Child,
    name: Name,
}

impl ManagedChild {
    pub fn new(child: Child, name: Name) -> Self {
        Self { child, name }
    }

    /// Attempt to kill the child process
    pub fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill()
    }

    /// Wait for the child process to exit
    pub fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait()
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        debug!(
            "Cleaning up daemon {:?} (PID {})",
            self.name,
            self.child.id()
        );
        // Attempt to kill if still running
        let _ = self.child.kill();
        // Wait to prevent zombie processes
        let _ = self.child.wait();
    }
}

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
    NVFabricmanager,
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let full_str = match self {
            Name::Persistenced => "nvidia-persistenced",
            Name::NVHostengine => "nv-hostengine",
            Name::DCGMExporter => "dcgm-exporter",
            Name::NVFabricmanager => "nv-fabricmanager",
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
    let processes = system.processes_by_name(OsStr::new(target_name));

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
        let managed = ManagedChild::new(child, daemon.clone());
        self.daemons.insert(daemon.clone(), managed);
        Ok(())
    }

    fn stop(&mut self, daemon: &Name) -> Result<()> {
        debug!("stop {}", daemon);
        if let Some(mut managed_child) = self.daemons.remove(daemon) {
            // Try to kill, but don't fail if already dead
            if let Err(e) = managed_child.kill() {
                if e.kind() != std::io::ErrorKind::InvalidInput {
                    return Err(anyhow!(e)).context("Failed to kill daemon process");
                }
                debug!("daemon {:?} already exited", daemon);
            }
            if let Err(e) = managed_child.wait() {
                if e.kind() != std::io::ErrorKind::InvalidInput {
                    return Err(anyhow!(e)).context("Failed to wait for daemon process");
                }
                debug!("daemon {:?} already exited (wait)", daemon);
            }
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

    // New generic helper to run a daemon according to Action
    fn run_daemon(&mut self, name: Name, cmd: &str, args: &[&str], mode: Action) -> Result<()> {
        match mode {
            Action::Start => self.start(&name, cmd, args)?,
            Action::Stop => self.stop(&name)?,
            Action::Restart => self.restart(cmd, args, &name)?,
        }
        Ok(())
    }

    pub fn nvidia_persistenced(&mut self, mode: Action) -> Result<()> {
        let uvm_flag = match self.uvm_persistence_mode.as_deref() {
            Some("off") => None,
            Some("on") | None => Some("--uvm-persistence-mode"),
            Some(other) => {
                warn!(
                    "Unknown UVM persistence mode '{}', defaulting to 'on'",
                    other
                );
                Some("--uvm-persistence-mode")
            }
        };

        const DIR: &str = "/var/run/nvidia-persistenced"; // scoped constant for readability
        if !Path::new(DIR).exists() {
            mkdir(DIR, Mode::S_IRWXU).with_context(|| format!("Failed to create dir {}", DIR))?;
        }
        chown(
            DIR,
            Some(self.identity.user_id),
            Some(self.identity.group_id),
        )
        .with_context(|| format!("Failed to chown {}", DIR))?;

        let mut args: Vec<&str> = vec!["--verbose"];
        if let Some(f) = uvm_flag {
            args.push(f);
        }

        #[cfg(feature = "confidential")]
        warn!("GPU CC mode build: not setting user/group for nvidia-persistenced");

        // TODO: nvidia-persistenced will not start with -u or -g flag in both modes
        #[cfg(not(feature = "confidential"))]
        {
            let user = self.identity.user_name.clone();
            let group = self.identity.group_name.clone();
            let _owned = [user, group];
            //args.extend_from_slice(&["-u", owned[0].as_str(), "-g", owned[1].as_str()]);
            self.run_daemon(Name::Persistenced, "/bin/nvidia-persistenced", &args, mode)
        }
        #[cfg(feature = "confidential")]
        {
            self.run_daemon(Name::Persistenced, "/bin/nvidia-persistenced", &args, mode)
        }
    }

    pub fn nv_hostengine(&mut self, mode: Action) -> Result<()> {
        if !self.dcgm_enabled {
            return Ok(());
        }
        self.run_daemon(
            Name::NVHostengine,
            "/bin/nv-hostengine",
            &["--service-account", "nvidia-dcgm", "--home-dir", "/tmp"],
            mode,
        )
    }

    pub fn dcgm_exporter(&mut self, mode: Action) -> Result<()> {
        if !self.dcgm_enabled {
            return Ok(());
        }
        self.run_daemon(
            Name::DCGMExporter,
            "/bin/dcgm-exporter",
            &["-k", "-f", "/etc/dcgm-exporter/default-counters.csv"],
            mode,
        )
    }

    /// Execute nvidia-smi Secure Remote Services (SRS) command
    ///
    /// This method delegates to the CC provider, which:
    /// - In confidential builds: Executes the actual SRS command if CC is enabled
    /// - In standard builds: No-ops (StandardGpuProvider)
    ///
    /// This allows feature-independent code while maintaining correct behavior.
    pub fn nvidia_smi_srs(&self) -> Result<()> {
        self.cc_provider
            .gpu()
            .execute_srs_command(self.nvidia_smi_srs.as_deref())
            .map_err(|e| anyhow::anyhow!(e))
    }

    pub fn nv_fabricmanager(&mut self, mode: Action) -> Result<()> {
        if !self.fabricmanager_enabled {
            return Ok(());
        }
        self.run_daemon(
            Name::NVFabricmanager,
            "/bin/nv-fabricmanager",
            &["-c", "/usr/share/nvidia/nvswitch/fabricmanager.cfg"],
            mode,
        )
    }
}
