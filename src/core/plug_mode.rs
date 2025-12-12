// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Hot-plug and cold-plug mode definitions.
//!
//! # Security Constraint
//!
//! In confidential computing builds, **only cold-plug mode is supported**.
//! Hot-plug requires dynamic device handling which is not secure in CC environments.

/// Plug mode for GPU device detection
///
/// Determines how GPU devices are handled:
/// - **Cold-plug**: GPUs present at boot time (required for confidential builds)
/// - **Hot-plug**: GPUs can be added/removed at runtime (standard builds only)
///
/// # Confidential Computing Constraint
///
/// When compiled with `feature = "confidential"`, hot-plug mode is **not supported**.
/// The system will always use cold-plug mode regardless of device detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlugMode {
    /// Cold-plug mode: GPUs present at boot
    /// Required for confidential computing builds
    Cold,
    /// Hot-plug mode: GPUs can be added/removed dynamically
    /// Only available in standard (non-confidential) builds
    Hot,
}

impl Default for PlugMode {
    /// Default plug mode depends on build configuration
    ///
    /// - Confidential builds: `Cold` (security requirement)
    /// - Standard builds: `Hot` (flexibility)
    fn default() -> Self {
        #[cfg(feature = "confidential")]
        {
            // Confidential builds always use cold-plug for security
            Self::Cold
        }

        #[cfg(not(feature = "confidential"))]
        {
            // Standard builds default to hot-plug for flexibility
            Self::Hot
        }
    }
}

impl PlugMode {
    /// Determine plug mode based on whether devices were detected
    ///
    /// # Confidential Computing
    ///
    /// In confidential builds, this always returns `Cold` regardless of
    /// device detection, as hot-plug is not supported for security reasons.
    pub fn from_devices_present(devices_present: bool) -> Self {
        #[cfg(feature = "confidential")]
        {
            // Always cold-plug in confidential builds
            let _ = devices_present; // Suppress unused warning
            debug!("Confidential build: forcing cold-plug mode");
            Self::Cold
        }

        #[cfg(not(feature = "confidential"))]
        {
            // Standard builds: use device detection
            if devices_present {
                Self::Cold
            } else {
                Self::Hot
            }
        }
    }

    /// Validate that the plug mode is allowed in the current build configuration
    ///
    /// # Panics
    ///
    /// Panics if hot-plug mode is used in a confidential build.
    pub fn validate(self) {
        #[cfg(feature = "confidential")]
        {
            if self == Self::Hot {
                panic!(
                    "Hot-plug mode is not supported in confidential builds. \
                     Confidential computing requires cold-plug for security."
                );
            }
        }

        #[cfg(not(feature = "confidential"))]
        {
            // All modes allowed in standard builds
            let _ = self;
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

    /// Check if hot-plug is supported in this build
    ///
    /// Returns false for confidential builds, true for standard builds.
    #[allow(dead_code)] // Useful for runtime checks
    pub const fn is_hot_plug_supported() -> bool {
        #[cfg(feature = "confidential")]
        {
            false
        }

        #[cfg(not(feature = "confidential"))]
        {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plug_mode_from_devices() {
        // Confidential builds: always cold-plug
        #[cfg(feature = "confidential")]
        {
            assert_eq!(PlugMode::from_devices_present(true), PlugMode::Cold);
            assert_eq!(PlugMode::from_devices_present(false), PlugMode::Cold);
        }

        // Standard builds: based on device detection
        #[cfg(not(feature = "confidential"))]
        {
            assert_eq!(PlugMode::from_devices_present(true), PlugMode::Cold);
            assert_eq!(PlugMode::from_devices_present(false), PlugMode::Hot);
        }
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
        // Confidential builds: default to cold-plug
        #[cfg(feature = "confidential")]
        {
            assert_eq!(PlugMode::default(), PlugMode::Cold);
        }

        // Standard builds: default to hot-plug
        #[cfg(not(feature = "confidential"))]
        {
            assert_eq!(PlugMode::default(), PlugMode::Hot);
        }
    }
}
