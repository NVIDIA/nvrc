use crate::execute::foreground;
use anyhow::Result;

const MODPROBE: &str = "/sbin/modprobe";

/// Load a kernel module via modprobe.
/// Used to load GPU drivers (nvidia, nvidia-uvm) during init.
pub fn load(module: &str) -> Result<()> {
    foreground(MODPROBE, &[module])
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use serial_test::serial;

    // Kernel module loading must be serialized - parallel modprobe
    // calls can race and cause spurious failures.

    #[test]
    #[serial]
    fn test_load_loop() {
        require_root();
        // 'loop' module is almost always available (loop devices)
        assert!(load("loop").is_ok());
    }

    #[test]
    #[serial]
    fn test_load_nonexistent() {
        require_root();
        let err = load("nonexistent_module_xyz123").unwrap_err();
        // modprobe exits non-zero for missing modules
        assert!(err.to_string().contains("modprobe"));
    }
}
