//! `cargo xtask parts [--check]` and `cargo xtask part pack <dir> <out.opart>` (`P1.4`).
//!
//! Validates `library/parts/**/*.toml` and `library/models/**/*` against `omnia-symbols`,
//! `omnia-parts` (`spice.rs` + `container.rs`), the shared `omnia-doc` validator (`PD10`),
//! the `PK11` bundled-model licence allow-list, `THIRD_PARTY.md` (`PQ7`), and unique
//! `[symbol] default = true` declarations (`PD11`), then compiles the built-in catalog into
//! deterministic, rustfmt-clean static Rust tables in `crates/omnia-parts/src/generated.rs`.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs::{self, File},
    io::Write as _,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use omnia_doc::{
    EmbeddedParamSpec, EmbeddedPartDef, EmbeddedPin, EmbeddedSimBinding, FiniteF64, ParamKind,
    ParamValue, PinRole, PrimitiveKind, Pt, Rect, SupplyDecl, SymbolId, parse_value,
    part_def::validate,
};
use omnia_parts::{
    LibraryError, LibrarySource, ResolvedPart, pack_opart,
    schema::{
        ParamKind as TomlParamKind, ParamLiteral, PartToml, PinRole as TomlPinRole,
        PrimitiveKind as TomlPrimitiveKind, SimToml, SupplyToml,
    },
};
use omnia_symbols::{SymbolDef, SymbolLibrary};

pub const LIBRARY_DIR: &str = "library";
pub const THIRD_PARTY_MD: &str = "THIRD_PARTY.md";
pub const GENERATED: &str = "crates/omnia-parts/src/generated.rs";

/// `PK11`: approved SPDX licence identifiers for bundled model files under `library/models/`.
/// Separate from `deny.toml`, which governs Rust crate licences.
pub const PK11_ALLOWED_LICENSES: &[&str] = &[
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "CC0-1.0",
    "LicenseRef-public-domain",
];

type Error = String;

struct CompiledPart {
    id: String,
    source_id: String,
    content_hash: [u8; 32],
    tags: Vec<String>,
    datasheet: Option<String>,
    symbol_default: bool,
    license: Option<(String, Option<String>)>,
    embedded: EmbeddedPartDef,
}

/// Runs `cargo xtask parts` (when `check == false`) or `cargo xtask parts --check` (`check == true`).
pub fn run(root: &Path, check: bool) -> Result<(), Error> {
    let out = render_from_files(root)?;
    let path = root.join(GENERATED);
    let unchanged = fs::read_to_string(&path).is_ok_and(|old| old.replace("\r\n", "\n") == out);
    if check {
        if unchanged {
            println!("parts: {GENERATED} is up to date");
            Ok(())
        } else {
            Err(format!(
                "{GENERATED} is stale; run `cargo xtask parts` to regenerate it"
            ))
        }
    } else if unchanged {
        println!("parts: {GENERATED} is up to date");
        Ok(())
    } else {
        fs::write(&path, out).map_err(|e| format!("write {}: {e}", path.display()))?;
        println!("parts: wrote {GENERATED}");
        Ok(())
    }
}

/// Runs `cargo xtask part pack <dir> <out.opart>`: validates the `.opart` extension, packs into an
/// exclusively reserved staging directory, reads the staged archive once into memory, validates
/// that exact buffer with `LibrarySource::from_zip` + `PD10`, cleans up only the owned staging
/// directory, and publishes the validated buffer exclusively (`File::create_new`).
pub fn run_part_pack(dir: &Path, out_path: &Path) -> Result<(), Error> {
    if out_path.extension().and_then(|ext| ext.to_str()) != Some("opart") {
        return Err(format!(
            "pack {} -> {}: output extension does not match container type (.opart required)",
            dir.display(),
            out_path.display()
        ));
    }
    if out_path.exists() {
        return Err(format!(
            "pack {} -> {}: destination already exists (refusing to overwrite)",
            dir.display(),
            out_path.display()
        ));
    }
    let stage_dir = reserve_staging_dir(out_path)?;
    let stage_path = stage_dir.join("staged.opart");
    let pack_result = (|| -> Result<(omnia_parts::ContentHash, Vec<u8>), Error> {
        let hash = pack_opart(dir, &stage_path)
            .map_err(|e| format!("pack {} -> {}: {e}", dir.display(), out_path.display()))?;
        let bytes = fs::read(&stage_path)
            .map_err(|e| format!("read staged archive {}: {e}", stage_path.display()))?;
        let archive = LibrarySource::from_zip(bytes.as_slice())
            .map_err(|e| format!("open staged archive {}: {e}", stage_path.display()))?;
        let part_path = "part.toml";
        let resolved = resolve_with_context(&archive, part_path, "")?;
        let symbols = SymbolLibrary::builtin();
        let embedded = build_embedded(&resolved, &symbols, format!("custom:{}", resolved.part.id))?;
        validate(&embedded)
            .map_err(|e| format!("{}/{part_path}: PD10 validation failed: {e}", dir.display()))?;
        Ok((hash, bytes))
    })();
    let _ = fs::remove_dir_all(&stage_dir);

    let (hash, bytes) = pack_result?;
    let mut output =
        File::create_new(out_path).map_err(|e| format!("create {}: {e}", out_path.display()))?;
    if let Err(err) = output.write_all(&bytes) {
        drop(output);
        let _ = fs::remove_file(out_path);
        return Err(format!("write {}: {err}", out_path.display()));
    }
    println!("part pack: wrote {} (sha256:{hash})", out_path.display());
    Ok(())
}

fn reserve_staging_dir(out_path: &Path) -> Result<PathBuf, Error> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = out_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = out_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("part");
    let pid = std::process::id();
    for _ in 0..1024 {
        let seq = NEXT.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{stem}.stage-{pid}-{seq}"));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(format!(
                    "create staging directory {}: {err}",
                    candidate.display()
                ));
            }
        }
    }
    Err(format!(
        "could not reserve exclusive staging directory in {}",
        parent.display()
    ))
}

/// Reads `library/` and `THIRD_PARTY.md` under `root`, validates every part and model licence,
/// and returns the rendered Rust source for `crates/omnia-parts/src/generated.rs`.
pub fn render_from_files(root: &Path) -> Result<String, Error> {
    let compiled = compile_library(root)?;
    Ok(render_rust(&compiled))
}

