//! Id, point, bounds, orientation and text-field codecs (AD12, AK8, AQ5).
//!
//! Every codec has exactly one spelling per value, so the same document and request give the
//! same bytes (AD8). Decoding is strict: ids are `kind:index` in canonical decimal with no sign,
//! no leading zero and no whitespace; points are `[x, y]` integer nanometres; orientations are
//! the names of the `Orient` constants in `omnia_doc::geom`.

use std::{cmp::Ordering, fmt, str::FromStr};

use omnia_doc::{ItemId, LabelId, Orient, PartId, Pt, Rect, TextField, WireId};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// Why a string is not a valid id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidId;

impl fmt::Display for InvalidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a canonical id")
    }
}

impl std::error::Error for InvalidId {}

/// Parses canonical decimal: `0`, or a non-zero digit followed by digits; no sign, no spaces.
fn parse_decimal(s: &str) -> Option<u64> {
    let bytes = s.as_bytes();
    let canonical = match bytes {
        [] => false,
        [b'0'] => true,
        [first, rest @ ..] => (b'1'..=b'9').contains(first) && rest.iter().all(u8::is_ascii_digit),
    };
    if canonical { s.parse().ok() } else { None }
}

fn parse_u32(s: &str) -> Option<u32> {
    parse_decimal(s).and_then(|v| u32::try_from(v).ok())
}

/// Serializes through `Display` and deserializes through `FromStr`, as one JSON string.
macro_rules! string_codec {
    ($name:ident, $expecting:literal) => {
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                text.parse()
                    .map_err(|_| de::Error::invalid_value(de::Unexpected::Str(&text), &$expecting))
            }
        }
    };
}

macro_rules! typed_ref {
    ($(#[$meta:meta])* $name:ident, $id:ident, $prefix:literal, $expecting:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u32);

        impl From<$id> for $name {
            fn from(id: $id) -> Self {
                Self(id.0)
            }
        }

        impl From<$name> for $id {
            fn from(id: $name) -> Self {
                $id(id.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, ":{}"), self.0)
            }
        }

        impl FromStr for $name {
            type Err = InvalidId;

            fn from_str(s: &str) -> Result<Self, InvalidId> {
                s.strip_prefix(concat!($prefix, ":"))
                    .and_then(parse_u32)
                    .map(Self)
                    .ok_or(InvalidId)
            }
        }

        string_codec!($name, $expecting);
    };
}

typed_ref!(
    /// A part id on the wire: `part:3` (AQ5).
    PartRef, PartId, "part", "a part id such as `part:3`"
);
typed_ref!(
    /// A wire id on the wire: `wire:7` (AQ5).
    WireRef, WireId, "wire", "a wire id such as `wire:7`"
);
typed_ref!(
    /// A label id on the wire: `label:2` (AQ5).
    LabelRef, LabelId, "label", "a label id such as `label:2`"
);

/// Any item id on the wire: `part:3`, `wire:7` or `label:2` (AD12, AQ5).
///
/// Ordered by kind (part, wire, label) and then by index, the order responses use (AD8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemRef(pub ItemId);

impl ItemRef {
    const fn key(self) -> (u8, u32) {
        match self.0 {
            ItemId::Part(id) => (0, id.0),
            ItemId::Wire(id) => (1, id.0),
            ItemId::Label(id) => (2, id.0),
        }
    }
}

impl Ord for ItemRef {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

impl PartialOrd for ItemRef {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl From<ItemId> for ItemRef {
    fn from(id: ItemId) -> Self {
        Self(id)
    }
}

impl From<ItemRef> for ItemId {
    fn from(id: ItemRef) -> Self {
        id.0
    }
}

impl fmt::Display for ItemRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ItemId::Part(id) => PartRef::from(id).fmt(f),
            ItemId::Wire(id) => WireRef::from(id).fmt(f),
            ItemId::Label(id) => LabelRef::from(id).fmt(f),
        }
    }
}

impl FromStr for ItemRef {
    type Err = InvalidId;

    fn from_str(s: &str) -> Result<Self, InvalidId> {
        let (kind, _) = s.split_once(':').ok_or(InvalidId)?;
        let id = match kind {
            "part" => ItemId::Part(s.parse::<PartRef>()?.into()),
            "wire" => ItemId::Wire(s.parse::<WireRef>()?.into()),
            "label" => ItemId::Label(s.parse::<LabelRef>()?.into()),
            _ => return Err(InvalidId),
        };
        Ok(Self(id))
    }
}

string_codec!(
    ItemRef,
    "an item id such as `part:3`, `wire:7` or `label:2`"
);

