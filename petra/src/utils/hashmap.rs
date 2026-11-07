// SPDX-License-Identifier: GPL-2.0

//! Native `#![no_std]` Open-Addressing Hash Table for the PetraOS kernel.
//!
//! Provides amortized O(1) lookups, insertions, and deletions with linear probing,
//! backward-shift cluster rehashing on deletion, and zero external crate dependencies.
//!
//! Uses a fast FNV-1a 64-bit hasher by default, optimized for kernel identifiers,
//! strings, PIDs, addresses, and integer keys.

use alloc::vec::Vec;
use core::borrow::Borrow;
use core::fmt::Debug;
use core::hash::{BuildHasher, Hash, Hasher};

/// 64-bit FNV-1a offset basis.
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
/// 64-bit FNV-1a prime.
const FNV_PRIME: u64 = 0x100000001b3;

/// FNV-1a 64-bit string hasher for high-speed directory and string component hashing.
#[inline]
pub fn dentry_hash(name: &str) -> u64 {
    fnv1a_hash_str(name)
}

/// Computes the 64-bit FNV-1a hash of a byte slice.
#[inline]
pub fn fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Computes the 64-bit FNV-1a hash of a string slice.
#[inline]
pub fn fnv1a_hash_str(s: &str) -> u64 {
    fnv1a_hash(s.as_bytes())
}

/// Fast FNV-1a hasher implementing [`core::hash::Hasher`].
#[derive(Clone, Copy, Debug)]
pub struct FnvHasher(u64);

impl Default for FnvHasher {
    #[inline]
    fn default() -> Self {
        Self(FNV_OFFSET_BASIS)
    }
}

impl Hasher for FnvHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= byte as u64;
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }
}

/// BuildHasher for [`FnvHasher`].
#[derive(Clone, Copy, Debug, Default)]
pub struct FnvBuildHasher;

impl BuildHasher for FnvBuildHasher {
    type Hasher = FnvHasher;

    #[inline]
    fn build_hasher(&self) -> FnvHasher {
        FnvHasher::default()
    }
}

/// An entry bucket inside [`HashMap`].
#[derive(Clone, Debug)]
struct Bucket<K, V> {
    hash: u64,
    key: K,
    value: V,
}

/// High-performance `#![no_std]` hash map using open addressing with linear probing.
///
/// Fully compatible with `#![no_std]` kernel environments without requiring external
/// crates like `hashbrown`.
pub struct HashMap<K, V, S = FnvBuildHasher> {
    entries: Vec<Option<Bucket<K, V>>>,
    count: usize,
    hash_builder: S,
}

impl<K, V> HashMap<K, V, FnvBuildHasher> {
    /// Creates a new empty [`HashMap`]. Does not allocate until insertion.
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            count: 0,
            hash_builder: FnvBuildHasher,
        }
    }

    /// Creates an empty [`HashMap`] with at least the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self::with_capacity_and_hasher(capacity, FnvBuildHasher)
    }
}

impl<K, V, S> HashMap<K, V, S> {
    /// Creates an empty [`HashMap`] with the given hash builder.
    pub const fn with_hasher(hash_builder: S) -> Self {
        Self {
            entries: Vec::new(),
            count: 0,
            hash_builder,
        }
    }

    /// Creates an empty [`HashMap`] with specified capacity and hash builder.
    pub fn with_capacity_and_hasher(capacity: usize, hash_builder: S) -> Self {
        let cap = if capacity == 0 {
            0
        } else {
            capacity.next_power_of_two().max(16)
        };
        let mut entries = Vec::with_capacity(cap);
        entries.resize_with(cap, || None);
        Self {
            entries,
            count: 0,
            hash_builder,
        }
    }