fn compile_library(root: &Path) -> Result<Vec<CompiledPart>, Error> {
    let library_root = root.join(LIBRARY_DIR);
    let source = LibrarySource::open(&library_root)
        .map_err(|e| format!("open {}: {e}", library_root.display()))?;

    let third_party_path = root.join(THIRD_PARTY_MD);
    let third_party = fs::read_to_string(&third_party_path)
        .map_err(|e| format!("read {}: {e}", third_party_path.display()))?;

    // Check every bundled model file under `library/models/` declares an allowed PK11 SPDX
    // header and has a matching attribution entry in `THIRD_PARTY.md` (PK11, PQ7).
    let model_paths: Vec<String> = source
        .paths()
        .filter(|path| path.starts_with("models/"))
        .map(str::to_owned)
        .collect();
    let mut model_licenses: BTreeMap<String, String> = BTreeMap::new();
    for model_rel in &model_paths {
        let repo_rel = format!("{LIBRARY_DIR}/{model_rel}");
        let bytes = source
            .read(model_rel)
            .map_err(|e| format!("{repo_rel}: {e}"))?;
        let text = std::str::from_utf8(&bytes).map_err(|e| format!("{repo_rel}: {e}"))?;
        let spdx = parse_model_spdx(&repo_rel, text)?;
        if !has_third_party_attribution(&third_party, &repo_rel, &spdx) {
            return Err(format!(
                "{repo_rel}: bundled model file with licence {spdx:?} is missing an exact attribution entry (`- `{repo_rel}` ... `{spdx}``) in {THIRD_PARTY_MD} (PK11, PQ7)"
            ));
        }
        model_licenses.insert(model_rel.clone(), spdx);
    }

    let part_paths: Vec<String> = source
        .paths()
        .filter(|path| is_part_toml_path(path))
        .map(str::to_owned)
        .collect();
    if part_paths.is_empty() {
        return Err(format!(
            "{LIBRARY_DIR}: no part definitions found under {LIBRARY_DIR}/parts/<category>/<id>.toml"
        ));
    }

    let symbols = SymbolLibrary::builtin();
    let mut compiled = Vec::with_capacity(part_paths.len());
    let mut seen_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut defaults_by_symbol: BTreeMap<SymbolId, (String, String)> = BTreeMap::new();

    for part_rel in &part_paths {
        let display_path = format!("{LIBRARY_DIR}/{part_rel}");
        let resolved = resolve_with_context(&source, part_rel, &format!("{LIBRARY_DIR}/"))?;
        let part = &resolved.part;

        // Enforce PK11 licence policy and PQ5 built-in model file separation.
        match &part.sim {
            SimToml::Model { file, spice, .. } | SimToml::Subckt { file, spice, .. } => {
                if spice.is_some() {
                    return Err(format!(
                        "{display_path}: built-in parts must reference a file under {LIBRARY_DIR}/models/ rather than inline spice (PQ5)"
                    ));
                }
                let model_file = file
                    .as_deref()
                    .ok_or_else(|| format!("{display_path}: missing sim.file"))?;
                if !model_file.starts_with("models/") {
                    return Err(format!(
                        "{display_path}: built-in model path {model_file:?} must live under models/"
                    ));
                }
                let license = part.license.as_ref().ok_or_else(|| {
                    format!(
                        "{display_path} ({LIBRARY_DIR}/{model_file}): missing [license] metadata required for bundled models (PK11)"
                    )
                })?;
                check_pk11_license(
                    &format!("{display_path} ({LIBRARY_DIR}/{model_file})"),
                    &license.spdx,
                )?;
                if let Some(model_spdx) = model_licenses.get(model_file)
                    && model_spdx != &license.spdx
                {
                    return Err(format!(
                        "{display_path} ({LIBRARY_DIR}/{model_file}): part [license].spdx {:?} does not match model file SPDX header {model_spdx:?} (PK11)",
                        license.spdx
                    ));
                }
            }
            SimToml::Primitive { .. } | SimToml::None => {
                if let Some(license) = &part.license {
                    check_pk11_license(&display_path, &license.spdx)?;
                }
            }
        }

        let id = part.id.as_str().to_owned();
        if let Some(prev) = seen_ids.insert(id.clone(), display_path.clone()) {
            return Err(format!(
                "{display_path}: duplicate part id {id:?} (already defined in {prev})"
            ));
        }

        let source_id = format!("omnia:{id}");
        let embedded = build_embedded(&resolved, &symbols, source_id.clone())
            .map_err(|e| format!("{display_path}: {e}"))?;
        validate(&embedded).map_err(|e| format!("{display_path}: PD10 validation failed: {e}"))?;

        if part.symbol.default
            && let Some((prev_id, prev_path)) =
                defaults_by_symbol.insert(embedded.symbol, (id.clone(), display_path.clone()))
        {
            return Err(format!(
                "{display_path}: symbol {:?} already has default part {prev_id:?} in {prev_path} (PD11)",
                part.symbol.builtin
            ));
        }

        compiled.push(CompiledPart {
            id,
            source_id,
            content_hash: resolved.content_hash.0,
            tags: part.tags.clone(),
            datasheet: part.datasheet.clone(),
            symbol_default: part.symbol.default,
            license: part
                .license
                .as_ref()
                .map(|lic| (lic.spdx.clone(), lic.source.clone())),
            embedded,
        });
    }

    // Canonical deterministic order by `source_id` (PINV2).
    compiled.sort_by(|a, b| a.source_id.cmp(&b.source_id));
    Ok(compiled)
}

fn parse_model_spdx(repo_rel: &str, text: &str) -> Result<String, Error> {
    let mut found: Option<String> = None;
    for line in text.lines() {
        // Require a SPICE/header comment marker (`*`, `;`, or `#`) followed immediately by
        // `SPDX-License-Identifier:` (rejecting bare uncommented lines and prose).
        let trimmed = line.trim_start();
        let Some(after_comment) = trimmed.strip_prefix(['*', ';', '#']) else {
            continue;
        };
        let body = after_comment
            .trim_start_matches(['*', ';', '#'])
            .trim_start();
        if let Some(rest) = body.strip_prefix("SPDX-License-Identifier:") {
            let spdx = rest.trim();
            if spdx.is_empty() || spdx.split_whitespace().count() != 1 {
                return Err(format!(
                    "{repo_rel}: malformed SPDX-License-Identifier declaration {spdx:?} (PK11)"
                ));
            }
            check_pk11_license(repo_rel, spdx)?;
            match &found {
                None => found = Some(spdx.to_owned()),
                Some(prev) if prev == spdx => {}
                Some(prev) => {
                    return Err(format!(
                        "{repo_rel}: conflicting SPDX-License-Identifier headers {prev:?} and {spdx:?} (PK11)"
                    ));
                }
            }
        }
    }
    found.ok_or_else(|| {
        format!("{repo_rel}: missing SPDX-License-Identifier header required by PK11")
    })
}

fn has_third_party_attribution(third_party: &str, repo_rel: &str, spdx: &str) -> bool {
    let path_bullet = format!("- `{repo_rel}`");
    let spdx_token = format!("`{spdx}`");
    third_party.lines().any(|line| {
        let Some(after_path) = line.trim_start().strip_prefix(&path_bullet) else {
            return false;
        };
        let after_ws = after_path.trim_start();
        let after_sep = after_ws
            .strip_prefix('—')
            .or_else(|| after_ws.strip_prefix("--"))
            .or_else(|| after_ws.strip_prefix('-'))
            .or_else(|| after_ws.strip_prefix(':'));
        let Some(after_sep) = after_sep else {
            return false;
        };
        let Some(after_spdx) = after_sep.trim_start().strip_prefix(&spdx_token) else {
            return false;
        };
        after_spdx.is_empty()
            || after_spdx
                .starts_with(|c: char| c.is_whitespace() || matches!(c, '(' | '.' | ',' | ';'))
    })
}

