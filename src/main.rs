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
mod init;
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

    // System-level setup (before NVRC initialization)
    must!(mount::setup());
    must!(kmsg::kernlog_setup());

    // Parse kernel configuration (immutable, idiomatic)
    let kernel_params = must_value!(KernelParams::from_cmdline(None));

    // Apply log configuration early (affects global state)
    must!(kernel_params.apply_log_config());

    // Build NVRC with configuration (no mutation after build!)
    let mut init = must_build!(NVRCBuilder::new()
        .with_auto_cc_provider()
        .map(|b| b.with_kernel_params(kernel_params)));

    // Hardware detection
    must!(mount::readonly("/"));
    debug!("init_or_sbin_init: {:?}", init::Invocation::from_argv0());
    must!(init.query_cpu_vendor());
    must!(init.get_nvidia_devices(None));

    // Log platform information
    info!(
        "Platform: {}",
        init.cc_provider.platform().platform_description()
    );

    // Execute handler based on plug mode
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
        #[cfg(feature = "confidential")]
        must!(self.nvidia_smi_srs());
    }
}
