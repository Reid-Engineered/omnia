//! Embedded part definitions, their one validator and the instance validator (`P2.1`).
//!
//! A document stores every definition its parts use as a self-contained
//! [`EmbeddedPartDef`] (`PD3`). [`validate`] is the single check every ingress runs on a
//! definition (`PD10`): `PK7` caps, the `PD5` pin table and terminal mapping, parameter specs
//! (`PD6`), binding arity, `PK12` `model` variants and the `PK10` screen of embedded SPICE text.
//! [`validate_instance`] checks one placed part against its definition (`PD8`).
//!
//! This module also holds the crate-private definition arena: ids are allocated at the end in
//! apply order and are never reused or renumbered; a record is *referenced* while a live part
//! uses it and *retained* otherwise, and interning only ever matches referenced records (`PD8`).

use std::{borrow::Cow, collections::BTreeMap, fmt, sync::Arc};

use crate::{
    FiniteF64, Pt, Rect, ScreenError, SymbolId, ValueError, parse_value, screen_embedded,
    subckt_ports,
};

/// `PK7`: at most this many parameters per definition.
pub const MAX_PARAMS: usize = 64;
/// `PK7`: every parameter string (name, label, unit, variant, real `raw` text, text value) is
/// at most this many bytes.
pub const MAX_PARAM_STRING_BYTES: usize = 256;
/// `PK7`: a description is at most this many bytes.
pub const MAX_DESCRIPTION_BYTES: usize = 2048;
/// `PK7`: at most this many pins per definition.
pub const MAX_PINS: usize = 256;
/// `PK7`/`PK3`: embedded SPICE text is at most this many bytes.
pub const MAX_SPICE_BYTES: usize = 2 * 1024 * 1024;

/// Local id of a definition in one document's definition table (`PD3`, `PD8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartDefId(pub u32);

/// A typed parameter value (`PD6`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParamValue {
    /// `raw` is the text as entered; `value` its `PK4` parse. Both take part in equality, and
    /// every stored real satisfies `parse_value(raw, unit) == value`.
    Real {
        raw: Arc<str>,
        value: FiniteF64,
    },
    Int(i64),
    Bool(bool),
    Enum(Arc<str>),
    Text(Arc<str>),
}

impl ParamValue {
    /// The kind's name, for diagnostics.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Real { .. } => "real",
            Self::Int(_) => "int",
            Self::Bool(_) => "bool",
            Self::Enum(_) => "enum",
            Self::Text(_) => "text",
        }
    }

    /// The display text `Part::value` carries when this is the resolved primary parameter
    /// (`PD6`): a real's `raw` text, an enum variant or a text value as stored, an integer in
    /// decimal, a boolean as `true` or `false`.
    pub fn display_text(&self) -> Cow<'_, str> {
        match self {
            Self::Real { raw, .. } => Cow::Borrowed(raw),
            Self::Enum(text) | Self::Text(text) => Cow::Borrowed(text),
            Self::Int(value) => Cow::Owned(value.to_string()),
            Self::Bool(value) => Cow::Borrowed(if *value { "true" } else { "false" }),
        }
    }
}

/// A part's parameter overrides: only values that differ from the definition's defaults,
/// in a deterministic sorted map (`PD6`). An unmodified part stores an empty map.
pub type Params = BTreeMap<Arc<str>, ParamValue>;

/// Electrical role of one pin (`PD5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PinRole {
    Passive,
    Input,
    Output,
    Bidir,
    Power,
    Ground,
    NoConnect,
}

/// One row of the embedded pin table, in stored-pin-index order (`PD5`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EmbeddedPin {
    /// The symbol's `Pin::number` at this stored index.
    pub number: Arc<str>,
    pub name: Arc<str>,
    pub role: PinRole,
}

/// The kind of a parameter, with its bounds or variants (`PD6`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParamKind {
    Real {
        unit: Option<Arc<str>>,
        min: Option<FiniteF64>,
        max: Option<FiniteF64>,
    },
    Int {
        min: Option<i64>,
        max: Option<i64>,
    },
    Bool,
    Enum(Vec<Arc<str>>),
    Text,
}

/// One declared parameter (`PD6`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EmbeddedParamSpec {
    pub name: Arc<str>,
    pub label: Arc<str>,
    pub kind: ParamKind,
    pub default: ParamValue,
    pub optional: bool,
}

/// Built-in primitive kinds (`PK6`). A primitive embeds no SPICE text (`PD3`, `PINV6`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveKind {
    Resistor,
    Capacitor,
    Inductor,
    Potentiometer,
    VSource,
    ISource,
    Vcvs,
    Cccs,
    Vccs,
    Ccvs,
    Ground,
    Switch,
    Rail,
}

impl PrimitiveKind {
    /// Number of simulation terminals (`PK6`, `PD5`). Two-terminal elements take 2; the
    /// potentiometer takes 3 (both ends and the wiper); every controlled source and the
    /// voltage-controlled `switch` take 4 (output or switched pair, then controlling pair: a
    /// current-controlled source senses the current through its controlling pair); `ground`
    /// and the supply `rail` take 1.
    pub fn arity(self) -> usize {
        match self {
            Self::Resistor | Self::Capacitor | Self::Inductor | Self::VSource | Self::ISource => 2,
            Self::Potentiometer => 3,
            Self::Vcvs | Self::Cccs | Self::Vccs | Self::Ccvs | Self::Switch => 4,
            Self::Ground | Self::Rail => 1,
        }
    }
}

/// Terminal counts admitted for a `.model` binding by SPICE element letter (`PD5`), matched
/// ASCII case-insensitively. Letters not listed are refused.
pub const MODEL_TERMINALS: [(char, &[usize]); 9] = [
    ('C', &[2]),
    ('D', &[2]),
    ('J', &[3]),
    ('L', &[2]),
    ('M', &[4]),
    ('Q', &[3, 4]),
    ('R', &[2]),
    ('S', &[4]),
    ('Z', &[3]),
];

/// The terminal counts a `.model` element letter admits, or `None` for an unknown letter.
pub fn model_terminals(spice_prefix: char) -> Option<&'static [usize]> {
    let prefix = spice_prefix.to_ascii_uppercase();
    MODEL_TERMINALS
        .iter()
        .find(|(letter, _)| *letter == prefix)
        .map(|(_, counts)| *counts)
}

/// Supply terminals of a `model` or `subckt` binding (`PD5`): read by simulation lowering and
/// the model-required supply check.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SupplyDecl {
    /// Stored pin indices of the positive and negative supply pins; both are `power` pins in
    /// `terminals`.
    pub pos: u16,
    pub neg: u16,
    /// Implicit-supply fallback voltages `(pos, neg)` in volts, `pos > neg`.
    pub implicit: Option<(FiniteF64, FiniteF64)>,
}

/// How a definition simulates (`PD3`, `PD5`). `terminals` are stored pin indices, one per
/// terminal of the primitive or the `.model`/`.subckt` entry, in terminal order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EmbeddedSimBinding {
    None,
    Primitive {
        kind: PrimitiveKind,
        terminals: Vec<u16>,
    },
    Model {
        spice_prefix: char,
        entry: Arc<str>,
        terminals: Vec<u16>,
        spice: Arc<str>,
        supply: Option<SupplyDecl>,
    },
    Subckt {
        entry: Arc<str>,
        terminals: Vec<u16>,
        spice: Arc<str>,
        supply: Option<SupplyDecl>,
    },
}

impl EmbeddedSimBinding {
    /// The terminal list; empty for `None`.
    pub fn terminals(&self) -> &[u16] {
        match self {
            Self::None => &[],
            Self::Primitive { terminals, .. }
            | Self::Model { terminals, .. }
            | Self::Subckt { terminals, .. } => terminals,
        }
    }

    /// The embedded SPICE text; `None` for `None` and primitives, which store none (`PINV6`).
    pub fn spice(&self) -> Option<&str> {
        match self {
            Self::None | Self::Primitive { .. } => None,
            Self::Model { spice, .. } | Self::Subckt { spice, .. } => Some(spice),
        }
    }

    /// The supply declaration of a `model` or `subckt` binding.
    pub fn supply(&self) -> Option<&SupplyDecl> {
        match self {
            Self::None | Self::Primitive { .. } => None,
            Self::Model { supply, .. } | Self::Subckt { supply, .. } => supply.as_ref(),
        }
    }
}

/// A self-contained part definition as a document stores it (`PD3`). Admitted only after
/// [`validate`] (`PD10`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedPartDef {
    pub source_id: Arc<str>,
    /// `PK5` hash; `None` for a definition with no library provenance (v2 migration,
    /// unmodelled placement).
    pub content_hash: Option<[u8; 32]>,
    pub name: Arc<str>,
    pub category: Arc<str>,
    pub description: Arc<str>,
    pub designator_prefix: Arc<str>,
    /// A built-in symbol (`PQ3`).
    pub symbol: SymbolId,
    pub local_pins: Vec<Pt>,
    pub local_bounds: Rect,
    /// Stored-pin-index order (`PD5`); empty only for a definition with `sim: None`.
    pub pins: Vec<EmbeddedPin>,
    /// Sorted by name, strictly.
    pub params: Vec<EmbeddedParamSpec>,
    pub primary_param: Option<Arc<str>>,
    pub sim: EmbeddedSimBinding,
}

/// Prefix of an unmodelled definition's `source_id`, followed by the symbol id (`PD8`).
pub const LEGACY_SOURCE_PREFIX: &str = "legacy:";

impl EmbeddedPartDef {
    /// The unmodelled definition `PD8` synthesises for a v2 part and for an unmodelled
    /// placement: `source_id = "legacy:<symbol id>"`, no hash, the given geometry, empty
    /// name, category, description and designator prefix, an empty pin table, no parameters,
    /// no primary parameter and `sim: None`.
    pub fn unmodelled(symbol: SymbolId, local_pins: &[Pt], local_bounds: Rect) -> Self {
        let empty: Arc<str> = Arc::from("");
        Self {
            source_id: format!("{LEGACY_SOURCE_PREFIX}{}", symbol.raw()).into(),
            content_hash: None,
            name: empty.clone(),
            category: empty.clone(),
            description: empty.clone(),
            designator_prefix: empty,
            symbol,
            local_pins: local_pins.to_vec(),
            local_bounds,
            pins: Vec::new(),
            params: Vec::new(),
            primary_param: None,
            sim: EmbeddedSimBinding::None,
        }
    }

    /// Whether this equals [`EmbeddedPartDef::unmodelled`] of its own symbol and geometry.
    /// Allocates nothing.
    pub fn is_unmodelled(&self) -> bool {
        self.content_hash.is_none()
            && self.name.is_empty()
            && self.category.is_empty()
            && self.description.is_empty()
            && self.designator_prefix.is_empty()
            && self.pins.is_empty()
            && self.params.is_empty()
            && self.primary_param.is_none()
            && self.sim == EmbeddedSimBinding::None
            && is_legacy_source(&self.source_id, self.symbol)
    }

    /// The declared parameter called `name`.
    pub fn param(&self, name: &str) -> Option<&EmbeddedParamSpec> {
        self.params.iter().find(|spec| &*spec.name == name)
    }

    fn matches_unmodelled(&self, symbol: SymbolId, local_pins: &[Pt], local_bounds: Rect) -> bool {
        self.symbol == symbol
            && self.local_bounds == local_bounds
            && self.local_pins == local_pins
            && self.is_unmodelled()
    }
}

/// `source == "legacy:<symbol.raw()>"` in canonical decimal, without allocating.
fn is_legacy_source(source: &str, symbol: SymbolId) -> bool {
    let Some(digits) = source.strip_prefix(LEGACY_SOURCE_PREFIX) else {
        return false;
    };
    let mut buf = [0u8; 5];
    let mut n = symbol.raw();
    let mut start = buf.len();
    loop {
        start -= 1;
        buf[start] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    digits.as_bytes() == &buf[start..]
}

/// Why a value does not fit its parameter spec (`PD6`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamFault {
    /// The value's kind is not the declared kind.
    Kind {
        expected: &'static str,
        actual: &'static str,
    },
    BelowMin,
    AboveMax,
    /// An enum value that is not a declared variant.
    UnknownVariant,
    /// A string longer than [`MAX_PARAM_STRING_BYTES`].
    TooLong,
    /// A real whose `raw` text does not parse under `PK4` with the declared unit.
    Unparseable(ValueError),
    /// A real whose `raw` text parses to a different `value`.
    RawMismatch,
}

