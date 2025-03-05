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
mod query_cc_mode;
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

    mount::setup();

    kmsg::kernlog_setup();
    // Now that we have the rand devices let's create our random user,group
    // and afterwards set the rootfs readonly
    init.identity = user_group::random_user_group();
    mount::readonly("/");

    init.process_kernel_params(None).unwrap();

    let init_or_sbin_init = init::InitInvocation::from_argv0();
    debug!("init_or_sbin_init: {:?}", init_or_sbin_init);

    init.query_cpu_vendor().unwrap();
    init.get_gpu_devices(None).unwrap();
    // At this this point we either have GPUs (cold-plug) or we do not have
    // any GPUs (hot-plug) depending on the mode of operation execute cold|hot-plug
    init.hot_or_cold_plug.get(&init.cold_plug).unwrap()(&mut init);
}

impl NVRC {
    fn cold_plug(&mut self) {
        debug!("cold-plug mode detected, starting GPU setup");
        self.setup_gpu();
        kata_agent().unwrap();
    }

    fn hot_plug(&mut self) {
        debug!(
            "hot-plug mode detected, starting udev and GPU setup as child, kata-agent as parent"
        );
        match unsafe { fork() } {
            Ok(ForkResult::Parent { child: _ }) => {
                kata_agent().unwrap();
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
                            self.get_gpu_devices(None).unwrap();
                            self.setup_gpu();
                        }
                        "hot-unplug" => {
                            self.nvidia_persistenced(daemon::Action::Stop).unwrap();
                            self.nv_hostengine(Action::Stop).unwrap();
                            self.dcgm_exporter(Action::Stop).unwrap();

                            sleep(Duration::from_millis(3000));
                            self.get_gpu_devices(None).unwrap();

                            // If we still have GPU devices present restart the
                            // daemons e.g. one container is done but we have
                            // more
                            if !self.gpu_bdfs.is_empty() {
                                self.nvidia_persistenced(Action::Start).unwrap();
                                self.nv_hostengine(Action::Start).unwrap();
                                self.dcgm_exporter(Action::Start).unwrap();
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                panic!("fork failed: {}", e);
            }
        }
    }

    fn setup_gpu(&mut self) {
        self.query_gpu_cc_mode().unwrap();
        self.check_gpu_supported(None).unwrap();
        // If we're running in a confidential environment we may need to set
        // specific kernel module parameters. Check those first and then load
        // the modules.
        nvidia_ctk_system().unwrap();
        // Once we have loaded the driver we can start persistenced
        // CDI will not pick up the daemon if it is not created
        self.nvidia_persistenced(Action::Restart).unwrap();
        // At this point we have all modules loaded lock down module loading
        lockdown::disable_modules_loading();
        // Create the CDI spec for the GPUs including persistenced
        nvidia_ctk_cdi().unwrap();
        // If user has enabled nvrc.dcgm=on in the kernel command line
        // we're starting the DCGM exporter
        self.nv_hostengine(Action::Restart).unwrap();
        self.dcgm_exporter(Action::Restart).unwrap();
        // If user has enabled nvidia_smi_srs in the kernel command line
        // we can optionally set the GPU to Ready
        self.nvidia_smi_srs().unwrap();
    }
}
