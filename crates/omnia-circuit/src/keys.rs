//! Stable circuit keys (CK1), display names (CK2), CK9 string escaping, and CD18 ordering.
//!
//! [`CircuitKey`] is a distinct type from `omnia-agent`'s `NetKey` (AD8), `omnia-doc`'s
//! [`NetId`](omnia_doc::NetId), and simulation's solver `NodeId` (CK1, XD4). It intentionally
//! implements no conversion to or from `NetId`.

use std::{cmp::Ordering, fmt, sync::Arc};

use omnia_doc::{LabelId, Orient, PartId, Pt, Rect, SymbolId, Wire, WireId};

/// Stable circuit key of a logical net derived from the document at the current revision (CK1).
///
/// Distinct from `omnia-doc`'s `NetId` (no conversion exists), `omnia-agent`'s `NetKey`, and
/// `omnia-sim`'s `NodeId` (CK1, XD4).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CircuitKey {
    /// Labelled net: `net:"<text>"`, using the lowest label text by byte order.
    Label(Arc<str>),
    /// Unlabelled net with at least one pin: `net:pin:<part id>.<pin index>` using the lowest
    /// member pin in CD18 order as the anchor.
    Pin { part: PartId, pin: u32 },
    /// Bare wire island (no labels and no pins): `net:wire:<wire id>` using the lowest wire by
    /// `(lower endpoint, upper endpoint, wire id)`.
    Wire(WireId),
}

impl CircuitKey {
    /// Returns the label text if this is a label key.
    #[inline]
    pub fn label_text(&self) -> Option<&str> {
        match self {
            Self::Label(text) => Some(text),
            Self::Pin { .. } | Self::Wire(_) => None,
        }
    }
}

impl Ord for CircuitKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Label(a), Self::Label(b)) => a.as_bytes().cmp(b.as_bytes()),
            (Self::Label(_), _) => Ordering::Less,
            (_, Self::Label(_)) => Ordering::Greater,
            (
                Self::Pin {
                    part: pa,
                    pin: pina,
                },
                Self::Pin {
                    part: pb,
                    pin: pinb,
                },
            ) => (pa.0, *pina).cmp(&(pb.0, *pinb)),
            (Self::Pin { .. }, Self::Wire(_)) => Ordering::Less,
            (Self::Wire(_), Self::Pin { .. }) => Ordering::Greater,
            (Self::Wire(a), Self::Wire(b)) => a.0.cmp(&b.0),
        }
    }
}

impl PartialOrd for CircuitKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for CircuitKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Label(text) => {
                f.write_str("net:")?;
                write_ck9_quoted(f, text)
            }
            Self::Pin { part, pin } => write!(f, "net:pin:{}.{pin}", part.0),
            Self::Wire(wire) => write!(f, "net:wire:{}", wire.0),
        }
    }
}

/// Natural string ordering (`R2` before `R10`, `2` before `10`), total and overflow-free (CD18,
/// section 9.1).
///
/// Compares non-digit bytes in byte order and contiguous ASCII digit runs by numeric magnitude
/// (leading zeros stripped). Strings whose canonical token streams tie (for example `"R01"` and
/// `"R1"`) break ties by raw byte order so the relation is a strict total order.
pub fn cmp_natural(a: &str, b: &str) -> Ordering {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let mut ia = 0usize;
    let mut ib = 0usize;

    while ia < ab.len() && ib < bb.len() {
        let ca = ab[ia];
        let cb = bb[ib];
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let start_a = ia;
            while ia < ab.len() && ab[ia].is_ascii_digit() {
                ia += 1;
            }
            let start_b = ib;
            while ib < bb.len() && bb[ib].is_ascii_digit() {
                ib += 1;
            }
            let trim_a = trim_leading_zeros(&ab[start_a..ia]);
            let trim_b = trim_leading_zeros(&bb[start_b..ib]);
            match trim_a
                .len()
                .cmp(&trim_b.len())
                .then_with(|| trim_a.cmp(trim_b))
            {
                Ordering::Equal => {}
                non_eq => return non_eq,
            }
        } else {
            // Because ASCII digits `0x30..=0x39` form one contiguous byte range, comparing
            // raw bytes when at least one side is non-digit places every byte < '0' before
            // every digit run and every byte > '9' after every digit run, preserving strict
            // transitivity.
            match ca.cmp(&cb) {
                Ordering::Equal => {
                    ia += 1;
                    ib += 1;
                }
                non_eq => return non_eq,
            }
        }
    }

    match (ia < ab.len(), ib < bb.len()) {
        (false, true) => Ordering::Less,
        (true, false) => Ordering::Greater,
        (false, false) => ab.cmp(bb),
        (true, true) => unreachable!(),
    }
}

