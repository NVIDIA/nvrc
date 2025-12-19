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
        // nvidia-ctk not installed on test system - exercises error path
        let result = ctk(&["--version"]);
        // Will fail: no nvidia-ctk binary
        assert!(result.is_err());
    }

    #[test]
    fn test_nvidia_ctk_cdi_fails_without_binary() {
        // Exercises the public function - fails without nvidia-ctk
        let result = nvidia_ctk_cdi();
        assert!(result.is_err());
    }
}