impl fmt::Display for ParamFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kind { expected, actual } => write!(f, "expected a {expected}, got a {actual}"),
            Self::BelowMin => f.write_str("below its minimum"),
            Self::AboveMax => f.write_str("above its maximum"),
            Self::UnknownVariant => f.write_str("not a declared variant"),
            Self::TooLong => write!(f, "longer than {MAX_PARAM_STRING_BYTES} bytes"),
            Self::Unparseable(error) => write!(f, "raw text does not parse: {error}"),
            Self::RawMismatch => f.write_str("raw text and value disagree"),
        }
    }
}

/// Why a definition was refused (`PD10`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefError {
    EmptySourceId,
    /// A `PK7` byte cap was exceeded.
    TooLong {
        field: &'static str,
        len: usize,
        max: usize,
    },
    /// A `PK7` count cap was exceeded.
    TooMany {
        field: &'static str,
        count: usize,
        max: usize,
    },
    /// Local pins or bounds outside the sheet limits, or inverted bounds.
    Geometry,
    /// The pin table does not have one row per local pin (`PD5`).
    PinCount {
        pins: usize,
        local_pins: usize,
    },
    /// An empty pin table with a simulation binding (`PD5`).
    PinlessBinding,
    EmptyPinNumber {
        index: usize,
    },
    DuplicatePinNumber {
        number: Arc<str>,
    },
    EmptyParamName,
    /// Parameters not strictly sorted by name (out of order or repeated).
    ParamOrder {
        name: Arc<str>,
    },
    /// A spec whose bounds are inverted or whose variants are empty or repeated.
    InvalidSpec {
        param: Arc<str>,
        reason: &'static str,
    },
    InvalidDefault {
        param: Arc<str>,
        fault: ParamFault,
    },
    /// `primary_param` names no declared parameter.
    UnknownPrimary {
        name: Arc<str>,
    },
    /// A primitive or subcircuit with the wrong number of terminals.
    TerminalCount {
        expected: usize,
        actual: usize,
    },
    /// A `.model` binding whose terminal count its element letter does not admit.
    ModelTerminalCount {
        prefix: char,
        actual: usize,
    },
    /// Terminal `terminal` names a pin index that is out of range, repeated or `no_connect`.
    Terminal {
        terminal: usize,
        pin: u16,
    },
    /// A non-`no_connect` pin that no terminal names (`PINV11`).
    MissingTerminal {
        pin: usize,
    },
    /// A `.model` element letter with no known terminal count.
    SpicePrefix(char),
    EmptyEntry,
    EmptySpice,
    /// The embedded deck defines no top-level `.model`/`.subckt` called `entry`.
    MissingEntry {
        entry: Arc<str>,
    },
    /// The embedded deck has malformed block structure (orphan continuation, nameless
    /// definition, duplicate top-level entry, unterminated `.subckt`, or mismatched `.ends`).
    MalformedSpice(&'static str),
    /// The embedded deck failed the `PK10` screen.
    Screen(ScreenError),
    /// Supply pins that are not two distinct `power` pins in the terminal list.
    SupplyPins,
    /// Implicit supply voltages with `pos <= neg`.
    SupplyImplicit,
    /// A `PK12` `model` parameter that is not an enum over compatible embedded entries.
    ModelParam {
        variant: Option<Arc<str>>,
    },
}

impl fmt::Display for DefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySourceId => f.write_str("definition has an empty source id"),
            Self::TooLong { field, len, max } => {
                write!(f, "{field} is {len} bytes; at most {max} are allowed")
            }
            Self::TooMany { field, count, max } => {
                write!(f, "{count} {field}; at most {max} are allowed")
            }
            Self::Geometry => f.write_str("local pins or bounds lie outside the sheet limits"),
            Self::PinCount { pins, local_pins } => {
                write!(f, "pin table has {pins} rows for {local_pins} local pins")
            }
            Self::PinlessBinding => f.write_str("a simulated definition needs a pin table"),
            Self::EmptyPinNumber { index } => write!(f, "pin {index} has an empty number"),
            Self::DuplicatePinNumber { number } => write!(f, "pin number {number:?} repeats"),
            Self::EmptyParamName => f.write_str("a parameter has an empty name"),
            Self::ParamOrder { name } => {
                write!(f, "parameter {name:?} is out of order or repeated")
            }
            Self::InvalidSpec { param, reason } => write!(f, "parameter {param:?}: {reason}"),
            Self::InvalidDefault { param, fault } => {
                write!(f, "default of parameter {param:?} is invalid: {fault}")
            }
            Self::UnknownPrimary { name } => {
                write!(f, "primary parameter {name:?} is not declared")
            }
            Self::TerminalCount { expected, actual } => {
                write!(f, "{actual} terminals; the binding needs {expected}")
            }
            Self::ModelTerminalCount { prefix, actual } => {
                write!(f, "{actual} terminals do not fit a `{prefix}` model")
            }
            Self::Terminal { terminal, pin } => write!(
                f,
                "terminal {terminal} names pin {pin}, which is out of range, repeated or no_connect"
            ),
            Self::MissingTerminal { pin } => write!(f, "pin {pin} is not mapped to a terminal"),
            Self::SpicePrefix(prefix) => write!(f, "unknown model element letter {prefix:?}"),
            Self::EmptyEntry => f.write_str("simulation entry is empty"),
            Self::EmptySpice => f.write_str("embedded SPICE text is empty"),
            Self::MissingEntry { entry } => {
                write!(f, "embedded SPICE text does not define {entry:?}")
            }
            Self::MalformedSpice(reason) => write!(f, "malformed embedded SPICE deck: {reason}"),
            Self::Screen(error) => error.fmt(f),
            Self::SupplyPins => {
                f.write_str("supply pins must be two distinct power pins in the terminal list")
            }
            Self::SupplyImplicit => {
                f.write_str("implicit positive supply must exceed the negative one")
            }
            Self::ModelParam {
                variant: Some(variant),
            } => {
                write!(
                    f,
                    "model variant {variant:?} names no compatible embedded entry"
                )
            }
            Self::ModelParam { variant: None } => {
                f.write_str("parameter `model` must be an enum over embedded entries")
            }
        }
    }
}

impl std::error::Error for DefError {}

/// Why a part does not fit its definition (`PD8`). The caller names the part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceError {
    Symbol {
        expected: SymbolId,
        actual: SymbolId,
    },
    /// Local pins differ from the definition's in count, value or order.
    LocalPins,
    LocalBounds,
    UndeclaredParam {
        name: Arc<str>,
    },
    Param {
        name: Arc<str>,
        fault: ParamFault,
    },
    /// An override equal to its default; overrides are stored canonically (`PD6`).
    DefaultOverride {
        name: Arc<str>,
    },
    /// `Part::value` is not the display text of the resolved primary parameter (`PD6`).
    Value {
        primary: Arc<str>,
    },
}

impl fmt::Display for InstanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Symbol { expected, actual } => write!(
                f,
                "part uses symbol {} but its definition names symbol {}",
                actual.raw(),
                expected.raw()
            ),
            Self::LocalPins => f.write_str("local pins differ from the definition's"),
            Self::LocalBounds => f.write_str("local bounds differ from the definition's"),
            Self::UndeclaredParam { name } => write!(f, "parameter {name:?} is not declared"),
            Self::Param { name, fault } => write!(f, "parameter {name:?} is invalid: {fault}"),
            Self::DefaultOverride { name } => {
                write!(f, "parameter {name:?} overrides with its default")
            }
            Self::Value { primary } => write!(
                f,
                "value text does not match the resolved primary parameter {primary:?}"
            ),
        }
    }
}

impl std::error::Error for InstanceError {}

/// Checks `value` against `spec`'s kind, bounds, variants and `PK7` caps; a real must also
/// satisfy `parse_value(raw, unit) == value` (`PD6`).
fn check_value(kind: &ParamKind, value: &ParamValue) -> Result<(), ParamFault> {
    let mismatch = |expected| ParamFault::Kind {
        expected,
        actual: value.kind_name(),
    };
    let long = |text: &str| text.len() > MAX_PARAM_STRING_BYTES;
    match (kind, value) {
        (ParamKind::Real { unit, min, max }, ParamValue::Real { raw, value }) => {
            if long(raw) {
                return Err(ParamFault::TooLong);
            }
            if parse_value(raw, unit.as_deref()).map_err(ParamFault::Unparseable)? != *value {
                return Err(ParamFault::RawMismatch);
            }
            if min.is_some_and(|min| *value < min) {
                return Err(ParamFault::BelowMin);
            }
            if max.is_some_and(|max| *value > max) {
                return Err(ParamFault::AboveMax);
            }
            Ok(())
        }
        (ParamKind::Real { .. }, _) => Err(mismatch("real")),
        (ParamKind::Int { min, max }, ParamValue::Int(value)) => {
            if min.is_some_and(|min| *value < min) {
                return Err(ParamFault::BelowMin);
            }
            if max.is_some_and(|max| *value > max) {
                return Err(ParamFault::AboveMax);
            }
            Ok(())
        }
        (ParamKind::Int { .. }, _) => Err(mismatch("int")),
        (ParamKind::Bool, ParamValue::Bool(_)) => Ok(()),
        (ParamKind::Bool, _) => Err(mismatch("bool")),
        (ParamKind::Enum(variants), ParamValue::Enum(variant)) => {
            if variants.contains(variant) {
                Ok(())
            } else {
                Err(ParamFault::UnknownVariant)
            }
        }
        (ParamKind::Enum(_), _) => Err(mismatch("enum")),
        (ParamKind::Text, ParamValue::Text(text)) => {
            if long(text) {
                Err(ParamFault::TooLong)
            } else {
                Ok(())
            }
        }
        (ParamKind::Text, _) => Err(mismatch("text")),
    }
}

fn check_len(field: &'static str, text: &str, max: usize) -> Result<(), DefError> {
    if text.len() > max {
        Err(DefError::TooLong {
            field,
            len: text.len(),
            max,
        })
    } else {
        Ok(())
    }
}

fn check_spec(spec: &EmbeddedParamSpec) -> Result<(), DefError> {
    let invalid = |reason| DefError::InvalidSpec {
        param: spec.name.clone(),
        reason,
    };
    check_len("parameter label", &spec.label, MAX_PARAM_STRING_BYTES)?;
    match &spec.kind {
        ParamKind::Real { unit, min, max } => {
            if let Some(unit) = unit {
                check_len("parameter unit", unit, MAX_PARAM_STRING_BYTES)?;
            }
            if let (Some(min), Some(max)) = (min, max)
                && min > max
            {
                return Err(invalid("minimum exceeds maximum"));
            }
        }
        ParamKind::Int { min, max } => {
            if let (Some(min), Some(max)) = (min, max)
                && min > max
            {
                return Err(invalid("minimum exceeds maximum"));
            }
        }
        ParamKind::Enum(variants) => {
            if variants.is_empty() {
                return Err(invalid("an enum needs at least one variant"));
            }
            for (i, variant) in variants.iter().enumerate() {
                if variant.is_empty() {
                    return Err(invalid("enum variants must not be empty"));
                }
                check_len("enum variant", variant, MAX_PARAM_STRING_BYTES)?;
                if variants[..i].contains(variant) {
                    return Err(invalid("enum variants must not repeat"));
                }
            }
        }
        ParamKind::Bool | ParamKind::Text => {}
    }
    check_value(&spec.kind, &spec.default).map_err(|fault| DefError::InvalidDefault {
        param: spec.name.clone(),
        fault,
    })
}

/// Checks the terminal mapping (`PD5`, `PINV11`): every terminal names an in-range,
/// non-`no_connect` pin exactly once, and every non-`no_connect` pin has a terminal.
fn check_terminals(terminals: &[u16], pins: &[EmbeddedPin]) -> Result<(), DefError> {
    let mut seen = vec![false; pins.len()];
    for (terminal, &pin) in terminals.iter().enumerate() {
        let index = usize::from(pin);
        match (seen.get(index), pins.get(index)) {
            (Some(false), Some(row)) if row.role != PinRole::NoConnect => seen[index] = true,
            _ => return Err(DefError::Terminal { terminal, pin }),
        }
    }
    match pins
        .iter()
        .zip(&seen)
        .position(|(row, &mapped)| row.role != PinRole::NoConnect && !mapped)
    {
        Some(pin) => Err(DefError::MissingTerminal { pin }),
        None => Ok(()),
    }
}

