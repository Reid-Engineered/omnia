//! World-space geometry primitives in integer nanometres (D1, K1, K2).

use std::fmt;
use std::ops::{Add, Neg, Sub};

/// K1: sheet coordinate limit on each axis (`±1,000,000,000 nm` = `±1 m`).
pub const SHEET_LIMIT_NM: i32 = 1_000_000_000;

/// K2: default schematic grid step (`2,540,000 nm` = `2.54 mm` = `100 mil`).
pub const GRID_NM: i32 = 2_540_000;

/// K2: pin lattice step (`1,270,000 nm` = `1.27 mm` = `50 mil`).
pub const PIN_LATTICE_NM: i32 = 1_270_000;

/// Errors returned by checked geometry constructors and bounded operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeomError {
    /// One or both coordinates lie outside `[-SHEET_LIMIT_NM, SHEET_LIMIT_NM]`.
    OutOfBounds { x: i32, y: i32 },
    /// A calculation exceeded the representable `i32` / K1 coordinate range.
    Overflow,
    /// A rectangle has `min.x > max.x` or `min.y > max.y`.
    InvertedRect { min: Pt, max: Pt },
}

impl fmt::Display for GeomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds { x, y } => write!(
                f,
                "point ({x}, {y}) nm lies outside sheet limit ±{SHEET_LIMIT_NM} nm"
            ),
            Self::Overflow => write!(f, "coordinate calculation overflowed i32"),
            Self::InvertedRect { min, max } => write!(
                f,
                "inverted rectangle: min ({}, {}) exceeds max ({}, {})",
                min.x, min.y, max.x, max.y
            ),
        }
    }
}

impl std::error::Error for GeomError {}

/// A point or displacement in world nanometres (`i32`, D1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Pt {
    pub x: i32,
    pub y: i32,
}

impl Pt {
    /// Origin `(0, 0)`.
    pub const ZERO: Self = Self { x: 0, y: 0 };

    /// Returns `true` if a single coordinate lies within `[-SHEET_LIMIT_NM, SHEET_LIMIT_NM]`.
    #[inline]
    pub const fn in_bounds_coord(v: i32) -> bool {
        v >= -SHEET_LIMIT_NM && v <= SHEET_LIMIT_NM
    }

    /// Checked constructor rejecting coordinates outside K1 (`±SHEET_LIMIT_NM`).
    #[inline]
    pub const fn new(x: i32, y: i32) -> Result<Self, GeomError> {
        if Self::in_bounds_coord(x) && Self::in_bounds_coord(y) {
            Ok(Self { x, y })
        } else {
            Err(GeomError::OutOfBounds { x, y })
        }
    }

    /// Returns `true` if both coordinates lie within K1 (`±SHEET_LIMIT_NM`).
    #[inline]
    pub const fn is_in_bounds(self) -> bool {
        Self::in_bounds_coord(self.x) && Self::in_bounds_coord(self.y)
    }

    /// Checked addition, rejecting results outside K1.
    #[inline]
    pub fn checked_add(self, rhs: Self) -> Result<Self, GeomError> {
        let x = i64::from(self.x) + i64::from(rhs.x);
        let y = i64::from(self.y) + i64::from(rhs.y);
        pt_from_i64(x, y)
    }

    /// Checked subtraction, rejecting results outside K1.
    #[inline]
    pub fn checked_sub(self, rhs: Self) -> Result<Self, GeomError> {
        let x = i64::from(self.x) - i64::from(rhs.x);
        let y = i64::from(self.y) - i64::from(rhs.y);
        pt_from_i64(x, y)
    }

    /// Checked negation, rejecting results outside K1.
    #[inline]
    pub fn checked_neg(self) -> Result<Self, GeomError> {
        let x = -i64::from(self.x);
        let y = -i64::from(self.y);
        pt_from_i64(x, y)
    }

    /// Squared Euclidean distance in `i64` (`nm^2`).
    ///
    /// For any two in-range points, `(2e9)^2 + (2e9)^2 = 8e18 < i64::MAX`, so this never overflows.
    #[inline]
    pub fn distance_sq(self, other: Self) -> i64 {
        let dx = i64::from(self.x) - i64::from(other.x);
        let dy = i64::from(self.y) - i64::from(other.y);
        dx * dx + dy * dy
    }

    /// Manhattan (`L1`) distance in `i64` nanometres.
    #[inline]
    pub fn manhattan_distance(self, other: Self) -> i64 {
        let dx = (i64::from(self.x) - i64::from(other.x)).abs();
        let dy = (i64::from(self.y) - i64::from(other.y)).abs();
        dx + dy
    }

