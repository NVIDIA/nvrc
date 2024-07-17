use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use super::NVRC;

pub fn check_gpu_supported(context: &mut NVRC, supported: Option<&Path>) -> Result<()> {

    if context.gpu_devids.is_empty() {
        debug!("No GPUs found, skipping GPU supported check");
        return Ok(())
    }


    let supported = match supported {
        Some(supported) => supported,
        None => Path::new("/supported-gpu.devids"),
    };

    if !supported.exists() {
        return Err(anyhow::anyhow!(
            "{} file not found, cannot verify GPU support",
            supported.display()
        ))
    }

    let file = File::open(supported).context(format!("Failed to open {:?}", supported))?;
    let reader = BufReader::new(file);

    let supported_ids: Vec<String> = reader
        .lines()
        .map(|line| line.expect("Could not read line"))
        .map(|line| line.to_lowercase())
        .collect();

    for devid in context.gpu_devids.iter() {
        let devid_lowercase = devid.to_lowercase();
        if !supported_ids.contains(&devid_lowercase) {
            return Err(anyhow::anyhow!("GPU {} is not supported", devid));
        }
    }

    context.gpu_supported = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_check_gpu_supported() {
        let mut context = NVRC::default();
        context.gpu_devids = vec!["0x2330".to_string()];
        check_gpu_supported(&mut context, None).unwrap();
        assert_eq!(context.gpu_supported, true);
    }

}
