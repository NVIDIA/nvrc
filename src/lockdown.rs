use nix::sys::reboot::{reboot, RebootMode};
use nix::unistd::sync;
use std::fs;
use std::panic;

// If something fails poweroff the VM we do not want to continue
pub fn set_panic_hook() {
    panic::set_hook(Box::new(|panic_info| {
        error!("{}", panic_info);
        sync();
        reboot(RebootMode::RB_POWER_OFF).unwrap();
    }));
}

pub fn disable_modules_loading() {
    // Disable loading of modules
    fs::write("/proc/sys/kernel/modules_disabled", b"1\n").unwrap();
}