fn check_supply(
    supply: &SupplyDecl,
    terminals: &[u16],
    pins: &[EmbeddedPin],
) -> Result<(), DefError> {
    let is_power_terminal = |pin: u16| {
        terminals.contains(&pin)
            && pins
                .get(usize::from(pin))
                .is_some_and(|row| row.role == PinRole::Power)
    };
    if supply.pos == supply.neg || !is_power_terminal(supply.pos) || !is_power_terminal(supply.neg)
    {
        return Err(DefError::SupplyPins);
    }
    if let Some((pos, neg)) = supply.implicit
        && pos <= neg
    {
        return Err(DefError::SupplyImplicit);
    }
    Ok(())
}

/// Cuts an inline SPICE comment: `;` anywhere, `$` or `//` after whitespace (or at the start),
/// with quoted text skipped (`P0.3` gap 6, matching `spice_screen.rs`).
fn deck_code_part(s: &str) -> &str {
    let b = s.as_bytes();
    let mut in_quote = false;
    for i in 0..b.len() {
        let c = b[i];
        if c == b'"' {
            in_quote = !in_quote;
            continue;
        }
        if in_quote {
            continue;
        }
        let prev_ws = i == 0 || b[i - 1].is_ascii_whitespace();
        match c {
            b';' => return &s[..i],
            b'$' if prev_ws && (i + 1 == b.len() || b[i + 1].is_ascii_whitespace()) => {
                return &s[..i];
            }
            b'/' if prev_ws && b.get(i + 1) == Some(&b'/') => return &s[..i],
            _ => {}
        }
    }
    s
}

/// ASCII whitespace tokens with `a = b`, `a= b` and `a =b` merged into `a=b` (`P0.3` gap 5,
/// matching `spice_screen.rs` and `omnia-parts/src/spice.rs`).
fn deck_tokens(code: &str) -> Vec<String> {
    let raw: Vec<&str> = code.split_ascii_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let t = raw[i];
        if t == "=" && !out.is_empty() && i + 1 < raw.len() {
            let last = out.pop().unwrap_or_default();
            out.push(format!("{last}={}", raw[i + 1]));
            i += 2;
        } else if t.ends_with('=') && t.len() > 1 && i + 1 < raw.len() {
            out.push(format!("{t}{}", raw[i + 1]));
            i += 2;
        } else if t.starts_with('=') && t.len() > 1 && !out.is_empty() {
            let last = out.pop().unwrap_or_default();
            out.push(format!("{last}{t}"));
            i += 1;
        } else {
            out.push(t.to_owned());
            i += 1;
        }
    }
    out
}

enum DeckEntry {
    Model { name: String },
    Subckt { name: String, ports: usize },
}

struct OpenSubckt {
    name: String,
    ports: usize,
    top_level: bool,
}

/// Parses an embedded deck's top-level `.model` and `.subckt` definitions and verifies
/// complete block structure (no orphan continuations, nameless definitions, duplicate
/// top-level entries, unterminated `.subckt` blocks, or unmatched/mismatched `.ends`), using
/// ASCII whitespace tokenization matching `spice_screen.rs` and `omnia-parts/src/spice.rs`.
fn parse_deck(spice: &str) -> Result<Vec<DeckEntry>, DefError> {
    let mut logical: Vec<String> = Vec::new();
    for raw in spice.split('\n') {
        let t = raw.trim_start();
        if t.starts_with('*') {
            continue;
        }
        if let Some(body) = t.strip_prefix('+') {
            let Some(cur) = logical.last_mut() else {
                return Err(DefError::MalformedSpice("orphan continuation line"));
            };
            let code = deck_code_part(body).trim();
            if !code.is_empty() {
                cur.push(' ');
                cur.push_str(code);
            }
            continue;
        }
        let code = deck_code_part(t).trim();
        if !code.is_empty() {
            logical.push(code.to_owned());
        }
    }

    let mut stack: Vec<OpenSubckt> = Vec::new();
    let mut entries: Vec<DeckEntry> = Vec::new();
    for line in &logical {
        let toks = deck_tokens(line);
        let Some(head) = toks.first() else {
            continue;
        };
        if head.eq_ignore_ascii_case(".subckt") {
            let Some(name) = toks.get(1) else {
                return Err(DefError::MalformedSpice(".subckt without a name"));
            };
            let rest = &toks[2..];
            let ports = rest
                .iter()
                .position(|t| t.eq_ignore_ascii_case("params:") || t.contains('='))
                .unwrap_or(rest.len());
            let top_level = stack.is_empty();
            if top_level
                && entries.iter().any(|e| {
                    matches!(e, DeckEntry::Subckt { name: n, .. } if n.eq_ignore_ascii_case(name))
                })
            {
                return Err(DefError::MalformedSpice("duplicate top-level .subckt"));
            }
            stack.push(OpenSubckt {
                name: name.clone(),
                ports,
                top_level,
            });
        } else if head.eq_ignore_ascii_case(".ends") {
            let Some(open) = stack.pop() else {
                return Err(DefError::MalformedSpice(".ends without matching .subckt"));
            };
            if let Some(close) = toks.get(1)
                && !close.eq_ignore_ascii_case(&open.name)
            {
                return Err(DefError::MalformedSpice(
                    ".ends name does not match .subckt",
                ));
            }
            if open.top_level {
                entries.push(DeckEntry::Subckt {
                    name: open.name,
                    ports: open.ports,
                });
            }
        } else if head.eq_ignore_ascii_case(".model") {
            let Some(raw) = toks.get(1) else {
                return Err(DefError::MalformedSpice(".model without a name"));
            };
            let name = raw.split('(').next().unwrap_or_default();
            if name.is_empty() {
                return Err(DefError::MalformedSpice(".model without a name"));
            }
            if stack.is_empty() {
                if entries.iter().any(
                    |e| matches!(e, DeckEntry::Model { name: n } if n.eq_ignore_ascii_case(name)),
                ) {
                    return Err(DefError::MalformedSpice("duplicate top-level .model"));
                }
                entries.push(DeckEntry::Model {
                    name: name.to_owned(),
                });
            }
        }
    }
    if !stack.is_empty() {
        return Err(DefError::MalformedSpice("unterminated .subckt"));
    }
    Ok(entries)
}

fn check_deck(entry: &str, spice: &str) -> Result<(), DefError> {
    if entry.trim().is_empty() {
        return Err(DefError::EmptyEntry);
    }
    if spice.trim().is_empty() {
        return Err(DefError::EmptySpice);
    }
    if spice.len() > MAX_SPICE_BYTES {
        return Err(DefError::TooLong {
            field: "embedded SPICE text",
            len: spice.len(),
            max: MAX_SPICE_BYTES,
        });
    }
    screen_embedded(spice).map_err(DefError::Screen)
}

/// The one validator for a self-contained definition (`PD10`). Every path that admits a
/// definition into a document runs it, and `omnia-parts` runs it on every definition it
/// builds.
pub fn validate(def: &EmbeddedPartDef) -> Result<(), DefError> {
    // PK7 caps and provenance.
    if def.source_id.trim().is_empty() {
        return Err(DefError::EmptySourceId);
    }
    check_len("description", &def.description, MAX_DESCRIPTION_BYTES)?;
    if def.params.len() > MAX_PARAMS {
        return Err(DefError::TooMany {
            field: "parameters",
            count: def.params.len(),
            max: MAX_PARAMS,
        });
    }
    if def.pins.len() > MAX_PINS {
        return Err(DefError::TooMany {
            field: "pins",
            count: def.pins.len(),
            max: MAX_PINS,
        });
    }
    if def.local_pins.len() > MAX_PINS {
        return Err(DefError::TooMany {
            field: "local pins",
            count: def.local_pins.len(),
            max: MAX_PINS,
        });
    }

    // Geometry the definition's parts are built from (PD8).
    let bounds = def.local_bounds;
    if Rect::new(bounds.min, bounds.max).is_err()
        || !bounds.min.is_in_bounds()
        || !bounds.max.is_in_bounds()
        || !def.local_pins.iter().all(|pin| pin.is_in_bounds())
    {
        return Err(DefError::Geometry);
    }

    // PD5 pin table: empty `pins` is permitted for zero-pin mechanical definitions and for
    // unmodelled definitions (`PD8`), never for a simulated definition or a non-unmodelled
    // definition with pins.
    if def.pins.is_empty() {
        if def.sim != EmbeddedSimBinding::None {
            return Err(DefError::PinlessBinding);
        }
        if !def.local_pins.is_empty() && !def.is_unmodelled() {
            return Err(DefError::PinCount {
                pins: 0,
                local_pins: def.local_pins.len(),
            });
        }
    } else if def.pins.len() != def.local_pins.len() {
        return Err(DefError::PinCount {
            pins: def.pins.len(),
            local_pins: def.local_pins.len(),
        });
    }
    for (index, pin) in def.pins.iter().enumerate() {
        if pin.number.trim().is_empty() {
            return Err(DefError::EmptyPinNumber { index });
        }
        if def.pins[..index].iter().any(|p| p.number == pin.number) {
            return Err(DefError::DuplicatePinNumber {
                number: pin.number.clone(),
            });
        }
    }

    // PD6 parameter specs, strictly sorted by name.
    for (i, spec) in def.params.iter().enumerate() {
        if spec.name.trim().is_empty() {
            return Err(DefError::EmptyParamName);
        }
        check_len("parameter name", &spec.name, MAX_PARAM_STRING_BYTES)?;
        if i > 0 && def.params[i - 1].name >= spec.name {
            return Err(DefError::ParamOrder {
                name: spec.name.clone(),
            });
        }
        check_spec(spec)?;
    }
    if let Some(primary) = &def.primary_param
        && def.param(primary).is_none()
    {
        return Err(DefError::UnknownPrimary {
            name: primary.clone(),
        });
    }

    // Binding arity, terminal mapping, deck screen, block structure and supply (PD5, PD10, PK10).
    let parsed_deck = match &def.sim {
        EmbeddedSimBinding::None => Vec::new(),
        EmbeddedSimBinding::Primitive { kind, terminals } => {
            if terminals.len() != kind.arity() {
                return Err(DefError::TerminalCount {
                    expected: kind.arity(),
                    actual: terminals.len(),
                });
            }
            check_terminals(terminals, &def.pins)?;
            Vec::new()
        }
        EmbeddedSimBinding::Model {
            spice_prefix,
            entry,
            terminals,
            spice,
            supply,
        } => {
            let counts =
                model_terminals(*spice_prefix).ok_or(DefError::SpicePrefix(*spice_prefix))?;
            check_deck(entry, spice)?;
            let entries = parse_deck(spice)?;
            if !entries
                .iter()
                .any(|e| matches!(e, DeckEntry::Model { name } if name.eq_ignore_ascii_case(entry)))
            {
                return Err(DefError::MissingEntry {
                    entry: entry.clone(),
                });
            }
            if !counts.contains(&terminals.len()) {
                return Err(DefError::ModelTerminalCount {
                    prefix: *spice_prefix,
                    actual: terminals.len(),
                });
            }
            check_terminals(terminals, &def.pins)?;
            if let Some(supply) = supply {
                check_supply(supply, terminals, &def.pins)?;
            }
            entries
        }
        EmbeddedSimBinding::Subckt {
            entry,
            terminals,
            spice,
            supply,
        } => {
            check_deck(entry, spice)?;
            let entries = parse_deck(spice)?;
            let ports = subckt_ports(spice, entry).ok_or_else(|| DefError::MissingEntry {
                entry: entry.clone(),
            })?;
            debug_assert!(entries.iter().any(|e| matches!(
                e,
                DeckEntry::Subckt { name, ports: p }
                    if name.eq_ignore_ascii_case(entry) && *p == ports
            )));
            if terminals.len() != ports {
                return Err(DefError::TerminalCount {
                    expected: ports,
                    actual: terminals.len(),
                });
            }
            check_terminals(terminals, &def.pins)?;
            if let Some(supply) = supply {
                check_supply(supply, terminals, &def.pins)?;
            }
            entries
        }
    };

    // PK12: `model` is an enum over compatible entries embedded in the same definition:
    // top-level `.model` entries for a `Model` binding, or top-level `.subckt` entries with
    // the binding's terminal count for a `Subckt` binding.
    if let Some(spec) = def.param("model") {
        let ParamKind::Enum(variants) = &spec.kind else {
            return Err(DefError::ModelParam { variant: None });
        };
        let compatible = |variant: &str| {
            match &def.sim {
            EmbeddedSimBinding::Model { .. } => parsed_deck.iter().any(|entry| {
                matches!(entry, DeckEntry::Model { name } if name.eq_ignore_ascii_case(variant))
            }),
            EmbeddedSimBinding::Subckt { terminals, .. } => parsed_deck.iter().any(|entry| {
                matches!(
                    entry,
                    DeckEntry::Subckt { name, ports }
                        if name.eq_ignore_ascii_case(variant) && *ports == terminals.len()
                )
            }),
            EmbeddedSimBinding::None | EmbeddedSimBinding::Primitive { .. } => false,
        }
        };
        if let Some(variant) = variants.iter().find(|variant| !compatible(variant)) {
            return Err(DefError::ModelParam {
                variant: Some(variant.clone()),
            });
        }
    }
    Ok(())
}