    /// Returns the number of elements in the map.
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Returns `true` if the map contains no elements.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Returns the current bucket capacity of the map.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.entries.len()
    }

    /// Clears the map, deallocating all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.count = 0;
    }

    /// Returns an iterator over all key-value references.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries
            .iter()
            .filter_map(|b| b.as_ref().map(|entry| (&entry.key, &entry.value)))
    }

    /// Returns an iterator over all key-value pairs with mutable value references.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.entries
            .iter_mut()
            .filter_map(|b| b.as_mut().map(|entry| (&entry.key, &mut entry.value)))
    }

    /// Returns an iterator visiting all keys in arbitrary order.
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.entries
            .iter()
            .filter_map(|b| b.as_ref().map(|entry| &entry.key))
    }

    /// Returns an iterator visiting all values in arbitrary order.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries
            .iter()
            .filter_map(|b| b.as_ref().map(|entry| &entry.value))
    }

    /// Returns a mutable iterator visiting all values in arbitrary order.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.entries
            .iter_mut()
            .filter_map(|b| b.as_mut().map(|entry| &mut entry.value))
    }
}

impl<K, V, S: BuildHasher> HashMap<K, V, S> {
    #[inline]
    fn make_hash<Q>(&self, key: &Q) -> u64
    where
        Q: ?Sized + Hash,
    {
        let mut hasher = self.hash_builder.build_hasher();
        key.hash(&mut hasher);
        hasher.finish()
    }

    /// Looks up a value by key in O(1) expected time.
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        if self.entries.is_empty() {
            return None;
        }
        let hash = self.make_hash(key);
        let mask = self.entries.len() - 1;
        let mut idx = (hash as usize) & mask;
        let mut probed = 0;

