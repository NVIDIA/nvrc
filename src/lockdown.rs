use anyhow::{Context, Result};
use nix::sys::reboot::{reboot, RebootMode};
use nix::unistd::sync;
use std::fs;
use std::panic;

const PATH: &str = "/proc/sys/kernel/modules_disabled";
const VALUE: &[u8] = b"1\n";

pub fn set_panic_hook() {
    panic::set_hook(Box::new(|panic_info| {
        log::error!("Critical panic occurred: {}", panic_info);
        sync();
        let _ = reboot(RebootMode::RB_POWER_OFF);
    }));
}

pub fn disable_modules_loading() -> Result<()> {
    fs::write(PATH, VALUE).with_context(|| format!("Failed to disable module loading via {}", PATH))
}
