//! Minimal collections with fixed size

/// Stack-allocated HashMap with max 4 entries
pub struct HashMap<K, V> {
    _phantom: core::marker::PhantomData<(K, V)>,
}

impl<K, V> HashMap<K, V> {
    pub fn from(_entries: impl IntoIterator<Item = (K, V)>) -> Self {
        todo!("HashMap::from")
    }

    pub fn get(&self, _key: &K) -> Option<&V> {
        todo!("HashMap::get")
    }

    pub fn copied(&self) -> Self
    where
        K: Clone,
        V: Clone,
    {
        todo!("HashMap::copied")
    }
}
