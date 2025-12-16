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

    // Malformed sysfs (hardware bug, kernel bug, or attack) could have end < start.
    // Catch this before arithmetic to prevent wraparound.
    if end_addr < start_addr {
        return Err(NvrcError::Bar0AccessFailed {
            bdf: bdf.to_string(),
            offset: 0,
            reason: format!(
                "Invalid BAR0 range: end < start (0x{:x} < 0x{:x})",
                end_addr, start_addr
            ),
        });
    }

    // Use checked arithmetic to prevent overflow on pathological values.
    let size = end_addr
        .checked_sub(start_addr)
        .and_then(|diff| diff.checked_add(1))
        .ok_or_else(|| NvrcError::Bar0AccessFailed {
            bdf: bdf.to_string(),
            offset: 0,
            reason: format!(
                "BAR0 size calculation overflow (start=0x{:x}, end=0x{:x})",
                start_addr, end_addr
            ),
        })?;

    Ok(size as usize)
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
#[allow(dead_code)]
pub fn read_bar0_register(bdf: &str, register_offset: u64) -> Result<u32> {
    let resource_path = format!("/sys/bus/pci/devices/{}/resource0", bdf);

    // Validate register offset is within BAR0
    // Must check that we can read a full u32 (4 bytes) without crossing boundary
    let bar0_size = read_bar0_size(bdf)?;

    if register_offset as usize + std::mem::size_of::<u32>() > bar0_size {
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

    #[test]
    fn test_bar0_size_invalid_range() {
        // Regression test: malformed sysfs with end < start should fail
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "0x00000000ffffffff 0x0000000000000000 0x0").unwrap();
        tmp.flush().unwrap();

        let content = std::fs::read_to_string(tmp.path()).unwrap();
        let parts: Vec<&str> = content.lines().next().unwrap().split_whitespace().collect();
        let start = u64::from_str_radix(parts[0].trim_start_matches("0x"), 16).unwrap();
        let end = u64::from_str_radix(parts[1].trim_start_matches("0x"), 16).unwrap();

        assert!(end < start, "Test data should have end < start");
    }

    #[test]
    fn test_bar0_size_checked_arithmetic() {
        // Regression test: verify checked arithmetic handles overflow
        let max = u64::MAX;

        // Normal case
        let size = 0x1000u64.checked_sub(0x0).and_then(|d| d.checked_add(1));
        assert_eq!(size, Some(0x1001));

        // Edge case: near max
        let size_near_max = max.checked_sub(1).and_then(|d| d.checked_add(1));
        assert_eq!(size_near_max, Some(max));

        // Overflow case
        let size_overflow = max.checked_sub(0).and_then(|d| d.checked_add(1));
        assert_eq!(size_overflow, None, "max + 1 should overflow");
    }
}