#[inline]
fn trim_leading_zeros(digits: &[u8]) -> &[u8] {
    let first_nonzero = digits
        .iter()
        .position(|&b| b != b'0')
        .unwrap_or(digits.len());
    &digits[first_nonzero..]
}

/// Full comparison tuple for a pin under CD18.
///
/// Order: designator (natural), pin number (natural), pin world position `(x, y)`, part symbol
/// (numeric id), part value (byte order), part orientation (encoded `Orient` raw order), part
/// world body bounds (`min.x, min.y, max.x, max.y`), part id, stored pin index.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PinSortKey<'a> {
    pub(crate) designator: &'a str,
    pub(crate) pin_number: Option<&'a str>,
    pub(crate) at: Pt,
    pub(crate) symbol: SymbolId,
    pub(crate) value: &'a str,
    pub(crate) orient: Orient,
    pub(crate) bounds: Rect,
    pub(crate) part: PartId,
    pub(crate) pin_index: u32,
}

impl PinSortKey<'_> {
    pub(crate) fn cmp_cd18(&self, other: &Self) -> Ordering {
        cmp_natural(self.designator, other.designator)
            .then_with(|| cmp_pin_number(self.pin_number, other.pin_number))
            .then_with(|| (self.at.x, self.at.y).cmp(&(other.at.x, other.at.y)))
            .then_with(|| self.symbol.raw().cmp(&other.symbol.raw()))
            .then_with(|| self.value.as_bytes().cmp(other.value.as_bytes()))
            .then_with(|| self.orient.to_raw().cmp(&other.orient.to_raw()))
            .then_with(|| {
                (
                    self.bounds.min.x,
                    self.bounds.min.y,
                    self.bounds.max.x,
                    self.bounds.max.y,
                )
                    .cmp(&(
                        other.bounds.min.x,
                        other.bounds.min.y,
                        other.bounds.max.x,
                        other.bounds.max.y,
                    ))
            })
            .then_with(|| self.part.0.cmp(&other.part.0))
            .then_with(|| self.pin_index.cmp(&other.pin_index))
    }
}

/// Compares pin numbers in natural order (CD18). Two pins without library pin metadata tie at the
/// pin-number step so geometry and the remaining CD18 tie-breakers decide before stored pin index.
#[inline]
fn cmp_pin_number(a_num: Option<&str>, b_num: Option<&str>) -> Ordering {
    match (a_num, b_num) {
        (Some(a), Some(b)) => cmp_natural(a, b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Full comparison tuple for a label under CD18: text in byte order, position `(x, y)`, label id.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LabelSortKey<'a> {
    pub(crate) text: &'a str,
    pub(crate) at: Pt,
    pub(crate) id: LabelId,
}

impl LabelSortKey<'_> {
    pub(crate) fn cmp_cd18(&self, other: &Self) -> Ordering {
        self.text
            .as_bytes()
            .cmp(other.text.as_bytes())
            .then_with(|| (self.at.x, self.at.y).cmp(&(other.at.x, other.at.y)))
            .then_with(|| self.id.0.cmp(&other.id.0))
    }
}

/// Full comparison tuple for a bare wire under CK1 and CD18: lower endpoint `(x, y)`, upper
/// endpoint `(x, y)`, then wire id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct WireSortKey {
    pub(crate) lower: (i32, i32),
    pub(crate) upper: (i32, i32),
    pub(crate) id: u32,
}

impl WireSortKey {
    #[inline]
    pub(crate) fn from_wire(id: WireId, wire: Wire) -> Self {
        let a = (wire.a.x, wire.a.y);
        let b = (wire.b.x, wire.b.y);
        let (lower, upper) = if a <= b { (a, b) } else { (b, a) };
        Self {
            lower,
            upper,
            id: id.0,
        }
    }
}

