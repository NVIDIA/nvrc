// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! NVIDIA Container Toolkit (nvidia-ctk) integration.
//!
//! Generates CDI (Container Device Interface) specs so container runtimes
//! can discover and mount GPU devices without needing the legacy hook.

use crate::execute::foreground;

const NVIDIA_CTK: &str = "/bin/nvidia-ctk";

/// Run nvidia-ctk with given arguments.
fn ctk(args: &[&str]) {
    foreground(NVIDIA_CTK, args);
}

/// Generate CDI spec for GPU device discovery.
/// CDI allows container runtimes (containerd, CRI-O) to inject GPU devices
/// without nvidia-docker. The spec is written to /var/run/cdi/nvidia.yaml
/// where runtimes expect to find it.
pub fn nvidia_ctk_cdi() {
    ctk(&["-d", "cdi", "generate", "--output=/var/run/cdi/nvidia.yaml"]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic;

    #[test]
    fn test_ctk_fails_without_binary() {
        let result = panic::catch_unwind(|| {
            ctk(&["--version"]);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_nvidia_ctk_cdi_fails_without_binary() {
        let result = panic::catch_unwind(|| {
            nvidia_ctk_cdi();
        });
        assert!(result.is_err());
    }
}
