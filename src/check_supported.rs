use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use super::NVRC;

impl NVRC {
    pub fn check_gpu_supported(&mut self, supported: Option<&Path>) -> Result<()> {
        if self.gpu_devids.is_empty() {
            debug!("No GPUs found, skipping GPU supported check");
            return Ok(());
        }

        let supported = match supported {
            Some(supported) => supported,
            None => Path::new("/supported-gpu.devids"),
        };

        if !supported.exists() {
            return Err(anyhow::anyhow!(
                "{} file not found, cannot verify GPU support",
                supported.display()
            ));
        }

        let file = File::open(supported).context(format!("Failed to open {:?}", supported))?;
        let reader = BufReader::new(file);

        let supported_ids: Vec<String> = reader
            .lines()
            .map(|line| line.expect("Could not read line"))
            .map(|line| line.to_lowercase())
            .collect();

        for devid in self.gpu_devids.iter() {
            let devid_lowercase = devid.to_lowercase();
            if !supported_ids.contains(&devid_lowercase) {
                self.gpu_supported = false;
                return Err(anyhow::anyhow!("GPU {} is not supported", devid));
            }
        }

        self.gpu_supported = true;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    #[test]
    fn test_check_gpu_supported() {
        let suppported_dir = tempdir().unwrap();
        // create temporary file in /tmp and populate it with 0x2330
        let supported = suppported_dir.path().join("supported-gpu.devids");
        let mut file = File::create(&supported).unwrap();
        file.write_all(b"0x2330\n").unwrap();

        let mut init = NVRC::default();
        init.gpu_devids = vec!["0x2330".to_string()];
        init.check_gpu_supported(Some(&supported.as_path()))
            .unwrap();
        assert_eq!(init.gpu_supported, true);

        let not_supported_dir = tempdir().unwrap();
        let not_supported = not_supported_dir.path().join("supported-gpu.devids");
        let mut file = File::create(&not_supported).unwrap();
        file.write_all(b"0x2331\n").unwrap();

        match init.check_gpu_supported(Some(&not_supported.as_path())) {
            Ok(_) => panic!("Expected an error"),
            _ => assert_ne!(init.gpu_supported, true),
        }
    }
}
