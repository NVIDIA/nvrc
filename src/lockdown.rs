use anyhow::{Context, Result};
use nix::sys::reboot::{reboot, RebootMode};
use nix::unistd::sync;
use std::fs;
use std::panic;

pub fn set_panic_hook() {
    panic::set_hook(Box::new(|panic_info| {
        log::error!("panic: {panic_info}");
        sync();
        let _ = reboot(RebootMode::RB_POWER_OFF);
    }));
}

pub fn disable_modules_loading() -> Result<()> {
    fs::write("/proc/sys/kernel/modules_disabled", b"1\n").context("disable module loading")
}
