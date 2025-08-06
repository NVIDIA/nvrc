use nix::unistd::chown;
use std::ffi::OsStr;
use std::fmt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::kmsg::kmsg;
use crate::nvrc::NVRC;
use nix::sys::stat::Mode;
use nix::unistd::mkdir;

use sysinfo::System;

use anyhow::{anyhow, Context, Result};

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
        .unwrap_or_else(|_| panic!("failed to start {command}"));

    match child.try_wait() {
        Ok(Some(status)) => Err(anyhow!("{} exited with status: {}", command, status)),
        Ok(None) => Ok(child),
        Err(e) => Err(anyhow!("error attempting to wait: {}", e)),
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
    fn start(&mut self, daemon: &Name, command: &str, args: &[&str]) {
        debug!("start {} {}", command, args.join(" "));
        let child = background(command, args).unwrap();
        self.daemons.insert(daemon.clone(), child);
    }

    fn stop(&mut self, daemon: &Name) {
        debug!("stop {}", daemon);
        if let Some(mut child) = self.daemons.remove(daemon) {
            child.kill().unwrap();
            child.wait().unwrap();

            let comm_name = format!("{daemon}");

            debug!("killing all processes named '{}'", comm_name);
            kill_processes_by_comm(comm_name.as_str());
        } else {
            debug!("daemon not running: {:?}", daemon);
        }
    }

    fn restart(&mut self, command: &str, args: &[&str], daemon: &Name) {
        debug!("restart {} {}", command, args.join(" "));
        self.stop(daemon);
        self.start(daemon, command, args);
    }

    pub fn nvidia_persistenced(&mut self, mode: Action) -> Result<()> {
        let mut uvm_persistence_mode = "";
        match self.uvm_persistence_mode {
            Some(ref mode) => {
                if mode == "on" {
                    uvm_persistence_mode = "--uvm-persistence-mode";
                } else if mode == "off" {
                    uvm_persistence_mode = "";
                }
            }
            None => {
                uvm_persistence_mode = "--uvm-persistence-mode";
            }
        }
        let u: &String = &self.identity.user_name.clone();
        let g = &self.identity.group_name.clone();

        let uid = self.identity.user_id;
        let gid = self.identity.group_id;

        let dir_path = "/var/run/nvidia-persistenced";

        if !Path::new(dir_path).exists() {
            mkdir(dir_path, Mode::S_IRWXU).unwrap();
        }

        chown("/var/run/nvidia-persistenced", Some(uid), Some(gid)).unwrap();

        let command = "/bin/nvidia-persistenced";

        let mut args = vec!["--verbose", uvm_persistence_mode];

        match self.gpu_cc_mode {
            Some(ref mode) if mode == "on" => {
                warn!("TODO: Running in GPU Confidential Computing mode, not setting user/group for nvidia-persistenced");
            }
            _ => {
                args.extend_from_slice(&["-u", u, "-g", g]);
            }
        }

        match mode {
            Action::Start => self.start(&Name::Persistenced, command, &args),
            Action::Stop => self.stop(&Name::Persistenced),
            Action::Restart => self.restart(command, &args, &Name::Persistenced),
        }

        Ok(())
    }

    pub fn nv_hostengine(&mut self, mode: Action) -> Result<()> {
        if !self.dcgm_enabled.unwrap_or(false) {
            return Ok(());
        }
        let command = "/bin/nv-hostengine";
        let args = ["--service-account", "nvidia-dcgm", "--home-dir", "/tmp"];

        match mode {
            Action::Start => self.start(&Name::NVHostengine, command, &args),
            Action::Stop => self.stop(&Name::NVHostengine),
            Action::Restart => self.restart(command, &args, &Name::NVHostengine),
        }
        Ok(())
    }

    pub fn dcgm_exporter(&mut self, mode: Action) -> Result<()> {
        if !self.dcgm_enabled.unwrap_or(false) {
            return Ok(());
        }
        let command = "/bin/dcgm-exporter";
        let args = ["-k"];

        match mode {
            Action::Start => self.start(&Name::DCGMExporter, command, &args),
            Action::Stop => self.stop(&Name::DCGMExporter),
            Action::Restart => self.restart(command, &args, &Name::DCGMExporter),
        }
        Ok(())
    }

    pub fn nvidia_smi_srs(&self) -> Result<()> {
        if self.gpu_cc_mode != Some("on".to_string()) {
            debug!("CC mode is off, skipping nvidia-smi conf-compute -srs");
            return Ok(());
        }
        let command = "/bin/nvidia-smi";
        let args = [
            "conf-compute",
            "-srs",
            self.nvidia_smi_srs.as_deref().unwrap_or("0"),
        ];
        foreground(command, &args)
    }
}
