use crate::daemon::Action;
use crate::kata_agent;
use crate::kmsg;
use crate::ndev;
use crate::nvrc::NVRC;
use log::{debug, error};
use nix::unistd::{fork, ForkResult};
use std::sync::mpsc;
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result};

impl NVRC {
    pub fn cold_plug(&mut self) -> Result<()> {
        debug!("cold-plug mode");
        self.setup_gpu();
        match unsafe { fork() }.expect("fork cold-plug") {
            ForkResult::Parent { .. } => {
                kata_agent().unwrap();
            }
            ForkResult::Child => loop {
                sleep(Duration::from_secs(1));
                if let Err(e) = self.poll_syslog() {
                    error!("poll syslog: {e}");
                    break;
                }
            },
        }
        Ok(())
    }

    pub fn manage_daemons(&mut self, action: Action) -> Result<()> {
        for f in [
            NVRC::nvidia_persistenced,
            NVRC::nv_hostengine,
            NVRC::dcgm_exporter,
        ] {
            f(self, action.clone())?;
        }
        Ok(())
    }

    pub fn hot_plug(&mut self) -> Result<()> {
        debug!("hot-plug mode");
        match unsafe { fork() }.expect("fork hot-plug") {
            ForkResult::Parent { .. } => {
                kata_agent().unwrap();
            }
            ForkResult::Child => {
                self.handle_hot_plug_events().context("hot-plug events")?;
            }
        }
        Ok(())
    }

    fn handle_hot_plug_events(&mut self) -> Result<()> {
        let (tx, rx) = mpsc::channel::<&str>();
        ndev::udev(tx.clone());
        kmsg::watch_for_pattern("NVRM: Attempting to remove device", tx.clone());
        for ev in rx {
            debug!("event: {ev}");
            match ev {
                "hot-plug" => {
                    self.get_nvidia_devices(None)?;
                    self.setup_gpu();
                }
                "hot-unplug" => {
                    self.manage_daemons(Action::Stop)?;
                    sleep(Duration::from_millis(3000));
                    self.get_nvidia_devices(None)?;
                    if !self.nvidia_devices.is_empty() {
                        self.manage_daemons(Action::Start)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}