macro_rules! numbered_handle {
    ($(#[$meta:meta])* $name:ident, $prefix:literal, $expecting:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "{}"), self.0)
            }
        }

        impl FromStr for $name {
            type Err = InvalidId;

            fn from_str(s: &str) -> Result<Self, InvalidId> {
                s.strip_prefix($prefix)
                    .and_then(parse_decimal)
                    .map(Self)
                    .ok_or(InvalidId)
            }
        }

        string_codec!($name, $expecting);
    };
}

numbered_handle!(
    /// A session's handle to an open document: `d1` (mcp-plan.md section 9.1). Never reused
    /// within a session (11.1).
    DocId, "d", "a document id such as `d1`"
);
numbered_handle!(
    /// A job handle: `j1` (section 11.5).
    JobId, "j", "a job id such as `j1`"
);

/// The 64-bit preview hash of section 11.3, spelled `pv-` and 16 lowercase hex digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PreviewId(pub u64);

impl fmt::Display for PreviewId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pv-{:016x}", self.0)
    }
}

impl FromStr for PreviewId {
    type Err = InvalidId;

    fn from_str(s: &str) -> Result<Self, InvalidId> {
        let hex = s.strip_prefix("pv-").ok_or(InvalidId)?;
        let canonical = hex.len() == 16
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !canonical {
            return Err(InvalidId);
        }
        u64::from_str_radix(hex, 16)
            .map(Self)
            .map_err(|_| InvalidId)
    }
}

string_codec!(PreviewId, "a preview id such as `pv-5f0c1d2e3a4b5c6d`");

/// A point or displacement on the wire: `[x, y]` in integer nanometres (AK8, D1).
///
/// The codec accepts any `i32` pair. Whether a position lies inside K1 is the document's
/// decision (`out_of_bounds`), not the codec's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Point(pub Pt);

impl From<Pt> for Point {
    fn from(p: Pt) -> Self {
        Self(p)
    }
}

impl From<Point> for Pt {
    fn from(p: Point) -> Self {
        p.0
    }
}

impl Serialize for Point {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        [self.0.x, self.0.y].serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Point {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let [x, y] = <[i32; 2]>::deserialize(deserializer)?;
        Ok(Self(Pt { x, y }))
    }
}

/// An axis-aligned rectangle on the wire: `{"min": [x, y], "max": [x, y]}` (section 9.3).
///
/// Decoding requires `min <= max` on both axes and both corners inside K1, as `Rect::new` does.
/// The field is private, so every `Bounds` holds such a rectangle and decodes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Bounds(Rect);

impl Bounds {
    /// `None` unless `min <= max` on both axes and both corners lie inside K1.
    pub fn new(rect: Rect) -> Option<Self> {
        Rect::new(rect.min, rect.max).ok().map(Self)
    }

    /// The rectangle.
    pub const fn rect(self) -> Rect {
        self.0
    }
}

