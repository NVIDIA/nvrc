
use crate::coreutils::{foreground, Result};

fn ctk(args: &[&str]) -> Result<()> {
    let status = foreground("/bin/nvidia-ctk", args)?;
    if status == 0 {
        Ok(())
    } else {
        Err(crate::coreutils::CoreUtilsError::Syscall(status as isize))
    }
}

pub fn nvidia_ctk_system() -> Result<()> {
    ctk(&[
        "-d",
        "system",
        "create-device-nodes",
        "--control-devices",
        "--load-kernel-modules",
    ])
}

pub fn nvidia_ctk_cdi() -> Result<()> {
    ctk(&["-d", "cdi", "generate", "--output=/var/run/cdi/nvidia.yaml"])
}
