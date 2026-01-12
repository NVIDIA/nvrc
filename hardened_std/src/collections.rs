// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Minimal collections with fixed size
//!
//! This module provides a stack-allocated HashMap with a fixed maximum size
//! to avoid heap allocations while maintaining the std::collections::HashMap API.
//!
//! Security constraint: Only supports &'static str keys and fn(&mut T) function pointer values.
//! This restriction ensures:
//! - No heap allocations
//! - No dynamic strings that could be controlled by attackers
//! - Only compile-time known keys
//! - Only function pointers of the form fn(&mut T) as values

/// Sealed traits to restrict HashMap key and value types.
/// Cannot be implemented outside this module - this is the security boundary.
mod sealed {
    pub trait SealedKey {}
    pub trait SealedValue {}

    // Only &'static str can be a key
    impl SealedKey for &'static str {}

    // Only function pointers with mutable reference parameters can be values
    // This matches nvrc's ModeFn = fn(&mut NVRC) pattern
    impl<T> SealedValue for fn(&mut T) {}
}

/// Marker trait for allowed HashMap key types.
/// Only &'static str implements this trait - no other types are allowed.
/// This is enforced by the sealed trait pattern: the SealedKey trait is in a
/// private module, so external code cannot implement it for new types.
pub trait HashMapKey: sealed::SealedKey + 'static + Copy {}
impl HashMapKey for &'static str {}

/// Marker trait for allowed HashMap value types.
/// Only function pointers of the form fn(&mut T) are allowed.
/// This matches nvrc's mode dispatch pattern: fn(&mut NVRC).
pub trait HashMapValue: sealed::SealedValue + Copy {}
impl<T> HashMapValue for fn(&mut T) {}

/// Stack-allocated HashMap with max 4 entries.
/// No heap allocation - all data stored inline on the stack.
/// Sufficient for nvrc's mode dispatch table (4 modes: gpu, cpu, nvswitch-nvl4, nvswitch-nvl5).
///
/// **Security constraints (MAXIMALLY RESTRICTIVE):**
/// - Keys: ONLY &'static str (enforced by HashMapKey sealed trait - cannot be bypassed)
/// - Values: ONLY fn(&mut T) function pointers (enforced by HashMapValue sealed trait - cannot be bypassed)
/// - Size: Fixed at 4 entries (no dynamic growth)
///
/// API-compatible with std::collections::HashMap<K, V> for easy drop-in replacement.
///
/// **What you CANNOT use (compile-time errors):**
/// - Keys: String, &str, i32, or any other type - ONLY &'static str works
/// - Values: i32, bool, String, Vec, fn(), fn(T), or any other type - ONLY fn(&mut T) works
#[derive(Copy, Clone)]
pub struct HashMap<K: HashMapKey, V: HashMapValue> {
    entries: [(K, V); 4],
}

impl<V: HashMapValue> HashMap<&'static str, V> {
    /// Create HashMap from an array of exactly 4 entries.
    /// The array must contain exactly 4 elements to match our fixed size.
    ///
    /// # Security
    /// Keys must be &'static str - compile-time constants only.
    /// No runtime strings allowed.
    pub fn from(entries: [(&'static str, V); 4]) -> Self {
        Self { entries }
    }

    /// Get a reference to the value associated with a key.
    /// Returns None if the key is not found.
    /// Time complexity: O(n) where n=4 (linear search, acceptable for small fixed size).
    pub fn get(&self, key: &str) -> Option<&V> {
        self.entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hashmap_from_and_get() {
        // Test with fn(&mut T) function pointers (only allowed type)
        type TestFn = fn(&mut usize);

        fn set_one(x: &mut usize) {
            *x = 1;
        }
        fn set_two(x: &mut usize) {
            *x = 2;
        }
        fn set_three(x: &mut usize) {
            *x = 3;
        }
        fn set_four(x: &mut usize) {
            *x = 4;
        }

        let map: HashMap<&str, TestFn> = HashMap::from([
            ("one", set_one as TestFn),
            ("two", set_two as TestFn),
            ("three", set_three as TestFn),
            ("four", set_four as TestFn),
        ]);

        let mut result = 0;
        if let Some(&func) = map.get("one") {
            func(&mut result);
        }
        assert_eq!(result, 1);

        if let Some(&func) = map.get("two") {
            func(&mut result);
        }
        assert_eq!(result, 2);

        assert!(map.get("five").is_none());
    }

    #[test]
    fn test_hashmap_with_function_pointers() {
        // This matches nvrc's actual use case: fn(&mut NVRC)
        type ModeFn = fn(&mut i32);

        fn add_one(x: &mut i32) {
            *x += 1;
        }
        fn add_two(x: &mut i32) {
            *x += 2;
        }
        fn noop(_: &mut i32) {}

        let modes: HashMap<&str, ModeFn> = HashMap::from([
            ("mode1", add_one as ModeFn),
            ("mode2", add_two as ModeFn),
            ("mode3", noop as ModeFn),
            ("mode4", noop as ModeFn),
        ]);

        let mut val = 10;
        if let Some(&func) = modes.get("mode1") {
            func(&mut val);
        }
        assert_eq!(val, 11);

        if let Some(&func) = modes.get("mode2") {
            func(&mut val);
        }
        assert_eq!(val, 13);
    }

    #[test]
    fn test_hashmap_copy() {
        type CounterFn = fn(&mut usize);

        fn inc(x: &mut usize) {
            *x += 1;
        }
        fn dec(x: &mut usize) {
            *x -= 1;
        }
        fn double(x: &mut usize) {
            *x *= 2;
        }
        fn reset(x: &mut usize) {
            *x = 0;
        }

        let map1: HashMap<&str, CounterFn> = HashMap::from([
            ("inc", inc as CounterFn),
            ("dec", dec as CounterFn),
            ("double", double as CounterFn),
            ("reset", reset as CounterFn),
        ]);

        // HashMap derives Copy, so we can copy it directly
        let map2: HashMap<&str, CounterFn> = map1;

        let mut val = 5;
        // Option::copied() works because fn pointers are Copy
        if let Some(func) = map2.get("double").copied() {
            func(&mut val);
        }
        assert_eq!(val, 10);

        // Verify map1 is still usable (it was copied, not moved)
        assert!(map1.get("inc").is_some());
    }
}