    /// 2D dot product in `i64`.
    #[inline]
    pub fn dot(self, other: Self) -> i64 {
        i64::from(self.x) * i64::from(other.x) + i64::from(self.y) * i64::from(other.y)
    }

    /// 2D scalar cross product (`x1*y2 - y1*x2`) in `i64`.
    #[inline]
    pub fn cross(self, other: Self) -> i64 {
        i64::from(self.x) * i64::from(other.y) - i64::from(self.y) * i64::from(other.x)
    }
}

impl Add for Pt {
    type Output = Self;

    /// Adds two points/displacements. By K1, any two in-range points sum within `i32`.
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl Sub for Pt {
    type Output = Self;

    /// Subtracts two points/displacements. By K1, any two in-range points differ within `i32`.
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl Neg for Pt {
    type Output = Self;

    /// Negates a point/displacement. By K1, any in-range point negates within `i32`.
    #[inline]
    fn neg(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}

#[inline]
fn pt_from_i64(x: i64, y: i64) -> Result<Pt, GeomError> {
    let x = i32::try_from(x).map_err(|_| GeomError::Overflow)?;
    let y = i32::try_from(y).map_err(|_| GeomError::Overflow)?;
    Pt::new(x, y)
}

/// An axis-aligned rectangle in world nanometres (`min <= max`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    pub min: Pt,
    pub max: Pt,
}

impl Rect {
    /// Checked constructor rejecting out-of-K1 coordinates and inverted bounds (`min > max`).
    #[inline]
    pub const fn new(min: Pt, max: Pt) -> Result<Self, GeomError> {
        if !min.is_in_bounds() {
            return Err(GeomError::OutOfBounds { x: min.x, y: min.y });
        }
        if !max.is_in_bounds() {
            return Err(GeomError::OutOfBounds { x: max.x, y: max.y });
        }
        if min.x > max.x || min.y > max.y {
            return Err(GeomError::InvertedRect { min, max });
        }
        Ok(Self { min, max })
    }

    /// Checked constructor from two arbitrary corner points, rejecting coordinates outside K1.
    #[inline]
    pub fn from_points(a: Pt, b: Pt) -> Result<Self, GeomError> {
        if !a.is_in_bounds() {
            return Err(GeomError::OutOfBounds { x: a.x, y: a.y });
        }
        if !b.is_in_bounds() {
            return Err(GeomError::OutOfBounds { x: b.x, y: b.y });
        }
        Ok(Self {
            min: Pt {
                x: a.x.min(b.x),
                y: a.y.min(b.y),
            },
            max: Pt {
                x: a.x.max(b.x),
                y: a.y.max(b.y),
            },
        })
    }

    /// Returns `true` if `min <= max` and both corners lie within K1.
    #[inline]
    pub const fn is_in_bounds(self) -> bool {
        self.min.is_in_bounds()
            && self.max.is_in_bounds()
            && self.min.x <= self.max.x
            && self.min.y <= self.max.y
    }

    /// Width in nanometres (`i64`, non-negative for valid rectangles).
    #[inline]
    pub const fn width(self) -> i64 {
        self.max.x as i64 - self.min.x as i64
    }

    /// Height in nanometres (`i64`, non-negative for valid rectangles).
    #[inline]
    pub const fn height(self) -> i64 {
        self.max.y as i64 - self.min.y as i64
    }

    /// Area in `nm^2` (`i64`, `<= 4e18 < i64::MAX` for in-range rectangles).
    #[inline]
    pub const fn area(self) -> i64 {
        self.width() * self.height()
    }

    /// Midpoint of the rectangle, rounded toward zero on odd extents.
    #[inline]
    pub const fn center(self) -> Pt {
        Pt {
            x: ((self.min.x as i64 + self.max.x as i64) / 2) as i32,
            y: ((self.min.y as i64 + self.max.y as i64) / 2) as i32,
        }
    }

    /// Returns `true` if `p` lies inside or on the boundary of `self`.
    #[inline]
    pub const fn contains(self, p: Pt) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    /// Returns `true` if `other` is completely contained within `self` (boundaries inclusive).
    #[inline]
    pub const fn contains_rect(self, other: Self) -> bool {
        other.min.x >= self.min.x
            && other.max.x <= self.max.x
            && other.min.y >= self.min.y
            && other.max.y <= self.max.y
    }

    /// Returns `true` if `self` and `other` overlap or touch on an edge/corner.
    #[inline]
    pub const fn intersects(self, other: Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }

    /// Smallest axis-aligned rectangle enclosing both `self` and `other`.
    #[inline]
    pub fn union(self, other: Self) -> Self {
        Self {
            min: Pt {
                x: self.min.x.min(other.min.x),
                y: self.min.y.min(other.min.y),
            },
            max: Pt {
                x: self.max.x.max(other.max.x),
                y: self.max.y.max(other.max.y),
            },
        }
    }

    /// Overlapping rectangle of `self` and `other`, or `None` if disjoint.
    #[inline]
    pub fn intersection(self, other: Self) -> Option<Self> {
        let min = Pt {
            x: self.min.x.max(other.min.x),
            y: self.min.y.max(other.min.y),
        };
        let max = Pt {
            x: self.max.x.min(other.max.x),
            y: self.max.y.min(other.max.y),
        };
        (min.x <= max.x && min.y <= max.y).then_some(Self { min, max })
    }

    /// Expands (or shrinks if `margin < 0`) each side by `margin` nm, checking K1 and validity.
    #[inline]
    pub fn expand(self, margin: i32) -> Result<Self, GeomError> {
        let m = i64::from(margin);
        let min = pt_from_i64(i64::from(self.min.x) - m, i64::from(self.min.y) - m)?;
        let max = pt_from_i64(i64::from(self.max.x) + m, i64::from(self.max.y) + m)?;
        Self::new(min, max)
    }

    /// Translates `self` by `delta`, rejecting results outside K1.
    #[inline]
    pub fn translate(self, delta: Pt) -> Result<Self, GeomError> {
        let min = self.min.checked_add(delta)?;
        let max = self.max.checked_add(delta)?;
        Self::new(min, max)
    }

    /// Reorients `self` around `pivot`, returning the normalized bounding rectangle.
    #[inline]
    pub fn checked_reorient_around(self, orient: Orient, pivot: Pt) -> Result<Self, GeomError> {
        let a = orient.checked_apply_around(self.min, pivot)?;
        let b = orient.checked_apply_around(self.max, pivot)?;
        Self::from_points(a, b)
    }
}

/// The 8 orthogonal orientations (dihedral group D4: 4 quarter-turn rotations × optional X-mirror).
///
/// Representation: bits `0..2` store the quarter-turn rotation `rot` in `0..4`, and bit `2` (`4`)
/// stores `flip_x`. Applying an orientation mirrors X first (`(-x, y)` when `flip_x` is set),
/// then applies `rot` quarter-turns of `(x, y) -> (-y, x)` (90° clockwise in screen Y-down
/// coordinates, matching `symbols.mjs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Orient(u8);

impl Orient {
    /// Identity: `(x, y) -> (x, y)`.
    pub const IDENTITY: Self = Self(0);
    /// 90° quarter-turn `(x, y) -> (-y, x)`.
    pub const ROT_90: Self = Self(1);
    /// 180° half-turn `(x, y) -> (-x, -y)`.
    pub const ROT_180: Self = Self(2);
    /// 270° three-quarter turn `(x, y) -> (y, -x)`.
    pub const ROT_270: Self = Self(3);
    /// Mirror across the Y axis (negate X): `(x, y) -> (-x, y)`.
    pub const FLIP_X: Self = Self(4);
    /// Mirror X then 90° turn: `(x, y) -> (-y, -x)`.
    pub const FLIP_X_ROT_90: Self = Self(5);
    /// Mirror across the X axis (negate Y): `(x, y) -> (x, -y)`.
    pub const FLIP_Y: Self = Self(6);
    /// Mirror X then 270° turn (transpose): `(x, y) -> (y, x)`.
    pub const FLIP_X_ROT_270: Self = Self(7);

    /// All 8 orientations in canonical index order `0..8`.
    pub const ALL: [Self; 8] = [
        Self::IDENTITY,
        Self::ROT_90,
        Self::ROT_180,
        Self::ROT_270,
        Self::FLIP_X,
        Self::FLIP_X_ROT_90,
        Self::FLIP_Y,
        Self::FLIP_X_ROT_270,
    ];

    /// Constructs an orientation from quarter-turn rotation count (mod 4) and X-mirror flag.
    #[inline]
    pub const fn from_rot_flip(quarter_turns: u8, flip_x: bool) -> Self {
        Self((quarter_turns & 3) | ((flip_x as u8) << 2))
    }

    /// Constructs an orientation from its raw `0..8` discriminant, if in range.
    #[inline]
    pub const fn from_raw(raw: u8) -> Option<Self> {
        if raw < 8 { Some(Self(raw)) } else { None }
    }

    /// Returns the raw `0..8` encoding.
    #[inline]
    pub const fn to_raw(self) -> u8 {
        self.0 & 7
    }

    /// Number of quarter-turns (`0..4`) applied after any X-mirror.
    #[inline]
    pub const fn quarter_turns(self) -> u8 {
        self.0 & 3
    }

    /// Returns `true` if this orientation includes a reflection.
    #[inline]
    pub const fn is_flipped(self) -> bool {
        (self.0 & 4) != 0
    }

    /// Applies this orientation about the origin to `p`.
    #[inline]
    pub const fn apply(self, p: Pt) -> Pt {
        match self.0 & 7 {
            0 => p,
            1 => Pt { x: -p.y, y: p.x },
            2 => Pt { x: -p.x, y: -p.y },
            3 => Pt { x: p.y, y: -p.x },
            4 => Pt { x: -p.x, y: p.y },
            5 => Pt { x: -p.y, y: -p.x },
            6 => Pt { x: p.x, y: -p.y },
            _ => Pt { x: p.y, y: p.x },
        }
    }

    /// Applies this orientation to `p` around `pivot`, validating the result against K1.
    #[inline]
    pub fn checked_apply_around(self, p: Pt, pivot: Pt) -> Result<Pt, GeomError> {
        let dx = i64::from(p.x) - i64::from(pivot.x);
        let dy = i64::from(p.y) - i64::from(pivot.y);
        let (rx, ry) = match self.0 & 7 {
            0 => (dx, dy),
            1 => (-dy, dx),
            2 => (-dx, -dy),
            3 => (dy, -dx),
            4 => (-dx, dy),
            5 => (-dy, -dx),
            6 => (dx, -dy),
            _ => (dy, dx),
        };
        pt_from_i64(i64::from(pivot.x) + rx, i64::from(pivot.y) + ry)
    }

    /// Composes `self` followed by `next`: `self.then(next).apply(p) == next.apply(self.apply(p))`.
    #[inline]
    pub const fn then(self, next: Self) -> Self {
        let r1 = self.0 & 3;
        let f1 = (self.0 & 4) != 0;
        let r2 = next.0 & 3;
        let f2 = (next.0 & 4) != 0;
        if f2 {
            Self::from_rot_flip((r2 + 4 - r1) & 3, !f1)
        } else {
            Self::from_rot_flip((r2 + r1) & 3, f1)
        }
    }

    /// Group inverse: `self.then(self.inverse()) == IDENTITY == self.inverse().then(self)`.
    #[inline]
    pub const fn inverse(self) -> Self {
        if self.is_flipped() {
            self
        } else {
            Self::from_rot_flip((4 - self.quarter_turns()) & 3, false)
        }
    }
}

/// Snaps a single coordinate to the nearest multiple of `step` (`step > 0`).
///
/// Exact midpoints round away from zero so `snap_to` commutes with all 8 `Orient` transforms.
#[inline]
fn snap_coord(v: i32, step: i32) -> i32 {
    assert!(step > 0, "snap step must be positive, got {step}");
    let v64 = i64::from(v);
    let s64 = i64::from(step);
    let half = s64 / 2;
    let snapped = if v64 >= 0 {
        ((v64 + half) / s64) * s64
    } else {
        ((v64 - half) / s64) * s64
    };
    i32::try_from(snapped).expect("snapped coordinate fits in i32")
}

/// Snaps `p` to the nearest point on a square lattice of spacing `step` nanometres (`step > 0`).
#[inline]
pub fn snap_to(p: Pt, step: i32) -> Pt {
    Pt {
        x: snap_coord(p.x, step),
        y: snap_coord(p.y, step),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn in_range_coord() -> impl Strategy<Value = i32> {
        -SHEET_LIMIT_NM..=SHEET_LIMIT_NM
    }

    fn in_range_pt() -> impl Strategy<Value = Pt> {
        (in_range_coord(), in_range_coord()).prop_map(|(x, y)| Pt::new(x, y).unwrap())
    }

    fn in_range_rect() -> impl Strategy<Value = Rect> {
        (in_range_pt(), in_range_pt()).prop_map(|(a, b)| Rect::from_points(a, b).unwrap())
    }

    fn any_orient() -> impl Strategy<Value = Orient> {
        (0u8..8).prop_map(|i| Orient::ALL[i as usize])
    }

    #[test]
    fn checked_constructors_reject_outside_k1() {
        assert_eq!(
            Pt::new(SHEET_LIMIT_NM, -SHEET_LIMIT_NM),
            Ok(Pt {
                x: SHEET_LIMIT_NM,
                y: -SHEET_LIMIT_NM,
            })
        );
        assert!(matches!(
            Pt::new(SHEET_LIMIT_NM + 1, 0),
            Err(GeomError::OutOfBounds { .. })
        ));
        assert!(matches!(
            Pt::new(0, -SHEET_LIMIT_NM - 1),
            Err(GeomError::OutOfBounds { .. })
        ));
        assert!(matches!(
            Pt::new(i32::MAX, i32::MIN),
            Err(GeomError::OutOfBounds { .. })
        ));

        let ok_min = Pt::new(-100, -200).unwrap();
        let ok_max = Pt::new(100, 200).unwrap();
        assert!(Rect::new(ok_min, ok_max).is_ok());
        assert!(matches!(
            Rect::new(ok_max, ok_min),
            Err(GeomError::InvertedRect { .. })
        ));

        let bad_pt = Pt {
            x: SHEET_LIMIT_NM + 1,
            y: 0,
        };
        assert!(matches!(
            Rect::new(Pt::ZERO, bad_pt),
            Err(GeomError::OutOfBounds { .. })
        ));
        assert!(matches!(
            Rect::from_points(bad_pt, Pt::ZERO),
            Err(GeomError::OutOfBounds { .. })
        ));
        assert!(!bad_pt.is_in_bounds());
        assert!(
            !Rect {
                min: Pt::ZERO,
                max: bad_pt
            }
            .is_in_bounds()
        );
    }

    #[test]
    fn each_orientation_applied_to_a_point() {
        let p = Pt::new(3_000_000, 5_000_000).unwrap();
        assert_eq!(
            Orient::IDENTITY.apply(p),
            Pt {
                x: 3_000_000,
                y: 5_000_000
            }
        );
        assert_eq!(
            Orient::ROT_90.apply(p),
            Pt {
                x: -5_000_000,
                y: 3_000_000
            }
        );
        assert_eq!(
            Orient::ROT_180.apply(p),
            Pt {
                x: -3_000_000,
                y: -5_000_000
            }
        );
        assert_eq!(
            Orient::ROT_270.apply(p),
            Pt {
                x: 5_000_000,
                y: -3_000_000
            }
        );
        assert_eq!(
            Orient::FLIP_X.apply(p),
            Pt {
                x: -3_000_000,
                y: 5_000_000
            }
        );
        assert_eq!(
            Orient::FLIP_X_ROT_90.apply(p),
            Pt {
                x: -5_000_000,
                y: -3_000_000,
            }
        );
        assert_eq!(
            Orient::FLIP_Y.apply(p),
            Pt {
                x: 3_000_000,
                y: -5_000_000
            }
        );
        assert_eq!(
            Orient::FLIP_X_ROT_270.apply(p),
            Pt {
                x: 5_000_000,
                y: 3_000_000,
            }
        );

        // Verify all 8 produce distinct images for an asymmetric point.
        let images: std::collections::HashSet<Pt> =
            Orient::ALL.iter().map(|o| o.apply(p)).collect();
        assert_eq!(images.len(), 8);
    }

    #[test]
    fn snap_to_known_values_and_ties() {
        assert_eq!(
            snap_to(Pt::new(1_270_000, -1_270_000).unwrap(), GRID_NM),
            Pt {
                x: GRID_NM,
                y: -GRID_NM,
            }
        );
        assert_eq!(
            snap_to(Pt::new(1_269_999, -1_269_999).unwrap(), GRID_NM),
            Pt::ZERO
        );
        // Odd step: half is 5, distance <= 5.
        assert_eq!(snap_to(Pt { x: 5, y: -6 }, 11), Pt { x: 0, y: -11 });
        assert_eq!(snap_to(Pt { x: 6, y: -5 }, 11), Pt { x: 11, y: 0 });
        // Step = 1 is exact.
        assert_eq!(snap_to(Pt { x: -42, y: 99 }, 1), Pt { x: -42, y: 99 });
        // Snapping a boundary point near K1 does not overflow i32.
        assert_eq!(
            snap_to(Pt::new(SHEET_LIMIT_NM, -SHEET_LIMIT_NM).unwrap(), GRID_NM),
            Pt {
                x: 1_000_760_000,
                y: -1_000_760_000,
            }
        );
    }

    proptest! {
        #[test]
        fn orient_forms_a_group(
            a in any_orient(),
            b in any_orient(),
            c in any_orient(),
            p in in_range_pt(),
        ) {
            // Closure and composition semantics.
            let ab = a.then(b);
            prop_assert!(Orient::ALL.contains(&ab));
            prop_assert_eq!(ab.apply(p), b.apply(a.apply(p)));

            // Identity.
            prop_assert_eq!(Orient::IDENTITY.then(a), a);
            prop_assert_eq!(a.then(Orient::IDENTITY), a);
            prop_assert_eq!(Orient::IDENTITY.apply(p), p);

            // Inverse.
            let inv = a.inverse();
            prop_assert!(Orient::ALL.contains(&inv));
            prop_assert_eq!(a.then(inv), Orient::IDENTITY);
            prop_assert_eq!(inv.then(a), Orient::IDENTITY);
            prop_assert_eq!(inv.apply(a.apply(p)), p);

            // Associativity.
            prop_assert_eq!(a.then(b).then(c), a.then(b.then(c)));
        }

        #[test]
        fn snap_to_inv9_and_half_step_bound(
            p in in_range_pt(),
            step in 1i32..=SHEET_LIMIT_NM,
            o in any_orient(),
        ) {
            let s1 = snap_to(p, step);
            let s2 = snap_to(s1, step);

            // INV9: idempotence.
            prop_assert_eq!(s2, s1);

            // Never moves a point more than half a step on either axis.
            let dx = (i64::from(s1.x) - i64::from(p.x)).abs();
            let dy = (i64::from(s1.y) - i64::from(p.y)).abs();
            let half = i64::from(step / 2);
            prop_assert!(dx <= half, "dx={dx} > half={half}");
            prop_assert!(dy <= half, "dy={dy} > half={half}");

            // Exact grid alignment.
            prop_assert_eq!(i64::from(s1.x) % i64::from(step), 0);
            prop_assert_eq!(i64::from(s1.y) % i64::from(step), 0);

            // Equivariance with D4 orientations.
            prop_assert_eq!(snap_to(o.apply(p), step), o.apply(s1));
        }

        #[test]
        fn no_overflow_on_in_range_points(
            a in in_range_pt(),
            b in in_range_pt(),
            r1 in in_range_rect(),
            r2 in in_range_rect(),
            o in any_orient(),
            step in 1i32..=i32::MAX,
            margin in in_range_coord(),
        ) {
            // Point arithmetic and products.
            let sum = a + b;
            let diff = a - b;
            let neg = -a;
            prop_assert_eq!(sum - b, a);
            prop_assert_eq!(diff + b, a);
            prop_assert_eq!(-neg, a);
            let _ = a.checked_add(b);
            let _ = a.checked_sub(b);
            let _ = a.checked_neg();
            prop_assert!(a.distance_sq(b) >= 0);
            prop_assert!(a.manhattan_distance(b) >= 0);
            let _ = a.dot(b);
            let _ = a.cross(b);

            // Orientation transforms.
            let oa = o.apply(a);
            prop_assert!(oa.is_in_bounds());
            let _ = o.checked_apply_around(a, b);

            // Snap at full positive step range.
            let sa = snap_to(a, step);
            prop_assert_eq!(snap_to(sa, step), sa);

            // Rectangle operations.
            prop_assert!(r1.width() >= 0);
            prop_assert!(r1.height() >= 0);
            prop_assert!(r1.area() >= 0);
            let center = r1.center();
            prop_assert!(center.is_in_bounds());
            prop_assert!(r1.contains(center));
            let _ = r1.contains(a);
            let _ = r1.contains_rect(r2);
            let _ = r1.intersects(r2);
            let u = r1.union(r2);
            prop_assert!(u.is_in_bounds());
            prop_assert!(u.contains_rect(r1));
            prop_assert!(u.contains_rect(r2));
            if let Some(inter) = r1.intersection(r2) {
                prop_assert!(inter.is_in_bounds());
                prop_assert!(r1.contains_rect(inter));
                prop_assert!(r2.contains_rect(inter));
            }
            let _ = r1.expand(margin);
            let _ = r1.translate(b);
            let _ = r1.checked_reorient_around(o, b);
        }
    }
}
