//! Metadata-only `.olib` / folder index prototype for spike PS1 (`PB3`).
//!
//! Reads `manifest.toml` and every `parts/**/*.toml`; never opens a model file and never
//! hashes (`PD9`). Enforces the `PK3` archive limits from the central directory before any
//! entry is decompressed, refuses unsafe or reserved entry names (`PD2`, `PD10`), parses each
//! `part.toml` with `deny_unknown_fields` (`PD4`), checks `PK1`, `PK2` and the `PK7` caps, and
//! returns entries sorted by qualified id so input order never matters (rule 15).
//!
//! Prototype only: P1.3 and P1.5 write the real `container.rs` and `catalog.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Seek};
use std::path::Path;

use serde::Deserialize;

/// `PK3` limits.
pub const MAX_ARCHIVE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_TOTAL_UNCOMPRESSED: u64 = 32 * 1024 * 1024;
pub const MAX_ENTRY_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 1024;
pub const MAX_RATIO: u64 = 100;

/// `PK7` caps used by the index.
pub const MAX_PARAMS: usize = 64;
pub const MAX_DESCRIPTION: usize = 2048;
pub const MAX_TAGS: usize = 64;
pub const MAX_TAG_BYTES: usize = 32;
pub const MAX_PINS: usize = 256;

#[derive(Debug)]
pub enum IndexError {
    Io(std::io::Error),
    Zip(String),
    TooLarge(u64),
    TooManyEntries(usize),
    EntryTooLarge(String, u64),
    TotalTooLarge(u64),
    Ratio(String),
    UnsafePath(String),
    Symlink(String),
    DuplicatePath(String),
    CustomSymbolUnsupported(String),
    NotUtf8(String),
    Toml(String, String),
    Schema(String, u32),
    BadId(String, String),
    IdMismatch(String, String),
    DuplicateId(String),
    Cap(String, &'static str),
    MissingManifest,
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for IndexError {}

impl From<std::io::Error> for IndexError {
    fn from(e: std::io::Error) -> Self {
        IndexError::Io(e)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub license: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartToml {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub category: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub datasheet: Option<String>,
    pub designator_prefix: Option<String>,
    pub primary_param: Option<String>,
    pub symbol: SymbolSec,
    #[serde(default)]
    pub pins: Vec<PinSec>,
    #[serde(default)]
    pub params: BTreeMap<String, ParamSec>,
    pub sim: SimSec,
    pub license: Option<LicenseSec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolSec {
    pub builtin: String,
    #[serde(default)]
    pub default: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinSec {
    pub number: String,
    pub name: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamSec {
    pub kind: String,
    pub unit: Option<String>,
    pub default: Option<toml::Value>,
    pub min: Option<String>,
    pub max: Option<String>,
    pub label: Option<String>,
    #[serde(default)]
    pub optional: bool,
    pub variants: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimSec {
    pub kind: String,
    pub primitive: Option<String>,
    pub spice_prefix: Option<String>,
    pub file: Option<String>,
    pub entry: Option<String>,
    #[serde(default)]
    pub pins: Vec<String>,
    pub supply: Option<SupplySec>,
    pub spice: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplySec {
    pub pins: Vec<String>,
    pub implicit: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LicenseSec {
    pub spdx: String,
    pub source: Option<String>,
}

/// One unverified catalog row (`PD9`: verified only on first resolve).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct IndexEntry {
    pub qualified_id: String,
    pub name: String,
    pub category: String,
    pub tags: Vec<String>,
    pub symbol: String,
    pub sim_kind: String,
    pub path: String,
}

#[derive(Debug)]
pub struct Index {
    pub pack: String,
    pub entries: Vec<IndexEntry>,
}

/// `PK2`: `^[a-z0-9][a-z0-9._-]{0,63}$`.
pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|&c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-')
        })
}

/// Archive entry path rules (`PD10`): relative, `/`-separated, no `..`, no drive, no NUL.
pub fn safe_entry_path(p: &str) -> bool {
    !p.is_empty()
        && !p.contains('\0')
        && !p.contains('\\')
        && !p.starts_with('/')
        && !p.contains(':')
        && p.split('/').all(|c| c != ".." && c != ".")
}

fn is_part_path(p: &str) -> bool {
    p.starts_with("parts/") && p.ends_with(".toml")
}

fn is_reserved(p: &str) -> bool {
    let base = p.rsplit('/').next().unwrap_or(p);
    base == "symbol.svg" || base == "pins.toml"
}

fn parse_manifest(text: &str) -> Result<Manifest, IndexError> {
    let m: Manifest = toml::from_str(text)
        .map_err(|e| IndexError::Toml("manifest.toml".into(), e.to_string()))?;
    if m.schema != 1 {
        return Err(IndexError::Schema("manifest.toml".into(), m.schema));
    }
    if !valid_id(&m.id) {
        return Err(IndexError::BadId("manifest.toml".into(), m.id));
    }
    Ok(m)
}

pub fn parse_part(pack: &str, path: &str, text: &str) -> Result<IndexEntry, IndexError> {
    let p: PartToml =
        toml::from_str(text).map_err(|e| IndexError::Toml(path.into(), e.to_string()))?;
    if p.schema != 1 {
        return Err(IndexError::Schema(path.into(), p.schema));
    }
    if !valid_id(&p.id) {
        return Err(IndexError::BadId(path.into(), p.id));
    }
    let stem = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".toml");
    if stem != p.id {
        return Err(IndexError::IdMismatch(path.into(), p.id));
    }
    if p.description.len() > MAX_DESCRIPTION {
        return Err(IndexError::Cap(path.into(), "description"));
    }
    if p.tags.len() > MAX_TAGS || p.tags.iter().any(|t| t.len() > MAX_TAG_BYTES) {
        return Err(IndexError::Cap(path.into(), "tags"));
    }
    if p.params.len() > MAX_PARAMS {
        return Err(IndexError::Cap(path.into(), "params"));
    }
    if p.pins.len() > MAX_PINS {
        return Err(IndexError::Cap(path.into(), "pins"));
    }
    Ok(IndexEntry {
        qualified_id: format!("{pack}/{}", p.id),
        name: p.name,
        category: p.category,
        tags: p.tags,
        symbol: p.symbol.builtin,
        sim_kind: p.sim.kind,
        path: path.to_owned(),
    })
}

fn finish(pack: String, mut entries: Vec<IndexEntry>) -> Result<Index, IndexError> {
    entries.sort();
    for w in entries.windows(2) {
        if w[0].qualified_id == w[1].qualified_id {
            return Err(IndexError::DuplicateId(w[0].qualified_id.clone()));
        }
    }
    Ok(Index { pack, entries })
}

/// Indexes an `.olib` file on disk (file size checked before it is opened for reading).
pub fn index_olib_file(path: &Path) -> Result<Index, IndexError> {
    let len = fs::metadata(path)?.len();
    if len > MAX_ARCHIVE_BYTES {
        return Err(IndexError::TooLarge(len));
    }
    let f = std::io::BufReader::new(fs::File::open(path)?);
    index_olib(f)
}

/// Indexes an `.olib` from any seekable reader.
pub fn index_olib<R: Read + Seek>(reader: R) -> Result<Index, IndexError> {
    let mut za = zip::ZipArchive::new(reader).map_err(|e| IndexError::Zip(e.to_string()))?;
    let n = za.len();
    if n > MAX_ENTRIES {
        return Err(IndexError::TooManyEntries(n));
    }
    // Pass 1: central-directory metadata only, no decompression.
    let mut total: u64 = 0;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut manifest_idx = None;
    let mut parts: Vec<(String, usize)> = Vec::new();
    for i in 0..n {
        let e = za
            .by_index_data(i)
            .map_err(|e| IndexError::Zip(e.to_string()))?;
        let name = e
            .name()
            .map_err(|e| IndexError::Zip(e.to_string()))?
            .into_owned();
        if e.is_symlink() {
            return Err(IndexError::Symlink(name));
        }
        if !safe_entry_path(&name) {
            return Err(IndexError::UnsafePath(name));
        }
        if is_reserved(&name) {
            return Err(IndexError::CustomSymbolUnsupported(name));
        }
        let size = e.size();
        let csize = e.compressed_size();
        if size > MAX_ENTRY_BYTES {
            return Err(IndexError::EntryTooLarge(name, size));
        }
        if size > csize.max(1).saturating_mul(MAX_RATIO) {
            return Err(IndexError::Ratio(name));
        }
        total += size;
        if total > MAX_TOTAL_UNCOMPRESSED {
            return Err(IndexError::TotalTooLarge(total));
        }
        if !seen.insert(name.clone()) {
            return Err(IndexError::DuplicatePath(name));
        }
        if e.is_dir() {
            continue;
        }
        if name == "manifest.toml" {
            manifest_idx = Some(i);
        } else if is_part_path(&name) {
            parts.push((name, i));
        }
    }
    parts.sort();
    let mut buf = String::new();
    let read_entry = |za: &mut zip::ZipArchive<R>, i: usize, name: &str, buf: &mut String| {
        buf.clear();
        let f = za.by_index(i).map_err(|e| IndexError::Zip(e.to_string()))?;
        // Stream under the declared size cap: a lying header cannot make us read more.
        let mut bytes = Vec::new();
        f.take(MAX_ENTRY_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_ENTRY_BYTES {
            return Err(IndexError::EntryTooLarge(
                name.to_owned(),
                bytes.len() as u64,
            ));
        }
        buf.push_str(
            std::str::from_utf8(&bytes).map_err(|_| IndexError::NotUtf8(name.to_owned()))?,
        );
        Ok::<(), IndexError>(())
    };
    let mi = manifest_idx.ok_or(IndexError::MissingManifest)?;
    read_entry(&mut za, mi, "manifest.toml", &mut buf)?;
    let manifest = parse_manifest(&buf)?;
    let mut entries = Vec::with_capacity(parts.len());
    for (name, i) in &parts {
        read_entry(&mut za, *i, name, &mut buf)?;
        entries.push(parse_part(&manifest.id, name, &buf)?);
    }
    finish(manifest.id, entries)
}

/// Indexes an unpacked folder with the same layout.
pub fn index_dir(root: &Path) -> Result<Index, IndexError> {
    let manifest = parse_manifest(&fs::read_to_string(root.join("manifest.toml"))?)?;
    let mut files: Vec<String> = Vec::new();
    let mut stack = vec![(root.join("parts"), String::from("parts"))];
    while let Some((dir, rel)) = stack.pop() {
        for e in fs::read_dir(&dir)? {
            let e = e?;
            let ty = e.file_type()?;
            let name = e.file_name().to_string_lossy().into_owned();
            let r = format!("{rel}/{name}");
            if ty.is_symlink() {
                return Err(IndexError::Symlink(r));
            } else if ty.is_dir() {
                stack.push((e.path(), r));
            } else if is_reserved(&r) {
                return Err(IndexError::CustomSymbolUnsupported(r));
            } else if is_part_path(&r) {
                files.push(r);
            }
        }
    }
    files.sort();
    let mut entries = Vec::with_capacity(files.len());
    for r in &files {
        let p = root.join(r);
        if fs::metadata(&p)?.len() > MAX_ENTRY_BYTES {
            return Err(IndexError::EntryTooLarge(r.clone(), 0));
        }
        let text = fs::read_to_string(&p).map_err(|_| IndexError::NotUtf8(r.clone()))?;
        entries.push(parse_part(&manifest.id, r, &text)?);
    }
    finish(manifest.id, entries)
}
