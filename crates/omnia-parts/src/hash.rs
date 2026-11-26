//! PK5 canonical encoding of a resolved v1 source definition.

use std::fmt;

use sha2::{Digest, Sha256};

use crate::schema::{ParamKind, ParamLiteral, PartToml, PinRole, PrimitiveKind, SimToml};

/// SHA-256 of canonical part content, independent of its library provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentHash(pub [u8; 32]);

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Each atom is prefixed by its little-endian u64 byte length. Records have fixed field
/// order, options start with a presence atom, and sequences start with a count atom.
struct Encoder(Sha256);

impl Encoder {
    fn bytes(&mut self, bytes: &[u8]) {
        self.0.update((bytes.len() as u64).to_le_bytes());
        self.0.update(bytes);
    }

    fn text(&mut self, text: &str) {
        self.bytes(text.as_bytes());
    }

    fn count(&mut self, count: usize) {
        self.bytes(&(count as u64).to_le_bytes());
    }

    fn option<T>(&mut self, value: Option<T>, field: impl FnOnce(&mut Self, T)) {
        self.bytes(&[u8::from(value.is_some())]);
        if let Some(value) = value {
            field(self, value);
        }
    }

    fn texts(&mut self, strings: &[String]) {
        self.count(strings.len());
        for string in strings {
            self.text(string);
        }
    }

    fn literal(&mut self, value: &ParamLiteral) {
        match value {
            ParamLiteral::String(text) => {
                self.text("string");
                self.text(text);
            }
            ParamLiteral::Int(int) => {
                self.text("int");
                self.bytes(&int.to_le_bytes());
            }
            ParamLiteral::Bool(boolean) => {
                self.text("bool");
                self.bytes(&[u8::from(*boolean)]);
            }
        }
    }
}

/// Hash the parsed definition with its extracted SPICE. Source identifiers and the
/// location or inline origin of the extracted deck are outside the content encoding.
pub(crate) fn hash_part(part: &PartToml, spice: Option<&str>) -> ContentHash {
    let mut out = Encoder(Sha256::new());
    out.bytes(&part.schema.to_le_bytes());
    out.text(&part.name);
    out.text(&part.category);
    out.text(&part.description);
    out.texts(&part.tags);
    out.option(part.datasheet.as_deref(), Encoder::text);
    out.option(part.designator_prefix.as_deref(), Encoder::text);
    out.option(part.primary_param.as_deref(), Encoder::text);
    out.text(&part.symbol.builtin);
    out.bytes(&[u8::from(part.symbol.default)]);

    let mut pins: Vec<_> = part.pins.iter().collect();
    pins.sort_unstable_by(|a, b| a.number.cmp(&b.number));
    out.count(pins.len());
    for pin in pins {
        out.text(&pin.number);
        out.text(&pin.name);
        out.text(match pin.role {
            PinRole::Passive => "passive",
            PinRole::Input => "input",
            PinRole::Output => "output",
            PinRole::Bidir => "bidir",
            PinRole::Power => "power",
            PinRole::Ground => "ground",
            PinRole::NoConnect => "no_connect",
        });
    }

    out.count(part.params.len());
    for (name, param) in &part.params {
        out.text(name);
        out.text(match param.kind {
            ParamKind::Real => "real",
            ParamKind::Int => "int",
            ParamKind::Bool => "bool",
            ParamKind::Enum => "enum",
            ParamKind::Text => "text",
        });
        out.literal(&param.default);
        out.option(param.unit.as_deref(), Encoder::text);
        out.option(param.min.as_ref(), Encoder::literal);
        out.option(param.max.as_ref(), Encoder::literal);
        out.option(param.variants.as_deref(), Encoder::texts);
        out.option(param.label.as_deref(), Encoder::text);
        out.bytes(&[u8::from(param.optional)]);
    }

    match &part.sim {
        SimToml::None => out.text("none"),
        SimToml::Primitive { primitive, pins } => {
            out.text("primitive");
            out.text(match primitive {
                PrimitiveKind::Resistor => "resistor",
                PrimitiveKind::Capacitor => "capacitor",
                PrimitiveKind::Inductor => "inductor",
                PrimitiveKind::Potentiometer => "potentiometer",
                PrimitiveKind::VSource => "vsource",
                PrimitiveKind::ISource => "isource",
                PrimitiveKind::Vcvs => "vcvs",
                PrimitiveKind::Cccs => "cccs",
                PrimitiveKind::Vccs => "vccs",
                PrimitiveKind::Ccvs => "ccvs",
                PrimitiveKind::Ground => "ground",
                PrimitiveKind::Switch => "switch",
                PrimitiveKind::Rail => "rail",
            });
            out.texts(pins);
        }
        SimToml::Model {
            entry,
            pins,
            supply,
            ..
        }
        | SimToml::Subckt {
            entry,
            pins,
            supply,
            ..
        } => {
            if let SimToml::Model { spice_prefix, .. } = &part.sim {
                out.text("model");
                out.bytes(&u32::from(*spice_prefix).to_le_bytes());
            } else {
                out.text("subckt");
            }
            out.text(entry);
            out.texts(pins);
            out.option(supply.as_ref(), |out, supply| {
                out.text(&supply.pins[0]);
                out.text(&supply.pins[1]);
                out.option(supply.implicit.as_ref(), |out, [pos, neg]| {
                    out.text(pos);
                    out.text(neg);
                });
            });
        }
    }
    out.option(spice, Encoder::text);
    out.option(part.license.as_ref(), |out, license| {
        out.text(&license.spdx);
        out.option(license.source.as_deref(), Encoder::text);
    });
    ContentHash(out.0.finalize().into())
}