fn is_part_toml_path(path: &str) -> bool {
    let mut parts = path.split('/');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some("parts"), Some(_), Some(file), None) if file.ends_with(".toml")
    )
}

fn check_pk11_license(where_ctx: &str, spdx: &str) -> Result<(), Error> {
    if PK11_ALLOWED_LICENSES.contains(&spdx) {
        Ok(())
    } else {
        Err(format!(
            "{where_ctx}: licence {spdx:?} is not in the PK11 allow-list ({})",
            PK11_ALLOWED_LICENSES.join(", ")
        ))
    }
}

/// Resolves `part_rel` and enriches terminal-count errors with the part file path and `.subckt`
/// entry name (`PTC-xtask-mismatch`, `PINV3`).
fn resolve_with_context(
    source: &LibrarySource,
    part_rel: &str,
    prefix: &str,
) -> Result<ResolvedPart, Error> {
    let display_path = format!("{prefix}{part_rel}");
    let parsed = source
        .part(part_rel)
        .map_err(|e| format!("{display_path}: {e}"))?;
    source.resolve_part(part_rel).map_err(|err| match err {
        LibraryError::TerminalCount { expected, actual } => match &parsed.sim {
            SimToml::Subckt { entry, .. } => format!(
                "{display_path}: .subckt {entry} expects {expected} ports, but [sim].pins has {actual} (PINV3)"
            ),
            SimToml::Primitive { primitive, .. } => format!(
                "{display_path}: primitive {primitive:?} expects {expected} terminals, but [sim].pins has {actual} (PINV3)"
            ),
            _ => format!("{display_path}: expected {expected} simulation terminals, got {actual} (PINV3)"),
        },
        other => format!("{display_path}: {other}"),
    })
}

fn build_embedded(
    resolved: &ResolvedPart,
    symbols: &SymbolLibrary,
    source_id: String,
) -> Result<EmbeddedPartDef, Error> {
    let part = &resolved.part;
    let symbol_id = symbols
        .by_name(&part.symbol.builtin)
        .ok_or_else(|| format!("unknown built-in symbol {:?}", part.symbol.builtin))?;
    let sym_def: &SymbolDef = symbols.get(symbol_id);

    // Map pin numbers in stored-pin-index order (PD5, PINV11).
    let mut pins = Vec::with_capacity(sym_def.pins.len());
    let mut index_by_number: BTreeMap<&str, u16> = BTreeMap::new();
    for (idx, sym_pin) in sym_def.pins.iter().enumerate() {
        let decl = part
            .pins
            .iter()
            .find(|decl| decl.number == sym_pin.number)
            .ok_or_else(|| {
                format!(
                    "symbol {:?} pin {:?} is missing from [[pins]]",
                    part.symbol.builtin, sym_pin.number
                )
            })?;
        let idx_u16 = u16::try_from(idx).map_err(|_| "pin index exceeds u16".to_owned())?;
        index_by_number.insert(sym_pin.number, idx_u16);
        pins.push(EmbeddedPin {
            number: Arc::from(sym_pin.number),
            name: Arc::from(decl.name.as_str()),
            role: convert_pin_role(decl.role),
        });
    }

    let params = convert_params(part)?;
    let sim = convert_sim(resolved, &index_by_number)?;

    Ok(EmbeddedPartDef {
        source_id: Arc::from(source_id),
        content_hash: Some(resolved.content_hash.0),
        name: Arc::from(part.name.as_str()),
        category: Arc::from(part.category.as_str()),
        description: Arc::from(part.description.as_str()),
        designator_prefix: Arc::from(part.designator_prefix.as_deref().unwrap_or("")),
        symbol: symbol_id,
        local_pins: sym_def.pins.iter().map(|p| p.at).collect(),
        local_bounds: sym_def.bounds,
        pins,
        params,
        primary_param: part.primary_param.as_deref().map(Arc::from),
        sim,
    })
}

fn convert_pin_role(role: TomlPinRole) -> PinRole {
    match role {
        TomlPinRole::Passive => PinRole::Passive,
        TomlPinRole::Input => PinRole::Input,
        TomlPinRole::Output => PinRole::Output,
        TomlPinRole::Bidir => PinRole::Bidir,
        TomlPinRole::Power => PinRole::Power,
        TomlPinRole::Ground => PinRole::Ground,
        TomlPinRole::NoConnect => PinRole::NoConnect,
    }
}

fn convert_primitive(kind: TomlPrimitiveKind) -> PrimitiveKind {
    match kind {
        TomlPrimitiveKind::Resistor => PrimitiveKind::Resistor,
        TomlPrimitiveKind::Capacitor => PrimitiveKind::Capacitor,
        TomlPrimitiveKind::Inductor => PrimitiveKind::Inductor,
        TomlPrimitiveKind::Potentiometer => PrimitiveKind::Potentiometer,
        TomlPrimitiveKind::VSource => PrimitiveKind::VSource,
        TomlPrimitiveKind::ISource => PrimitiveKind::ISource,
        TomlPrimitiveKind::Vcvs => PrimitiveKind::Vcvs,
        TomlPrimitiveKind::Cccs => PrimitiveKind::Cccs,
        TomlPrimitiveKind::Vccs => PrimitiveKind::Vccs,
        TomlPrimitiveKind::Ccvs => PrimitiveKind::Ccvs,
        TomlPrimitiveKind::Ground => PrimitiveKind::Ground,
        TomlPrimitiveKind::Switch => PrimitiveKind::Switch,
        TomlPrimitiveKind::Rail => PrimitiveKind::Rail,
    }
}

