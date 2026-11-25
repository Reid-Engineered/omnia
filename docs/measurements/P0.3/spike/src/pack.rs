//! Deterministic generator for the 500-part `PB3` index fixture.
//!
//! The pack mirrors `library/` (`PD1`, `PD2`): `manifest.toml`, `parts/<category>/<id>.toml`
//! and shared `models/<family>/<file>.lib` decks, ten parts per deck. Part files follow the
//! `PD4` examples (pins, typed parameters, `[sim]`, `[sim.supply]`, `[license]`). The ZIP is
//! written in a fixed pseudo-random entry order with fixed timestamps, so the same inputs
//! give byte-identical archives and the index has to sort for itself.

use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

pub const PACK_ID: &str = "spike-pack";

/// The pack's files, sorted by archive path.
pub struct Pack {
    pub files: Vec<(String, Vec<u8>)>,
}

#[derive(Clone, Copy)]
enum Kind {
    Primitive,
    Model,
    Subckt,
}

fn kind_of(i: usize) -> Kind {
    match i % 10 {
        0..=3 => Kind::Primitive,
        4..=6 => Kind::Model,
        _ => Kind::Subckt,
    }
}

const PRIMS: [(&str, &str, &str, &str, &str); 4] = [
    ("passives", "resistance", "ohm", "10k", "resistor"),
    ("passives", "capacitance", "F", "100n", "capacitor"),
    ("passives", "inductance", "H", "10u", "inductor"),
    ("sources", "dc", "V", "5", "vsource"),
];

const MODELS: [(&str, &str, &str, &[&str]); 4] = [
    ("diodes", "D", "D", &["1", "2"]),
    ("bjts", "Q", "NPN", &["1", "2", "3"]),
    ("bjts", "Q", "PNP", &["1", "2", "3"]),
    ("mosfets", "M", "NMOS", &["1", "2", "3", "4"]),
];

fn pins_toml(out: &mut String, pins: &[(&str, &str, &str)]) {
    for (number, name, role) in pins {
        let _ = write!(
            out,
            "\n[[pins]]\nnumber = \"{number}\"\nname = \"{name}\"\nrole = \"{role}\"\n"
        );
    }
}

fn description(i: usize, what: &str) -> String {
    format!(
        "Generated spike part {i:03}: {what}. Metadata sized like a real library entry so that \
         the index parses a realistic amount of TOML per part without reading any model file."
    )
}

fn part_toml(i: usize) -> (String, String, Option<(String, String)>) {
    let mut s = String::new();
    match kind_of(i) {
        Kind::Primitive => {
            let (cat, param, unit, default, prim) = PRIMS[i % PRIMS.len()];
            let id = format!("{prim}-{i:03}");
            let _ = write!(
                s,
                "schema = 1\nid = \"{id}\"\nname = \"{prim} {i}\"\ncategory = \"{cat}\"\n\
                 description = \"{}\"\ntags = [\"{cat}\", \"{prim}\", \"linear\", \"spike\"]\n\
                 designator_prefix = \"R\"\nprimary_param = \"{param}\"\n\n[symbol]\nbuiltin = \"Resistor-IEEE-Standard\"\n",
                description(i, prim)
            );
            pins_toml(&mut s, &[("1", "a", "passive"), ("2", "b", "passive")]);
            let _ = write!(
                s,
                "\n[params.{param}]\nkind = \"real\"\nunit = \"{unit}\"\ndefault = \"{default}\"\nmin = \"1e-12\"\nlabel = \"{param}\"\n\
                 \n[params.tc1]\nkind = \"real\"\nunit = \"1/K\"\ndefault = \"0\"\nlabel = \"Linear tempco\"\noptional = true\n\
                 \n[sim]\nkind = \"primitive\"\nprimitive = \"{prim}\"\npins = [\"1\", \"2\"]\n"
            );
            (format!("parts/{cat}/{id}.toml"), s, None)
        }
        Kind::Model => {
            let (cat, prefix, ty, pins) = MODELS[i % MODELS.len()];
            let id = format!("{}-{i:03}", ty.to_ascii_lowercase());
            let deck = format!("models/{cat}/{cat}-{:02}.lib", i / 10);
            let entry = format!("{ty}_SPIKE_{i:03}");
            let _ = write!(
                s,
                "schema = 1\nid = \"{id}\"\nname = \"{ty} {i}\"\ncategory = \"{cat}\"\n\
                 description = \"{}\"\ntags = [\"{cat}\", \"{}\", \"spike\"]\n\
                 designator_prefix = \"{prefix}\"\n\n[symbol]\nbuiltin = \"Diode-Standard\"\n",
                description(i, ty),
                ty.to_ascii_lowercase()
            );
            let named: Vec<(&str, &str, &str)> = pins.iter().map(|p| (*p, *p, "passive")).collect();
            pins_toml(&mut s, &named);
            let list: Vec<String> = pins.iter().map(|p| format!("\"{p}\"")).collect();
            let _ = write!(
                s,
                "\n[params.area]\nkind = \"real\"\nunit = \"ratio\"\ndefault = \"1\"\nlabel = \"Area factor\"\noptional = true\n\
                 \n[sim]\nkind = \"model\"\nspice_prefix = \"{prefix}\"\nfile = \"{deck}\"\nentry = \"{entry}\"\npins = [{}]\n\
                 \n[license]\nspdx = \"CC0-1.0\"\nsource = \"{deck}\"\n",
                list.join(", ")
            );
            let card = format!(
                ".model {entry} {ty}(IS=1e-14 RS=0.{i:03} N=1.0{}\n+ CJO=2p M=0.4 VJ=0.7 TT=5n\n+ BV=75 IBV=1u EG=1.11 XTI=3)\n",
                i % 7
            );
            (format!("parts/{cat}/{id}.toml"), s, Some((deck, card)))
        }
        Kind::Subckt => {
            let id = format!("opamp-{i:03}");
            let deck = format!("models/opamps/opamps-{:02}.lib", i / 10);
            let entry = format!("OPAMP_SPIKE_{i:03}");
            let _ = write!(
                s,
                "schema = 1\nid = \"{id}\"\nname = \"Op-amp {i}\"\ncategory = \"Op-Amps\"\n\
                 description = \"{}\"\ntags = [\"opamp\", \"analog\", \"spike\"]\n\
                 designator_prefix = \"U\"\n\n[symbol]\nbuiltin = \"IC-COM-OpAmp\"\n",
                description(i, "op-amp macromodel")
            );
            pins_toml(
                &mut s,
                &[
                    ("3", "in+", "input"),
                    ("2", "in-", "input"),
                    ("8", "v+", "power"),
                    ("4", "v-", "power"),
                    ("1", "out", "output"),
                ],
            );
            let _ = write!(
                s,
                "\n[params.gbw]\nkind = \"real\"\nunit = \"Hz\"\ndefault = \"1Meg\"\nlabel = \"Gain-bandwidth\"\noptional = true\n\
                 \n[sim]\nkind = \"subckt\"\nfile = \"{deck}\"\nentry = \"{entry}\"\npins = [\"3\", \"2\", \"8\", \"4\", \"1\"]\n\
                 \n[sim.supply]\npins = [\"8\", \"4\"]\nimplicit = [\"15\", \"-15\"]\n\
                 \n[license]\nspdx = \"CC0-1.0\"\nsource = \"{deck}\"\n"
            );
            let body = format!(
                ".subckt {entry} inp inn vcc vee out\n+ PARAMS: AOL=200k GBW=1Meg\nRIN inp inn 10Meg\n\
                 EGAIN g 0 inp inn {{AOL}}\nRP g p {{AOL/(6.2832*GBW)}}\nCP p 0 1\nEOUT o 0 p 0 1\nRO o out 75\n.ends {entry}\n"
            );
            (format!("parts/opamps/{id}.toml"), s, Some((deck, body)))
        }
    }
}

