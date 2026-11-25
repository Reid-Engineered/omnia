//! Version 1 TOML schemas (PK1, PD4).
//!
//! Use `PartToml::parse` and `ManifestToml::parse` to validate source text.
//! Model and symbol resolution happen after parsing.

use std::{collections::BTreeMap, fmt, str::FromStr, sync::Arc};

use omnia_doc::{FiniteF64, parse_value};
use serde::{Deserialize, Deserializer, de};

const SCHEMA: i64 = 1;
const MAX_PARAMS: usize = 64;
const MAX_PARAM_BYTES: usize = 256;
const MAX_DESCRIPTION_BYTES: usize = 2048;
const MAX_TAGS: usize = 64;
const MAX_TAG_BYTES: usize = 32;
const MAX_PINS: usize = 256;
const MAX_SPICE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PartIdSlug(Arc<str>);

impl PartIdSlug {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for PartIdSlug {
    type Err = InvalidPartId;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let initial = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
        match text.as_bytes() {
            [first, rest @ ..]
                if text.len() <= 64
                    && initial(*first)
                    && rest
                        .iter()
                        .all(|&b| initial(b) || matches!(b, b'.' | b'_' | b'-')) =>
            {
                Ok(Self(text.into()))
            }
            _ => Err(InvalidPartId),
        }
    }
}

impl fmt::Display for PartIdSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidPartId;

impl fmt::Display for InvalidPartId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("id must match [a-z0-9][a-z0-9._-]{0,63}")
    }
}

impl std::error::Error for InvalidPartId {}

impl<'de> Deserialize<'de> for PartIdSlug {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

#[derive(Debug)]
pub enum SchemaError {
    Toml(toml::de::Error),
    UnsupportedVersion(i64),
    InvalidField { field: String, reason: String },
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(error) => error.fmt(f),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported schema version {version}; expected {SCHEMA}")
            }
            Self::InvalidField { field, reason } => write!(f, "{field}: {reason}"),
        }
    }
}

