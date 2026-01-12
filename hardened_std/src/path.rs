// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Path handling with minimal allocation

use alloc::string::String;

/// Path reference (unsized)
#[repr(transparent)]
pub struct Path {
    inner: str,
}

impl Path {
    /// Create a Path reference from a string reference.
    ///
    /// # Lifetime
    /// The returned Path reference has the same lifetime as the input string.
    /// This is safe because Path is repr(transparent) over str.
    pub fn new<S: AsRef<str> + ?Sized>(s: &S) -> &Path {
        // SAFETY: Path is repr(transparent) over str, so they have identical memory layout.
        // The pointer cast is safe, and the lifetime relationship ensures the Path reference
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

impl AsRef<Path> for str {
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}

impl AsRef<Path> for String {
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}

impl AsRef<Path> for Path {
    fn as_ref(&self) -> &Path {
        self
    }
}

/// Owned path buffer
pub struct PathBuf {
    #[allow(dead_code)]
    inner: String,
}

impl PathBuf {
    /// Create an empty PathBuf
    pub fn new() -> Self {
        Self {
            inner: String::new(),
        }
    }
}

impl Default for PathBuf {
    fn default() -> Self {
        Self::new()
    }
}
