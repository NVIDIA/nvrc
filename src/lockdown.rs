use crate::coreutils::{fs_write, Result};
use sc::syscall;
use std::panic;

// From <sys/reboot.h>
const LINUX_REBOOT_MAGIC1: usize = 0xfee1dead;
const LINUX_REBOOT_MAGIC2: usize = 672274793;
const LINUX_REBOOT_CMD_POWER_OFF: usize = 0x4321fedc;

pub fn set_panic_hook() {
    panic::set_hook(Box::new(|panic_info| {
        log::error!("panic: {}", panic_info);

        // Sync filesystems before rebooting.
        unsafe {
            syscall!(SYNC);
        }

        // Power off the system.
        unsafe {
            syscall!(
                REBOOT,
                LINUX_REBOOT_MAGIC1,
                LINUX_REBOOT_MAGIC2,
                LINUX_REBOOT_CMD_POWER_OFF,
                0
            );
        }
    }));
}


pub fn disable_modules_loading() -> Result<()> {
    fs_write("/proc/sys/kernel/modules_disabled", b"1\n")
}
