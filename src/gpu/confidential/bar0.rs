// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! BAR0 register reading utilities for GPU CC detection.
//!
//! This module provides low-level functions for reading GPU registers
//! via memory-mapped BAR0 access.

use crate::core::error::{NvrcError, Result};
use anyhow::Context;
use nix::sys::mman::{mmap, munmap, MapFlags, ProtFlags};
use std::fs::{self, File};
use std::ptr;

/// Read BAR0 size from sysfs resource file
///
/// The resource file contains lines with format: `start_addr end_addr flags`
/// We read the first line (BAR0) and calculate the size.
#[allow(dead_code)] // Used by read_bar0_register
fn read_bar0_size(bdf: &str) -> Result<usize> {
    let resource_path = format!("/sys/bus/pci/devices/{}/resource", bdf);
    let content =
        fs::read_to_string(&resource_path).map_err(|e| NvrcError::FileOperationFailed {
            path: resource_path.clone().into(),
            source: e,
        })?;

    let first_line = content
        .lines()
        .next()
        .ok_or_else(|| NvrcError::Bar0AccessFailed {
            bdf: bdf.to_string(),
            offset: 0,
            reason: "Empty resource file".to_string(),
        })?;

    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(NvrcError::Bar0AccessFailed {
            bdf: bdf.to_string(),
            offset: 0,
            reason: "Invalid resource file format".to_string(),
        });
    }

    let start_addr = u64::from_str_radix(parts[0].trim_start_matches("0x"), 16)
        .context("Failed to parse start address")
        .map_err(|e| NvrcError::Bar0AccessFailed {
            bdf: bdf.to_string(),
            offset: 0,
            reason: e.to_string(),
        })?;

    let end_addr = u64::from_str_radix(parts[1].trim_start_matches("0x"), 16)
        .context("Failed to parse end address")
        .map_err(|e| NvrcError::Bar0AccessFailed {
            bdf: bdf.to_string(),
            offset: 0,
            reason: e.to_string(),
        })?;

    Ok((end_addr - start_addr + 1) as usize)
}

/// Read a 32-bit register from GPU BAR0
///
/// This function performs memory-mapped I/O to read a hardware register
/// from the GPU's BAR0 region.
///
/// # Arguments
///
/// * `bdf` - Bus:Device.Function identifier (e.g., "0000:01:00.0")
/// * `register_offset` - Offset of the register within BAR0
///
/// # Returns
///
/// The 32-bit value read from the register
///
/// # Errors
///
/// Returns an error if:
/// - Register offset exceeds BAR0 size
/// - Cannot open resource0 file
/// - Memory mapping fails
///
/// # Safety
///
/// This function uses `unsafe` for:
/// - Memory mapping via `mmap()`
/// - Volatile register reads
/// - Memory unmapping
#[allow(dead_code)] // Will be used in existing code migration
pub fn read_bar0_register(bdf: &str, register_offset: u64) -> Result<u32> {
    let resource_path = format!("/sys/bus/pci/devices/{}/resource0", bdf);

    // Validate register offset is within BAR0
    let bar0_size = read_bar0_size(bdf)?;

    if register_offset as usize >= bar0_size {
        return Err(NvrcError::RegisterOutOfBounds {
            bdf: bdf.to_string(),
            offset: register_offset,
            size: bar0_size,
        });
    }

    // Open BAR0 resource
    let file = File::open(&resource_path).map_err(|e| NvrcError::Bar0AccessFailed {
        bdf: bdf.to_string(),
        offset: register_offset,
        reason: format!("Failed to open resource0: {}", e),
    })?;

    // Calculate page-aligned mapping
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize };
    let page_offset = (register_offset as usize / page_size) * page_size;
    let offset_in_page = register_offset as usize - page_offset;

    // Map one page
    let map = unsafe {
        mmap(
            None,
            std::num::NonZeroUsize::new(page_size).unwrap(),
            ProtFlags::PROT_READ,
            MapFlags::MAP_SHARED,
            &file,
            page_offset as i64,
        )
        .map_err(|e| NvrcError::Bar0AccessFailed {
            bdf: bdf.to_string(),
            offset: register_offset,
            reason: format!("mmap failed: {}", e),
        })?
    };

    // Read register value
    let value = unsafe {
        let reg_ptr = map.as_ptr().cast::<u8>().add(offset_in_page).cast::<u32>();
        ptr::read_volatile(reg_ptr)
    };

    // Unmap
    unsafe {
        munmap(map, page_size).map_err(|e| NvrcError::Bar0AccessFailed {
            bdf: bdf.to_string(),
            offset: register_offset,
            reason: format!("munmap failed: {}", e),
        })?;
    }

    debug!(
        "Read BAR0 register for {}: offset=0x{:x}, value=0x{:x}",
        bdf, register_offset, value
    );

    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_bar0_size_nonexistent() {
        let result = read_bar0_size("9999:99:99.9");
        assert!(result.is_err());
    }

    // Note: Real BAR0 tests require actual GPU hardware
    // and root privileges, so we only test error paths
}
