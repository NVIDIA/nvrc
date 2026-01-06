// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! NVIDIA Container Toolkit (nvidia-ctk) integration.
//!
//! Generates CDI (Container Device Interface) specs so container runtimes
//! can discover and mount GPU devices without needing the legacy hook.

use crate::execute::foreground;
use anyhow::Result;

const NVIDIA_CTK: &str = "/bin/nvidia-ctk";

/// Run nvidia-ctk with given arguments.
fn ctk(args: &[&str]) -> Result<()> {
    foreground(NVIDIA_CTK, args)
}

/// Generate CDI spec for GPU device discovery.
/// CDI allows container runtimes (containerd, CRI-O) to inject GPU devices
/// without nvidia-docker. The spec is written to /var/run/cdi/nvidia.yaml
/// where runtimes expect to find it.
pub fn nvidia_ctk_cdi() -> Result<()> {
    ctk(&["-d", "cdi", "generate", "--output=/var/run/cdi/nvidia.yaml"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ctk_fails_without_binary() {
        let err = ctk(&["--version"]).unwrap_err();
        // Should fail because nvidia-ctk binary doesn't exist
        assert!(err.to_string().contains("nvidia-ctk"));
    }

    #[test]
    fn test_nvidia_ctk_cdi_fails_without_binary() {
        let err = nvidia_ctk_cdi().unwrap_err();
        // Should fail because nvidia-ctk binary doesn't exist
        assert!(err.to_string().contains("nvidia-ctk"));
    }
}
