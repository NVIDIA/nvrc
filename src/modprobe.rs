use crate::execute::foreground;

const MODPROBE: &str = "/sbin/modprobe";

/// Load a kernel module via modprobe.
/// Used to load GPU drivers (nvidia, nvidia-uvm) during init.
pub fn load(module: &str) {
    foreground(MODPROBE, &[module]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use serial_test::serial;
    use std::panic;

    // Kernel module loading must be serialized - parallel modprobe
    // calls can race and cause spurious failures.

    #[test]
    #[serial]
    fn test_load_loop() {
        require_root();
        // 'loop' module is almost always available (loop devices)
        load("loop");
    }

    #[test]
    #[serial]
    fn test_load_nonexistent() {
        require_root();
        let result = panic::catch_unwind(|| {
            load("nonexistent_module_xyz123");
        });
        assert!(result.is_err());
    }
}