        while probed < self.entries.len() {
            match &self.entries[idx] {
                Some(bucket) => {
                    if bucket.hash == hash && bucket.key.borrow() == key {
                        return Some(&bucket.value);
                    }
                }
                None => return None,
            }
            idx = (idx + 1) & mask;
            probed += 1;
        }
        None
    }

    /// Looks up a key and value pair in O(1) expected time.
    pub fn get_key_value<Q>(&self, key: &Q) -> Option<(&K, &V)>
    where
        K: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        if self.entries.is_empty() {
            return None;
        }
        let hash = self.make_hash(key);
        let mask = self.entries.len() - 1;
        let mut idx = (hash as usize) & mask;
        let mut probed = 0;

        while probed < self.entries.len() {
            match &self.entries[idx] {
                Some(bucket) => {
                    if bucket.hash == hash && bucket.key.borrow() == key {
                        return Some((&bucket.key, &bucket.value));
                    }
                }
                None => return None,
            }
            idx = (idx + 1) & mask;
            probed += 1;
        }
        None
    }

    /// Looks up a mutable reference to a value by key in O(1) expected time.
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        if self.entries.is_empty() {
            return None;
        }
        let hash = self.make_hash(key);
        let mask = self.entries.len() - 1;
        let mut idx = (hash as usize) & mask;
        let mut probed = 0;

        let found_idx = loop {
            if probed >= self.entries.len() {
                break None;
            }
            match &self.entries[idx] {
                Some(bucket) => {
                    if bucket.hash == hash && bucket.key.borrow() == key {
                        break Some(idx);
                    }
                }
                None => break None,
            }
            idx = (idx + 1) & mask;
            probed += 1;
        };

        found_idx.map(move |i| &mut self.entries[i].as_mut().unwrap().value)
    }

    /// Checks whether the map contains the given key.
    #[inline]
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        self.get(key).is_some()
    }

    /// Inserts a key-value pair into the map.
    ///
    /// If the key was already present, returns the old value.
    pub fn insert(&mut self, key: K, value: V) -> Option<V>
    where
        K: Hash + Eq,
    {
        if self.entries.is_empty() || (self.count + 1) * 4 >= self.entries.len() * 3 {
            self.grow();
        }
        let hash = self.make_hash(&key);
        let mask = self.entries.len() - 1;
        let mut idx = (hash as usize) & mask;

        loop {
            match &mut self.entries[idx] {
                Some(bucket) => {
                    if bucket.hash == hash && bucket.key == key {
                        return Some(core::mem::replace(&mut bucket.value, value));
                    }
                }
                None => {
                    self.entries[idx] = Some(Bucket { hash, key, value });
                    self.count += 1;
                    return None;
                }
            }
            idx = (idx + 1) & mask;
        }
    }

    /// Removes a key from the map, returning its value if it was present.
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        self.remove_entry(key).map(|(_, v)| v)
    }

    /// Removes a key from the map, returning the key and value tuple if it was present.
    pub fn remove_entry<Q>(&mut self, key: &Q) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        if self.entries.is_empty() {
            return None;
        }
        let hash = self.make_hash(key);
        let mask = self.entries.len() - 1;
        let mut idx = (hash as usize) & mask;
        let mut probed = 0;

        while probed < self.entries.len() {
            if let Some(bucket) = &self.entries[idx] {
                if bucket.hash == hash && bucket.key.borrow() == key {
                    let removed = self.entries[idx].take().unwrap();
                    self.count -= 1;
                    self.rehash_cluster((idx + 1) & mask);
                    return Some((removed.key, removed.value));
                }
            } else {
                return None;
            }
            idx = (idx + 1) & mask;
            probed += 1;
        }
        None
    }

    /// Rehashes the cluster following a deletion to maintain linear probing invariant.
    fn rehash_cluster(&mut self, mut idx: usize) {
        let mask = self.entries.len() - 1;
        while let Some(bucket) = self.entries[idx].take() {
            self.count -= 1;
            let target_hash = bucket.hash;
            let mut insert_idx = (target_hash as usize) & mask;
            while self.entries[insert_idx].is_some() {
                insert_idx = (insert_idx + 1) & mask;
            }
            self.entries[insert_idx] = Some(bucket);
            self.count += 1;
            idx = (idx + 1) & mask;
        }
    }

    /// Retains only entries satisfying predicate `f`.
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&K, &mut V) -> bool,
    {
        if self.entries.is_empty() {
            return;
        }
        let cap = self.entries.len();
        let mut new_entries = Vec::with_capacity(cap);
        new_entries.resize_with(cap, || None);
        let old_entries = core::mem::replace(&mut self.entries, new_entries);
        self.count = 0;
        let mask = cap - 1;

        for mut bucket in old_entries.into_iter().flatten() {
            if f(&bucket.key, &mut bucket.value) {
                let mut idx = (bucket.hash as usize) & mask;
                while self.entries[idx].is_some() {
                    idx = (idx + 1) & mask;
                }
                self.entries[idx] = Some(bucket);
                self.count += 1;
            }
        }
    }

    /// Grows the hash table to the next power of 2.
    fn grow(&mut self) {
        let new_cap = if self.entries.is_empty() {
            16
        } else {
            self.entries.len() * 2
        };
        let mut new_entries = Vec::with_capacity(new_cap);
        new_entries.resize_with(new_cap, || None);
        let old_entries = core::mem::replace(&mut self.entries, new_entries);
        self.count = 0;
        let mask = new_cap - 1;

        for bucket in old_entries.into_iter().flatten() {
            let mut idx = (bucket.hash as usize) & mask;
            while self.entries[idx].is_some() {
                idx = (idx + 1) & mask;
            }
            self.entries[idx] = Some(bucket);
            self.count += 1;
        }
    }

    /// Gets the given key's corresponding entry in the map for in-place manipulation.
    pub fn entry(&mut self, key: K) -> Entry<'_, K, V, S>
    where
        K: Hash + Eq,
    {
        if !self.entries.is_empty() {
            let hash = self.make_hash(&key);
            let mask = self.entries.len() - 1;
            let mut idx = (hash as usize) & mask;
            let mut probed = 0;
            while probed < self.entries.len() {
                match &self.entries[idx] {
                    Some(bucket) => {
                        if bucket.hash == hash && bucket.key == key {
                            return Entry::Occupied(OccupiedEntry {
                                key,
                                map: self,
                                idx,
                            });
                        }
                    }
                    None => break,
                }
                idx = (idx + 1) & mask;
                probed += 1;
            }
        }
        Entry::Vacant(VacantEntry { key, map: self })
    }
}