fn convert_params(part: &PartToml) -> Result<Vec<EmbeddedParamSpec>, Error> {
    let mut out = Vec::with_capacity(part.params.len());
    for (name, param) in &part.params {
        let label = param.label.as_deref().unwrap_or(name.as_str());
        let (kind, default) = match param.kind {
            TomlParamKind::Real => {
                let unit = param.unit.as_deref();
                let parse_lit = |lit: &ParamLiteral| -> Result<(String, FiniteF64), Error> {
                    let ParamLiteral::String(raw) = lit else {
                        return Err(format!("params.{name}: expected string literal for real"));
                    };
                    let val = parse_value(raw, unit)
                        .map_err(|e| format!("params.{name}: invalid real {raw:?}: {e}"))?;
                    Ok((raw.clone(), val))
                };
                let (default_raw, default_val) = parse_lit(&param.default)?;
                let min = param
                    .min
                    .as_ref()
                    .map(|lit| parse_lit(lit).map(|(_, v)| v))
                    .transpose()?;
                let max = param
                    .max
                    .as_ref()
                    .map(|lit| parse_lit(lit).map(|(_, v)| v))
                    .transpose()?;
                (
                    ParamKind::Real {
                        unit: unit.map(Arc::from),
                        min,
                        max,
                    },
                    ParamValue::Real {
                        raw: Arc::from(default_raw),
                        value: default_val,
                    },
                )
            }
            TomlParamKind::Int => {
                let as_int = |lit: &ParamLiteral| -> Result<i64, Error> {
                    match lit {
                        ParamLiteral::Int(v) => Ok(*v),
                        _ => Err(format!("params.{name}: expected integer literal")),
                    }
                };
                let default = ParamValue::Int(as_int(&param.default)?);
                let min = param.min.as_ref().map(as_int).transpose()?;
                let max = param.max.as_ref().map(as_int).transpose()?;
                (ParamKind::Int { min, max }, default)
            }
            TomlParamKind::Bool => {
                let ParamLiteral::Bool(v) = param.default else {
                    return Err(format!("params.{name}: expected boolean literal"));
                };
                (ParamKind::Bool, ParamValue::Bool(v))
            }
            TomlParamKind::Enum => {
                let ParamLiteral::String(default_raw) = &param.default else {
                    return Err(format!("params.{name}: expected string literal for enum"));
                };
                let variants = param
                    .variants
                    .as_ref()
                    .ok_or_else(|| format!("params.{name}: missing variants"))?
                    .iter()
                    .map(|v| Arc::from(v.as_str()))
                    .collect();
                (
                    ParamKind::Enum(variants),
                    ParamValue::Enum(Arc::from(default_raw.as_str())),
                )
            }
            TomlParamKind::Text => {
                let ParamLiteral::String(default_raw) = &param.default else {
                    return Err(format!("params.{name}: expected string literal for text"));
                };
                (
                    ParamKind::Text,
                    ParamValue::Text(Arc::from(default_raw.as_str())),
                )
            }
        };
        out.push(EmbeddedParamSpec {
            name: Arc::from(name.as_str()),
            label: Arc::from(label),
            kind,
            default,
            optional: param.optional,
        });
    }
    Ok(out)
}

fn convert_sim(
    resolved: &ResolvedPart,
    index_by_number: &BTreeMap<&str, u16>,
) -> Result<EmbeddedSimBinding, Error> {
    let map_terminals = |pins: &[String]| -> Result<Vec<u16>, Error> {
        pins.iter()
            .map(|num| {
                index_by_number
                    .get(num.as_str())
                    .copied()
                    .ok_or_else(|| format!("sim.pins references unknown pin {num:?}"))
            })
            .collect()
    };
    let map_supply = |supply: &Option<SupplyToml>| -> Result<Option<SupplyDecl>, Error> {
        let Some(supply) = supply else {
            return Ok(None);
        };
        let pos = index_by_number
            .get(supply.pins[0].as_str())
            .copied()
            .ok_or_else(|| {
                format!(
                    "sim.supply.pins references unknown pin {:?}",
                    supply.pins[0]
                )
            })?;
        let neg = index_by_number
            .get(supply.pins[1].as_str())
            .copied()
            .ok_or_else(|| {
                format!(
                    "sim.supply.pins references unknown pin {:?}",
                    supply.pins[1]
                )
            })?;
        let implicit = supply
            .implicit_volts()
            .map_err(|e| format!("sim.supply.implicit: {e}"))?
            .map(|[p, n]| (p, n));
        Ok(Some(SupplyDecl { pos, neg, implicit }))
    };

    match &resolved.part.sim {
        SimToml::None => Ok(EmbeddedSimBinding::None),
        SimToml::Primitive { primitive, pins } => Ok(EmbeddedSimBinding::Primitive {
            kind: convert_primitive(*primitive),
            terminals: map_terminals(pins)?,
        }),
        SimToml::Model {
            spice_prefix,
            entry,
            pins,
            supply,
            ..
        } => {
            let spice = resolved
                .spice
                .as_ref()
                .ok_or_else(|| "missing extracted SPICE text for model".to_owned())?;
            Ok(EmbeddedSimBinding::Model {
                spice_prefix: *spice_prefix,
                entry: Arc::from(entry.as_str()),
                terminals: map_terminals(pins)?,
                spice: Arc::from(spice.text.as_str()),
                supply: map_supply(supply)?,
            })
        }
        SimToml::Subckt {
            entry,
            pins,
            supply,
            ..
        } => {
            let spice = resolved
                .spice
                .as_ref()
                .ok_or_else(|| "missing extracted SPICE text for subckt".to_owned())?;
            Ok(EmbeddedSimBinding::Subckt {
                entry: Arc::from(entry.as_str()),
                terminals: map_terminals(pins)?,
                spice: Arc::from(spice.text.as_str()),
                supply: map_supply(supply)?,
            })
        }
    }
}

