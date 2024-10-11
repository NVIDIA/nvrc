use anyhow::{Context, Result};
use std::process::Command;

use super::NVRC;

impl NVRC {
    pub fn query_gpu_cc_mode(&mut self) -> Result<()> {
        let mut mode: Option<String> = None;

        if self.gpu_bdfs.is_empty() {
            debug!("No GPUs found, skipping CC mode query");
            return Ok(());
        }

        for bdf in &self.gpu_bdfs {
            let output = Command::new("/sbin/nvidia_gpu_tools")
                .args([
                    "--mmio-access-type=sysfs",
                    "--query-cc-mode",
                    "--gpu-bdf",
                    bdf,
                ])
                .output()
                .with_context(|| format!("Failed to execute nvidia_gpu_tools for BDF: {}", bdf))?;

            let combined_output = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );

            debug!("{}", combined_output);

            let current_mode = if combined_output.contains("CC mode is on") {
                "on".to_string()
            } else {
                "off".to_string()
            };

            match &mode {
                Some(m) if m != &current_mode => {
                    return Err(anyhow::anyhow!(
                        "Inconsistent CC mode detected: {} has mode '{}', expected '{}'",
                        bdf,
                        current_mode,
                        m
                    ));
                }
                _ => mode = Some(current_mode),
            }
        }
        debug!("CC mode is: {}", mode.as_ref().unwrap());
        self.gpu_cc_mode = mode;

        Ok(())
    }
}