/// Checks one part against its definition (`PD8`): symbol, local pins (in order) and bounds,
/// every override (declared, of the declared kind, in bounds, `raw`/`value` agreeing, not
/// equal to its default), and `Part::value` against the resolved primary parameter.
pub fn validate_instance(
    def: &EmbeddedPartDef,
    symbol: SymbolId,
    local_pins: &[Pt],
    local_bounds: Rect,
    value: &str,
    params: &Params,
) -> Result<(), InstanceError> {
    if symbol != def.symbol {
        return Err(InstanceError::Symbol {
            expected: def.symbol,
            actual: symbol,
        });
    }
    if local_pins != def.local_pins.as_slice() {
        return Err(InstanceError::LocalPins);
    }
    if local_bounds != def.local_bounds {
        return Err(InstanceError::LocalBounds);
    }
    for (name, value) in params {
        let spec = def
            .param(name)
            .ok_or_else(|| InstanceError::UndeclaredParam { name: name.clone() })?;
        check_value(&spec.kind, value).map_err(|fault| InstanceError::Param {
            name: name.clone(),
            fault,
        })?;
        if *value == spec.default {
            return Err(InstanceError::DefaultOverride { name: name.clone() });
        }
    }
    if let Some(primary) = &def.primary_param {
        let resolved = match params.get(primary) {
            Some(value) => value,
            None => {
                &def.param(primary)
                    .ok_or_else(|| InstanceError::UndeclaredParam {
                        name: primary.clone(),
                    })?
                    .default
            }
        };
        if resolved.display_text() != value {
            return Err(InstanceError::Value {
                primary: primary.clone(),
            });
        }
    }
    Ok(())
}

/// The document's definition table (`PD8`): records by `PartDefId`, with the number of live
/// parts using each. Ids are never reused or renumbered; a record with no live part is
/// retained for undo and redo and is never matched by interning.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DefArena {
    records: Vec<Arc<EmbeddedPartDef>>,
    refs: Vec<u32>,
}

