//! In-crate string interner backed by one contiguous UTF-8 buffer (CD7, section 11.2).
//!
//! Interner ids reflect insertion order, so [`StrId`] intentionally does not implement
//! [`Ord`] or [`PartialOrd`]: callers must never sort output by interned id (CD7, CINV10).

use std::{
    borrow::Borrow,
    hash::{Hash, Hasher},
};

use rustc_hash::FxHasher;
use smallvec::SmallVec;

/// Non-iterable O(1) hash lookup table backed by `rustc_hash::FxHasher` (CD7, CINV10).
///
/// Intentionally exposes no `iter`, `keys`, `values`, `drain`, or `IntoIterator` implementation so
/// unordered traversal (including via generic `IntoIterator` helpers or `std::iter::zip`) is
/// impossible at compile time.
#[derive(Debug, Clone)]
pub(crate) struct LookupMap<K, V> {
    slots: Vec<Option<(K, V)>>,
    len: usize,
}

impl<K, V> Default for LookupMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> LookupMap<K, V> {
    #[inline]
    pub(crate) const fn new() -> Self {
        Self {
            slots: Vec::new(),
            len: 0,
        }
    }

    pub(crate) fn with_capacity(capacity: usize) -> Self {
        if capacity == 0 {
            return Self::new();
        }
        let target = capacity
            .saturating_mul(10)
            .div_ceil(7)
            .max(8)
            .next_power_of_two();
        let mut slots = Vec::with_capacity(target);
        slots.resize_with(target, || None);
        Self { slots, len: 0 }
    }
}

impl<K: Eq + Hash, V> LookupMap<K, V> {
    #[inline]
    fn hash_key<Q: Hash + ?Sized>(key: &Q) -> usize {
        let mut hasher = FxHasher::default();
        key.hash(&mut hasher);
        hasher.finish() as usize
    }

    #[inline]
    fn needs_grow_for_new_entry(&self) -> bool {
        let cap = self.slots.len();
        cap == 0 || (self.len + 1).saturating_mul(10) > cap.saturating_mul(7)
    }

    fn grow_for_new_entry(&mut self) {
        let cap = self.slots.len();
        if cap == 0 {
            self.rehash(8);
        } else {
            self.rehash(cap.checked_mul(2).expect("LookupMap capacity overflow"));
        }
    }

    fn rehash(&mut self, new_cap: usize) {
        debug_assert!(new_cap.is_power_of_two());
        let mask = new_cap - 1;
        let mut new_slots = Vec::with_capacity(new_cap);
        new_slots.resize_with(new_cap, || None);
        for slot in self.slots.drain(..) {
            let Some((k, v)) = slot else {
                continue;
            };
            let mut idx = Self::hash_key(&k) & mask;
            while new_slots[idx].is_some() {
                idx = (idx + 1) & mask;
            }
            new_slots[idx] = Some((k, v));
        }
        self.slots = new_slots;
    }

    #[inline]
    pub(crate) fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let cap = self.slots.len();
        if cap == 0 {
            return None;
        }
        let mask = cap - 1;
        let mut idx = Self::hash_key(key) & mask;
        loop {
            match &self.slots[idx] {
                Some((k, v)) if k.borrow() == key => return Some(v),
                Some(_) => idx = (idx + 1) & mask,
                None => return None,
            }
        }
    }

    /// Probes for `key` using a precomputed `hash`: returns `Ok(idx)` if `key` is already stored
    /// or `Err(vacant_idx)` for the first empty slot when the table is non-empty.
    #[inline]
    fn probe_slot(&self, hash: usize, key: &K) -> Result<usize, usize> {
        let mask = self.slots.len() - 1;
        let mut idx = hash & mask;
        loop {
            match &self.slots[idx] {
                Some((k, _)) if k == key => return Ok(idx),
                Some(_) => idx = (idx + 1) & mask,
                None => return Err(idx),
            }
        }
    }

    #[inline]
    fn find_vacant_slot(&self, hash: usize) -> usize {
        let mask = self.slots.len() - 1;
        let mut idx = hash & mask;
        while self.slots[idx].is_some() {
            idx = (idx + 1) & mask;
        }
        idx
    }

    pub(crate) fn insert(&mut self, key: K, value: V) -> Option<V> {
        let hash = Self::hash_key(&key);
        let vacant_idx = if self.slots.is_empty() {
            self.grow_for_new_entry();
            self.find_vacant_slot(hash)
        } else {
            match self.probe_slot(hash, &key) {
                Ok(idx) => {
                    let slot = self.slots[idx].as_mut().expect("matched slot populated");
                    return Some(std::mem::replace(&mut slot.1, value));
                }
                Err(vacant) => {
                    if self.needs_grow_for_new_entry() {
                        self.grow_for_new_entry();
                        self.find_vacant_slot(hash)
                    } else {
                        vacant
                    }
                }
            }
        };
        self.slots[vacant_idx] = Some((key, value));
        self.len += 1;
        None
    }

    pub(crate) fn get_or_insert_with(&mut self, key: K, make: impl FnOnce() -> V) -> &mut V {
        let hash = Self::hash_key(&key);
        let vacant_idx = if self.slots.is_empty() {
            self.grow_for_new_entry();
            self.find_vacant_slot(hash)
        } else {
            match self.probe_slot(hash, &key) {
                Ok(idx) => {
                    return &mut self.slots[idx].as_mut().expect("matched slot populated").1;
                }
                Err(vacant) => {
                    if self.needs_grow_for_new_entry() {
                        self.grow_for_new_entry();
                        self.find_vacant_slot(hash)
                    } else {
                        vacant
                    }
                }
            }
        };
        self.slots[vacant_idx] = Some((key, make()));
        self.len += 1;
        &mut self.slots[vacant_idx].as_mut().expect("slot populated").1
    }

    #[inline]
    pub(crate) fn get_or_default(&mut self, key: K) -> &mut V
    where
        V: Default,
    {
        self.get_or_insert_with(key, V::default)
    }
}

