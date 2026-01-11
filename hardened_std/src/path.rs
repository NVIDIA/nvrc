// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Path handling with minimal allocation

use alloc::string::String;

/// Path reference (unsized)
#[repr(transparent)]
pub struct Path {
    inner: str,
}

/// Owned path buffer
#[allow(dead_code)]
pub struct PathBuf {
    inner: String,
}

impl Path {
    /// Create a Path reference from a string reference.
    ///
    /// # Lifetime
    /// The returned Path reference has the same lifetime as the input string.
    /// This is safe because Path is repr(transparent) over str.
    pub fn new<'a, S: AsRef<str> + ?Sized>(s: &'a S) -> &'a Path {
        // SAFETY: Path is repr(transparent) over str, so they have identical memory layout.
        // The pointer cast is safe, and the lifetime 'a ensures the Path reference
        // cannot outlive the source string.
        unsafe { &*(s.as_ref() as *const str as *const Path) }
    }

    pub fn as_str(&self) -> &str {
        &self.inner
    }
}

impl AsRef<str> for Path {
    fn as_ref(&self) -> &str {
        &self.inner
    }
}

impl PathBuf {
    #[allow(dead_code)]
    pub fn new() -> Self {
        todo!("PathBuf::new")
    }
}

impl Default for PathBuf {
    fn default() -> Self {
        Self::new()
    }
}
