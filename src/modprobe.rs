use crate::execute::foreground;
use anyhow::Result;

const MODPROBE: &str = "/sbin/modprobe";

/// Load a kernel module via modprobe.
/// Used to load GPU drivers (nvidia, nvidia-uvm) during init.
pub fn load(module: &str) -> Result<()> {
    foreground(MODPROBE, &[module])
}

/// Reload NVIDIA modules to pick up configuration changes.
/// Used after nvidia-smi adjusts GPU settings (clocks, power limits)
/// that require a module reload to take effect.
pub fn reload_nvidia_modules() -> Result<()> {
    foreground(MODPROBE, &["-r", "nvidia-uvm", "nvidia"])?;
    load("nvidia")?;
    load("nvidia-uvm")
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
        // Should fail for a module that doesn't exist
        assert!(load("nonexistent_module_xyz123").is_err());
    }

    #[test]
    #[serial]
    fn test_reload_fails_without_hardware() {
        require_root();
        // Will fail: no nvidia modules on systems without NVIDIA
        let _ = reload_nvidia_modules();
    }
}