/// Builds an `n`-part pack.
pub fn pack(n: usize) -> Pack {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    files.push((
        "manifest.toml".into(),
        format!(
            "schema = 1\nid = \"{PACK_ID}\"\nname = \"P0.3 spike pack\"\nversion = \"1.0.0\"\n\
             description = \"Generated {n}-part pack for the PB3 metadata-index timing\"\nlicense = \"CC0-1.0\"\n"
        )
        .into_bytes(),
    ));
    files.push((
        "LICENSE".into(),
        b"CC0-1.0: generated spike fixture, no rights reserved.\n".to_vec(),
    ));
    let mut decks: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for i in 0..n {
        let (path, text, model) = part_toml(i);
        files.push((path, text.into_bytes()));
        if let Some((deck, card)) = model {
            let d = decks.entry(deck.clone()).or_insert_with(|| {
                format!(
                    "* {deck}\n* SPDX-License-Identifier: CC0-1.0\n* Generated P0.3 spike models.\n"
                )
            });
            d.push_str(&card);
        }
    }
    for (deck, text) in decks {
        files.push((deck, text.into_bytes()));
    }
    files.sort();
    Pack { files }
}

/// Fixed pseudo-random permutation (64-bit LCG), so ZIP order is never sorted.
fn shuffled(n: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..n).collect();
    let mut s: u64 = 0x5EED_0003;
    for i in (1..n).rev() {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = (s >> 33) as usize % (i + 1);
        order.swap(i, j);
    }
    order
}

/// Writes the pack as a Deflate `.olib` with fixed timestamps and a fixed entry order.
pub fn write_olib(pack: &Pack, path: &Path) -> io::Result<()> {
    let f = fs::File::create(path)?;
    let mut w = zip::ZipWriter::new(io::BufWriter::new(f));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    for i in shuffled(pack.files.len()) {
        let (name, bytes) = &pack.files[i];
        w.start_file(name.as_str(), opts)
            .map_err(io::Error::other)?;
        w.write_all(bytes)?;
    }
    w.finish().map_err(io::Error::other)?.flush()?;
    Ok(())
}

/// Writes the pack unpacked under `root` (the folder variant of `PB3`).
pub fn write_dir(pack: &Pack, root: &Path) -> io::Result<()> {
    if root.exists() {
        fs::remove_dir_all(root)?;
    }
    for (name, bytes) in &pack.files {
        let p = root.join(name);
        fs::create_dir_all(p.parent().expect("has parent"))?;
        fs::write(p, bytes)?;
    }
    Ok(())
}