fn render_rust(parts: &[CompiledPart]) -> String {
    let mut out = String::new();
    let w = &mut out;
    let _ = writeln!(
        w,
        "// @generated by `cargo xtask parts` from {LIBRARY_DIR}/."
    );
    let _ = writeln!(w, "// Do not edit by hand.");
    let _ = writeln!(w);
    let _ = writeln!(
        w,
        "use omnia_doc::{{PinRole, PrimitiveKind, Pt, Rect, SymbolId}};"
    );
    let _ = writeln!(w);
    let _ = writeln!(w, "use crate::catalog::{{");
    let _ = writeln!(
        w,
        "    BuiltinLicense, BuiltinParamKind, BuiltinParamSpec, BuiltinParamValue, BuiltinPart, BuiltinPin,"
    );
    let _ = writeln!(w, "    BuiltinSimBinding, BuiltinSupplyDecl,");
    let _ = writeln!(w, "}};");
    let _ = writeln!(w);

    for part in parts {
        let ident = const_ident(&part.id);
        let emb = &part.embedded;

        if emb.local_pins.len() == 1 {
            let _ = writeln!(
                w,
                "static {ident}_LOCAL_PINS: &[Pt] = &[{}];",
                fmt_pt(emb.local_pins[0], "")
            );
        } else {
            let _ = writeln!(w, "static {ident}_LOCAL_PINS: &[Pt] = &[");
            for &pt in &emb.local_pins {
                let _ = writeln!(w, "    {},", fmt_pt(pt, "    "));
            }
            let _ = writeln!(w, "];");
        }
        let _ = writeln!(w);

        if emb.pins.len() == 1 {
            let pin = &emb.pins[0];
            let _ = writeln!(w, "static {ident}_PINS: &[BuiltinPin] = &[BuiltinPin {{");
            let _ = writeln!(w, "    number: {:?},", &*pin.number);
            let _ = writeln!(w, "    name: {:?},", &*pin.name);
            let _ = writeln!(w, "    role: {},", fmt_pin_role(pin.role));
            let _ = writeln!(w, "}}];");
        } else {
            let _ = writeln!(w, "static {ident}_PINS: &[BuiltinPin] = &[");
            for pin in &emb.pins {
                let _ = writeln!(w, "    BuiltinPin {{");
                let _ = writeln!(w, "        number: {:?},", &*pin.number);
                let _ = writeln!(w, "        name: {:?},", &*pin.name);
                let _ = writeln!(w, "        role: {},", fmt_pin_role(pin.role));
                let _ = writeln!(w, "    }},");
            }
            let _ = writeln!(w, "];");
        }
        let _ = writeln!(w);

        if emb.params.is_empty() {
            let _ = writeln!(w, "static {ident}_PARAMS: &[BuiltinParamSpec] = &[];");
        } else {
            let _ = writeln!(w, "static {ident}_PARAMS: &[BuiltinParamSpec] = &[");
            for spec in &emb.params {
                let _ = writeln!(w, "    BuiltinParamSpec {{");
                let _ = writeln!(w, "        name: {:?},", &*spec.name);
                let _ = writeln!(w, "        label: {:?},", &*spec.label);
                let _ = writeln!(
                    w,
                    "        kind: {},",
                    fmt_param_kind(&spec.kind, "        ")
                );
                let _ = writeln!(
                    w,
                    "        default: {},",
                    fmt_param_value(&spec.default, "        ")
                );
                let _ = writeln!(w, "        optional: {},", spec.optional);
                let _ = writeln!(w, "    }},");
            }
            let _ = writeln!(w, "];");
        }
        let _ = writeln!(w);
    }

    let _ = writeln!(w, "pub static BUILTINS: &[BuiltinPart] = &[");
    for part in parts {
        let ident = const_ident(&part.id);
        let emb = &part.embedded;
        let _ = writeln!(w, "    BuiltinPart {{");
        let _ = writeln!(w, "        id: {:?},", part.id);
        let _ = writeln!(w, "        source_id: {:?},", part.source_id);
        let _ = writeln!(
            w,
            "        content_hash: {},",
            fmt_hash_bytes(&part.content_hash)
        );
        let _ = writeln!(w, "        name: {:?},", &*emb.name);
        let _ = writeln!(w, "        category: {:?},", &*emb.category);
        let _ = writeln!(w, "        description: {:?},", &*emb.description);
        let _ = writeln!(w, "        tags: &{:?},", part.tags);
        let _ = writeln!(w, "        datasheet: {:?},", part.datasheet.as_deref());
        let _ = writeln!(
            w,
            "        designator_prefix: {:?},",
            &*emb.designator_prefix
        );
        let _ = writeln!(w, "        symbol: SymbolId({}),", emb.symbol.raw());
        let _ = writeln!(w, "        symbol_default: {},", part.symbol_default);
        let _ = writeln!(w, "        local_pins: {ident}_LOCAL_PINS,");
        let _ = writeln!(w, "        local_bounds: {},", fmt_rect(emb.local_bounds));
        let _ = writeln!(w, "        pins: {ident}_PINS,");
        let _ = writeln!(w, "        params: {ident}_PARAMS,");
        let _ = writeln!(
            w,
            "        primary_param: {:?},",
            emb.primary_param.as_deref()
        );
        let _ = writeln!(w, "        sim: {},", fmt_sim_binding(&emb.sim));
        match &part.license {
            None => {
                let _ = writeln!(w, "        license: None,");
            }
            Some((spdx, source)) => {
                let _ = writeln!(w, "        license: Some(BuiltinLicense {{");
                let _ = writeln!(w, "            spdx: {:?},", spdx);
                let _ = writeln!(w, "            source: {:?},", source.as_deref());
                let _ = writeln!(w, "        }}),");
            }
        }
        let _ = writeln!(w, "    }},");
    }
    let _ = writeln!(w, "];");
    out
}

/// Deterministically encodes any `PK2` part slug (`[a-z0-9._-]`) into a valid,
/// collision-free Rust `UPPER_SNAKE_CASE` identifier prefix.
fn const_ident(name: &str) -> String {
    let mut out = String::with_capacity("PART_".len() + name.len() * 2);
    out.push_str("PART_");
    for ch in name.chars() {
        match ch {
            'a'..='z' => out.push(ch.to_ascii_uppercase()),
            'A'..='Z' | '0'..='9' => out.push(ch),
            '-' => out.push_str("_H"),
            '_' => out.push_str("_U"),
            '.' => out.push_str("_D"),
            _ => {
                let _ = write!(out, "_X{:X}_", ch as u32);
            }
        }
    }
    out
}

fn fmt_i32(v: i32) -> String {
    let sign = if v < 0 { "-" } else { "" };
    let digits = i64::from(v).abs().to_string();
    let mut grouped = String::new();
    for (i, ch) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped.push('_');
        }
        grouped.push(ch);
    }
    let grouped: String = grouped.chars().rev().collect();
    format!("{sign}{grouped}")
}

fn fmt_pt(pt: Pt, indent: &str) -> String {
    let sx = fmt_i32(pt.x);
    let sy = fmt_i32(pt.y);
    let inline = format!("x: {sx}, y: {sy}");
    if inline.len() <= 18 {
        format!("Pt {{ {inline} }}")
    } else {
        format!("Pt {{\n{indent}    x: {sx},\n{indent}    y: {sy},\n{indent}}}")
    }
}

fn fmt_rect(r: Rect) -> String {
    format!(
        "Rect {{\n            min: {},\n            max: {},\n        }}",
        fmt_pt(r.min, "            "),
        fmt_pt(r.max, "            ")
    )
}

fn fmt_pin_role(role: PinRole) -> &'static str {
    match role {
        PinRole::Passive => "PinRole::Passive",
        PinRole::Input => "PinRole::Input",
        PinRole::Output => "PinRole::Output",
        PinRole::Bidir => "PinRole::Bidir",
        PinRole::Power => "PinRole::Power",
        PinRole::Ground => "PinRole::Ground",
        PinRole::NoConnect => "PinRole::NoConnect",
    }
}

fn fmt_primitive_kind(kind: PrimitiveKind) -> &'static str {
    match kind {
        PrimitiveKind::Resistor => "PrimitiveKind::Resistor",
        PrimitiveKind::Capacitor => "PrimitiveKind::Capacitor",
        PrimitiveKind::Inductor => "PrimitiveKind::Inductor",
        PrimitiveKind::Potentiometer => "PrimitiveKind::Potentiometer",
        PrimitiveKind::VSource => "PrimitiveKind::VSource",
        PrimitiveKind::ISource => "PrimitiveKind::ISource",
        PrimitiveKind::Vcvs => "PrimitiveKind::Vcvs",
        PrimitiveKind::Cccs => "PrimitiveKind::Cccs",
        PrimitiveKind::Vccs => "PrimitiveKind::Vccs",
        PrimitiveKind::Ccvs => "PrimitiveKind::Ccvs",
        PrimitiveKind::Ground => "PrimitiveKind::Ground",
        PrimitiveKind::Switch => "PrimitiveKind::Switch",
        PrimitiveKind::Rail => "PrimitiveKind::Rail",
    }
}

