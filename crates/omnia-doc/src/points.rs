//! Geometric incidence only: wire bodies and labels never contribute ends.

use rustc_hash::FxHashMap;

use crate::{PartId, Pt, Rect, WireId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum End {
    Wire(WireId),
    Pin { part: PartId, pin: usize },
}

/// Compact 8-byte encoding of [`End`] so [`PointEnds`] fits up to 3 ends inline in
/// 32 bytes (down from 80 bytes for `SmallVec<[End; 3]>`), keeping the point map
/// L3-cache-resident while avoiding heap allocations up to 3-way junctions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PackedEnd(u64);

const PIN_BIT: u64 = 1u64 << 63;
const PIN_INDEX_MASK: u64 = (1u64 << 31) - 1;

impl PackedEnd {
    #[inline]
    fn pack(end: End) -> Self {
        match end {
            End::Wire(id) => Self(u64::from(id.0)),
            End::Pin { part, pin } => {
                assert!(
                    pin <= PIN_INDEX_MASK as usize,
                    "pin index exceeds 31-bit packed storage"
                );
                Self(PIN_BIT | (u64::from(part.0) << 31) | (pin as u64))
            }
        }
    }

    #[inline]
    fn unpack(self) -> End {
        if self.0 & PIN_BIT == 0 {
            End::Wire(WireId(self.0 as u32))
        } else {
            End::Pin {
                part: PartId(((self.0 & !PIN_BIT) >> 31) as u32),
                pin: (self.0 & PIN_INDEX_MASK) as usize,
            }
        }
    }
}

#[derive(Debug, Clone)]
enum PointEnds {
    Inline { items: [PackedEnd; 3], len: u8 },
    Heap(Vec<PackedEnd>),
}

impl Default for PointEnds {
    #[inline]
    fn default() -> Self {
        Self::Inline {
            items: [PackedEnd(0); 3],
            len: 0,
        }
    }
}

impl PointEnds {
    #[inline]
    fn as_slice(&self) -> &[PackedEnd] {
        match self {
            Self::Inline { items, len } => &items[..usize::from(*len)],
            Self::Heap(v) => v.as_slice(),
        }
    }

    #[inline]
    fn len(&self) -> usize {
        match self {
            Self::Inline { len, .. } => usize::from(*len),
            Self::Heap(v) => v.len(),
        }
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    fn push(&mut self, item: PackedEnd) {
        match self {
            Self::Inline { items, len } if usize::from(*len) < 3 => {
                items[usize::from(*len)] = item;
                *len += 1;
            }
            Self::Inline { items, .. } => {
                let mut v = Vec::with_capacity(4);
                v.extend_from_slice(items);
                v.push(item);
                *self = Self::Heap(v);
            }
            Self::Heap(v) => v.push(item),
        }
    }

    #[inline]
    fn retain_ne(&mut self, target: PackedEnd) {
        match self {
            Self::Inline { items, len } => {
                let mut write = 0usize;
                let n = usize::from(*len);
                for read in 0..n {
                    if items[read] != target {
                        items[write] = items[read];
                        write += 1;
                    }
                }
                *len = write as u8;
            }
            Self::Heap(v) => v.retain(|&e| e != target),
        }
    }
}

/// Read-only view of the ends stored at a point.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ends<'a>(&'a [PackedEnd]);

/// Zero-overhead iterator yielding unpacked [`End`]s from [`Ends`].
pub(crate) struct EndsIter<'a>(std::slice::Iter<'a, PackedEnd>);

impl Iterator for EndsIter<'_> {
    type Item = End;

    #[inline]
    fn next(&mut self) -> Option<End> {
        self.0.next().map(|e| e.unpack())
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl ExactSizeIterator for EndsIter<'_> {}

impl<'a> Ends<'a> {
    #[inline]
    pub(crate) fn len(self) -> usize {
        self.0.len()
    }

    #[inline]
    pub(crate) fn iter(self) -> EndsIter<'a> {
        EndsIter(self.0.iter())
    }

    #[inline]
    pub(crate) fn two_wires(self) -> Option<(WireId, WireId)> {
        if let [a, b] = *self.0
            && let (End::Wire(x), End::Wire(y)) = (a.unpack(), b.unpack())
        {
            Some((x, y))
        } else {
            None
        }
    }
}

impl<'a> IntoIterator for Ends<'a> {
    type Item = End;
    type IntoIter = EndsIter<'a>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct Points(FxHashMap<Pt, PointEnds>);

impl Points {
    pub(crate) fn insert(&mut self, at: Pt, end: End) {
        self.0.entry(at).or_default().push(PackedEnd::pack(end));
    }

    pub(crate) fn remove(&mut self, at: Pt, end: End) {
        let packed = PackedEnd::pack(end);
        if let Some(ends) = self.0.get_mut(&at) {
            ends.retain_ne(packed);
            if ends.is_empty() {
                self.0.remove(&at);
            }
        }
    }

    /// Wire ends and pins at `at`, in insertion order; empty when nothing is there.
    pub(crate) fn at(&self, at: Pt) -> Ends<'_> {
        Ends(self.0.get(&at).map_or(&[], PointEnds::as_slice))
    }

    pub(crate) fn junctions(&self, area: Rect, out: &mut Vec<Pt>) {
        out.clear();
        out.extend(
            self.0
                .iter()
                .filter_map(|(&at, ends)| (ends.len() >= 3 && area.contains(at)).then_some(at)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_end_round_trips_wire_and_pin_extremes() {
        assert_eq!(std::mem::size_of::<PackedEnd>(), 8);
        assert_eq!(std::mem::size_of::<PointEnds>(), 32);
        for end in [
            End::Wire(WireId(0)),
            End::Wire(WireId(u32::MAX)),
            End::Pin {
                part: PartId(0),
                pin: 0,
            },
            End::Pin {
                part: PartId(u32::MAX),
                pin: 0,
            },
            End::Pin {
                part: PartId(12345),
                pin: (1 << 31) - 1,
            },
        ] {
            assert_eq!(PackedEnd::pack(end).unpack(), end);
        }
        // Verify inline -> heap spill and removal preserve order.
        let mut pts = Points::default();
        let p = Pt { x: 0, y: 0 };
        for i in 0..5 {
            pts.insert(p, End::Wire(WireId(i)));
        }
        let got: Vec<_> = pts.at(p).iter().collect();
        assert_eq!(got.len(), 5);
        pts.remove(p, End::Wire(WireId(2)));
        let got: Vec<_> = pts.at(p).iter().collect();
        assert_eq!(
            got,
            vec![
                End::Wire(WireId(0)),
                End::Wire(WireId(1)),
                End::Wire(WireId(3)),
                End::Wire(WireId(4)),
            ]
        );
    }
}
