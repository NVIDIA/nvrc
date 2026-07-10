// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! NVIDIA Container Toolkit (nvidia-ctk) integration.
//!
//! Generates CDI (Container Device Interface) specs so container runtimes
//! can discover and mount GPU devices without needing the legacy hook.

use crate::execute::foreground;
use crate::gpu_extension;

const NVIDIA_CTK: &str = "/bin/nvidia-ctk";

/// Run nvidia-ctk with given arguments.
fn ctk(args: &[&str]) {
    foreground(&gpu_extension::path(NVIDIA_CTK), args);
}

/// Generate the CDI spec (`/var/run/cdi/nvidia.yaml`) so runtimes can inject GPU
/// devices without the legacy hook. With composable images the extension-derived
/// flags point nvidia-ctk at `/run/kata-extensions/gpu`; see the [`gpu_extension`] helpers
/// for why each is needed (all no-ops for the monolithic image).
pub fn nvidia_ctk_cdi() {
    let args = cdi_args(gpu_extension::driver_root(), gpu_extension::cdi_hook_path());
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    ctk(&arg_refs);
}

/// Assemble the `nvidia-ctk cdi generate` arguments from the (possibly empty)
/// extension-derived paths.
fn cdi_args(driver_root: Option<String>, cdi_hook_path: Option<String>) -> Vec<String> {
    let mut args = vec![
        "-d".to_owned(),
        "cdi".to_owned(),
        "generate".to_owned(),
        "--output=/var/run/cdi/nvidia.yaml".to_owned(),
    ];
    if let Some(root) = driver_root {
        args.push(format!("--driver-root={root}"));
        args.push(format!("--dev-root={}", gpu_extension::DEV_ROOT));
    }
    if let Some(path) = cdi_hook_path {
        args.push(format!("--nvidia-cdi-hook-path={path}"));
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic;

    #[test]
    #[cfg_attr(miri, ignore = "spawns a process, which miri cannot emulate")]
    fn test_ctk_fails_without_binary() {
        let result = panic::catch_unwind(|| {
            ctk(&["--version"]);
        });
        assert!(result.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore = "spawns a process, which miri cannot emulate")]
    fn test_nvidia_ctk_cdi_fails_without_binary() {
        let result = panic::catch_unwind(|| {
            nvidia_ctk_cdi();
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_cdi_args_monolithic() {
        // No extension: only the base generate args, no extension-specific flags.
        assert_eq!(
            cdi_args(None, None),
            vec!["-d", "cdi", "generate", "--output=/var/run/cdi/nvidia.yaml",]
        );
    }

    #[test]
    fn test_cdi_args_with_extension() {
        let root = gpu_extension::ROOT;
        let args = cdi_args(
            Some(root.to_owned()),
            Some(format!("{root}/bin/nvidia-cdi-hook")),
        );
        assert_eq!(
            args,
            vec![
                "-d".to_owned(),
                "cdi".to_owned(),
                "generate".to_owned(),
                "--output=/var/run/cdi/nvidia.yaml".to_owned(),
                format!("--driver-root={root}"),
                "--dev-root=/".to_owned(),
                format!("--nvidia-cdi-hook-path={root}/bin/nvidia-cdi-hook"),
            ]
        );
    }
}