impl DefArena {
    /// Arena length: one past the highest id ever allocated.
    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }

    /// The record at `id`, referenced or retained.
    pub(crate) fn get(&self, id: PartDefId) -> Option<&EmbeddedPartDef> {
        self.records.get(id.0 as usize).map(|record| &**record)
    }

    /// Clones the shared record handle at `id`, referenced or retained.
    pub(crate) fn get_arc(&self, id: PartDefId) -> Option<Arc<EmbeddedPartDef>> {
        self.records.get(id.0 as usize).cloned()
    }

    pub(crate) fn is_referenced(&self, id: PartDefId) -> bool {
        self.refs.get(id.0 as usize).is_some_and(|&refs| refs > 0)
    }

    /// Referenced records in id order.
    pub(crate) fn referenced(&self) -> impl Iterator<Item = (PartDefId, &EmbeddedPartDef)> {
        self.records
            .iter()
            .zip(&self.refs)
            .enumerate()
            .filter(|(_, (_, refs))| **refs > 0)
            .map(|(i, (record, _))| (PartDefId(i as u32), &**record))
    }

    /// Persistent equality (`PD8`): equal arena length and equal records wherever either side
    /// is referenced. Retained records are history state and are not compared.
    pub(crate) fn referenced_eq(&self, other: &Self) -> bool {
        self.len() == other.len()
            && (0..self.len()).all(|i| {
                let referenced = self.refs[i] > 0 || other.refs[i] > 0;
                !referenced
                    || ((self.refs[i] > 0) == (other.refs[i] > 0)
                        && self.records[i] == other.records[i])
            })
    }

    /// Returns the lowest referenced id whose record satisfies `matches`, or allocates `make()`
    /// at the arena end (`PD8` interning). The caller then counts the new reference.
    fn intern_with(
        &mut self,
        matches: impl Fn(&EmbeddedPartDef) -> bool,
        make: impl FnOnce() -> Arc<EmbeddedPartDef>,
    ) -> PartDefId {
        if let Some(i) = self
            .records
            .iter()
            .zip(&self.refs)
            .position(|(record, &refs)| refs > 0 && matches(record))
        {
            return PartDefId(i as u32);
        }
        let id = PartDefId(u32::try_from(self.records.len()).expect("definition id space"));
        self.records.push(make());
        self.refs.push(0);
        id
    }

    /// Interns `def` by full structural equality against referenced records (`content_hash` is
    /// a prefilter only, `PD8`).
    pub(crate) fn intern(&mut self, def: Arc<EmbeddedPartDef>) -> PartDefId {
        let probe = def.clone();
        self.intern_with(
            |record| record.content_hash == probe.content_hash && *record == *probe,
            || def,
        )
    }

    /// Interns the unmodelled definition of `(symbol, local_pins, local_bounds)`, allocating
    /// only when no referenced record equals it.
    pub(crate) fn intern_unmodelled(
        &mut self,
        symbol: SymbolId,
        local_pins: &[Pt],
        local_bounds: Rect,
    ) -> PartDefId {
        self.intern_with(
            |record| record.matches_unmodelled(symbol, local_pins, local_bounds),
            || {
                Arc::new(EmbeddedPartDef::unmodelled(
                    symbol,
                    local_pins,
                    local_bounds,
                ))
            },
        )
    }

    /// Counts a live part using `id`. Non-test builds require `id` to exist in this arena;
    /// test builds ignore unknown ids because crate-private test scaffolding (such as
    /// `follow::view_tests::materialize`) copies `StoredPart`s into a fresh `Doc`.
    pub(crate) fn acquire(&mut self, id: PartDefId) {
        #[cfg(not(test))]
        let refs = self
            .refs
            .get_mut(id.0 as usize)
            .expect("definition id in arena");
        #[cfg(test)]
        let Some(refs) = self.refs.get_mut(id.0 as usize) else {
            return;
        };
        *refs = refs.checked_add(1).expect("definition reference count");
    }

    /// Releases a live part's use of `id`; see [`DefArena::acquire`].
    pub(crate) fn release(&mut self, id: PartDefId) {
        #[cfg(not(test))]
        let refs = self
            .refs
            .get_mut(id.0 as usize)
            .expect("definition id in arena");
        #[cfg(test)]
        let Some(refs) = self.refs.get_mut(id.0 as usize) else {
            return;
        };
        *refs = refs.checked_sub(1).expect("definition reference count");
    }

    /// Drops every record allocated after the arena had length `len` (`PD8` refusal
    /// rollback). Those records have no live part by then.
    pub(crate) fn truncate(&mut self, len: usize) {
        debug_assert!(self.refs.iter().skip(len).all(|&refs| refs == 0));
        self.records.truncate(len);
        self.refs.truncate(len);
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;
    use crate::{Command, CommandError, Doc, Orient, Part, PlaceDef, SHEET_LIMIT_NM, Wire};

    fn real(raw: &str, unit: Option<&str>) -> ParamValue {
        ParamValue::Real {
            raw: raw.into(),
            value: parse_value(raw, unit).unwrap(),
        }
    }

    fn f(v: f64) -> FiniteF64 {
        FiniteF64::new(v).unwrap()
    }

    fn pin(number: &str, name: &str, role: PinRole) -> EmbeddedPin {
        EmbeddedPin {
            number: number.into(),
            name: name.into(),
            role,
        }
    }

    fn resistor_def() -> EmbeddedPartDef {
        EmbeddedPartDef {
            source_id: "omnia:resistor".into(),
            content_hash: Some([0x11; 32]),
            name: "Resistor".into(),
            category: "Passives".into(),
            description: "Ideal linear two-terminal resistor".into(),
            designator_prefix: "R".into(),
            symbol: SymbolId(0),
            local_pins: vec![Pt { x: -10, y: 0 }, Pt { x: 10, y: 0 }],
            local_bounds: Rect::new(Pt { x: -10, y: -5 }, Pt { x: 10, y: 5 }).unwrap(),
            pins: vec![
                pin("1", "a", PinRole::Passive),
                pin("2", "b", PinRole::Passive),
            ],
            params: vec![
                EmbeddedParamSpec {
                    name: "resistance".into(),
                    label: "Resistance".into(),
                    kind: ParamKind::Real {
                        unit: Some("ohm".into()),
                        min: Some(f(1e-6)),
                        max: Some(f(1e12)),
                    },
                    default: real("10k", Some("ohm")),
                    optional: false,
                },
                EmbeddedParamSpec {
                    name: "tc1".into(),
                    label: "Linear tempco".into(),
                    kind: ParamKind::Real {
                        unit: Some("1/K".into()),
                        min: None,
                        max: None,
                    },
                    default: real("0", Some("1/K")),
                    optional: true,
                },
            ],
            primary_param: Some("resistance".into()),
            sim: EmbeddedSimBinding::Primitive {
                kind: PrimitiveKind::Resistor,
                terminals: vec![0, 1],
            },
        }
    }

    fn opamp_def() -> EmbeddedPartDef {
        EmbeddedPartDef {
            source_id: "omnia:opamp-generic".into(),
            content_hash: Some([0x22; 32]),
            name: "Op-amp (generic)".into(),
            category: "Op-Amps".into(),
            description: "Behavioural voltage-feedback operational amplifier macromodel".into(),
            designator_prefix: "U".into(),
            symbol: SymbolId(5),
            local_pins: vec![
                Pt { x: -10, y: -5 },
                Pt { x: -10, y: 5 },
                Pt { x: 0, y: -10 },
                Pt { x: 0, y: 10 },
                Pt { x: 10, y: 0 },
            ],
            local_bounds: Rect::new(Pt { x: -10, y: -10 }, Pt { x: 10, y: 10 }).unwrap(),
            pins: vec![
                pin("3", "in+", PinRole::Input),
                pin("2", "in-", PinRole::Input),
                pin("8", "v+", PinRole::Power),
                pin("4", "v-", PinRole::Power),
                pin("1", "out", PinRole::Output),
            ],
            params: vec![],
            primary_param: None,
            sim: EmbeddedSimBinding::Subckt {
                entry: "OPAMP_GENERIC".into(),
                terminals: vec![0, 1, 2, 3, 4],
                spice: ".subckt OPAMP_GENERIC inp inn vcc vee out PARAMS: gain=1e5\nE1 out 0 inp inn {gain}\n.ends OPAMP_GENERIC\n".into(),
                supply: Some(SupplyDecl {
                    pos: 2,
                    neg: 3,
                    implicit: Some((f(15.0), f(-15.0))),
                }),
            },
        }
    }

    fn diode_def() -> EmbeddedPartDef {
        EmbeddedPartDef {
            source_id: "omnia:diode-signal".into(),
            content_hash: Some([0x33; 32]),
            name: "Signal Diode".into(),
            category: "Diodes".into(),
            description: "Generic small-signal silicon diode".into(),
            designator_prefix: "D".into(),
            symbol: SymbolId(3),
            local_pins: vec![Pt { x: -10, y: 0 }, Pt { x: 10, y: 0 }],
            local_bounds: Rect::new(Pt { x: -10, y: -5 }, Pt { x: 10, y: 5 }).unwrap(),
            pins: vec![
                pin("1", "anode", PinRole::Passive),
                pin("2", "cathode", PinRole::Passive),
            ],
            params: vec![],
            primary_param: None,
            sim: EmbeddedSimBinding::Model {
                spice_prefix: 'D',
                entry: "DIODE_SIG".into(),
                terminals: vec![0, 1],
                spice: ".model DIODE_SIG D(IS=2.5e-9 RS=0.5 N=1.75)\n".into(),
                supply: None,
            },
        }
    }

    fn resistor_part(at: Pt, value: &str) -> Part {
        Part {
            symbol: SymbolId(0),
            at,
            orient: Orient::IDENTITY,
            designator: "R1".into(),
            value: value.into(),
        }
    }

    #[test]
    fn pinv5_deduplication_and_def_lifecycle_without_renumbering() {
        let mut doc = Doc::new();
        let bounds = Rect::new(Pt { x: -5, y: -5 }, Pt { x: 5, y: 5 }).unwrap();
        let pins = [Pt { x: -5, y: 0 }, Pt { x: 5, y: 0 }];

        // N instances of the same unmodelled definition share 1 referenced record.
        let mut unmodelled_ids = Vec::new();
        for i in 0..10 {
            let pid = doc
                .insert_part(resistor_part(Pt { x: i * 20, y: 0 }, "1k"), &pins, bounds)
                .unwrap();
            assert_eq!(doc.part_def_id(pid), Some(PartDefId(0)));
            unmodelled_ids.push(pid);
        }
        assert_eq!(doc.part_defs().count(), 1);
        assert!(doc.part_def(PartDefId(0)).unwrap().is_unmodelled());

        // N instances of the same modelled definition share 1 referenced record.
        let res = Arc::new(resistor_def());
        let mut modelled_ids = Vec::new();
        for i in 0..5 {
            let pid = doc
                .insert_part_with_def(
                    resistor_part(Pt { x: i * 20, y: 30 }, "10k"),
                    res.clone(),
                    Params::new(),
                )
                .unwrap();
            assert_eq!(doc.part_def_id(pid), Some(PartDefId(1)));
            modelled_ids.push(pid);
        }
        assert_eq!(doc.part_defs().count(), 2);

        // PD9: equal content under a different `source_id` is a distinct record in `Doc`.
        let mut other_source = resistor_def();
        other_source.source_id = "custom:resistor".into();
        let other_pid = doc
            .insert_part_with_def(
                resistor_part(Pt { x: 0, y: 60 }, "10k"),
                Arc::new(other_source),
                Params::new(),
            )
            .unwrap();
        assert_eq!(doc.part_def_id(other_pid), Some(PartDefId(2)));
        assert_eq!(doc.part_defs().count(), 3);

        // Removing all instances of id 1 turns it retained: hidden from `part_defs()`,
        // still readable via `part_def(id)`, and never matched by interning (`PD8`).
        for pid in modelled_ids {
            doc.remove_part(pid).unwrap();
        }
        assert!(!doc.part_defs.is_referenced(PartDefId(1)));
        assert!(doc.part_def(PartDefId(1)).is_some());
        let live_ids: Vec<_> = doc.part_defs().map(|(id, _)| id).collect();
        assert_eq!(live_ids, vec![PartDefId(0), PartDefId(2)]);

        // Re-inserting `res` allocates id 3 (never reuses retained id 1).
        let fresh_pid = doc
            .insert_part_with_def(
                resistor_part(Pt { x: 20, y: 60 }, "10k"),
                res,
                Params::new(),
            )
            .unwrap();
        assert_eq!(doc.part_def_id(fresh_pid), Some(PartDefId(3)));
        assert_eq!(
            doc.part_defs().map(|(id, _)| id).collect::<Vec<_>>(),
            vec![PartDefId(0), PartDefId(2), PartDefId(3)]
        );
        assert_eq!(unmodelled_ids.len(), 10);
    }

    #[test]
    fn pinv6_lean_primitives_and_arities() {
        assert_eq!(EmbeddedSimBinding::None.spice(), None);
        assert_eq!(EmbeddedSimBinding::None.supply(), None);
        assert!(EmbeddedSimBinding::None.terminals().is_empty());

        for (kind, expected_arity) in [
            (PrimitiveKind::Ground, 1),
            (PrimitiveKind::Rail, 1),
            (PrimitiveKind::Resistor, 2),
            (PrimitiveKind::Capacitor, 2),
            (PrimitiveKind::Inductor, 2),
            (PrimitiveKind::VSource, 2),
            (PrimitiveKind::ISource, 2),
            (PrimitiveKind::Potentiometer, 3),
            (PrimitiveKind::Vcvs, 4),
            (PrimitiveKind::Cccs, 4),
            (PrimitiveKind::Vccs, 4),
            (PrimitiveKind::Ccvs, 4),
            (PrimitiveKind::Switch, 4),
        ] {
            assert_eq!(kind.arity(), expected_arity, "{kind:?}");
            let terminals: Vec<u16> = (0..expected_arity as u16).collect();
            let binding = EmbeddedSimBinding::Primitive {
                kind,
                terminals: terminals.clone(),
            };
            assert_eq!(binding.spice(), None, "PINV6: primitive stores no SPICE");
            assert_eq!(binding.supply(), None);
            assert_eq!(binding.terminals(), terminals.as_slice());

            let local_pins: Vec<Pt> = (0..expected_arity as i32)
                .map(|i| Pt { x: i * 10, y: 0 })
                .collect();
            let pins: Vec<EmbeddedPin> = (0..expected_arity)
                .map(|i| pin(&(i + 1).to_string(), "p", PinRole::Passive))
                .collect();
            let def = EmbeddedPartDef {
                source_id: format!("omnia:{kind:?}").into(),
                content_hash: None,
                name: "P".into(),
                category: "Test".into(),
                description: "".into(),
                designator_prefix: "X".into(),
                symbol: SymbolId(0),
                local_pins,
                local_bounds: Rect::new(Pt::ZERO, Pt { x: 40, y: 10 }).unwrap(),
                pins,
                params: vec![],
                primary_param: None,
                sim: binding,
            };
            assert_eq!(validate(&def), Ok(()), "{kind:?}");
        }
    }

    #[test]
    fn pinv11_terminal_mapping_and_pin_order() {
        // 3 pins, with middle pin `NoConnect`: terminals map only the 2 non-NC pins.
        let mut def = resistor_def();
        def.local_pins.push(Pt { x: 0, y: 10 });
        def.pins = vec![
            pin("1", "a", PinRole::Passive),
            pin("2", "nc", PinRole::NoConnect),
            pin("3", "b", PinRole::Passive),
        ];
        def.sim = EmbeddedSimBinding::Primitive {
            kind: PrimitiveKind::Resistor,
            terminals: vec![0, 2],
        };
        assert_eq!(validate(&def), Ok(()));

        // Swapping terminal order `[2, 0]` is valid and distinct.
        let mut swapped = def.clone();
        swapped.sim = EmbeddedSimBinding::Primitive {
            kind: PrimitiveKind::Resistor,
            terminals: vec![2, 0],
        };
        assert_eq!(validate(&swapped), Ok(()));
        assert_ne!(def.sim.terminals(), swapped.sim.terminals());

        // Repeated pin index at the right count is refused.
        let mut dup = def.clone();
        dup.sim = EmbeddedSimBinding::Primitive {
            kind: PrimitiveKind::Resistor,
            terminals: vec![0, 0],
        };
        assert_eq!(
            validate(&dup),
            Err(DefError::Terminal {
                terminal: 1,
                pin: 0
            })
        );

        // Naming the `NoConnect` pin as a terminal is refused.
        let mut nc = def.clone();
        nc.sim = EmbeddedSimBinding::Primitive {
            kind: PrimitiveKind::Resistor,
            terminals: vec![0, 1],
        };
        assert_eq!(
            validate(&nc),
            Err(DefError::Terminal {
                terminal: 1,
                pin: 1
            })
        );

        // Out-of-range pin index is refused.
        let mut oob = def.clone();
        oob.sim = EmbeddedSimBinding::Primitive {
            kind: PrimitiveKind::Resistor,
            terminals: vec![0, 99],
        };
        assert_eq!(
            validate(&oob),
            Err(DefError::Terminal {
                terminal: 1,
                pin: 99
            })
        );

        // Unmapped non-`NoConnect` pin is refused (`MissingTerminal`).
        let mut unmapped = def.clone();
        unmapped.pins[1].role = PinRole::Passive;
        assert_eq!(
            validate(&unmapped),
            Err(DefError::MissingTerminal { pin: 1 })
        );

        // Wrong arity for primitive is refused (`TerminalCount`).
        let mut wrong_count = resistor_def();
        wrong_count.sim = EmbeddedSimBinding::Primitive {
            kind: PrimitiveKind::Resistor,
            terminals: vec![0],
        };
        assert_eq!(
            validate(&wrong_count),
            Err(DefError::TerminalCount {
                expected: 2,
                actual: 1
            })
        );
    }

    #[test]
    fn pd10_validate_caps_geometry_pins_and_params() {
        assert_eq!(validate(&resistor_def()), Ok(()));
        assert_eq!(validate(&opamp_def()), Ok(()));
        assert_eq!(validate(&diode_def()), Ok(()));

        // Unmodelled definition with pins validates cleanly.
        let unmod = EmbeddedPartDef::unmodelled(
            SymbolId(2),
            &[Pt::ZERO, Pt { x: 10, y: 0 }],
            Rect::new(Pt::ZERO, Pt { x: 10, y: 10 }).unwrap(),
        );
        assert!(unmod.is_unmodelled());
        assert_eq!(validate(&unmod), Ok(()));

        // Non-unmodelled zero-pin mechanical definition (`sim: None`) validates cleanly.
        let mut mech = EmbeddedPartDef::unmodelled(
            SymbolId(2),
            &[],
            Rect::new(Pt::ZERO, Pt { x: 10, y: 10 }).unwrap(),
        );
        mech.source_id = "omnia:mounting-hole".into();
        mech.name = "Mounting hole".into();
        assert!(!mech.is_unmodelled());
        assert_eq!(validate(&mech), Ok(()));

        // Non-unmodelled `sim: None` with non-empty `local_pins` and empty `pins` is refused (`PD5`).
        let mut mech_bad = mech.clone();
        mech_bad.local_pins = vec![Pt::ZERO];
        assert_eq!(
            validate(&mech_bad),
            Err(DefError::PinCount {
                pins: 0,
                local_pins: 1
            })
        );

        // Empty / whitespace source_id.
        for bad_id in ["", "   "] {
            let mut d = resistor_def();
            d.source_id = bad_id.into();
            assert_eq!(validate(&d), Err(DefError::EmptySourceId));
        }

        // PK7 description cap.
        let mut d = resistor_def();
        d.description = "a".repeat(MAX_DESCRIPTION_BYTES).into();
        assert_eq!(validate(&d), Ok(()));
        d.description = "a".repeat(MAX_DESCRIPTION_BYTES + 1).into();
        assert_eq!(
            validate(&d),
            Err(DefError::TooLong {
                field: "description",
                len: MAX_DESCRIPTION_BYTES + 1,
                max: MAX_DESCRIPTION_BYTES,
            })
        );

        // PK7 parameter count cap.
        let mut d = mech.clone();
        d.params = (0..=MAX_PARAMS)
            .map(|i| EmbeddedParamSpec {
                name: format!("p{i:03}").into(),
                label: "".into(),
                kind: ParamKind::Bool,
                default: ParamValue::Bool(false),
                optional: false,
            })
            .collect();
        assert_eq!(
            validate(&d),
            Err(DefError::TooMany {
                field: "parameters",
                count: MAX_PARAMS + 1,
                max: MAX_PARAMS,
            })
        );
        d.params.pop();
        assert_eq!(validate(&d), Ok(()));

        // PK7 pin count cap (both `pins` and unmodelled `local_pins`).
        let mut d = mech.clone();
        d.local_pins = vec![Pt::ZERO; MAX_PINS + 1];
        d.pins = (0..=MAX_PINS)
            .map(|i| pin(&i.to_string(), "p", PinRole::NoConnect))
            .collect();
        assert_eq!(
            validate(&d),
            Err(DefError::TooMany {
                field: "pins",
                count: MAX_PINS + 1,
                max: MAX_PINS,
            })
        );
        let too_many_unmod = EmbeddedPartDef::unmodelled(
            SymbolId(0),
            &vec![Pt::ZERO; MAX_PINS + 1],
            Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
        );
        assert_eq!(
            validate(&too_many_unmod),
            Err(DefError::TooMany {
                field: "local pins",
                count: MAX_PINS + 1,
                max: MAX_PINS,
            })
        );

        // Geometry checks: inverted bounds, OOB bounds, OOB local_pins.
        let mut inv = resistor_def();
        inv.local_bounds = Rect {
            min: Pt { x: 10, y: 0 },
            max: Pt { x: -10, y: 0 },
        };
        assert_eq!(validate(&inv), Err(DefError::Geometry));

        let mut oob_bounds = resistor_def();
        oob_bounds.local_bounds = Rect {
            min: Pt::ZERO,
            max: Pt {
                x: SHEET_LIMIT_NM + 1,
                y: 0,
            },
        };
        assert_eq!(validate(&oob_bounds), Err(DefError::Geometry));

        let mut oob_pin = resistor_def();
        oob_pin.local_pins[0] = Pt {
            x: -SHEET_LIMIT_NM - 1,
            y: 0,
        };
        assert_eq!(validate(&oob_pin), Err(DefError::Geometry));

        // Pin table checks: PinlessBinding, PinCount, EmptyPinNumber, DuplicatePinNumber.
        let mut pinless = resistor_def();
        pinless.pins.clear();
        assert_eq!(validate(&pinless), Err(DefError::PinlessBinding));

        let mut bad_count = resistor_def();
        bad_count.pins.pop();
        assert_eq!(
            validate(&bad_count),
            Err(DefError::PinCount {
                pins: 1,
                local_pins: 2
            })
        );

        for empty_num in ["", " \t "] {
            let mut d = resistor_def();
            d.pins[1].number = empty_num.into();
            assert_eq!(validate(&d), Err(DefError::EmptyPinNumber { index: 1 }));
        }

        let mut dup_num = resistor_def();
        dup_num.pins[1].number = "1".into();
        assert_eq!(
            validate(&dup_num),
            Err(DefError::DuplicatePinNumber { number: "1".into() })
        );

        // Parameter specs: empty name, order/duplicate, PK7 param string caps, bounds, enum, defaults, primary.
        let mut empty_name = mech.clone();
        empty_name.params.push(EmbeddedParamSpec {
            name: "  ".into(),
            label: "".into(),
            kind: ParamKind::Bool,
            default: ParamValue::Bool(true),
            optional: false,
        });
        assert_eq!(validate(&empty_name), Err(DefError::EmptyParamName));

        let mut unsorted = resistor_def();
        unsorted.params.swap(0, 1);
        assert_eq!(
            validate(&unsorted),
            Err(DefError::ParamOrder {
                name: "resistance".into()
            })
        );

        let mut dup_param = resistor_def();
        dup_param.params[1].name = "resistance".into();
        assert_eq!(
            validate(&dup_param),
            Err(DefError::ParamOrder {
                name: "resistance".into()
            })
        );

        let mut long_label = resistor_def();
        long_label.params[0].label = "x".repeat(MAX_PARAM_STRING_BYTES + 1).into();
        assert!(matches!(
            validate(&long_label),
            Err(DefError::TooLong { .. })
        ));

        let mut inv_real = resistor_def();
        inv_real.params[0].kind = ParamKind::Real {
            unit: Some("ohm".into()),
            min: Some(f(100.0)),
            max: Some(f(10.0)),
        };
        assert!(matches!(
            validate(&inv_real),
            Err(DefError::InvalidSpec { .. })
        ));

        let mut inv_int = mech.clone();
        inv_int.params.push(EmbeddedParamSpec {
            name: "n".into(),
            label: "N".into(),
            kind: ParamKind::Int {
                min: Some(5),
                max: Some(2),
            },
            default: ParamValue::Int(3),
            optional: false,
        });
        assert!(matches!(
            validate(&inv_int),
            Err(DefError::InvalidSpec { .. })
        ));

        for bad_variants in [
            vec![],
            vec![Arc::from("")],
            vec![Arc::from("a"), Arc::from("a")],
        ] {
            let mut d = mech.clone();
            d.params.push(EmbeddedParamSpec {
                name: "mode".into(),
                label: "Mode".into(),
                kind: ParamKind::Enum(bad_variants),
                default: ParamValue::Enum("a".into()),
                optional: false,
            });
            assert!(matches!(validate(&d), Err(DefError::InvalidSpec { .. })));
        }

        // Default faults: wrong kind, below min, above max, unknown variant, unparseable, raw/value mismatch, too long.
        let mut wrong_kind = resistor_def();
        wrong_kind.params[0].default = ParamValue::Int(10);
        assert_eq!(
            validate(&wrong_kind),
            Err(DefError::InvalidDefault {
                param: "resistance".into(),
                fault: ParamFault::Kind {
                    expected: "real",
                    actual: "int",
                },
            })
        );

        let mut below_min = resistor_def();
        below_min.params[0].default = real("0", Some("ohm"));
        assert_eq!(
            validate(&below_min),
            Err(DefError::InvalidDefault {
                param: "resistance".into(),
                fault: ParamFault::BelowMin,
            })
        );

        let mut above_max = resistor_def();
        above_max.params[0].default = real("1e15", Some("ohm"));
        assert_eq!(
            validate(&above_max),
            Err(DefError::InvalidDefault {
                param: "resistance".into(),
                fault: ParamFault::AboveMax,
            })
        );

        let mut raw_mismatch = resistor_def();
        raw_mismatch.params[0].default = ParamValue::Real {
            raw: "10M".into(),
            value: f(0.01),
        };
        assert_eq!(
            validate(&raw_mismatch),
            Err(DefError::InvalidDefault {
                param: "resistance".into(),
                fault: ParamFault::RawMismatch,
            })
        );

        let mut unparseable = resistor_def();
        unparseable.params[0].default = ParamValue::Real {
            raw: "10F".into(),
            value: f(10.0),
        };
        assert_eq!(
            validate(&unparseable),
            Err(DefError::InvalidDefault {
                param: "resistance".into(),
                fault: ParamFault::Unparseable(ValueError::InvalidSuffix),
            })
        );

        let mut long_text = mech.clone();
        long_text.params.push(EmbeddedParamSpec {
            name: "note".into(),
            label: "Note".into(),
            kind: ParamKind::Text,
            default: ParamValue::Text("t".repeat(MAX_PARAM_STRING_BYTES + 1).into()),
            optional: false,
        });
        assert_eq!(
            validate(&long_text),
            Err(DefError::InvalidDefault {
                param: "note".into(),
                fault: ParamFault::TooLong,
            })
        );

        let mut bad_enum_def = mech.clone();
        bad_enum_def.params.push(EmbeddedParamSpec {
            name: "wave".into(),
            label: "Wave".into(),
            kind: ParamKind::Enum(vec!["dc".into(), "sin".into()]),
            default: ParamValue::Enum("pulse".into()),
            optional: false,
        });
        assert_eq!(
            validate(&bad_enum_def),
            Err(DefError::InvalidDefault {
                param: "wave".into(),
                fault: ParamFault::UnknownVariant,
            })
        );

        let mut unknown_primary = resistor_def();
        unknown_primary.primary_param = Some("missing".into());
        assert_eq!(
            validate(&unknown_primary),
            Err(DefError::UnknownPrimary {
                name: "missing".into()
            })
        );
    }

    #[test]
    fn pd10_validate_model_subckt_screen_and_pk12_model_param() {
        // Model: case-insensitive prefix, continuation & inline comment in `.model` header.
        let mut d = diode_def();
        d.sim = EmbeddedSimBinding::Model {
            spice_prefix: 'd',
            entry: "diode_sig".into(),
            terminals: vec![0, 1],
            spice: "* comment\n.model\n+ DIODE_SIG;inline\n+ D(IS=1e-14)\n".into(),
            supply: None,
        };
        assert_eq!(validate(&d), Ok(()));

        // BJT (`Q`) accepts 3 or 4 terminals, refuses 2.
        let mut q = diode_def();
        q.sim = EmbeddedSimBinding::Model {
            spice_prefix: 'Q',
            entry: "NPN_GEN".into(),
            terminals: vec![0, 1],
            spice: ".model NPN_GEN NPN(BF=100)\n".into(),
            supply: None,
        };
        assert_eq!(
            validate(&q),
            Err(DefError::ModelTerminalCount {
                prefix: 'Q',
                actual: 2
            })
        );
        q.local_pins.push(Pt { x: 0, y: 10 });
        q.pins.push(pin("3", "e", PinRole::Passive));
        q.sim = EmbeddedSimBinding::Model {
            spice_prefix: 'Q',
            entry: "NPN_GEN".into(),
            terminals: vec![0, 1, 2],
            spice: ".model NPN_GEN NPN(BF=100)\n".into(),
            supply: None,
        };
        assert_eq!(validate(&q), Ok(()));

        // Unknown `spice_prefix`.
        let mut bad_prefix = diode_def();
        if let EmbeddedSimBinding::Model { spice_prefix, .. } = &mut bad_prefix.sim {
            *spice_prefix = 'X';
        }
        assert_eq!(validate(&bad_prefix), Err(DefError::SpicePrefix('X')));

        // Empty entry / empty spice / missing top-level entry (nested `.model` ignored).
        let mut empty_entry = diode_def();
        if let EmbeddedSimBinding::Model { entry, .. } = &mut empty_entry.sim {
            *entry = "  ".into();
        }
        assert_eq!(validate(&empty_entry), Err(DefError::EmptyEntry));

        let mut empty_spice = diode_def();
        if let EmbeddedSimBinding::Model { spice, .. } = &mut empty_spice.sim {
            *spice = " \n ".into();
        }
        assert_eq!(validate(&empty_spice), Err(DefError::EmptySpice));

        let mut nested_only = diode_def();
        if let EmbeddedSimBinding::Model { spice, .. } = &mut nested_only.sim {
            *spice = ".subckt WRAP a b\n.model DIODE_SIG D\n.ends;comment\n".into();
        }
        assert_eq!(
            validate(&nested_only),
            Err(DefError::MissingEntry {
                entry: "DIODE_SIG".into()
            })
        );

        // Oversized embedded SPICE deck (`PK7` 2 MiB).
        let mut huge = diode_def();
        if let EmbeddedSimBinding::Model { spice, .. } = &mut huge.sim {
            let mut s = String::from(".model DIODE_SIG D\n* ");
            s.push_str(&"x".repeat(MAX_SPICE_BYTES));
            *spice = s.into();
        }
        assert!(matches!(
            validate(&huge),
            Err(DefError::TooLong {
                field: "embedded SPICE text",
                max: MAX_SPICE_BYTES,
                ..
            })
        ));

        // `PK10` screen refusals on both `Model` and `Subckt`.
        for hostile in [
            ".model DIODE_SIG D\n.control\nshell rm -rf /\n.endc\n",
            ".model DIODE_SIG D\n.include other.lib\n",
            ".model DIODE_SIG D\n.lib other.lib TT\n",
            "\u{feff}.model DIODE_SIG D\n",
            ".model DIODE_SIG D\r\n",
            ".model DIODE_SIG D\n*\0\n",
        ] {
            let mut m = diode_def();
            if let EmbeddedSimBinding::Model { spice, .. } = &mut m.sim {
                *spice = hostile.into();
            }
            assert!(
                matches!(validate(&m), Err(DefError::Screen(_))),
                "hostile model deck must fail screen: {hostile:?}"
            );
        }

        // `Subckt`: port mismatch, missing entry, hostile screen, incomplete/mismatched blocks.
        let mut wrong_ports = opamp_def();
        if let EmbeddedSimBinding::Subckt { spice, .. } = &mut wrong_ports.sim {
            *spice = ".subckt OPAMP_GENERIC a b c d\n.ends\n".into();
        }
        assert_eq!(
            validate(&wrong_ports),
            Err(DefError::TerminalCount {
                expected: 4,
                actual: 5
            })
        );

        let mut missing_subckt = opamp_def();
        if let EmbeddedSimBinding::Subckt { entry, .. } = &mut missing_subckt.sim {
            *entry = "OTHER".into();
        }
        assert_eq!(
            validate(&missing_subckt),
            Err(DefError::MissingEntry {
                entry: "OTHER".into()
            })
        );

        let mut hostile_subckt = opamp_def();
        if let EmbeddedSimBinding::Subckt { spice, .. } = &mut hostile_subckt.sim {
            *spice = ".subckt OPAMP_GENERIC a b c d e\n.control\n.endc\n.ends\n".into();
        }
        assert!(matches!(
            validate(&hostile_subckt),
            Err(DefError::Screen(ScreenError::Forbidden { .. }))
        ));

        for malformed in [
            ".subckt OPAMP_GENERIC a b c d e\nE1 e 0 a b 1e5\n",
            ".subckt OPAMP_GENERIC a b c d e\n.ends OTHER\n",
            ".subckt OPAMP_GENERIC a b c d e\n.ends OPAMP_GENERIC\n.ends\n",
            "+ orphan\n.subckt OPAMP_GENERIC a b c d e\n.ends\n",
            ".subckt\n.ends\n.subckt OPAMP_GENERIC a b c d e\n.ends\n",
            ".subckt OPAMP_GENERIC a b c d e\n.ends\n.subckt opamp_generic x y\n.ends\n",
            ".subckt\u{a0}OUTER x y\n.subckt OPAMP_GENERIC a b c d e\n.ends\n.ends\n",
        ] {
            let mut bad = opamp_def();
            if let EmbeddedSimBinding::Subckt { spice, .. } = &mut bad.sim {
                *spice = malformed.into();
            }
            assert!(
                matches!(validate(&bad), Err(DefError::MalformedSpice(_))),
                "malformed subckt deck must be refused: {malformed:?}"
            );
        }

        // Non-ASCII whitespace must not split `.model` directives (matches `spice_screen.rs` / `spice.rs`).
        let mut nbsp_model = diode_def();
        if let EmbeddedSimBinding::Model { spice, .. } = &mut nbsp_model.sim {
            *spice = ".model\u{a0}DIODE_SIG D(IS=1e-14)\n".into();
        }
        assert_eq!(
            validate(&nbsp_model),
            Err(DefError::MissingEntry {
                entry: "DIODE_SIG".into()
            })
        );

        // Duplicate top-level `.model` is refused.
        let mut dup_model = diode_def();
        if let EmbeddedSimBinding::Model { spice, .. } = &mut dup_model.sim {
            *spice = ".model DIODE_SIG D(IS=1e-14)\n.model diode_sig D(IS=2e-14)\n".into();
        }
        assert!(matches!(
            validate(&dup_model),
            Err(DefError::MalformedSpice(_))
        ));

        // `PK12` `model` parameter: must be an enum whose variants name compatible embedded entries.
        let mut good_model_param = diode_def();
        if let EmbeddedSimBinding::Model { spice, .. } = &mut good_model_param.sim {
            *spice = ".model DIODE_SIG D(IS=1e-14)\n.model DIODE_FAST D(IS=2e-14)\n".into();
        }
        good_model_param.params.push(EmbeddedParamSpec {
            name: "model".into(),
            label: "Model".into(),
            kind: ParamKind::Enum(vec!["diode_sig".into(), "DIODE_FAST".into()]),
            default: ParamValue::Enum("diode_sig".into()),
            optional: false,
        });
        assert_eq!(validate(&good_model_param), Ok(()));

        // A `Model` binding refuses a `model` variant naming a `.subckt`.
        let mut model_names_subckt = diode_def();
        if let EmbeddedSimBinding::Model { spice, .. } = &mut model_names_subckt.sim {
            *spice = ".model DIODE_SIG D(IS=1e-14)\n.subckt HELPER a b\n.ends HELPER\n".into();
        }
        model_names_subckt.params.push(EmbeddedParamSpec {
            name: "model".into(),
            label: "Model".into(),
            kind: ParamKind::Enum(vec!["DIODE_SIG".into(), "HELPER".into()]),
            default: ParamValue::Enum("DIODE_SIG".into()),
            optional: false,
        });
        assert_eq!(
            validate(&model_names_subckt),
            Err(DefError::ModelParam {
                variant: Some("HELPER".into())
            })
        );

        // A `Subckt` binding accepts another top-level `.subckt` with the same port count,
        // and refuses both a `.model` helper and a `.subckt` helper with a different port count.
        let mut good_subckt_model_param = opamp_def();
        if let EmbeddedSimBinding::Subckt { spice, .. } = &mut good_subckt_model_param.sim {
            *spice = "\
.model DCLAMP D(IS=1e-14)
.subckt STAGE a b
R1 a b 1k
.ends STAGE
.subckt OPAMP_GENERIC inp inn vcc vee out
X1 inp inn STAGE
.ends OPAMP_GENERIC
.subckt OPAMP_FAST inp inn vcc vee out
X1 inp inn STAGE
.ends OPAMP_FAST
"
            .into();
        }
        good_subckt_model_param.params.push(EmbeddedParamSpec {
            name: "model".into(),
            label: "Model".into(),
            kind: ParamKind::Enum(vec!["OPAMP_GENERIC".into(), "opamp_fast".into()]),
            default: ParamValue::Enum("OPAMP_GENERIC".into()),
            optional: false,
        });
        assert_eq!(validate(&good_subckt_model_param), Ok(()));

        let mut subckt_names_model = good_subckt_model_param.clone();
        subckt_names_model.params[0].kind =
            ParamKind::Enum(vec!["OPAMP_GENERIC".into(), "DCLAMP".into()]);
        assert_eq!(
            validate(&subckt_names_model),
            Err(DefError::ModelParam {
                variant: Some("DCLAMP".into())
            })
        );

        let mut subckt_wrong_ports = good_subckt_model_param.clone();
        subckt_wrong_ports.params[0].kind =
            ParamKind::Enum(vec!["OPAMP_GENERIC".into(), "STAGE".into()]);
        assert_eq!(
            validate(&subckt_wrong_ports),
            Err(DefError::ModelParam {
                variant: Some("STAGE".into())
            })
        );

        let mut bad_variant = good_model_param.clone();
        bad_variant.params[0].kind = ParamKind::Enum(vec!["diode_sig".into(), "MISSING".into()]);
        assert_eq!(
            validate(&bad_variant),
            Err(DefError::ModelParam {
                variant: Some("MISSING".into())
            })
        );

        let mut non_enum_model = diode_def();
        non_enum_model.params.push(EmbeddedParamSpec {
            name: "model".into(),
            label: "Model".into(),
            kind: ParamKind::Text,
            default: ParamValue::Text("DIODE_SIG".into()),
            optional: false,
        });
        assert_eq!(
            validate(&non_enum_model),
            Err(DefError::ModelParam { variant: None })
        );

        // `model` param on a primitive (no embedded entries) is refused.
        let mut prim_model = resistor_def();
        prim_model.params.insert(
            0,
            EmbeddedParamSpec {
                name: "model".into(),
                label: "Model".into(),
                kind: ParamKind::Enum(vec!["X".into()]),
                default: ParamValue::Enum("X".into()),
                optional: false,
            },
        );
        assert_eq!(
            validate(&prim_model),
            Err(DefError::ModelParam {
                variant: Some("X".into())
            })
        );
    }

    #[test]
    fn pd5_validate_supply_decl_rules() {
        // Valid subckt with implicit ±15 V and without implicit fallback.
        let mut op = opamp_def();
        assert_eq!(validate(&op), Ok(()));
        if let EmbeddedSimBinding::Subckt { supply, .. } = &mut op.sim {
            supply.as_mut().unwrap().implicit = None;
        }
        assert_eq!(validate(&op), Ok(()));

        // Valid `SupplyDecl` on a `Model` binding (e.g. 4-terminal MOSFET with 2 power pins).
        let mut m = diode_def();
        m.local_pins = vec![
            Pt { x: 0, y: 0 },
            Pt { x: 10, y: 0 },
            Pt { x: 0, y: 10 },
            Pt { x: 10, y: 10 },
        ];
        m.pins = vec![
            pin("1", "d", PinRole::Passive),
            pin("2", "g", PinRole::Input),
            pin("3", "vdd", PinRole::Power),
            pin("4", "vss", PinRole::Power),
        ];
        m.sim = EmbeddedSimBinding::Model {
            spice_prefix: 'M',
            entry: "MOS_GEN".into(),
            terminals: vec![0, 1, 2, 3],
            spice: ".model MOS_GEN NMOS\n".into(),
            supply: Some(SupplyDecl {
                pos: 2,
                neg: 3,
                implicit: Some((f(5.0), f(0.0))),
            }),
        };
        assert_eq!(validate(&m), Ok(()));

        // Same pin for pos and neg -> SupplyPins.
        let mut same = opamp_def();
        if let EmbeddedSimBinding::Subckt { supply, .. } = &mut same.sim {
            supply.as_mut().unwrap().neg = 2;
        }
        assert_eq!(validate(&same), Err(DefError::SupplyPins));

        // Non-power pin (`Input`, `Output`, `Ground`, `Passive`) -> SupplyPins.
        for non_power in [0u16, 1, 4] {
            let mut d = opamp_def();
            if let EmbeddedSimBinding::Subckt { supply, .. } = &mut d.sim {
                supply.as_mut().unwrap().pos = non_power;
            }
            assert_eq!(validate(&d), Err(DefError::SupplyPins));
        }

        // Out-of-range pin -> SupplyPins.
        let mut oob = opamp_def();
        if let EmbeddedSimBinding::Subckt { supply, .. } = &mut oob.sim {
            supply.as_mut().unwrap().neg = 99;
        }
        assert_eq!(validate(&oob), Err(DefError::SupplyPins));

        // `NoConnect` pin not in terminals -> SupplyPins.
        let mut nc = opamp_def();
        nc.local_pins.push(Pt { x: 20, y: 0 });
        nc.pins.push(pin("6", "nc", PinRole::NoConnect));
        if let EmbeddedSimBinding::Subckt { supply, .. } = &mut nc.sim {
            supply.as_mut().unwrap().neg = 5;
        }
        assert_eq!(validate(&nc), Err(DefError::SupplyPins));

        // Implicit voltages with `pos <= neg` -> SupplyImplicit.
        for (pos, neg) in [(f(5.0), f(5.0)), (f(-5.0), f(5.0)), (f(0.0), f(0.0))] {
            let mut d = opamp_def();
            if let EmbeddedSimBinding::Subckt { supply, .. } = &mut d.sim {
                supply.as_mut().unwrap().implicit = Some((pos, neg));
            }
            assert_eq!(validate(&d), Err(DefError::SupplyImplicit));
        }
    }

    #[test]
    fn pd8_validate_instance_rules() {
        let def = resistor_def();
        let ok = |pins: &[Pt], bounds: Rect, val: &str, params: &Params| {
            validate_instance(&def, def.symbol, pins, bounds, val, params)
        };

        // Valid default instance.
        assert_eq!(
            ok(&def.local_pins, def.local_bounds, "10k", &Params::new()),
            Ok(())
        );

        // Valid override on primary param + non-primary param.
        let mut overrides = Params::new();
        overrides.insert("resistance".into(), real("4.7k", Some("ohm")));
        overrides.insert("tc1".into(), real("1e-3", Some("1/K")));
        assert_eq!(
            ok(&def.local_pins, def.local_bounds, "4.7k", &overrides),
            Ok(())
        );

        // Symbol mismatch.
        assert_eq!(
            validate_instance(
                &def,
                SymbolId(9),
                &def.local_pins,
                def.local_bounds,
                "10k",
                &Params::new(),
            ),
            Err(InstanceError::Symbol {
                expected: SymbolId(0),
                actual: SymbolId(9),
            })
        );

        // Two pins swapped at equal count -> LocalPins (`PD8`, `PTC-instance-refuse`).
        let swapped_pins = vec![def.local_pins[1], def.local_pins[0]];
        assert_eq!(swapped_pins.len(), def.local_pins.len());
        assert_eq!(
            ok(&swapped_pins, def.local_bounds, "10k", &Params::new()),
            Err(InstanceError::LocalPins)
        );
        // Different count or different coordinate -> LocalPins.
        assert_eq!(
            ok(
                &[def.local_pins[0]],
                def.local_bounds,
                "10k",
                &Params::new()
            ),
            Err(InstanceError::LocalPins)
        );
        // Bounds mismatch -> LocalBounds.
        let bad_bounds = Rect::new(Pt::ZERO, Pt::ZERO).unwrap();
        assert_eq!(
            ok(&def.local_pins, bad_bounds, "10k", &Params::new()),
            Err(InstanceError::LocalBounds)
        );

        // Undeclared override.
        let mut undeclared = Params::new();
        undeclared.insert("bogus".into(), ParamValue::Int(1));
        assert_eq!(
            ok(&def.local_pins, def.local_bounds, "10k", &undeclared),
            Err(InstanceError::UndeclaredParam {
                name: "bogus".into()
            })
        );

        // Wrong kind.
        let mut wrong_kind = Params::new();
        wrong_kind.insert("resistance".into(), ParamValue::Bool(true));
        assert_eq!(
            ok(&def.local_pins, def.local_bounds, "10k", &wrong_kind),
            Err(InstanceError::Param {
                name: "resistance".into(),
                fault: ParamFault::Kind {
                    expected: "real",
                    actual: "bool",
                },
            })
        );

        // Below min and above max.
        let mut low = Params::new();
        low.insert("resistance".into(), real("-5", Some("ohm")));
        assert_eq!(
            ok(&def.local_pins, def.local_bounds, "-5", &low),
            Err(InstanceError::Param {
                name: "resistance".into(),
                fault: ParamFault::BelowMin,
            })
        );
        let mut high = Params::new();
        high.insert("resistance".into(), real("1e15", Some("ohm")));
        assert_eq!(
            ok(&def.local_pins, def.local_bounds, "1e15", &high),
            Err(InstanceError::Param {
                name: "resistance".into(),
                fault: ParamFault::AboveMax,
            })
        );

        // Real `raw`/`value` disagreement (`PD6`, `PTC-instance-refuse` (e)).
        let mut mismatch = Params::new();
        mismatch.insert(
            "resistance".into(),
            ParamValue::Real {
                raw: "10M".into(),
                value: f(0.01),
            },
        );
        assert_eq!(
            ok(&def.local_pins, def.local_bounds, "10M", &mismatch),
            Err(InstanceError::Param {
                name: "resistance".into(),
                fault: ParamFault::RawMismatch,
            })
        );

        // Override equal to its default (`PD6`, `PTC-instance-refuse` (d)).
        let mut eq_default = Params::new();
        eq_default.insert("resistance".into(), real("10k", Some("ohm")));
        assert_eq!(
            ok(&def.local_pins, def.local_bounds, "10k", &eq_default),
            Err(InstanceError::DefaultOverride {
                name: "resistance".into()
            })
        );

        // `Part::value` mismatch against default and against override (`PD6`, `PTC-instance-refuse` (f)).
        assert_eq!(
            ok(&def.local_pins, def.local_bounds, "22k", &Params::new()),
            Err(InstanceError::Value {
                primary: "resistance".into()
            })
        );
        assert_eq!(
            ok(&def.local_pins, def.local_bounds, "10k", &overrides),
            Err(InstanceError::Value {
                primary: "resistance".into()
            })
        );

        // No `primary_param`: any `Part::value` is accepted.
        let op = opamp_def();
        assert_eq!(
            validate_instance(
                &op,
                op.symbol,
                &op.local_pins,
                op.local_bounds,
                "anything",
                &Params::new()
            ),
            Ok(())
        );

        // Int / Bool / Enum / Text primary and override checks.
        let mut multi =
            EmbeddedPartDef::unmodelled(SymbolId(1), &[], Rect::new(Pt::ZERO, Pt::ZERO).unwrap());
        multi.source_id = "test:multi".into();
        multi.params = vec![
            EmbeddedParamSpec {
                name: "flag".into(),
                label: "Flag".into(),
                kind: ParamKind::Bool,
                default: ParamValue::Bool(false),
                optional: false,
            },
            EmbeddedParamSpec {
                name: "mode".into(),
                label: "Mode".into(),
                kind: ParamKind::Enum(vec!["a".into(), "b".into()]),
                default: ParamValue::Enum("a".into()),
                optional: false,
            },
            EmbeddedParamSpec {
                name: "n".into(),
                label: "N".into(),
                kind: ParamKind::Int {
                    min: Some(1),
                    max: Some(10),
                },
                default: ParamValue::Int(5),
                optional: false,
            },
            EmbeddedParamSpec {
                name: "note".into(),
                label: "Note".into(),
                kind: ParamKind::Text,
                default: ParamValue::Text("ok".into()),
                optional: false,
            },
        ];
        multi.primary_param = Some("n".into());
        assert_eq!(validate(&multi), Ok(()));
        assert_eq!(
            validate_instance(
                &multi,
                SymbolId(1),
                &[],
                multi.local_bounds,
                "5",
                &Params::new()
            ),
            Ok(())
        );

        let mut bad_enum = Params::new();
        bad_enum.insert("mode".into(), ParamValue::Enum("z".into()));
        assert_eq!(
            validate_instance(&multi, SymbolId(1), &[], multi.local_bounds, "5", &bad_enum),
            Err(InstanceError::Param {
                name: "mode".into(),
                fault: ParamFault::UnknownVariant,
            })
        );

        let mut bad_int = Params::new();
        bad_int.insert("n".into(), ParamValue::Int(0));
        assert_eq!(
            validate_instance(&multi, SymbolId(1), &[], multi.local_bounds, "0", &bad_int),
            Err(InstanceError::Param {
                name: "n".into(),
                fault: ParamFault::BelowMin,
            })
        );

        let mut bad_text = Params::new();
        bad_text.insert(
            "note".into(),
            ParamValue::Text("x".repeat(MAX_PARAM_STRING_BYTES + 1).into()),
        );
        assert_eq!(
            validate_instance(&multi, SymbolId(1), &[], multi.local_bounds, "5", &bad_text),
            Err(InstanceError::Param {
                name: "note".into(),
                fault: ParamFault::TooLong,
            })
        );
    }

    #[test]
    fn pd8_doc_queries_equality_and_save_refusal() {
        let mut doc = Doc::new();
        let res = Arc::new(resistor_def());
        let mut overrides = Params::new();
        overrides.insert("resistance".into(), real("4.7k", Some("ohm")));

        let p0 = doc
            .insert_part_with_def(
                resistor_part(Pt::ZERO, "4.7k"),
                res.clone(),
                overrides.clone(),
            )
            .unwrap();
        let p1 = doc
            .insert_part_with_def(
                resistor_part(Pt { x: 40, y: 0 }, "10k"),
                res.clone(),
                Params::new(),
            )
            .unwrap();

        // Read queries.
        assert_eq!(doc.part_def_id(p0), Some(PartDefId(0)));
        assert_eq!(doc.part_def_id(p1), Some(PartDefId(0)));
        assert_eq!(doc.part_def_id(crate::PartId(99)), None);
        assert_eq!(doc.part_params(p0), Some(&overrides));
        assert_eq!(doc.part_params(p1), Some(&Params::new()));
        assert_eq!(doc.part_params(crate::PartId(99)), None);
        assert_eq!(
            doc.resolved_param(p0, "resistance"),
            Some(&real("4.7k", Some("ohm")))
        );
        assert_eq!(doc.resolved_param(p0, "tc1"), Some(&real("0", Some("1/K"))));
        assert_eq!(
            doc.resolved_param(p1, "resistance"),
            Some(&real("10k", Some("ohm")))
        );
        assert_eq!(doc.resolved_param(p0, "missing"), None);
        assert_eq!(doc.resolved_param(crate::PartId(99), "resistance"), None);

        // Save refusal (`PD8`, `PTC-no-lossy-save`): non-unmodelled referenced definition
        // returns `InvalidData` with the exact message and writes 0 bytes.
        let mut out = Vec::new();
        let err = doc.save(&mut out).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            err.to_string(),
            "document holds part definitions or parameters that this build cannot save"
        );
        assert!(out.is_empty(), "refused save must write nothing");

        // Removing both modelled parts leaves def 0 retained; adding an unmodelled part makes
        // the document v2-representable again (`PD8`: only referenced definitions block save).
        doc.remove_part(p0).unwrap();
        doc.remove_part(p1).unwrap();
        let unmod_pid = doc
            .insert_part(
                resistor_part(Pt { x: 80, y: 0 }, "1k"),
                &[Pt::ZERO],
                Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
            )
            .unwrap();
        let mut v2_bytes = Vec::new();
        doc.save(&mut v2_bytes).unwrap();
        assert!(!v2_bytes.is_empty());

        // Non-empty `params` on an unmodelled part is also refused (`PD8`).
        let mut with_params = doc.fork();
        with_params
            .parts
            .get_mut(unmod_pid.0)
            .unwrap()
            .params
            .insert("k".into(), ParamValue::Int(1));
        let mut out2 = Vec::new();
        let err2 = with_params.save(&mut out2).unwrap_err();
        assert_eq!(err2.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            err2.to_string(),
            "document holds part definitions or parameters that this build cannot save"
        );
        assert!(out2.is_empty());

        // `PartialEq` vs `content_eq`: retained records are ignored by `PartialEq`, while
        // referenced record differences and arena length differences matter.
        let mut a = doc.fork();
        let mut b = doc.fork();
        assert_eq!(a, b);
        assert!(a.content_eq(&b));
        // Mutating the retained record at id 0 does not break `PartialEq` or `content_eq`,
        // because neither side references id 0.
        let mut alt = resistor_def();
        alt.description = "different retained copy".into();
        b.part_defs.records[0] = Arc::new(alt);
        assert_eq!(a, b, "retained records are history state, not persistent");
        assert!(a.content_eq(&b));

        // Differing `params` breaks both `PartialEq` and `content_eq`.
        assert_ne!(a, with_params);
        assert!(!a.content_eq(&with_params));

        // Differing referenced `def` arena length breaks `PartialEq` even when live content matches.
        let extra_pid = a
            .insert_part(
                resistor_part(Pt { x: 120, y: 0 }, "1k"),
                &[Pt { x: 5, y: 0 }],
                Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
            )
            .unwrap();
        a.remove_part(extra_pid).unwrap();
        assert!(a.content_eq(&b));
        assert_ne!(a, b);
    }

    #[test]
    fn pinv7_refused_batch_rolls_back_interned_def_and_preserves_history() {
        let mut doc = Doc::new();
        let bounds_a = Rect::new(Pt { x: -2, y: -2 }, Pt { x: 2, y: 2 }).unwrap();
        let bounds_b = Rect::new(Pt { x: -4, y: -4 }, Pt { x: 4, y: 4 }).unwrap();
        let bounds_c = Rect::new(Pt { x: -6, y: -6 }, Pt { x: 6, y: 6 }).unwrap();

        // Build non-empty undo and redo stacks with 1 referenced def (0) and 1 retained def (1).
        doc.apply(Command::PlacePart {
            part: resistor_part(Pt::ZERO, "1k"),
            local_pins: vec![Pt::ZERO],
            local_bounds: bounds_a,
            def: PlaceDef::Unmodelled,
            params: Params::new(),
        })
        .unwrap();
        doc.apply(Command::PlacePart {
            part: resistor_part(Pt { x: 20, y: 0 }, "2k"),
            local_pins: vec![Pt::ZERO],
            local_bounds: bounds_b,
            def: PlaceDef::Unmodelled,
            params: Params::new(),
        })
        .unwrap();
        let after_b = doc.fork();
        doc.undo().unwrap();
        assert!(doc.can_undo());
        assert!(doc.can_redo());
        assert_eq!(doc.part_defs.len(), 2);
        assert!(doc.part_defs.is_referenced(PartDefId(0)));
        assert!(!doc.part_defs.is_referenced(PartDefId(1)));

        let before_snap = doc.snapshot();
        let before_cursor = doc.history_cursor();
        let before_defs_len = doc.part_defs.len();

        // Step 1 interns a brand-new unmodelled definition (id 2); step 2 fails.
        let err = doc.apply(Command::Batch(vec![
            Command::PlacePart {
                part: resistor_part(Pt { x: 40, y: 0 }, "3k"),
                local_pins: vec![Pt::ZERO],
                local_bounds: bounds_c,
                def: PlaceDef::Unmodelled,
                params: Params::new(),
            },
            Command::AddWires(vec![Wire {
                a: Pt::ZERO,
                b: Pt { x: 1, y: 1 },
            }]),
        ]));
        assert_eq!(err, Err(CommandError::InvalidWire));
        assert_eq!(doc.snapshot(), before_snap);
        assert_eq!(doc.part_defs.len(), before_defs_len);
        assert!(doc.part_def(PartDefId(2)).is_none());
        // History is outside `Snapshot` and must also be untouched (`PINV7`).
        assert_eq!(doc.history_cursor(), before_cursor);
        assert!(doc.can_undo());
        assert!(doc.can_redo());

        // Step 1 reuses an already-referenced definition (id 0); step 2 fails.
        let err2 = doc.apply(Command::Batch(vec![
            Command::PlacePart {
                part: resistor_part(Pt { x: 60, y: 0 }, "4k"),
                local_pins: vec![Pt::ZERO],
                local_bounds: bounds_a,
                def: PlaceDef::Unmodelled,
                params: Params::new(),
            },
            Command::AddWires(vec![Wire {
                a: Pt::ZERO,
                b: Pt { x: 1, y: 1 },
            }]),
        ]));
        assert_eq!(err2, Err(CommandError::InvalidWire));
        assert_eq!(doc.snapshot(), before_snap);
        assert_eq!(doc.history_cursor(), before_cursor);
        assert!(doc.can_undo());
        assert!(doc.can_redo());

        // Redo stack survived both refused batches intact and restores part B with id 1.
        doc.redo().unwrap();
        assert!(doc.content_eq(&after_b));
        assert_eq!(doc.part_def_id(crate::PartId(1)), Some(PartDefId(1)));
        assert!(doc.part_defs.is_referenced(PartDefId(1)));
    }
}
