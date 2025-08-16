use anyhow::{anyhow, Context, Result};
use nix::sys::stat::Mode;
use nix::unistd::{chown, mkdir};
use std::fmt;
use std::path::Path;


use crate::coreutils::{cstr_as_str, background};
use crate::nvrc::NVRC;

#[cfg(feature = "confidential")]
use crate::gpu::confidential::CC;
#[cfg(feature = "confidential")]
use crate::coreutils::foreground;

use crate::coreutils::kill_processes_by_comm;

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

impl NVRC {
    fn start(&mut self, daemon: &Name, command: &str, args: &[&str]) -> Result<()> {
        debug!("start {} {}", command, args.join(" "));
        let child = background(command, args).unwrap();
        self.daemons.insert(daemon.clone(), child);
        Ok(())
    }

    fn stop(&mut self, daemon: &Name) -> Result<()> {
        debug!("stop {}", daemon);
        if let Some(mut child) = self.daemons.remove(daemon) {
            child.kill().unwrap();
            child.wait().unwrap();
            let comm_name = daemon.to_string();
            debug!("killing all processes named '{}'", comm_name);
            kill_processes_by_comm(&comm_name).unwrap();
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
            Some(self.identity.user_id.into()),
            Some(self.identity.group_id.into()),
        )
        .with_context(|| format!("Failed to chown {}", DIR))?;

        let mut args: Vec<&str> = vec!["--verbose"];
        if let Some(f) = uvm_flag {
            args.push(f);
        }

        #[cfg(feature = "confidential")]
        warn!("GPU CC mode build: not setting user/group for nvidia-persistenced");

        #[cfg(not(feature = "confidential"))]
        {
            let user = cstr_as_str(&self.identity.user_name)
                .map_err(|e| anyhow!("Failed to convert user name: {:?}", e))?
                .to_string();
            let group = cstr_as_str(&self.identity.group_name)
                .map_err(|e| anyhow!("Failed to convert group name: {:?}", e))?
                .to_string();

            let owned = [user, group];
            args.extend_from_slice(&["-u", &owned[0], "-g", &owned[1]]);
            self.run_daemon(Name::Persistenced, "/bin/nvidia-persistenced", &args, mode)
        }
        #[cfg(feature = "confidential")]
        {
            self.run_daemon(Name::Persistenced, "/bin/nvidia-persistenced", &args, mode)
        }
    }

    pub fn nv_hostengine(&mut self, mode: Action) -> Result<()> {
        if !self.dcgm_enabled.unwrap_or(false) {
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
        if !self.dcgm_enabled.unwrap_or(false) {
            return Ok(());
        }
        self.run_daemon(Name::DCGMExporter, "/bin/dcgm-exporter", &["-k"], mode)
    }

    #[cfg(feature = "confidential")]
    pub fn nvidia_smi_srs(&self) -> Result<()> {
        if self.gpu_cc_mode != Some(CC::On) {
            debug!("CC mode off; skip nvidia-smi conf-compute -srs");
            return Ok(());
        }
        let status = foreground(
            "/bin/nvidia-smi",
            &[
                "conf-compute",
                "-srs",
                self.nvidia_smi_srs.as_deref().unwrap_or("0"),
            ],
        )
        .map_err(|e| anyhow!("failed to run nvidia-smi: {:?}", e))?;

        if status != 0 {
            return Err(anyhow!("nvidia-smi failed with exit code: {}", status));
        }

        Ok(())
    }
}