fn fmt_opt_u64_hex(v: Option<FiniteF64>) -> String {
    match v {
        None => "None".to_owned(),
        Some(f) => format!("Some({:#018x})", f.get().to_bits()),
    }
}

fn fmt_param_kind(kind: &ParamKind, indent: &str) -> String {
    match kind {
        ParamKind::Real { unit, min, max } => format!(
            "BuiltinParamKind::Real {{\n{indent}    unit: {:?},\n{indent}    min_bits: {},\n{indent}    max_bits: {},\n{indent}}}",
            unit.as_deref(),
            fmt_opt_u64_hex(*min),
            fmt_opt_u64_hex(*max),
        ),
        ParamKind::Int { min, max } => format!(
            "BuiltinParamKind::Int {{\n{indent}    min: {min:?},\n{indent}    max: {max:?},\n{indent}}}"
        ),
        ParamKind::Bool => "BuiltinParamKind::Bool".to_owned(),
        ParamKind::Enum(variants) => {
            let list: Vec<&str> = variants.iter().map(Arc::as_ref).collect();
            format!("BuiltinParamKind::Enum(&{list:?})")
        }
        ParamKind::Text => "BuiltinParamKind::Text".to_owned(),
    }
}

fn fmt_param_value(val: &ParamValue, indent: &str) -> String {
    match val {
        ParamValue::Real { raw, value } => format!(
            "BuiltinParamValue::Real {{\n{indent}    raw: {:?},\n{indent}    value_bits: {:#018x},\n{indent}}}",
            &**raw,
            value.get().to_bits(),
        ),
        ParamValue::Int(v) => format!("BuiltinParamValue::Int({v})"),
        ParamValue::Bool(v) => format!("BuiltinParamValue::Bool({v})"),
        ParamValue::Enum(v) => format!("BuiltinParamValue::Enum({:?})", &**v),
        ParamValue::Text(v) => format!("BuiltinParamValue::Text({:?})", &**v),
    }
}

fn fmt_supply(supply: &Option<SupplyDecl>, indent: &str) -> String {
    match supply {
        None => "None".to_owned(),
        Some(s) => {
            let implicit = match s.implicit {
                None => "None".to_owned(),
                Some((pos, neg)) => format!(
                    "Some(({:#018x}, {:#018x}))",
                    pos.get().to_bits(),
                    neg.get().to_bits()
                ),
            };
            format!(
                "Some(BuiltinSupplyDecl {{\n{indent}    pos: {},\n{indent}    neg: {},\n{indent}    implicit_bits: {implicit},\n{indent}}})",
                s.pos, s.neg
            )
        }
    }
}

fn fmt_sim_binding(sim: &EmbeddedSimBinding) -> String {
    match sim {
        EmbeddedSimBinding::None => "BuiltinSimBinding::None".to_owned(),
        EmbeddedSimBinding::Primitive { kind, terminals } => format!(
            "BuiltinSimBinding::Primitive {{\n            kind: {},\n            terminals: &{terminals:?},\n        }}",
            fmt_primitive_kind(*kind)
        ),
        EmbeddedSimBinding::Model {
            spice_prefix,
            entry,
            terminals,
            spice,
            supply,
        } => format!(
            "BuiltinSimBinding::Model {{\n            spice_prefix: {spice_prefix:?},\n            entry: {:?},\n            terminals: &{terminals:?},\n            spice: {:?},\n            supply: {},\n        }}",
            &**entry,
            &**spice,
            fmt_supply(supply, "            "),
        ),
        EmbeddedSimBinding::Subckt {
            entry,
            terminals,
            spice,
            supply,
        } => format!(
            "BuiltinSimBinding::Subckt {{\n            entry: {:?},\n            terminals: &{terminals:?},\n            spice: {:?},\n            supply: {},\n        }}",
            &**entry,
            &**spice,
            fmt_supply(supply, "            "),
        ),
    }
}

