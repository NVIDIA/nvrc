use nix::unistd::{fork, ForkResult};
use std::panic;
use std::sync::mpsc;
use std::thread::sleep;
use std::time::Duration;

mod check_supported;
mod container_toolkit;
mod coreutils;
mod cpu_vendor;
mod daemon;
mod get_devices;
mod init;
mod kata_agent;
mod kmsg;
mod lockdown;
mod mount;
mod ndev;
mod nvrc;
mod pci_ids;
mod query_cc_mode;
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

fn main() {
    lockdown::set_panic_hook();

    let mut init = NVRC::default();

    mount::setup().expect("Failed to setup mounts");

    kmsg::kernlog_setup();
    init.setup_syslog().expect("Failed to setup syslog");
    // Now that we have the rand devices let's create our random user,group
    // and afterwards set the rootfs readonly
    init.identity = user_group::random_user_group();
    mount::readonly("/").expect("Failed to set rootfs to readonly");

    init.process_kernel_params(None)
        .expect("Failed to process kernel parameters");

    let init_or_sbin_init = init::InitInvocation::from_argv0();
    debug!("init_or_sbin_init: {:?}", init_or_sbin_init);

    init.query_cpu_vendor().expect("Failed to query CPU vendor");
    init.get_nvidia_devices(None)
        .expect("Failed to get NVIDIA devices");
    // At this point we either have NVIDIA devices (cold-plug) or we do not have
    // any NVIDIA devices (hot-plug) depending on the mode of operation execute cold|hot-plug
    init.hot_or_cold_plug
        .get(&init.cold_plug)
        .expect("Failed to determine hot or cold plug mode")(&mut init);
}

impl NVRC {
    fn cold_plug(&mut self) {
        debug!("cold-plug mode detected, starting GPU setup");
        self.setup_gpu();
        //        kata_agent().expect("Failed to initialize Kata agent in cold-plug mode");
        match unsafe { fork() } {
            Ok(ForkResult::Parent { child: _ }) => {
                kata_agent().expect("Failed to initialize Kata agent in hot-plug parent process");
            }
            Ok(ForkResult::Child) => {
                loop {
                    // In cold-plug mode we do not expect any hot-unplug events
                    // so we can just wait for the Kata agent to finish
                    sleep(Duration::from_secs(1));
                    self.poll_syslog()
                        .expect("Failed to poll syslog in cold-plug child process");
                }
            }
            Err(e) => {
                panic!("fork failed: {e}");
            }
        }
    }

    fn hot_plug(&mut self) {
        debug!(
            "hot-plug mode detected, starting udev and GPU setup as child, kata-agent as parent"
        );
        match unsafe { fork() } {
            Ok(ForkResult::Parent { child: _ }) => {
                kata_agent().expect("Failed to initialize Kata agent in hot-plug parent process");
            }
            Ok(ForkResult::Child) => {
                let (tx_to_nvrc, rx_in_nvrc) = mpsc::channel::<&str>();
                {
                    let tx = tx_to_nvrc.clone();
                    ndev::udev(tx);
                }

                {
                    let tx = tx_to_nvrc.clone();
                    kmsg::watch_for_pattern("NVRM: Attempting to remove device", tx);
                }
                // Events are processed sequentially no need to mutex guard
                // the restart of the daemons
                for event in rx_in_nvrc {
                    debug!("received event: {}", event);
                    match event {
                        "hot-plug" => {
                            self.get_nvidia_devices(None)
                                .expect("Failed to get NVIDIA devices during hot-plug event");
                            self.setup_gpu();
                        }
                        "hot-unplug" => {
                            self.nvidia_persistenced(daemon::Action::Stop).expect(
                                "Failed to stop NVIDIA persistence daemon during hot-unplug",
                            );
                            self.nv_hostengine(Action::Stop)
                                .expect("Failed to stop NVIDIA host engine during hot-unplug");
                            self.dcgm_exporter(Action::Stop)
                                .expect("Failed to stop DCGM exporter during hot-unplug");

                            sleep(Duration::from_millis(3000));
                            self.get_nvidia_devices(None)
                                .expect("Failed to get NVIDIA devices after hot-unplug event");

                            // If we still have NVIDIA devices present restart the
                            // daemons e.g. one container is done but we have
                            // more
                            if !self.nvidia_devices.is_empty() {
                                self.nvidia_persistenced(Action::Start).expect(
                                    "Failed to start NVIDIA persistence daemon after hot-unplug",
                                );
                                self.nv_hostengine(Action::Start)
                                    .expect("Failed to start NVIDIA host engine after hot-unplug");
                                self.dcgm_exporter(Action::Start)
                                    .expect("Failed to start DCGM exporter after hot-unplug");
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                panic!("fork failed: {e}");
            }
        }
    }

    fn setup_gpu(&mut self) {
        self.check_gpu_supported(None)
            .expect("Failed to check if GPU is supported");

        #[cfg(feature = "confidential")]
        self.query_gpu_cc_mode()
            .expect("Failed to query GPU confidential computing mode");
        // Check if we have GPUs and if they are supported
        self.check_gpu_supported(None)
            .expect("Failed to check if GPU is supported");
        // If we're running in a confidential environment we may need to set
        // specific kernel module parameters. Check those first and then load
        // the modules.
        nvidia_ctk_system().expect("Failed to setup NVIDIA container toolkit system");
        // Once we have loaded the driver we can start persistenced
        // CDI will not pick up the daemon if it is not created

        self.nvidia_persistenced(Action::Restart)
            .expect("Failed to restart NVIDIA persistence daemon");
        // At this point we have all modules loaded lock down module loading
        lockdown::disable_modules_loading();
        // Create the CDI spec for the GPUs including persistenced
        nvidia_ctk_cdi().expect("Failed to generate NVIDIA CDI specification");
        // If user has enabled nvrc.dcgm=on in the kernel command line
        // we're starting the DCGM exporter
        self.nv_hostengine(Action::Restart)
            .expect("Failed to restart NVIDIA host engine");
        self.dcgm_exporter(Action::Restart)
            .expect("Failed to restart DCGM exporter");
        // If user has enabled nvidia_smi_srs in the kernel command line
        // we can optionally set the GPU to Ready
        self.nvidia_smi_srs()
            .expect("Failed to set GPU to ready state via nvidia-smi");
    }
}
