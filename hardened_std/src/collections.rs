// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Minimal collections with fixed size
//!
//! **Note:** This module is currently a stub and not yet implemented.
//! NVRC uses std::collections::HashMap until the hardened version is complete.
//! See separate PR for full HashMap implementation with security constraints.

/// Stack-allocated HashMap with max 4 entries (NOT YET IMPLEMENTED)
///
/// **WARNING:** This is a placeholder stub. Do not use in production code.
/// NVRC currently uses `std::collections::HashMap` instead.
///
/// The full implementation will provide:
/// - Fixed 4-entry capacity (stack-allocated, no heap)
/// - Compile-time restricted to &'static str keys only
/// - Compile-time restricted to fn(&mut T) values only
/// - See dedicated HashMap implementation PR for details
#[doc(hidden)]
#[allow(dead_code)]
pub struct HashMap<K, V> {
    _phantom: core::marker::PhantomData<(K, V)>,
}

#[allow(dead_code)]
impl<K, V> HashMap<K, V> {
    #[doc(hidden)]
    pub fn from(_entries: impl IntoIterator<Item = (K, V)>) -> Self {
        unimplemented!("HashMap not yet implemented - use std::collections::HashMap")
    }

    #[doc(hidden)]
    pub fn get(&self, _key: &K) -> Option<&V> {
        unimplemented!("HashMap not yet implemented - use std::collections::HashMap")
    }

    #[doc(hidden)]
    pub fn copied(&self) -> Self
    where
        K: Clone,
        V: Clone,
    {
        unimplemented!("HashMap not yet implemented - use std::collections::HashMap")
    }
}