/// A view into a single entry in a map, which may either be vacant or occupied.
pub enum Entry<'a, K, V, S = FnvBuildHasher> {
    /// An occupied entry.
    Occupied(OccupiedEntry<'a, K, V, S>),
    /// A vacant entry.
    Vacant(VacantEntry<'a, K, V, S>),
}

impl<'a, K: Hash + Eq, V, S: BuildHasher> Entry<'a, K, V, S> {
    /// Ensures a value is in the entry by inserting the default if empty, and returns
    /// a mutable reference to the value in the entry.
    pub fn or_insert(self, default: V) -> &'a mut V {
        match self {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(default),
        }
    }

    /// Ensures a value is in the entry by inserting the result of the default function if empty,
    /// and returns a mutable reference to the value in the entry.
    pub fn or_insert_with<F: FnOnce() -> V>(self, default: F) -> &'a mut V {
        match self {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(default()),
        }
    }

    /// Ensures a value is in the entry by inserting the default value if empty,
    /// and returns a mutable reference to the value in the entry.
    pub fn or_default(self) -> &'a mut V
    where
        V: Default,
    {
        self.or_insert_with(Default::default)
    }

    /// Returns a reference to this entry's key.
    pub fn key(&self) -> &K {
        match self {
            Entry::Occupied(entry) => entry.key(),
            Entry::Vacant(entry) => entry.key(),
        }
    }

    /// Provides in-place mutable access to an occupied entry before any potential inserts into the map.
    pub fn and_modify<F: FnOnce(&mut V)>(mut self, f: F) -> Self {
        if let Entry::Occupied(entry) = &mut self {
            f(entry.get_mut());
        }
        self
    }
}

/// A view into an occupied entry in a [`HashMap`].
pub struct OccupiedEntry<'a, K, V, S> {
    key: K,
    map: &'a mut HashMap<K, V, S>,
    idx: usize,
}

impl<'a, K, V, S> OccupiedEntry<'a, K, V, S> {
    /// Gets a reference to the key in the entry.
    pub fn key(&self) -> &K {
        &self.key
    }

    /// Gets a reference to the value in the entry.
    pub fn get(&self) -> &V {
        &self.map.entries[self.idx].as_ref().unwrap().value
    }

    /// Gets a mutable reference to the value in the entry.
    pub fn get_mut(&mut self) -> &mut V {
        &mut self.map.entries[self.idx].as_mut().unwrap().value
    }

    /// Converts the `OccupiedEntry` into a mutable reference to the value in the entry
    /// with a lifetime bound to the map itself.
    pub fn into_mut(self) -> &'a mut V {
        &mut self.map.entries[self.idx].as_mut().unwrap().value
    }

    /// Sets the value of the entry, and returns the entry's old value.
    pub fn insert(&mut self, value: V) -> V {
        core::mem::replace(&mut self.map.entries[self.idx].as_mut().unwrap().value, value)
    }

    /// Takes the value out of the entry, and returns it.
    pub fn remove(self) -> V
    where
        K: Borrow<K> + Hash + Eq,
        S: BuildHasher,
    {
        let removed = self.map.entries[self.idx].take().unwrap();
        self.map.count -= 1;
        let mask = self.map.entries.len() - 1;
        self.map.rehash_cluster((self.idx + 1) & mask);
        removed.value
    }
}

/// A view into a vacant entry in a [`HashMap`].
pub struct VacantEntry<'a, K, V, S> {
    key: K,
    map: &'a mut HashMap<K, V, S>,
}

impl<'a, K, V, S> VacantEntry<'a, K, V, S> {
    /// Gets a reference to the key that would be used when inserting a value through the `VacantEntry`.
    pub fn key(&self) -> &K {
        &self.key
    }

    /// Take ownership of the key.
    pub fn into_key(self) -> K {
        self.key
    }
}

