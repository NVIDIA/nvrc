use anyhow::{Context, Result};
use std::fs::File;
use std::ptr;

// For mmap functionality
use nix::sys::mman::{mmap, munmap, MapFlags, ProtFlags};

use super::NVRC;

// NVIDIA GPU Register definitions
const NV_PGC6_AON_SECURE_SCRATCH_GROUP_20: u64 = 0x001182cc; /* RW-4R */

// CC Mode register values and their corresponding mode strings
const CC_MODE_LOOKUP: &[(u32, &str)] = &[(0x5, "on"), (0x3, "devtools"), (0x0, "off")];

impl NVRC {
    /// Query CC mode by reading BAR0 memory mapped register
    fn query_cc_mode_bar0(&self, bdf: &str) -> Result<String> {
        let resource_path = format!("/sys/bus/pci/devices/{bdf}/resource0");
        debug!("Reading BAR0 resource for BDF {}: {}", bdf, resource_path);

        let file = File::open(&resource_path)
            .with_context(|| format!("Failed to open BAR0 resource file for BDF: {bdf}"))?;

        // Get page size for mmap alignment
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;

        // Calculate aligned offset and the offset within the page
        let aligned_offset = (NV_PGC6_AON_SECURE_SCRATCH_GROUP_20 as usize / page_size) * page_size;
        let offset_in_page = NV_PGC6_AON_SECURE_SCRATCH_GROUP_20 as usize - aligned_offset;

        // Map a page starting from aligned offset
        let map_size = page_size;
        let mapped_ptr = unsafe {
            mmap(
                None,
                std::num::NonZeroUsize::new(map_size).unwrap(),
                ProtFlags::PROT_READ,
                MapFlags::MAP_SHARED,
                &file,
                aligned_offset as i64,
            )
            .with_context(|| format!("Failed to mmap BAR0 resource for BDF: {bdf}"))?
        };

        let result = unsafe {
            // Calculate the actual register address within the mapped region
            let reg_ptr = mapped_ptr
                .as_ptr()
                .cast::<u8>()
                .add(offset_in_page)
                .cast::<u32>();

            // Read the 32-bit register value
            let reg_value = ptr::read_volatile(reg_ptr);

            debug!(
                "Register value at 0x{:x}: 0x{:x}",
                NV_PGC6_AON_SECURE_SCRATCH_GROUP_20, reg_value
            );

            // Determine CC mode based on register value using lookup table
            let mode = CC_MODE_LOOKUP
                .iter()
                .find(|(value, _)| *value == reg_value)
                .map(|(_, mode)| *mode)
                .unwrap_or_else(|| {
                    debug!(
                        "CC mode for BDF {} (via BAR0): unknown value 0x{:x}, assuming off",
                        bdf, reg_value
                    );
                    "off"
                });

            debug!(
                "CC mode for BDF {} (via BAR0): {} (0x{:x})",
                bdf, mode, reg_value
            );

            mode.to_string()
        };

        // Unmap the memory
        unsafe {
            munmap(mapped_ptr, map_size)
                .with_context(|| format!("Failed to unmap BAR0 resource for BDF: {bdf}"))?;
        }

        Ok(result)
    }

    pub fn query_gpu_cc_mode(&mut self) -> Result<()> {
        let mut mode: Option<String> = None;

        if self.gpu_bdfs.is_empty() {
            debug!("No GPUs found, skipping CC mode query");
            return Ok(());
        }

        for bdf in &self.gpu_bdfs {
            // Query CC mode directly via BAR0
            let current_mode = self
                .query_cc_mode_bar0(bdf)
                .with_context(|| format!("Failed to query CC mode via BAR0 for BDF: {bdf}"))?;

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
        self.gpu_cc_mode = mode;

        Ok(())
    }
}