impl From<Bounds> for Rect {
    fn from(b: Bounds) -> Self {
        b.0
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundsWire {
    min: Point,
    max: Point,
}

impl Serialize for Bounds {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        BoundsWire {
            min: self.0.min.into(),
            max: self.0.max.into(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Bounds {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = BoundsWire::deserialize(deserializer)?;
        Rect::new(wire.min.0, wire.max.0)
            .map(Self)
            .map_err(de::Error::custom)
    }
}

/// The eight orientation names of AD12, indexed by `Orient::to_raw`.
pub const ORIENT_NAMES: [&str; 8] = [
    "identity",
    "rot_90",
    "rot_180",
    "rot_270",
    "flip_x",
    "flip_x_rot_90",
    "flip_y",
    "flip_x_rot_270",
];

/// An orientation on the wire, named after its `Orient` constant (AD12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct OrientName(pub Orient);

impl OrientName {
    /// The AD12 name of this orientation.
    pub const fn as_str(self) -> &'static str {
        ORIENT_NAMES[self.0.to_raw() as usize]
    }
}

impl From<Orient> for OrientName {
    fn from(o: Orient) -> Self {
        Self(o)
    }
}

impl From<OrientName> for Orient {
    fn from(o: OrientName) -> Self {
        o.0
    }
}

impl fmt::Display for OrientName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OrientName {
    type Err = InvalidId;

    fn from_str(s: &str) -> Result<Self, InvalidId> {
        Orient::ALL
            .into_iter()
            .find(|o| ORIENT_NAMES[o.to_raw() as usize] == s)
            .map(Self)
            .ok_or(InvalidId)
    }
}

string_codec!(
    OrientName,
    "an orientation: identity, rot_90, rot_180, rot_270, flip_x, flip_x_rot_90, flip_y or flip_x_rot_270"
);

/// The text field of a `set_text` operation (section 9.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextFieldName(pub TextField);

/// Wire names of `TextField`, in declaration order.
pub const TEXT_FIELD_NAMES: [&str; 3] = ["designator", "value", "label"];

impl TextFieldName {
    /// The wire name of this field. Exhaustive: a new `TextField` needs a name.
    pub const fn as_str(self) -> &'static str {
        match self.0 {
            TextField::Designator => "designator",
            TextField::Value => "value",
            TextField::Label => "label",
        }
    }
}

impl fmt::Display for TextFieldName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TextFieldName {
    type Err = InvalidId;

    fn from_str(s: &str) -> Result<Self, InvalidId> {
        let field = match s {
            "designator" => TextField::Designator,
            "value" => TextField::Value,
            "label" => TextField::Label,
            _ => return Err(InvalidId),
        };
        Ok(Self(field))
    }
}

string_codec!(TextFieldName, "a text field: designator, value or label");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ad12_orientation_names_follow_the_orient_constants() {
        let named = [
            (Orient::IDENTITY, "identity"),
            (Orient::ROT_90, "rot_90"),
            (Orient::ROT_180, "rot_180"),
            (Orient::ROT_270, "rot_270"),
            (Orient::FLIP_X, "flip_x"),
            (Orient::FLIP_X_ROT_90, "flip_x_rot_90"),
            (Orient::FLIP_Y, "flip_y"),
            (Orient::FLIP_X_ROT_270, "flip_x_rot_270"),
        ];
        for (orient, name) in named {
            assert_eq!(OrientName(orient).as_str(), name);
            assert_eq!(name.parse::<OrientName>(), Ok(OrientName(orient)));
        }
        assert_eq!("rot_45".parse::<OrientName>(), Err(InvalidId));
    }

    #[test]
    fn aq5_ids_are_canonical() {
        assert_eq!(
            "part:3".parse::<ItemRef>(),
            Ok(ItemRef(ItemId::Part(PartId(3))))
        );
        assert_eq!(
            "label:0".parse::<ItemRef>(),
            Ok(ItemRef(ItemId::Label(LabelId(0))))
        );
        assert_eq!("wire:4294967295".parse::<WireRef>(), Ok(WireRef(u32::MAX)));
        for bad in [
            "",
            "part",
            "part:",
            "part:03",
            "part:+3",
            "part:-3",
            "part: 3",
            "Part:3",
            "part:3 ",
            "part:4294967296",
            "net:1",
            "wire:1a",
            "part:३",
        ] {
            assert_eq!(bad.parse::<ItemRef>(), Err(InvalidId), "{bad:?}");
        }
        assert_eq!("wire:7".parse::<PartRef>(), Err(InvalidId));
        assert_eq!("d1".parse::<DocId>(), Ok(DocId(1)));
        assert_eq!("d01".parse::<DocId>(), Err(InvalidId));
        assert_eq!("j0".parse::<JobId>(), Ok(JobId(0)));
        assert_eq!(
            "pv-5f0c1d2e3a4b5c6d".parse::<PreviewId>(),
            Ok(PreviewId(0x5f0c_1d2e_3a4b_5c6d))
        );
        for bad in [
            "pv-5F0C1D2E3A4B5C6D",
            "pv-5f0c",
            "pv-+f0c1d2e3a4b5c6d",
            "5f0c1d2e3a4b5c6d",
        ] {
            assert_eq!(bad.parse::<PreviewId>(), Err(InvalidId), "{bad:?}");
        }
        assert_eq!(PreviewId(1).to_string(), "pv-0000000000000001");
    }

    #[test]
    fn ak8_points_are_integer_pairs_and_bounds_are_checked() {
        let p: Point = serde_json::from_str("[-5, 2540000]").unwrap();
        assert_eq!(
            p,
            Point(Pt {
                x: -5,
                y: 2_540_000
            })
        );
        assert_eq!(serde_json::to_string(&p).unwrap(), "[-5,2540000]");
        for bad in [
            "[1.0, 2]",
            "[1]",
            "[1, 2, 3]",
            "{\"x\": 1, \"y\": 2}",
            "[2147483648, 0]",
        ] {
            assert!(serde_json::from_str::<Point>(bad).is_err(), "{bad}");
        }
        assert!(serde_json::from_str::<Bounds>(r#"{"min": [1, 0], "max": [0, 0]}"#).is_err());
        assert!(
            serde_json::from_str::<Bounds>(r#"{"min": [0, 0], "max": [1, 1], "z": 1}"#).is_err()
        );
        let b: Bounds = serde_json::from_str(r#"{"max": [3, 4], "min": [1, 2]}"#).unwrap();
        assert_eq!(
            serde_json::to_string(&b).unwrap(),
            r#"{"min":[1,2],"max":[3,4]}"#
        );
    }
}
