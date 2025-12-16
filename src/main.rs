// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

mod attach;
mod config;
mod core;
mod coreutils;
mod cpu;
mod daemon;
mod devices;
mod gpu;
mod kata_agent;
mod kmsg;
mod lockdown;
mod mount;
mod ndev;
mod nvrc;
mod pci_ids;
mod platform;
mod providers;
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

macro_rules! must_build {
    ($expr:expr) => {
        match $expr {
            Ok(builder) => match builder.build() {
                Ok(nvrc) => nvrc,
                Err(e) => panic!("build failure: {e}"),
            },
            Err(e) => panic!("provider setup failure: {e}"),
        }
    };
}

macro_rules! must_value {
    ($expr:expr) => {
        match $expr {
            Ok(val) => val,
            Err(e) => panic!("value creation failure: {e}"),
        }
    };
}

use config::KernelParams;
use core::builder::NVRCBuilder;
use core::PlugMode;
use daemon::Action;
use nvrc::NVRC;
use toolkit::{nvidia_ctk_cdi, nvidia_ctk_system};

fn main() {
    lockdown::set_panic_hook();

    must!(mount::setup());
    must!(kmsg::kernlog_setup());

    let kernel_params = must_value!(KernelParams::from_cmdline(None));

    // Apply early because it affects global log state
    must!(kernel_params.apply_log_config());

    NVRC::print_version_banner();

    let mut init = must_build!(NVRCBuilder::new()
        .with_auto_cc_provider()
        .map(|b| b.with_kernel_params(kernel_params)));

    must!(mount::readonly("/"));
    must!(init.query_cpu_vendor());
    must!(init.get_nvidia_devices(None));

    info!(
        "Platform: {}",
        init.cc_provider.platform().platform_description()
    );

    // Enforces cold-plug for confidential builds (security requirement)
    init.plug_mode.validate();

    match init.plug_mode {
        PlugMode::Cold => must!(init.cold_plug()),
        PlugMode::Hot => must!(init.hot_plug()),
    }
}

impl NVRC {
    fn setup_gpu(&mut self) {
        must!(self.check_gpu_supported(None));
        must!(nvidia_ctk_system());
        must!(self.manage_daemons(Action::Restart));
        must!(lockdown::disable_modules_loading());
        must!(nvidia_ctk_cdi());
        must!(self.nvidia_smi_srs());
    }
}