impl std::error::Error for SchemaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Toml(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestToml {
    pub schema: i64,
    pub id: PartIdSlug,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub license: Option<String>,
}

impl ManifestToml {
    pub fn parse(text: &str) -> Result<Self, SchemaError> {
        let manifest: Self = parse_toml(text, |manifest: &Self| manifest.schema)?;
        check_cap(
            "description",
            manifest.description.len(),
            MAX_DESCRIPTION_BYTES,
            "bytes",
        )?;
        Ok(manifest)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartToml {
    pub schema: i64,
    pub id: PartIdSlug,
    pub name: String,
    pub category: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub datasheet: Option<String>,
    pub designator_prefix: Option<String>,
    pub primary_param: Option<String>,
    pub symbol: SymbolToml,
    #[serde(default)]
    pub pins: Vec<PinToml>,
    #[serde(default)]
    pub params: BTreeMap<String, ParamToml>,
    pub sim: SimToml,
    pub license: Option<LicenseToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolToml {
    pub builtin: String,
    #[serde(default)]
    pub default: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinToml {
    pub number: String,
    pub name: String,
    pub role: PinRole,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PinRole {
    Passive,
    Input,
    Output,
    Bidir,
    Power,
    Ground,
    NoConnect,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamToml {
    pub kind: ParamKind,
    pub default: ParamLiteral,
    pub unit: Option<String>,
    pub min: Option<ParamLiteral>,
    pub max: Option<ParamLiteral>,
    pub variants: Option<Vec<String>>,
    pub label: Option<String>,
    #[serde(default)]
    pub optional: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    Real,
    Int,
    Bool,
    Enum,
    Text,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ParamLiteral {
    String(String),
    Int(i64),
    Bool(bool),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SimToml {
    Primitive {
        primitive: PrimitiveKind,
        #[serde(default)]
        pins: Vec<String>,
    },
    Model {
        spice_prefix: char,
        entry: String,
        file: Option<String>,
        spice: Option<String>,
        #[serde(default)]
        pins: Vec<String>,
        supply: Option<SupplyToml>,
    },
    Subckt {
        entry: String,
        file: Option<String>,
        spice: Option<String>,
        #[serde(default)]
        pins: Vec<String>,
        supply: Option<SupplyToml>,
    },
    None {
        #[serde(default)]
        pins: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplyToml {
    pub pins: [String; 2],
    pub implicit: Option<[String; 2]>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LicenseToml {
    pub spdx: String,
    pub source: Option<String>,
}

fn parse_toml<T: de::DeserializeOwned>(
    text: &str,
    version: impl FnOnce(&T) -> i64,
) -> Result<T, SchemaError> {
    #[derive(Deserialize)]
    struct Header {
        schema: i64,
    }
    match toml::from_str(text) {
        Ok(dto) => match version(&dto) {
            SCHEMA => Ok(dto),
            version => Err(SchemaError::UnsupportedVersion(version)),
        },
        Err(error) => {
            // A future schema can use fields unknown to this version.
            if let Ok(header) = toml::from_str::<Header>(text)
                && header.schema != SCHEMA
            {
                return Err(SchemaError::UnsupportedVersion(header.schema));
            }
            Err(SchemaError::Toml(error))
        }
    }
}

fn invalid(field: impl Into<String>, reason: impl Into<String>) -> SchemaError {
    SchemaError::InvalidField {
        field: field.into(),
        reason: reason.into(),
    }
}

fn check_cap(field: &str, count: usize, limit: usize, unit: &str) -> Result<(), SchemaError> {
    if count > limit {
        return Err(invalid(
            field,
            format!("limit is {limit} {unit}, got {count}"),
        ));
    }
    Ok(())
}

impl PartToml {
    /// Reads and validates v1 part metadata, including terminal order and parameter defaults.
    pub fn parse(text: &str) -> Result<Self, SchemaError> {
        let part: Self = parse_toml(text, |part: &Self| part.schema)?;
        part.validate()?;
        Ok(part)
    }

    fn validate(&self) -> Result<(), SchemaError> {
        check_cap(
            "description",
            self.description.len(),
            MAX_DESCRIPTION_BYTES,
            "bytes",
        )?;
        check_cap("tags", self.tags.len(), MAX_TAGS, "tags")?;
        for tag in &self.tags {
            check_cap("tags", tag.len(), MAX_TAG_BYTES, "bytes per tag")?;
        }
        check_cap("pins", self.pins.len(), MAX_PINS, "pins")?;
        check_cap("params", self.params.len(), MAX_PARAMS, "parameters")?;
        if let Some(primary) = &self.primary_param {
            check_cap("primary_param", primary.len(), MAX_PARAM_BYTES, "bytes")?;
            if !self.params.contains_key(primary) {
                return Err(invalid("primary_param", "must name a declared parameter"));
            }
        }
        for (name, param) in &self.params {
            check_cap("parameter name", name.len(), MAX_PARAM_BYTES, "bytes")?;
            param.validate(name)?;
        }
        self.sim.validate(&self.pins)?;
        if let SimToml::Primitive {
            primitive: PrimitiveKind::Rail,
            pins,
        } = &self.sim
            && (pins.len() != 1
                || !self.params.get("dc").is_some_and(|param| {
                    param.kind == ParamKind::Real && param.unit.as_deref() == Some("V")
                }))
        {
            return Err(invalid(
                "sim.primitive",
                "rail requires one terminal and a real dc parameter in V",
            ));
        }
        Ok(())
    }
}

impl ParamToml {
    fn validate(&self, name: &str) -> Result<(), SchemaError> {
        let field = format!("params.{name}");
        let fail = |reason| invalid(&field, reason);
        let strings = self
            .unit
            .iter()
            .chain(&self.label)
            .chain(self.variants.iter().flatten());
        let literals = std::iter::once(&self.default)
            .chain(&self.min)
            .chain(&self.max);
        for text in strings.chain(literals.filter_map(|literal| match literal {
            ParamLiteral::String(text) => Some(text),
            _ => None,
        })) {
            check_cap(
                &field,
                text.len(),
                MAX_PARAM_BYTES,
                "bytes per parameter string",
            )?;
        }

        if self.kind != ParamKind::Real && self.unit.is_some() {
            return Err(fail("unit is only valid for real parameters"));
        }
        if !matches!(self.kind, ParamKind::Real | ParamKind::Int)
            && (self.min.is_some() || self.max.is_some())
        {
            return Err(fail("min and max require a numeric parameter"));
        }
        if self.kind != ParamKind::Enum && self.variants.is_some() {
            return Err(fail("variants require an enum parameter"));
        }

        match (self.kind, &self.default) {
            (ParamKind::Real, _) => {
                let parse = |literal: &ParamLiteral| match literal {
                    ParamLiteral::String(raw) => parse_value(raw, self.unit.as_deref())
                        .map_err(|error| invalid(&field, error.to_string())),
                    _ => Err(fail("real values must be decimal strings")),
                };
                check_bounds(
                    &field,
                    parse(&self.default)?,
                    self.min.as_ref().map(parse).transpose()?,
                    self.max.as_ref().map(parse).transpose()?,
                )
            }
            (ParamKind::Int, ParamLiteral::Int(default)) => {
                let parse = |literal: &ParamLiteral| match literal {
                    ParamLiteral::Int(value) => Ok(*value),
                    _ => Err(fail("integer bounds must be TOML integers")),
                };
                check_bounds(
                    &field,
                    *default,
                    self.min.as_ref().map(parse).transpose()?,
                    self.max.as_ref().map(parse).transpose()?,
                )
            }
            (ParamKind::Enum, ParamLiteral::String(default)) => {
                if self
                    .variants
                    .as_ref()
                    .is_some_and(|variants| variants.contains(default))
                {
                    Ok(())
                } else {
                    Err(fail("default must name a declared enum variant"))
                }
            }
            (ParamKind::Bool, ParamLiteral::Bool(_))
            | (ParamKind::Text, ParamLiteral::String(_)) => Ok(()),
            _ => Err(fail("default must match the parameter kind")),
        }
    }
}

fn check_bounds<T: Ord>(
    field: &str,
    default: T,
    min: Option<T>,
    max: Option<T>,
) -> Result<(), SchemaError> {
    if min.is_some_and(|min| default < min) || max.is_some_and(|max| default > max) {
        return Err(invalid(field, "default must be within min and max"));
    }
    Ok(())
}

impl SimToml {
    fn validate(&self, pins: &[PinToml]) -> Result<(), SchemaError> {
        let terminals = match self {
            Self::Primitive { pins, .. } | Self::None { pins } => pins,
            Self::Model {
                file,
                spice,
                pins: terminals,
                supply,
                ..
            }
            | Self::Subckt {
                file,
                spice,
                pins: terminals,
                supply,
                ..
            } => {
                match (file, spice) {
                    (Some(_), None) => {}
                    (None, Some(spice)) => {
                        check_cap("sim.spice", spice.len(), MAX_SPICE_BYTES, "bytes")?
                    }
                    _ => {
                        return Err(invalid(
                            "sim",
                            "model and subckt require exactly one of file and spice",
                        ));
                    }
                }
                if let Some(supply) = supply {
                    supply.validate(terminals, pins)?;
                }
                terminals
            }
        };

        let mut by_number = BTreeMap::new();
        for pin in pins {
            if by_number.insert(pin.number.as_str(), pin.role).is_some() {
                return Err(invalid(
                    "pins",
                    format!("duplicate pin number {:?}", pin.number),
                ));
            }
        }
        by_number.retain(|_, role| *role != PinRole::NoConnect);
        for terminal in terminals {
            if by_number.remove(terminal.as_str()).is_none() {
                return Err(invalid(
                    "sim.pins",
                    format!("unknown, repeated or no_connect pin {terminal:?}"),
                ));
            }
        }
        if let Some((missing, _)) = by_number.first_key_value() {
            return Err(invalid(
                "sim.pins",
                format!("missing terminal for pin {missing:?}"),
            ));
        }
        Ok(())
    }
}

impl SupplyToml {
    /// Returns the parsed fallback voltages in positive, negative terminal order.
    pub fn implicit_volts(&self) -> Result<Option<[FiniteF64; 2]>, SchemaError> {
        self.implicit
            .as_ref()
            .map(|[pos, neg]| {
                let parse = |text| {
                    parse_value(text, Some("V"))
                        .map_err(|error| invalid("sim.supply.implicit", error.to_string()))
                };
                let volts = [parse(pos.as_str())?, parse(neg.as_str())?];
                if volts[0] <= volts[1] {
                    return Err(invalid(
                        "sim.supply.implicit",
                        "positive voltage must exceed negative voltage",
                    ));
                }
                Ok(volts)
            })
            .transpose()
    }

    fn validate(&self, terminals: &[String], pins: &[PinToml]) -> Result<(), SchemaError> {
        if self.pins[0] == self.pins[1]
            || !self.pins.iter().all(|number| {
                terminals.contains(number)
                    && pins
                        .iter()
                        .any(|pin| pin.number == *number && pin.role == PinRole::Power)
            })
        {
            return Err(invalid(
                "sim.supply.pins",
                "must name two distinct power pins in sim.pins",
            ));
        }
        self.implicit_volts()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // PD4 examples: resistor.toml and opamp-generic.toml.
    const RESISTOR: &str = r#"
schema = 1
id = "resistor"
name = "Resistor"
category = "Passives"
description = "Ideal linear two-terminal resistor"
tags = ["passive", "resistor", "linear"]
designator_prefix = "R"
primary_param = "resistance"

[symbol]
builtin = "Resistor-IEEE-Standard"
default = true

[[pins]]
number = "1"
name = "a"
role = "passive"

[[pins]]
number = "2"
name = "b"
role = "passive"

[params.resistance]
kind = "real"
unit = "ohm"
default = "10k"
min = "1e-6"
label = "Resistance"

[params.tc1]
kind = "real"
unit = "1/K"
default = "0"
label = "Linear tempco"
optional = true

[sim]
kind = "primitive"
primitive = "resistor"
pins = ["1", "2"]
"#;

    const OPAMP: &str = r#"
schema = 1
id = "opamp-generic"
name = "Op-amp (generic)"
category = "Op-Amps"
description = "Behavioural voltage-feedback operational amplifier macromodel"
tags = ["opamp", "analog"]
designator_prefix = "U"

[symbol]
builtin = "IC-COM-OpAmp"
default = true             # default definition for this symbol (PD11)

[[pins]]
number = "3"
name = "in+"
role = "input"

[[pins]]
number = "2"
name = "in-"
role = "input"

[[pins]]
number = "8"
name = "v+"
role = "power"

[[pins]]
number = "4"
name = "v-"
role = "power"

[[pins]]
number = "1"
name = "out"
role = "output"

[sim]
kind = "subckt"
file = "models/opamps/opamp-generic.lib"
entry = "OPAMP_GENERIC"
# SPICE terminal order, matching `.subckt OPAMP_GENERIC inp inn vcc vee out`
pins = ["3", "2", "8", "4", "1"]

[sim.supply]
pins = ["8", "4"]          # positive, then negative supply terminal
implicit = ["15", "-15"]   # optional implicit-supply fallback voltages (V)

[license]
spdx = "CC0-1.0"
source = "models/opamps/opamp-generic.lib"
"#;

    const MANIFEST: &str = r#"
schema = 1
id = "test-pack"
name = "Test pack"
version = "1.0.0"
"#;

    #[test]
    fn pd4_examples() {
        let part = PartToml::parse(RESISTOR).unwrap();
        assert_eq!(part.id.as_str(), "resistor");
        assert_eq!(part.primary_param.as_deref(), Some("resistance"));
        assert_eq!(part.params["resistance"].unit.as_deref(), Some("ohm"));
        let SimToml::Primitive { primitive, pins } = part.sim else {
            panic!("expected primitive")
        };
        assert_eq!(primitive, PrimitiveKind::Resistor);
        assert_eq!(pins, ["1", "2"]);

        let part = PartToml::parse(OPAMP).unwrap();
        assert_eq!(part.id.as_str(), "opamp-generic");
        assert!(part.symbol.default);
        let SimToml::Subckt {
            entry,
            pins,
            supply: Some(supply),
            ..
        } = part.sim
        else {
            panic!("expected subckt with supply")
        };
        assert_eq!(entry, "OPAMP_GENERIC");
        assert_eq!(pins, ["3", "2", "8", "4", "1"]);
        assert_eq!(supply.pins, ["8", "4"]);
        assert_eq!(
            supply
                .implicit_volts()
                .unwrap()
                .unwrap()
                .map(FiniteF64::get),
            [15.0, -15.0]
        );
    }

    #[test]
    fn pk1_pk2_and_unknown_fields() {
        assert_eq!(
            ManifestToml::parse(MANIFEST).unwrap().id.as_str(),
            "test-pack"
        );
        for bad in ["", "-bad", "_bad", ".bad", "Bad", "a/b", "é", "a b", "a🦀"] {
            assert!(bad.parse::<PartIdSlug>().is_err(), "{bad}");
            assert!(ManifestToml::parse(&MANIFEST.replace("test-pack", bad)).is_err());
            assert!(
                PartToml::parse(&RESISTOR.replace("id = \"resistor\"", &format!("id = {bad:?}")))
                    .is_err()
            );
        }
        assert!("a".repeat(64).parse::<PartIdSlug>().is_ok());
        assert!("a".repeat(65).parse::<PartIdSlug>().is_err());
        assert!(matches!(
            PartToml::parse("schema = 2\nfuture = true"),
            Err(SchemaError::UnsupportedVersion(2))
        ));
        assert!(matches!(
            ManifestToml::parse(&MANIFEST.replace("schema = 1", "schema = -1")),
            Err(SchemaError::UnsupportedVersion(-1))
        ));
        assert!(ManifestToml::parse(&format!("{MANIFEST}unknown = true")).is_err());
        for table in [
            "",
            "[symbol]",
            "[[pins]]",
            "[params.resistance]",
            "[sim]",
            "[sim.supply]",
            "[license]",
        ] {
            let malformed = if table.is_empty() {
                format!("unknown = true\n{OPAMP}")
            } else {
                let source = if table == "[params.resistance]" {
                    RESISTOR
                } else {
                    OPAMP
                };
                source.replacen(table, &format!("{table}\nunknown = true"), 1)
            };
            assert!(PartToml::parse(&malformed).is_err(), "{table}");
        }
    }

    #[test]
    fn pd4_pd5_pin_mapping() {
        let extra = RESISTOR.replace(
            "[params.resistance]",
            r#"
[[pins]]
number = "3"
name = "unused"
role = "no_connect"
[params.resistance]"#,
        );
        assert!(PartToml::parse(&extra).is_ok());
        assert!(
            PartToml::parse(&extra.replace(
                "role = \"no_connect\"",
                "role = \"passive\"\nno_connect = true"
            ))
            .is_err()
        );
        for terminals in [
            r#"["1"]"#,
            r#"["1", "1"]"#,
            r#"["1", "3"]"#,
            r#"["1", "2", "3"]"#,
            r#"["1", "2", "4"]"#,
        ] {
            assert!(
                PartToml::parse(&extra.replace(r#"["1", "2"]"#, terminals)).is_err(),
                "{terminals}"
            );
        }
        assert!(PartToml::parse(&extra.replace("number = \"3\"", "number = \"2\"")).is_err());
        assert!(PartToml::parse(&extra.replace(r#"["1", "2"]"#, r#"["2", "1"]"#)).is_ok());
    }

    #[test]
    fn pd5_supply_validation() {
        let part =
            PartToml::parse(&OPAMP.replace(r#"["15", "-15"]"#, r#"["+15 V", "-15V"]"#)).unwrap();
        let SimToml::Subckt {
            supply: Some(supply),
            ..
        } = part.sim
        else {
            panic!("missing supply")
        };
        assert_eq!(
            supply
                .implicit_volts()
                .unwrap()
                .unwrap()
                .map(FiniteF64::get),
            [15.0, -15.0]
        );
        assert!(PartToml::parse(&OPAMP.replace("implicit =", "# implicit =")).is_ok());
        for (from, bad) in [
            (r#"["8", "4"]"#, r#"["8", "8"]"#),
            (r#"["8", "4"]"#, r#"["3", "4"]"#),
            (r#"["8", "4"]"#, r#"["9", "4"]"#),
            (r#"["8", "4"]"#, r#"["8"]"#),
            (r#"["3", "2", "8", "4", "1"]"#, r#"["3", "2", "4", "1"]"#),
            (r#"["15", "-15"]"#, r#"["15 A", "-15V"]"#),
            (r#"["15", "-15"]"#, r#"["1e309", "-15"]"#),
            (r#"["15", "-15"]"#, r#"["-15", "15"]"#),
            (r#"["15", "-15"]"#, r#"["15", "15"]"#),
            (r#"["15", "-15"]"#, r#"["15"]"#),
        ] {
            assert!(PartToml::parse(&OPAMP.replace(from, bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn pd4_model_and_spice_source() {
        let path = r#"file = "models/opamps/opamp-generic.lib""#;
        let model = OPAMP.replace(
            r#"kind = "subckt""#,
            "kind = \"model\"\nspice_prefix = \"Q\"",
        );
        assert!(matches!(
            PartToml::parse(&model).unwrap().sim,
            SimToml::Model {
                spice_prefix: 'Q',
                ..
            }
        ));
        assert!(PartToml::parse(&model.replace("spice_prefix =", "# spice_prefix =")).is_err());
        assert!(PartToml::parse(&OPAMP.replace(path, "")).is_err());
        assert!(PartToml::parse(&OPAMP.replace(path, "spice = \"model text\"")).is_ok());
        assert!(
            PartToml::parse(&OPAMP.replace(path, &format!("{path}\nspice = \"model text\"")))
                .is_err()
        );
    }

    #[test]
    fn pd6_parameter_kinds_and_pk4_values() {
        let part = format!(
            r#"{RESISTOR}
[params.int]
kind = "int"
default = 9223372036854775807
min = 0
[params.bool]
kind = "bool"
default = true
[params.mode]
kind = "enum"
default = "dc"
variants = ["dc", "sin"]
[params.text]
kind = "text"
default = "example"
"#
        );
        assert!(PartToml::parse(&part).is_ok());
        assert!(
            PartToml::parse(&part.replace(r#"default = "dc""#, r#"default = "pulse""#)).is_err()
        );
        let real = RESISTOR.replace(r#"min = "1e-6""#, "min = \"1 kΩ\"\nmax = \"1Mohm\"");
        assert!(PartToml::parse(&real).is_ok());
        for (from, bad) in [
            ("10k", "4k7"),
            ("10k", "1e309"),
            ("10k", "1F"),
            ("1 kΩ", "1  kΩ"),
            ("1Mohm", "1F"),
            ("1Mohm", "1ohm"),
        ] {
            assert!(PartToml::parse(&real.replace(from, bad)).is_err(), "{bad}");
        }
        assert!(PartToml::parse(&real.replace("10k", "1 kΩ")).is_ok());
    }

    #[test]
    fn pk6_pk12_rail() {
        let rail = r#"
schema = 1
id = "rail"
name = "Rail"
category = "Power"
[symbol]
builtin = "rail"
[[pins]]
number = "1"
name = "rail"
role = "power"
[params.dc]
kind = "real"
unit = "V"
default = "5V"
[sim]
kind = "primitive"
primitive = "rail"
pins = ["1"]
"#;
        assert!(matches!(
            PartToml::parse(rail).unwrap().sim,
            SimToml::Primitive {
                primitive: PrimitiveKind::Rail,
                ..
            }
        ));
        assert!(PartToml::parse(&rail.replace("[params.dc]", "[params.voltage]")).is_err());
        assert!(PartToml::parse(&rail.replace("\"V\"", "\"A\"")).is_err());
    }

    #[test]
    fn pk7_field_caps_count_utf8_bytes() {
        fn check(make_part: impl Fn(usize) -> String, cap: usize, expected_field: &str) {
            assert!(PartToml::parse(&make_part(cap)).is_ok(), "{expected_field}");
            assert!(
                matches!(
                    PartToml::parse(&make_part(cap + 1)),
                    Err(SchemaError::InvalidField { field, .. }) if field == expected_field
                ),
                "{expected_field}"
            );
        }

        fn sized(size: usize) -> String {
            format!("{}{}", "é".repeat(size / 2), "x".repeat(size % 2))
        }

        check(
            |size| RESISTOR.replace("Ideal linear two-terminal resistor", &sized(size)),
            2048,
            "description",
        );
        check(
            |size| {
                RESISTOR.replace(
                    "\"passive\", \"resistor\", \"linear\"",
                    &vec!["\"tag\""; size].join(","),
                )
            },
            64,
            "tags",
        );
        check(
            |size| RESISTOR.replace("\"linear\"", &format!("\"{}\"", sized(size))),
            32,
            "tags",
        );
        check(
            |size| RESISTOR.replace("Linear tempco", &sized(size)),
            256,
            "params.tc1",
        );
        check(
            |size| RESISTOR.replace("\"0\"", &format!("\"0{}\"", " ".repeat(size - 1))),
            256,
            "params.tc1",
        );
        check(
            |size| {
                let mut part = RESISTOR.to_owned();
                for i in 2..size {
                    part.push_str(&format!(
                        "\n[params.p{i}]\nkind = \"bool\"\ndefault = true\n"
                    ));
                }
                part
            },
            64,
            "params",
        );
        check(
            |size| {
                let mut part = RESISTOR.to_owned();
                for i in 2..size {
                    part.push_str(&format!(
                        "\n[[pins]]\nnumber = \"p{i}\"\nname = \"unused\"\nrole = \"no_connect\"\n"
                    ));
                }
                part
            },
            256,
            "pins",
        );
        check(
            |size| {
                OPAMP.replace(
                    "file = \"models/opamps/opamp-generic.lib\"",
                    &format!("spice = \"{}\"", sized(size)),
                )
            },
            2 * 1024 * 1024,
            "sim.spice",
        );
    }

    proptest! {
        #[test]
        fn pk2_slug_length_and_alphabet(initial in 0usize..36, rest in prop::collection::vec(0usize..39, 0..80)) {
            let alphabet = b"abcdefghijklmnopqrstuvwxyz0123456789._-";
            let mut text = String::from(char::from(alphabet[initial]));
            text.extend(rest.iter().map(|index| char::from(alphabet[*index])));
            match text.parse::<PartIdSlug>() {
                Ok(id) => {
                    prop_assert!(text.len() <= 64);
                    prop_assert_eq!(id.as_str(), text.as_str());
                }
                Err(_) => prop_assert!(text.len() > 64),
            }
            let bad_initial = format!("-{text}");
            let qualified = format!("{text}/a");
            prop_assert!(bad_initial.parse::<PartIdSlug>().is_err());
            prop_assert!(qualified.parse::<PartIdSlug>().is_err());
        }
    }
}