/// Formats an unlabelled pin-net display name per CK2, CK9 and CK13:
/// `N(<designator>.<pin number>)`, or `N(<designator>#<pin index>)` if library metadata for the
/// pin is absent.
pub(crate) fn format_pin_net_name(
    designator: &str,
    pin_number: Option<&str>,
    pin_index: u32,
    out: &mut String,
) {
    out.clear();
    out.push_str("N(");
    push_ck9_field(out, designator);
    match pin_number {
        Some(number) => {
            out.push('.');
            push_ck9_field(out, number);
        }
        None => {
            use std::fmt::Write as _;
            let _ = write!(out, "#{pin_index}");
        }
    }
    out.push(')');
}

/// Formats a bare wire-island display name per CK2: `N(wire:<wire id>)`.
pub(crate) fn format_wire_net_name(wire: WireId, out: &mut String) {
    use std::fmt::Write as _;
    out.clear();
    let _ = write!(out, "N(wire:{})", wire.0);
}

/// Returns `true` if `s` is one of the fixed `lines/1` record tags (section 9.2).
#[inline]
fn is_line_record_tag(s: &str) -> bool {
    matches!(
        s,
        "#" | "=" | "P" | "N" | "A" | "M" | "E" | "F" | "S" | "+" | "."
    )
}

/// Returns `true` if `s` can be written bare as a CK9 compound-field string component
/// (`[A-Za-z0-9_+-]{1,64}` and not a section 9.2 record tag).
#[inline]
pub(crate) fn is_ck9_bare_component(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && !is_line_record_tag(s)
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-'))
}

/// Appends `s` in CK9 string form for a compound-field component (bare if plain identifier,
/// otherwise quoted with CK9 escapes).
pub(crate) fn push_ck9_field(out: &mut String, s: &str) {
    if is_ck9_bare_component(s) {
        out.push_str(s);
    } else {
        let _ = write_ck9_quoted(out, s);
    }
}

/// Writes `s` enclosed in `"` with CK9 escaping (`\\`, `\"`, and `\u{hex}` for anything outside
/// printable ASCII `0x20..=0x7E`).
pub fn write_ck9_quoted(w: &mut impl fmt::Write, s: &str) -> fmt::Result {
    w.write_str("\"")?;
    for ch in s.chars() {
        match ch {
            '\\' => w.write_str("\\\\")?,
            '"' => w.write_str("\\\"")?,
            ' '..='~' => w.write_char(ch)?,
            other => write!(w, "\\u{{{:x}}}", other as u32)?,
        }
    }
    w.write_str("\"")
}

/// ASCII case-folding comparison against an already-lowercase ASCII target (section 9.4, XD6).
#[inline]
pub(crate) fn eq_ascii_folded(text: &str, lowercase_ascii: &str) -> bool {
    text.eq_ignore_ascii_case(lowercase_ascii)
}