/// Opaque interned string handle (`u32`, CD7).
///
/// Does not implement `PartialOrd` or `Ord` because interning order depends on insertion order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StrId(u32);

impl StrId {
    /// Returns the raw `u32` slot index. For diagnostics only; never use for output ordering.
    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Compact string interner: one contiguous byte buffer, one offset vector, and a `rustc-hash`
/// lookup table that is never iterated (CD7, CINV10).
#[derive(Debug, Clone)]
pub struct Interner {
    bytes: String,
    /// Byte offsets into `bytes`; length is `len + 1` with `offsets[0] == 0`.
    offsets: Vec<u32>,
    /// Hash -> bucket of matching interned ids; zero string duplication and zero allocation on hit.
    buckets: LookupMap<u64, SmallVec<[StrId; 1]>>,
}

impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}

impl Interner {
    /// Creates an empty interner.
    pub fn new() -> Self {
        Self {
            bytes: String::new(),
            offsets: vec![0],
            buckets: LookupMap::new(),
        }
    }

    /// Creates an empty interner with preallocated capacity for `strings` entries and `bytes`
    /// total string bytes.
    pub fn with_capacity(strings: usize, bytes: usize) -> Self {
        let mut offsets = Vec::with_capacity(strings + 1);
        offsets.push(0);
        Self {
            bytes: String::with_capacity(bytes),
            offsets,
            buckets: LookupMap::with_capacity(strings),
        }
    }

    /// Number of distinct interned strings.
    #[inline]
    pub fn len(&self) -> usize {
        self.offsets.len() - 1
    }

    /// Returns `true` if no string has been interned.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Looks up `s` without inserting or allocating.
    #[inline]
    pub fn get(&self, s: &str) -> Option<StrId> {
        let hash = hash_str(s);
        let bucket = self.buckets.get(&hash)?;
        bucket.iter().copied().find(|&id| self.resolve(id) == s)
    }

    /// Returns the [`StrId`] for `s`, inserting it into the byte buffer if not already present.
    ///
    /// Allocates nothing when `s` is already interned.
    pub fn intern(&mut self, s: &str) -> StrId {
        let hash = hash_str(s);
        if let Some(bucket) = self.buckets.get(&hash)
            && let Some(&existing) = bucket.iter().find(|&&id| self.resolve(id) == s)
        {
            return existing;
        }
        let idx = u32::try_from(self.len()).expect("interner string count overflow");
        self.bytes.push_str(s);
        let end = u32::try_from(self.bytes.len()).expect("interner byte buffer overflow");
        self.offsets.push(end);
        let id = StrId(idx);
        self.buckets.get_or_default(hash).push(id);
        id
    }

    /// Resolves an interned [`StrId`] back to its string slice in O(1).
    #[inline]
    pub fn resolve(&self, id: StrId) -> &str {
        let idx = id.0 as usize;
        let start = self.offsets[idx] as usize;
        let end = self.offsets[idx + 1] as usize;
        &self.bytes[start..end]
    }
}

#[inline]
fn hash_str(s: &str) -> u64 {
    let mut hasher = FxHasher::default();
    hasher.write(s.as_bytes());
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_roundtrip_dedup_and_lookup() {
        let mut interner = Interner::new();
        assert!(interner.is_empty());
        assert_eq!(interner.get("VCC"), None);

        let a = interner.intern("VCC");
        let b = interner.intern("GND");
        let a2 = interner.intern("VCC");
        let empty = interner.intern("");
        let utf8 = interner.intern("In → buffer");

        assert_eq!(a, a2);
        assert_ne!(a, b);
        assert_eq!(interner.len(), 4);
        assert_eq!(interner.get("VCC"), Some(a));
        assert_eq!(interner.get("GND"), Some(b));
        assert_eq!(interner.get(""), Some(empty));
        assert_eq!(interner.get("In → buffer"), Some(utf8));
        assert_eq!(interner.get("missing"), None);

        assert_eq!(interner.resolve(a), "VCC");
        assert_eq!(interner.resolve(b), "GND");
        assert_eq!(interner.resolve(empty), "");
        assert_eq!(interner.resolve(utf8), "In → buffer");
    }

    #[test]
    fn handles_hash_bucket_collisions_gracefully() {
        let mut interner = Interner::with_capacity(4, 16);
        let first = interner.intern("alpha");
        let second = interner.intern("beta");
        // Simulate a hash collision by adding `second` into `first`'s hash bucket.
        let h_first = hash_str("alpha");
        interner.buckets.get_or_default(h_first).push(second);
        assert_eq!(interner.get("alpha"), Some(first));
        assert_eq!(interner.intern("alpha"), first);
        assert_eq!(interner.resolve(first), "alpha");
        assert_eq!(interner.resolve(second), "beta");
    }

    #[test]
    fn lookup_map_hits_do_not_rehash_at_load_threshold() {
        let mut map: LookupMap<u32, u32> = LookupMap::with_capacity(5);
        assert_eq!(map.slots.len(), 8);
        for k in 0..5 {
            assert_eq!(map.insert(k, k * 10), None);
        }
        assert_eq!(map.slots.len(), 8);
        // Updating or querying existing keys at the 5/8 load threshold must not rehash.
        for k in 0..5 {
            assert_eq!(map.insert(k, k * 20), Some(k * 10));
            assert_eq!(*map.get_or_insert_with(k, || 999), k * 20);
        }
        assert_eq!(map.slots.len(), 8);
    }
}
