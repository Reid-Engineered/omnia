//! In-crate string interner backed by one contiguous UTF-8 buffer (CD7, section 11.2).
//!
//! Interner ids reflect insertion order, so [`StrId`] intentionally does not implement
//! [`Ord`] or [`PartialOrd`]: callers must never sort output by interned id (CD7, CINV10).

use std::hash::Hasher;

use rustc_hash::{FxHashMap, FxHasher};
use smallvec::SmallVec;

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
    buckets: FxHashMap<u64, SmallVec<[StrId; 1]>>,
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
            buckets: FxHashMap::default(),
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
            buckets: FxHashMap::with_capacity_and_hasher(strings, Default::default()),
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
        self.buckets.entry(hash).or_default().push(id);
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
        interner.buckets.entry(h_first).or_default().push(second);
        assert_eq!(interner.get("alpha"), Some(first));
        assert_eq!(interner.intern("alpha"), first);
        assert_eq!(interner.resolve(first), "alpha");
        assert_eq!(interner.resolve(second), "beta");
    }
}