impl<'a, K: Hash + Eq, V, S: BuildHasher> VacantEntry<'a, K, V, S> {
    /// Sets the value of the entry with the `VacantEntry`'s key, and returns a mutable reference to it.
    pub fn insert(self, value: V) -> &'a mut V {
        if self.map.entries.is_empty() || (self.map.count + 1) * 4 >= self.map.entries.len() * 3 {
            self.map.grow();
        }
        let hash = self.map.make_hash(&self.key);
        let mask = self.map.entries.len() - 1;
        let mut idx = (hash as usize) & mask;
        loop {
            match &mut self.map.entries[idx] {
                Some(bucket) => {
                    if bucket.hash == hash && bucket.key == self.key {
                        bucket.value = value;
                        return &mut self.map.entries[idx].as_mut().unwrap().value;
                    }
                }
                None => {
                    self.map.entries[idx] = Some(Bucket {
                        hash,
                        key: self.key,
                        value,
                    });
                    self.map.count += 1;
                    return &mut self.map.entries[idx].as_mut().unwrap().value;
                }
            }
            idx = (idx + 1) & mask;
        }
    }
}

impl<K: Clone, V: Clone, S: Clone> Clone for HashMap<K, V, S> {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            count: self.count,
            hash_builder: self.hash_builder.clone(),
        }
    }
}

impl<K, V, S: Default> Default for HashMap<K, V, S> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            count: 0,
            hash_builder: S::default(),
        }
    }
}

impl<K: Debug, V: Debug, S> Debug for HashMap<K, V, S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<K, V, S> IntoIterator for HashMap<K, V, S> {
    type Item = (K, V);
    type IntoIter = alloc::vec::IntoIter<(K, V)>;

    fn into_iter(self) -> Self::IntoIter {
        let mut vec = Vec::with_capacity(self.count);
        for bucket in self.entries.into_iter().flatten() {
            vec.push((bucket.key, bucket.value));
        }
        vec.into_iter()
    }
}

impl<K: Hash + Eq, V, S: BuildHasher + Default> FromIterator<(K, V)> for HashMap<K, V, S> {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        let mut map = Self::default();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

impl<K: Hash + Eq, V, S: BuildHasher> Extend<(K, V)> for HashMap<K, V, S> {
    fn extend<T: IntoIterator<Item = (K, V)>>(&mut self, iter: T) {
        for (k, v) in iter {
            self.insert(k, v);
        }
    }
}

/// A `#![no_std]` hash set implemented as a thin wrapper on [`HashMap`].
#[derive(Default)]
pub struct HashSet<T, S = FnvBuildHasher> {
    map: HashMap<T, (), S>,
}

impl<T: Clone, S: Clone> Clone for HashSet<T, S> {
    fn clone(&self) -> Self {
        Self {
            map: self.map.clone(),
        }
    }
}

impl<T: Debug, S> Debug for HashSet<T, S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl<T> HashSet<T, FnvBuildHasher> {
    /// Creates a new empty [`HashSet`].
    pub const fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Creates an empty [`HashSet`] with at least the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            map: HashMap::with_capacity(capacity),
        }
    }
}

impl<T, S> HashSet<T, S> {
    /// Returns the number of elements in the set.
    #[inline]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns `true` if the set contains no elements.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Clears the set.
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// Returns an iterator over the items in the set.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.map.keys()
    }
}

impl<T, S: BuildHasher> HashSet<T, S> {
    /// Checks whether the set contains the given item.
    #[inline]
    pub fn contains<Q>(&self, value: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        self.map.contains_key(value)
    }

    /// Inserts an item into the set. Returns `true` if item was not previously present.
    #[inline]
    pub fn insert(&mut self, value: T) -> bool
    where
        T: Hash + Eq,
    {
        self.map.insert(value, ()).is_none()
    }

    /// Removes an item from the set. Returns `true` if item was present.
    #[inline]
    pub fn remove<Q>(&mut self, value: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        self.map.remove(value).is_some()
    }
}
