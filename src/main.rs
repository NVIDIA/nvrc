// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

mod attach;
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
mod supported;
mod syslog;
mod toolkit;
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

use daemon::Action;
use kata_agent::kata_agent;
use nvrc::NVRC;
use toolkit::{nvidia_ctk_cdi, nvidia_ctk_system};

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
    let handler = init
        .hot_or_cold_plug
        .get(&init.cold_plug)
        .expect("hot_or_cold_plug handler not found");
    must!(handler(&mut init));
}

impl NVRC {
    fn setup_gpu(&mut self) {
        must!(self.check_gpu_supported(None));
        #[cfg(feature = "confidential")]
        must!(self.query_gpu_cc_mode());
        must!(nvidia_ctk_system());
        must!(self.manage_daemons(Action::Restart));
        must!(lockdown::disable_modules_loading());
        must!(nvidia_ctk_cdi());
        #[cfg(feature = "confidential")]
        must!(self.nvidia_smi_srs());
    }
}