/// Returns `true` if `text` folds (ASCII case-folding only, section 9.4) to one of the five
/// advisory ground-name hints (`gnd`, `agnd`, `dgnd`, `pgnd`, `vss`).
#[inline]
pub(crate) fn is_ground_name_hint(text: &str) -> bool {
    eq_ascii_folded(text, "gnd")
        || eq_ascii_folded(text, "agnd")
        || eq_ascii_folded(text, "dgnd")
        || eq_ascii_folded(text, "pgnd")
        || eq_ascii_folded(text, "vss")
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn key_form_label_formatting_and_escaping() {
        let plain = CircuitKey::Label(Arc::from("VCC"));
        assert_eq!(plain.to_string(), r#"net:"VCC""#);
        assert_eq!(plain.label_text(), Some("VCC"));

        let escaped = CircuitKey::Label(Arc::from("a\"b\\c\n→"));
        assert_eq!(escaped.to_string(), r#"net:"a\"b\\c\u{a}\u{2192}""#);
    }

    #[test]
    fn key_form_pin_formatting() {
        let key = CircuitKey::Pin {
            part: PartId(3),
            pin: 1,
        };
        assert_eq!(key.to_string(), "net:pin:3.1");
        assert_eq!(key.label_text(), None);
    }

    #[test]
    fn key_form_wire_formatting() {
        let key = CircuitKey::Wire(WireId(7));
        assert_eq!(key.to_string(), "net:wire:7");
        assert_eq!(key.label_text(), None);
    }

    #[test]
    fn display_name_helpers_follow_ck2_ck9_ck13() {
        let mut buf = String::new();

        format_pin_net_name("R3", Some("2"), 1, &mut buf);
        assert_eq!(buf, "N(R3.2)");

        format_pin_net_name("A.B", Some("C"), 0, &mut buf);
        assert_eq!(buf, r#"N("A.B".C)"#);

        format_pin_net_name("A", Some("B.C"), 0, &mut buf);
        // "A" is a section 9.2 record tag (Alias), so CK9 quotes it too.
        assert_eq!(buf, r#"N("A"."B.C")"#);

        format_pin_net_name("X", Some("B.C"), 0, &mut buf);
        assert_eq!(buf, r#"N(X."B.C")"#);

        format_pin_net_name("R3", None, 1, &mut buf);
        assert_eq!(buf, "N(R3#1)");

        format_pin_net_name("", Some("1"), 0, &mut buf);
        assert_eq!(buf, r#"N("".1)"#);

        // CK9 / section 9.2: every reserved record tag must be quoted in both components.
        for tag in ["#", "=", "P", "N", "A", "M", "E", "F", "S", "+", "."] {
            format_pin_net_name(tag, Some("1"), 0, &mut buf);
            assert_eq!(buf, format!(r#"N("{tag}".1)"#));
            format_pin_net_name("R1", Some(tag), 0, &mut buf);
            assert_eq!(buf, format!(r#"N(R1."{tag}")"#));
        }

        format_wire_net_name(WireId(7), &mut buf);
        assert_eq!(buf, "N(wire:7)");
    }

    #[test]
    fn natural_order_known_cases() {
        assert_eq!(cmp_natural("R2", "R10"), Ordering::Less);
        assert_eq!(cmp_natural("R1", "R2"), Ordering::Less);
        assert_eq!(cmp_natural("2", "10"), Ordering::Less);
        assert_eq!(cmp_natural("R01", "R1"), Ordering::Less);
        assert_eq!(cmp_natural("R01A", "R1B"), Ordering::Less);
        assert_eq!(
            cmp_natural(
                "R999999999999999999999999999999999999999",
                "R1000000000000000000000000000000000000000"
            ),
            Ordering::Less
        );
    }

    #[test]
    fn wire_sort_key_orders_endpoints_before_comparing() {
        let pos_y = WireSortKey::from_wire(
            WireId(1),
            Wire {
                a: Pt { x: 0, y: 10 },
                b: Pt { x: 0, y: 0 },
            },
        );
        let pos_x = WireSortKey::from_wire(
            WireId(0),
            Wire {
                a: Pt { x: 0, y: 0 },
                b: Pt { x: 10, y: 0 },
            },
        );
        // Same lower endpoint (0, 0); upper endpoint (0, 10) < (10, 0) regardless of wire id or direction.
        assert!(pos_y < pos_x);
    }

    proptest! {
        #[test]
        fn cmp_natural_is_a_strict_total_order(
            a in "[A-Za-z0-9_!.+-]{0,12}",
            b in "[A-Za-z0-9_!.+-]{0,12}",
            c in "[A-Za-z0-9_!.+-]{0,12}",
        ) {
            // Reflexive
            prop_assert_eq!(cmp_natural(&a, &a), Ordering::Equal);
            // Antisymmetric
            prop_assert_eq!(cmp_natural(&a, &b), cmp_natural(&b, &a).reverse());
            // Equal iff byte-equal
            prop_assert_eq!(cmp_natural(&a, &b) == Ordering::Equal, a == b);
            // Transitive
            let ab = cmp_natural(&a, &b);
            let bc = cmp_natural(&b, &c);
            let ac = cmp_natural(&a, &c);
            if ab != Ordering::Greater && bc != Ordering::Greater {
                prop_assert_ne!(ac, Ordering::Greater);
                if ab == Ordering::Less || bc == Ordering::Less {
                    prop_assert_eq!(ac, Ordering::Less);
                }
            }
        }
    }
}
