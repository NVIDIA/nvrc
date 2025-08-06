use anyhow::{Context, Result};
use nix::sys::reboot::{reboot, RebootMode};
use nix::unistd::sync;
use std::fs;
use std::panic;

const MODULES_DISABLED_PATH: &str = "/proc/sys/kernel/modules_disabled";
const DISABLE_MODULES_VALUE: &[u8] = b"1\n";

pub fn set_panic_hook() {
    panic::set_hook(Box::new(|panic_info| {
        log::error!("Critical panic occurred: {}", panic_info);
        sync();

        // Power off the system immediately
        // Note: reboot() should not return on success, but handle errors just in case
        let _ = reboot(RebootMode::RB_POWER_OFF).map_err(|e| {
            log::error!("Failed to power off after panic: {}", e);
        });
    }));
}

pub fn disable_modules_loading() -> Result<()> {
    fs::write(MODULES_DISABLED_PATH, DISABLE_MODULES_VALUE).with_context(|| {
        format!(
            "Failed to disable module loading via {}",
            MODULES_DISABLED_PATH
        )
    })
}
