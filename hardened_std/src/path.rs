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
    pub fn new<S: AsRef<str> + ?Sized>(s: &S) -> &Path {
        unsafe { &*(s.as_ref() as *const str as *const Path) }
    }

    #[allow(dead_code)]
    pub fn exists(&self) -> bool {
        todo!("Path::exists")
    }

    #[allow(dead_code)]
    pub fn is_symlink(&self) -> bool {
        todo!("Path::is_symlink")
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
