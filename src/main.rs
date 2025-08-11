use anyhow::Result;
use nix::unistd::{fork, ForkResult};
use std::sync::mpsc;
use std::thread::sleep;
use std::time::Duration;

mod check_supported;
mod container_toolkit;
mod coreutils;
mod cpu;
mod daemon;
mod devices;
mod gpu;
mod init;
mod kata_agent;
mod kmsg;
mod lockdown;
mod mount;
mod ndev;
mod nvrc;
mod pci_ids;
mod syslog;
mod user_group;

#[macro_use]
extern crate log;
extern crate kernlog;

macro_rules! must {
    ($expr:expr) => {
        if let Err(e) = $expr {
            panic!("init failure: {} => {e}", stringify!($expr));
        }
    };
    ($expr:expr, $msg:literal) => {
        if let Err(e) = $expr {
            panic!("init failure: {}: {e}", $msg);
        }
    };
}

use container_toolkit::{nvidia_ctk_cdi, nvidia_ctk_system};
use daemon::Action;
use kata_agent::kata_agent;
use nvrc::NVRC;

fn main() {
    lockdown::set_panic_hook();
    let mut init = NVRC::default();
    must!(mount::setup());
    must!(kmsg::kernlog_setup());
    must!(init.setup_syslog());
    must!(init.set_random_identity());
    must!(mount::readonly("/"));
    must!(init.process_kernel_params(None));
    debug!("init_or_sbin_init: {:?}", init::Invocation::from_argv0());
    must!(init.query_cpu_vendor());
    #[cfg(feature = "confidential")]
    must!(init.query_cpu_cc_mode());
    must!(init.get_nvidia_devices(None));
    init.hot_or_cold_plug.get(&init.cold_plug).unwrap()(&mut init);
}

impl NVRC {
    fn manage_daemons(&mut self, action: Action) -> Result<()> {
        for f in [
            NVRC::nvidia_persistenced,
            NVRC::nv_hostengine,
            NVRC::dcgm_exporter,
        ] {
            f(self, action.clone())?;
        }
        Ok(())
    }

    fn cold_plug(&mut self) {
        debug!("cold-plug mode");
        self.setup_gpu();
        match unsafe { fork() }.expect("fork cold-plug") {
            ForkResult::Parent { .. } => {
                kata_agent().expect("kata-agent cold-plug parent");
            }
            ForkResult::Child => loop {
                sleep(Duration::from_secs(1));
                if let Err(e) = self.poll_syslog() {
                    error!("poll syslog: {e}");
                    break;
                }
            },
        }
    }

    fn hot_plug(&mut self) {
        debug!("hot-plug mode");
        match unsafe { fork() }.expect("fork hot-plug") {
            ForkResult::Parent { .. } => {
                kata_agent().expect("kata-agent hot-plug parent");
            }
            ForkResult::Child => {
                self.handle_hot_plug_events().expect("hot-plug events");
            }
        }
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

    fn setup_gpu(&mut self) {
        must!(self.check_gpu_supported(None));
        #[cfg(feature = "confidential")]
        must!(self.query_gpu_cc_mode());
        must!(self.check_gpu_supported(None));
        must!(nvidia_ctk_system());
        must!(self.manage_daemons(Action::Restart));
        must!(lockdown::disable_modules_loading());
        must!(nvidia_ctk_cdi());
        #[cfg(feature = "confidential")]
        must!(self.nvidia_smi_srs());
    }
}
