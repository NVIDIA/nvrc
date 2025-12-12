// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Hot-plug and cold-plug mode definitions.

/// Plug mode for GPU device detection
///
/// Determines how GPU devices are handled:
/// - **Cold-plug**: GPUs present at boot time
/// - **Hot-plug**: GPUs can be added/removed at runtime
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlugMode {
    /// Cold-plug mode: GPUs present at boot
    Cold,
    /// Hot-plug mode: GPUs can be added/removed dynamically
    #[default]
    Hot,
}

impl PlugMode {
    /// Determine plug mode based on whether devices were detected
    pub fn from_devices_present(devices_present: bool) -> Self {
        if devices_present {
            Self::Cold
        } else {
            Self::Hot
        }
    }

    /// Check if this is cold-plug mode
    #[allow(dead_code)] // May be used in future
    pub const fn is_cold(self) -> bool {
        matches!(self, Self::Cold)
    }

    /// Check if this is hot-plug mode
    #[allow(dead_code)] // May be used in future
    pub const fn is_hot(self) -> bool {
        matches!(self, Self::Hot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plug_mode_from_devices() {
        assert_eq!(PlugMode::from_devices_present(true), PlugMode::Cold);
        assert_eq!(PlugMode::from_devices_present(false), PlugMode::Hot);
    }

    #[test]
    fn test_plug_mode_checks() {
        assert!(PlugMode::Cold.is_cold());
        assert!(!PlugMode::Cold.is_hot());

        assert!(PlugMode::Hot.is_hot());
        assert!(!PlugMode::Hot.is_cold());
    }

    #[test]
    fn test_plug_mode_default() {
        assert_eq!(PlugMode::default(), PlugMode::Hot);
    }
}