fn fmt_hash_bytes(bytes: &[u8; 32]) -> String {
    let mut out = String::from("[\n");
    for chunk in bytes.chunks(14) {
        out.push_str("            ");
        for (i, byte) in chunk.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            let _ = write!(out, "0x{byte:02x},");
        }
        out.push('\n');
    }
    out.push_str("        ]");
    out
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::tokens::workspace_root;

    fn temp_workspace(tag: &str) -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "omnia-xtask-parts-{tag}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp workspace");
        dir
    }

    #[test]
    fn generated_rs_is_up_to_date() {
        let root = workspace_root();
        let expected = render_from_files(&root).expect("render built-in parts");
        let actual = fs::read_to_string(root.join(GENERATED)).expect("read generated.rs");
        let actual = actual.replace("\r\n", "\n");
        assert!(
            expected == actual,
            "{GENERATED} is stale; run `cargo xtask parts`"
        );
    }

    #[test]
    fn pinv2_render_is_deterministic() {
        let root = workspace_root();
        let first = render_from_files(&root).expect("first render");
        let second = render_from_files(&root).expect("second render");
        assert_eq!(first, second);
    }

    /// `PTC-xtask-mismatch` (part 1, `PINV3`): `[sim].pins` has 4 entries for a 5-port `.subckt`.
    #[test]
    fn ptc_xtask_mismatch_subckt_port_count() {
        let root = temp_workspace("subckt-ports");
        fs::create_dir_all(root.join("library/parts/opamps")).unwrap();
        fs::create_dir_all(root.join("library/models/opamps")).unwrap();
        fs::write(
            root.join("THIRD_PARTY.md"),
            "- `library/models/opamps/bad.lib` — `CC0-1.0`\n",
        )
        .unwrap();
        fs::write(
            root.join("library/models/opamps/bad.lib"),
            "* SPDX-License-Identifier: CC0-1.0\n.subckt AMP5 a b c d e\nR1 a b 1k\n.ends AMP5\n",
        )
        .unwrap();
        fs::write(
            root.join("library/parts/opamps/bad.toml"),
            r#"
schema = 1
id = "bad-opamp"
name = "Bad op-amp"
category = "Op-Amps"

[symbol]
builtin = "IC-COM-OpAmp"

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
role = "no_connect"

[[pins]]
number = "1"
name = "out"
role = "output"

[sim]
kind = "subckt"
file = "models/opamps/bad.lib"
entry = "AMP5"
pins = ["3", "2", "8", "1"]

[license]
spdx = "CC0-1.0"
source = "models/opamps/bad.lib"
"#,
        )
        .unwrap();

        let err = run(&root, true).expect_err("port count mismatch must fail");
        assert!(
            err.contains("library/parts/opamps/bad.toml"),
            "diagnostic must name part file: {err}"
        );
        assert!(
            err.contains("AMP5"),
            "diagnostic must name .subckt entry: {err}"
        );
        assert!(
            err.contains('5') && err.contains('4'),
            "diagnostic must report expected (5) vs actual (4) port count: {err}"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// `PTC-xtask-mismatch` (part 2, `PK11`): model licence outside the `PK11` allow-list.
    #[test]
    fn ptc_xtask_mismatch_disallowed_license() {
        let root = temp_workspace("bad-license");
        fs::create_dir_all(root.join("library/parts/diodes")).unwrap();
        fs::create_dir_all(root.join("library/models/diodes")).unwrap();
        fs::write(
            root.join("THIRD_PARTY.md"),
            "- `library/models/diodes/bad.lib` — `CC0-1.0`\n",
        )
        .unwrap();
        fs::write(
            root.join("library/models/diodes/bad.lib"),
            "* SPDX-License-Identifier: CC0-1.0\n.model D_BAD D(IS=1e-12)\n",
        )
        .unwrap();
        fs::write(
            root.join("library/parts/diodes/bad.toml"),
            r#"
schema = 1
id = "bad-diode"
name = "Bad diode"
category = "Diodes"

[symbol]
builtin = "Diode-COM-Standard"

[[pins]]
number = "1"
name = "a"
role = "passive"

[[pins]]
number = "2"
name = "k"
role = "passive"

[sim]
kind = "model"
spice_prefix = "D"
file = "models/diodes/bad.lib"
entry = "D_BAD"
pins = ["1", "2"]

[license]
spdx = "GPL-3.0-only"
source = "models/diodes/bad.lib"
"#,
        )
        .unwrap();

        let err = run(&root, true).expect_err("disallowed PK11 licence must fail");
        assert!(
            err.contains("library/parts/diodes/bad.toml") && err.contains("models/diodes/bad.lib"),
            "diagnostic must name part and model file: {err}"
        );
        assert!(
            err.contains("GPL-3.0-only"),
            "diagnostic must name rejected licence id: {err}"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// B1 (`PK11`): model files without an SPDX-License-Identifier header, or with a mismatched
    /// `THIRD_PARTY.md` licence entry, are refused.
    #[test]
    fn refuses_model_without_spdx_header_or_exact_third_party_entry() {
        let root = temp_workspace("missing-spdx");
        fs::create_dir_all(root.join("library/parts/diodes")).unwrap();
        fs::create_dir_all(root.join("library/models/diodes")).unwrap();
        fs::write(
            root.join("THIRD_PARTY.md"),
            "- `library/models/diodes/nohdr.lib` — `CC0-1.0`\n",
        )
        .unwrap();
        fs::write(
            root.join("library/models/diodes/nohdr.lib"),
            "* No SPDX-License-Identifier: MIT\n.model D_NOHDR D(IS=1e-12)\n",
        )
        .unwrap();
        let err = run(&root, true).expect_err("model without leading SPDX declaration must fail");
        assert!(
            err.contains("library/models/diodes/nohdr.lib")
                && err.contains("SPDX-License-Identifier"),
            "unexpected diagnostic: {err}"
        );

        // Bare uncommented SPDX-License-Identifier line must also be rejected.
        fs::write(
            root.join("library/models/diodes/nohdr.lib"),
            "SPDX-License-Identifier: MIT\n.model D_NOHDR D(IS=1e-12)\n",
        )
        .unwrap();
        let err = run(&root, true).expect_err("bare uncommented SPDX declaration must fail");
        assert!(
            err.contains("library/models/diodes/nohdr.lib")
                && err.contains("SPDX-License-Identifier"),
            "unexpected diagnostic: {err}"
        );

        // Now give it a valid SPDX header, but a THIRD_PARTY.md entry that declares Apache-2.0
        // and only mentions MIT in trailing prose.
        fs::write(
            root.join("THIRD_PARTY.md"),
            "- `library/models/diodes/nohdr.lib` — `Apache-2.0` (not `MIT`)\n",
        )
        .unwrap();
        fs::write(
            root.join("library/models/diodes/nohdr.lib"),
            "* SPDX-License-Identifier: MIT\n.model D_NOHDR D(IS=1e-12)\n",
        )
        .unwrap();
        let err = run(&root, true).expect_err("mismatched THIRD_PARTY.md licence must fail");
        assert!(
            err.contains("library/models/diodes/nohdr.lib")
                && err.contains("MIT")
                && err.contains("THIRD_PARTY.md"),
            "unexpected diagnostic: {err}"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// B2: `const_ident` yields valid, collision-free Rust identifiers across the `PK2` alphabet.
    #[test]
    fn const_ident_is_valid_and_injective_across_pk2_alphabet() {
        let samples = [
            "2-stage", "a-b", "a_b", "a.b", "a_hb", "a_ub", "a_db", "0.1_2-3",
        ];
        let mut seen = std::collections::BTreeSet::new();
        for s in samples {
            let ident = const_ident(s);
            let first = ident.chars().next().expect("non-empty ident");
            assert!(
                first.is_ascii_alphabetic(),
                "{s:?} -> {ident:?} must start with ASCII letter"
            );
            assert!(
                ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{s:?} -> {ident:?} must contain only Rust ident chars"
            );
            assert!(
                seen.insert(ident.clone()),
                "collision for {s:?} -> {ident:?}"
            );
        }
    }

    /// B3: a licensed shared model file included via `.include` is accepted by `xtask parts`.
    #[test]
    fn allows_shared_included_model_file() {
        let root = temp_workspace("shared-include");
        fs::create_dir_all(root.join("library/parts/diodes")).unwrap();
        fs::create_dir_all(root.join("library/models/diodes")).unwrap();
        fs::write(
            root.join("THIRD_PARTY.md"),
            concat!(
                "- `library/models/diodes/shared.inc` — `CC0-1.0`\n",
                "- `library/models/diodes/main.lib` — `CC0-1.0`\n",
            ),
        )
        .unwrap();
        fs::write(
            root.join("library/models/diodes/shared.inc"),
            "* SPDX-License-Identifier: CC0-1.0\n.param D_IS=1e-12\n",
        )
        .unwrap();
        fs::write(
            root.join("library/models/diodes/main.lib"),
            "* SPDX-License-Identifier: CC0-1.0\n.include shared.inc\n.model D_SHARED D(IS={D_IS})\n",
        )
        .unwrap();
        fs::write(
            root.join("library/parts/diodes/d.toml"),
            r#"
schema = 1
id = "d-shared"
name = "Shared diode"
category = "Diodes"

[symbol]
builtin = "Diode-COM-Standard"

[[pins]]
number = "1"
name = "a"
role = "passive"

[[pins]]
number = "2"
name = "k"
role = "passive"

[sim]
kind = "model"
spice_prefix = "D"
file = "models/diodes/main.lib"
entry = "D_SHARED"
pins = ["1", "2"]

[license]
spdx = "CC0-1.0"
source = "models/diodes/main.lib"
"#,
        )
        .unwrap();

        let rendered = render_from_files(&root).expect("shared .include model must compile");
        assert!(rendered.contains("d-shared"));
        let _ = fs::remove_dir_all(&root);
    }

    /// `PD11`: two parts declaring `[symbol] default = true` for the same symbol are refused.
    #[test]
    fn refuses_duplicate_symbol_default() {
        let root = temp_workspace("dup-default");
        fs::create_dir_all(root.join("library/parts/passives")).unwrap();
        fs::write(root.join("THIRD_PARTY.md"), "# Third-party\n").unwrap();
        let make_res = |id: &str| {
            format!(
                r#"
schema = 1
id = "{id}"
name = "Resistor {id}"
category = "Passives"

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

[sim]
kind = "primitive"
primitive = "resistor"
pins = ["1", "2"]
"#
            )
        };
        fs::write(
            root.join("library/parts/passives/a.toml"),
            make_res("res-a"),
        )
        .unwrap();
        fs::write(
            root.join("library/parts/passives/b.toml"),
            make_res("res-b"),
        )
        .unwrap();

        let err = run(&root, true).expect_err("duplicate symbol default must fail");
        assert!(
            err.contains("Resistor-IEEE-Standard"),
            "diagnostic must name symbol: {err}"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// `PK11` / `PQ7`: bundled model file missing from `THIRD_PARTY.md` is refused.
    #[test]
    fn refuses_model_missing_from_third_party() {
        let root = temp_workspace("missing-third-party");
        fs::create_dir_all(root.join("library/parts/diodes")).unwrap();
        fs::create_dir_all(root.join("library/models/diodes")).unwrap();
        fs::write(root.join("THIRD_PARTY.md"), "# Empty\n").unwrap();
        fs::write(
            root.join("library/models/diodes/unlisted.lib"),
            "* SPDX-License-Identifier: CC0-1.0\n.model D_TEST D(IS=1e-12)\n",
        )
        .unwrap();
        fs::write(
            root.join("library/parts/diodes/d.toml"),
            r#"
schema = 1
id = "d-test"
name = "Test diode"
category = "Diodes"

[symbol]
builtin = "Diode-COM-Standard"

[[pins]]
number = "1"
name = "a"
role = "passive"

[[pins]]
number = "2"
name = "k"
role = "passive"

[sim]
kind = "model"
spice_prefix = "D"
file = "models/diodes/unlisted.lib"
entry = "D_TEST"
pins = ["1", "2"]

[license]
spdx = "CC0-1.0"
"#,
        )
        .unwrap();

        let err = run(&root, true).expect_err("unlisted model file must fail");
        assert!(
            err.contains("library/models/diodes/unlisted.lib") && err.contains("THIRD_PARTY.md"),
            "unexpected diagnostic: {err}"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// `PQ5`: built-in repo parts must use `library/models/` rather than inline `spice`.
    #[test]
    fn refuses_inline_spice_in_builtin_library() {
        let root = temp_workspace("inline-spice");
        fs::create_dir_all(root.join("library/parts/diodes")).unwrap();
        fs::write(root.join("THIRD_PARTY.md"), "# Empty\n").unwrap();
        fs::write(
            root.join("library/parts/diodes/inline.toml"),
            r#"
schema = 1
id = "inline-diode"
name = "Inline diode"
category = "Diodes"

[symbol]
builtin = "Diode-COM-Standard"

[[pins]]
number = "1"
name = "a"
role = "passive"

[[pins]]
number = "2"
name = "k"
role = "passive"

[sim]
kind = "model"
spice_prefix = "D"
entry = "D_INLINE"
spice = ".model D_INLINE D(IS=1e-12)"
pins = ["1", "2"]

[license]
spdx = "CC0-1.0"
"#,
        )
        .unwrap();

        let err = run(&root, true).expect_err("inline spice in built-in library must fail");
        assert!(
            err.contains("library/parts/diodes/inline.toml") && err.contains("PQ5"),
            "unexpected diagnostic: {err}"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn part_pack_validates_and_writes_opart() {
        let root = temp_workspace("part-pack");
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("part.toml"),
            r#"
schema = 1
id = "custom-res"
name = "Custom resistor"
category = "Passives"
primary_param = "resistance"

[symbol]
builtin = "Resistor-IEEE-Standard"

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
default = "4.7k"
min = "1e-6"

[sim]
kind = "primitive"
primitive = "resistor"
pins = ["1", "2"]
"#,
        )
        .unwrap();
        let out = root.join("custom-res.opart");
        run_part_pack(&src, &out).expect("pack valid part");
        let original_bytes = fs::read(&out).expect("read original .opart");
        let archive = LibrarySource::open(&out).expect("reopen packed .opart");
        let resolved = archive.resolve_part("part.toml").expect("resolve packed");
        assert_eq!(resolved.part.id.as_str(), "custom-res");

        // B4: refusing to overwrite an existing destination leaves it intact, and wrong
        // extensions are refused before writing.
        run_part_pack(&src, &out).expect_err("existing output must not be overwritten");
        assert_eq!(
            fs::read(&out).expect("existing output preserved"),
            original_bytes
        );
        let wrong_ext = root.join("custom-res.zip");
        run_part_pack(&src, &wrong_ext).expect_err("non-.opart extension must fail");
        assert!(!wrong_ext.exists());

        // B4: a fixture that passes `pack_opart` (valid TOML + SPICE) yet fails `PD10`
        // (`model` enum variant not present in the extracted deck) must be caught on the staged
        // archive and leave neither the destination nor any staging file behind.
        let bad_dir = root.join("bad-case");
        let bad_src = bad_dir.join("src");
        fs::create_dir_all(&bad_src).unwrap();
        fs::write(
            bad_src.join("model.lib"),
            "* SPDX-License-Identifier: CC0-1.0\n.model D_REAL D(IS=1e-12)\n",
        )
        .unwrap();
        fs::write(
            bad_src.join("part.toml"),
            r#"
schema = 1
id = "bad-custom"
name = "Bad custom diode"
category = "Diodes"

[symbol]
builtin = "Diode-COM-Standard"

[[pins]]
number = "1"
name = "a"
role = "passive"

[[pins]]
number = "2"
name = "k"
role = "passive"

[params.model]
kind = "enum"
variants = ["D_REAL", "D_MISSING"]
default = "D_REAL"

[sim]
kind = "model"
spice_prefix = "D"
file = "model.lib"
entry = "D_REAL"
pins = ["1", "2"]
"#,
        )
        .unwrap();
        let bad_out_dir = bad_dir.join("out");
        fs::create_dir_all(&bad_out_dir).unwrap();
        let bad_out = bad_out_dir.join("bad-custom.opart");
        let err = run_part_pack(&bad_src, &bad_out).expect_err("staged PD10 check must fail");
        assert!(
            err.contains("PD10"),
            "diagnostic must come from staged PD10 validator: {err}"
        );
        assert!(
            fs::read_dir(&bad_out_dir).unwrap().next().is_none(),
            "neither output nor staging archive may remain after failure"
        );

        let _ = fs::remove_dir_all(&root);
    }
}
