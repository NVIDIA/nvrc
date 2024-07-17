use nix::sys::reboot::{reboot, RebootMode};
use nix::unistd::{fork, ForkResult};

use std::os::unix::process::CommandExt;
use std::process::Command;
use std::panic;
use std::collections::HashMap;

mod check_supported;
mod container_toolkit;
mod cpu_vendor;
mod daemons;
mod get_devices;
mod mount;
mod ndev;
mod proc_cmdline;
mod query_cc_mode;

#[macro_use]
extern crate log;
extern crate kernlog;

use check_supported::check_gpu_supported;
use container_toolkit::{nvidia_ctk_cdi, nvidia_ctk_system};
use cpu_vendor::query_cpu_vendor;
use daemons::nvidia_persistenced;
use get_devices::get_gpu_devices;
use mount::mount_setup;
use ndev::udev;
use proc_cmdline::{process_kernel_params, NVRC};
use query_cc_mode::query_gpu_cc_mode;

fn main() {

    panic::set_hook(Box::new(|panic_info| {
        error!("{}", panic_info);
        reboot(RebootMode::RB_POWER_OFF).unwrap();
    }));

    let mut hot_or_cold_plug: HashMap<bool, fn(&mut NVRC)> = HashMap::new();
    hot_or_cold_plug.insert(true, cold_plug);
    hot_or_cold_plug.insert(false, hot_plug);

    let mut context = NVRC::default();

    mount_setup();
    kernlog::init().unwrap();
    process_kernel_params(&mut context, None).unwrap();
    query_cpu_vendor(&mut context).unwrap();

    get_gpu_devices(&mut context, None).unwrap();

    // At this this point we either have GPUs (cold-plug) or we do not have
    // any GPUs (hot-plug) depending on the mode of operation execute cold|hot-plug
    hot_or_cold_plug.get(&context.cold_plug).unwrap()(&mut context);

}

fn cold_plug(context: &mut NVRC) {
    debug!("cold-plug mode detected, starting GPU setup");
    setup_gpu(context);
    Command::new("/sbin/init").exec();
}

fn hot_plug(context: &mut NVRC) {
    debug!("hot-plug mode detected, starting udev and GPU setup");
    match unsafe { fork() } {
        Ok(ForkResult::Parent { child: _ }) => {
            Command::new("/sbin/init").exec();
        }
        Ok(ForkResult::Child) => {
            loop {
                udev();
                get_gpu_devices(context, None).unwrap();
                setup_gpu(context);

            }
        }
        Err(e) => {
            panic!("Fork failed: {}", e);
        }
    }

}

fn setup_gpu(context: &mut NVRC) {
    query_gpu_cc_mode(context).unwrap();
    check_gpu_supported(context, None).unwrap();

    nvidia_ctk_system().unwrap();
    // Once we have loaded the driver we can start persistenced
    // CDI will not pick up the daemon if it is not created
    nvidia_persistenced(&context).unwrap();
    // Create the CDI spec for the GPUs including persistenced
    nvidia_ctk_cdi().unwrap();
}