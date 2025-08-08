use anyhow::{Context, Result};
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

//use cgroup::set_cgroup_subtree_control;
use container_toolkit::{nvidia_ctk_cdi, nvidia_ctk_system};
use daemon::Action;
use kata_agent::kata_agent;
use nvrc::NVRC;

const COLD_PLUG_POLL_INTERVAL: Duration = Duration::from_secs(1);
const HOT_UNPLUG_WAIT_DURATION: Duration = Duration::from_millis(3000);
const GPU_REMOVAL_PATTERN: &str = "NVRM: Attempting to remove device";

fn main() {
    lockdown::set_panic_hook();

    let mut init = NVRC::default();

    mount::setup().expect("Failed to setup mounts");
    kmsg::kernlog_setup().expect("Failed to setup kernel logging");
    init.setup_syslog().expect("Failed to setup syslog");

    init.identity = user_group::random_user_group();
    mount::readonly("/").expect("Failed to set rootfs to readonly");

    init.process_kernel_params(None)
        .expect("Failed to process kernel parameters");

    let init_or_sbin_init = init::InitInvocation::from_argv0();
    debug!("init_or_sbin_init: {:?}", init_or_sbin_init);

    init.query_cpu_vendor().expect("Failed to query CPU vendor");

    let cpu_vendor = init.cpu_vendor.as_ref().expect("CPU vendor not set");

    cpu::confidential::detect(cpu_vendor).expect("Failed to query confidential computing mode");

    init.get_nvidia_devices(None)
        .expect("Failed to get NVIDIA devices");

    let plug_mode_fn = init
        .hot_or_cold_plug
        .get(&init.cold_plug)
        .expect("Failed to determine hot or cold plug mode");

    plug_mode_fn(&mut init);
}

impl NVRC {
    /// Helper function to manage daemon states during hot-plug events
    fn manage_daemons(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Stop => {
                self.nvidia_persistenced(Action::Stop)
                    .context("Failed to stop NVIDIA persistence daemon")?;
                self.nv_hostengine(Action::Stop)
                    .context("Failed to stop NVIDIA host engine")?;
                self.dcgm_exporter(Action::Stop)
                    .context("Failed to stop DCGM exporter")?;
            }
            Action::Start => {
                self.nvidia_persistenced(Action::Start)
                    .context("Failed to start NVIDIA persistence daemon")?;
                self.nv_hostengine(Action::Start)
                    .context("Failed to start NVIDIA host engine")?;
                self.dcgm_exporter(Action::Start)
                    .context("Failed to start DCGM exporter")?;
            }
            Action::Restart => {
                self.nvidia_persistenced(Action::Restart)
                    .context("Failed to restart NVIDIA persistence daemon")?;
                self.nv_hostengine(Action::Restart)
                    .context("Failed to restart NVIDIA host engine")?;
                self.dcgm_exporter(Action::Restart)
                    .context("Failed to restart DCGM exporter")?;
            }
        }
        Ok(())
    }

    fn cold_plug(&mut self) {
        debug!("cold-plug mode detected, starting GPU setup");
        self.setup_gpu();

        match unsafe { fork() }.expect("Failed to fork in cold-plug mode") {
            ForkResult::Parent { child: _ } => {
                kata_agent().expect("Failed to initialize Kata agent in cold-plug parent");
            }
            ForkResult::Child => loop {
                sleep(COLD_PLUG_POLL_INTERVAL);
                if let Err(e) = self.poll_syslog() {
                    log::error!("Failed to poll syslog in cold-plug child: {}", e);
                    break;
                }
            },
        }
    }

    fn hot_plug(&mut self) {
        debug!(
            "hot-plug mode detected, starting udev and GPU setup as child, kata-agent as parent"
        );
        match unsafe { fork() }.expect("Failed to fork in hot-plug mode") {
            ForkResult::Parent { child: _ } => {
                kata_agent().expect("Failed to initialize Kata agent in hot-plug parent");
            }
            ForkResult::Child => {
                self.handle_hot_plug_events()
                    .expect("Hot-plug event handling failed");
            }
        }
    }

    /// Handles hot-plug events in the child process
    fn handle_hot_plug_events(&mut self) -> Result<()> {
        let (tx_to_nvrc, rx_in_nvrc) = mpsc::channel::<&str>();

        // Setup event watchers
        {
            let tx = tx_to_nvrc.clone();
            ndev::udev(tx);
        }

        {
            let tx = tx_to_nvrc.clone();
            kmsg::watch_for_pattern(GPU_REMOVAL_PATTERN, tx);
        }

        // Process events sequentially
        for event in rx_in_nvrc {
            debug!("received event: {}", event);
            match event {
                "hot-plug" => {
                    self.handle_hot_plug_event()
                        .expect("Failed to handle hot-plug event");
                }
                "hot-unplug" => {
                    self.handle_hot_unplug_event()
                        .expect("Failed to handle hot-unplug event");
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Handles a single hot-plug event
    fn handle_hot_plug_event(&mut self) -> Result<()> {
        self.get_nvidia_devices(None)
            .context("Failed to get NVIDIA devices during hot-plug")?;
        self.setup_gpu();
        Ok(())
    }

    /// Handles a single hot-unplug event
    fn handle_hot_unplug_event(&mut self) -> Result<()> {
        // Stop all daemons
        self.manage_daemons(Action::Stop)
            .context("Failed to stop daemons during hot-unplug")?;

        sleep(HOT_UNPLUG_WAIT_DURATION);

        self.get_nvidia_devices(None)
            .context("Failed to get NVIDIA devices after hot-unplug")?;

        // Restart daemons if devices still present
        if !self.nvidia_devices.is_empty() {
            self.manage_daemons(Action::Start)
                .context("Failed to restart daemons after hot-unplug")?;
        }

        Ok(())
    }

    fn setup_gpu(&mut self) {
        self.check_gpu_supported(None)
            .expect("Failed to check if GPU is supported");

        #[cfg(feature = "confidential")]
        self.query_gpu_cc_mode()
            .expect("Failed to query GPU confidential computing mode");

        self.check_gpu_supported(None)
            .expect("Failed to verify GPU support after CC mode check");

        nvidia_ctk_system().expect("Failed to setup NVIDIA container toolkit system");

        self.manage_daemons(Action::Restart)
            .expect("Failed to restart NVIDIA daemons");

        lockdown::disable_modules_loading().expect("Failed to disable module loading");

        nvidia_ctk_cdi().expect("Failed to generate NVIDIA CDI specification");

        // Set GPU to ready state if configured
        self.nvidia_smi_srs()
            .expect("Failed to set GPU to ready state via nvidia-smi");
    }
}
