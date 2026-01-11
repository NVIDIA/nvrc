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
pub struct PathBuf {
    inner: String,
}

impl Path {
    pub fn new<S: AsRef<str> + ?Sized>(s: &S) -> &Path {
        unsafe { &*(s.as_ref() as *const str as *const Path) }
    }

    pub fn exists(&self) -> bool {
        todo!("Path::exists")
    }

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
    pub fn new() -> Self {
        todo!("PathBuf::new")
    }
}
