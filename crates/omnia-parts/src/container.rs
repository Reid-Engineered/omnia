//! Bounded, read-only library sources and deterministic portable containers (P1.3).

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, Metadata};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use omnia_symbols::SymbolLibrary;
use zip::read::Config;
use zip::result::ZipError;
use zip::unstable::read::ZipIterable;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, PreparedZipFile, ZipArchive, ZipWriter};

use crate::hash::{ContentHash, hash_part};
use crate::schema::{ManifestToml, PartToml, PrimitiveKind, SchemaError, SimToml};
use crate::spice::{DeckSource, EntryKind, Extraction, MemSource, SpiceError, extract};

const MAX_ARCHIVE: u64 = 8 * 1024 * 1024;
const MAX_TOTAL: u64 = 32 * 1024 * 1024;
const MAX_ENTRY: u64 = 2 * 1024 * 1024;
const MAX_ENTRIES: usize = 1024;
const MAX_RATIO: u64 = 100;

#[derive(Debug)]
pub enum LibraryError {
    Io(io::Error),
    Zip(ZipError),
    InvalidContainer(&'static str),
    UnsafePath(String),
    Link(String),
    DuplicatePath(String),
    CustomSymbolUnsupported(String),
    MissingEntry(String),
    ArchiveTooLarge,
    TooManyEntries,
    EntryTooLarge(String),
    TotalTooLarge,
    ExpansionRatio(String),
    SizeMismatch(String),
    CorruptEntry { path: String, error: io::Error },
    NotUtf8 { path: String, offset: usize },
    Schema { path: String, error: SchemaError },
    Spice(SpiceError),
    UnknownSymbol(String),
    PinMismatch(String),
    TerminalCount { expected: usize, actual: usize },
    DuplicatePartId(String),
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Zip(error) => write!(f, "{error}"),
            Self::InvalidContainer(reason) => f.write_str(reason),
            Self::UnsafePath(path) => write!(f, "unsafe library path {path:?}"),
            Self::Link(path) => write!(f, "links are not allowed in libraries: {path:?}"),
            Self::DuplicatePath(path) => write!(f, "duplicate library path {path:?}"),
            Self::CustomSymbolUnsupported(path) => {
                write!(f, "custom symbol file {path:?} is unsupported")
            }
            Self::MissingEntry(path) => write!(f, "missing library entry {path:?}"),
            Self::ArchiveTooLarge => write!(f, "archive exceeds {MAX_ARCHIVE} bytes"),
            Self::TooManyEntries => write!(f, "library exceeds {MAX_ENTRIES} entries"),
            Self::EntryTooLarge(path) => write!(f, "{path:?} exceeds {MAX_ENTRY} bytes"),
            Self::TotalTooLarge => write!(f, "library exceeds {MAX_TOTAL} decoded bytes"),
            Self::ExpansionRatio(path) => {
                write!(f, "{path:?} exceeds the {MAX_RATIO}:1 expansion ratio")
            }
            Self::SizeMismatch(path) => write!(f, "size of {path:?} differs from its indexed size"),
            Self::CorruptEntry { path, error } => write!(f, "could not decode {path:?}: {error}"),
            Self::NotUtf8 { path, offset } => write!(f, "{path:?} is not UTF-8 at byte {offset}"),
            Self::Schema { path, error } => write!(f, "{path:?}: {error}"),
            Self::Spice(error) => write!(f, "{error}"),
            Self::UnknownSymbol(name) => write!(f, "unknown built-in symbol {name:?}"),
            Self::PinMismatch(name) => write!(f, "pins do not match built-in symbol {name:?}"),
            Self::TerminalCount { expected, actual } => write!(
                f,
                "expected {expected} simulation terminals, found {actual}"
            ),
            Self::DuplicatePartId(id) => write!(f, "duplicate part id {id:?}"),
        }
    }
}

impl std::error::Error for LibraryError {}

impl From<io::Error> for LibraryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ZipError> for LibraryError {
    fn from(error: ZipError) -> Self {
        Self::Zip(error)
    }
}

#[derive(Debug)]
struct Entry {
    size: u64,
    zip: Option<ZipEntry>,
}

#[derive(Debug)]
struct ZipEntry {
    index: usize,
    compressed: u64,
}

#[derive(Debug)]
enum Source {
    Directory(PathBuf),
    Archive(ZipArchive<Cursor<Arc<[u8]>>>),
}

/// Indexed directory or ZIP contents. Opening a source inspects metadata only. Model
/// files are decoded on demand and each read is bounded by the indexed entry budget.
#[derive(Debug)]
pub struct LibrarySource {
    source: Source,
    files: BTreeMap<String, Entry>,
}

/// Parsed source metadata and its self-contained, screened simulation content.
#[derive(Debug)]
pub struct ResolvedPart {
    pub part: PartToml,
    pub spice: Option<Extraction>,
    pub content_hash: ContentHash,
}

/// Metadata for a packed library.
#[derive(Debug)]
pub struct PackSummary {
    pub manifest: ManifestToml,
    pub part_count: usize,
}

impl LibrarySource {
    /// Open a directory, a single-part archive or a library pack.
    pub fn open(path: &Path) -> Result<Self, LibraryError> {
        let path = std::path::absolute(path)?;
        let metadata = checked_metadata(&path)?;
        if metadata.is_dir() {
            return Self::index_directory(path);
        }
        if !metadata.is_file() {
            return Err(LibraryError::UnsafePath(path.display().to_string()));
        }
        let layout = match path.extension().and_then(|ext| ext.to_str()) {
            Some("opart") => "part.toml",
            Some("olib") => "manifest.toml",
            _ => return Err(LibraryError::InvalidContainer("expected .opart or .olib")),
        };
        let source = Self::from_zip(open_regular(&path, || checked_metadata(&path))?)?;
        if !source.files.contains_key(layout) {
            return Err(LibraryError::MissingEntry(layout.into()));
        }
        Ok(source)
    }

    /// Index a ZIP stream within the PK3 archive size limit, including in-memory inputs.
    pub fn from_zip(reader: impl Read) -> Result<Self, LibraryError> {
        let bytes: Arc<[u8]> =
            read_bounded(reader, MAX_ARCHIVE, || LibraryError::ArchiveTooLarge)?.into();
        let cursor = Cursor::new(bytes);
        let mut headers = ZipIterable::try_new(cursor.clone(), Config::default())?;
        check_count(headers.len())?;
        let mut files = BTreeMap::new();
        let mut paths = BTreeSet::new();
        let mut directories = Vec::new();
        let mut total = 0;
        for (index, member) in headers.files()?.enumerate() {
            let member = member?;
            let raw = utf8("ZIP entry name", member.name_raw())?;
            let path = if member.is_dir() {
                raw.strip_suffix('/').unwrap_or(raw)
            } else {
                raw
            };
            check_path(path)?;
            if member.is_symlink() {
                return Err(LibraryError::Link(path.into()));
            }
            if !paths.insert(path.to_owned()) {
                return Err(LibraryError::DuplicatePath(path.into()));
            }
            if member.encrypted()
                || !matches!(
                    member.compression(),
                    CompressionMethod::Stored | CompressionMethod::Deflated
                )
                || member.unix_mode().is_some_and(|mode| {
                    let class = mode & 0o170000;
                    class != 0 && class != if member.is_dir() { 0o040000 } else { 0o100000 }
                })
            {
                return Err(LibraryError::InvalidContainer("unsupported ZIP entry type"));
            }
            let size = member.size();
            let compressed = member.compressed_size();
            check_size(path, size, &mut total)?;
            if size > compressed.saturating_mul(MAX_RATIO) {
                return Err(LibraryError::ExpansionRatio(path.into()));
            }
            if member.is_dir() {
                if size != 0 {
                    return Err(LibraryError::SizeMismatch(path.into()));
                }
                directories.push((index, path.to_owned()));
                continue;
            }
            files.insert(
                path.to_owned(),
                Entry {
                    size,
                    zip: Some(ZipEntry { index, compressed }),
                },
            );
        }
        for path in &paths {
            if path
                .match_indices('/')
                .any(|(offset, _)| files.contains_key(&path[..offset]))
            {
                return Err(LibraryError::DuplicatePath(path.clone()));
            }
        }
        let mut archive = ZipArchive::new(cursor)?;
        if archive.len() != paths.len() {
            return Err(LibraryError::InvalidContainer(
                "inconsistent ZIP entry count",
            ));
        }
        archive.validate_header_consistency()?;
        for (index, path) in directories {
            read_zip_entry(archive.by_index(index)?, &path, 0, 0)?;
        }
        Ok(Self {
            source: Source::Archive(archive),
            files,
        })
    }

    fn from_directory(root: PathBuf) -> Result<Self, LibraryError> {
        require_directory(&root)?;
        Self::index_directory(root)
    }

    fn index_directory(root: PathBuf) -> Result<Self, LibraryError> {
        let mut files = BTreeMap::new();
        let mut pending = vec![(String::new(), root.clone())];
        let mut count = 0;
        let mut total = 0;
        while let Some((prefix, dir)) = pending.pop() {
            for entry in fs::read_dir(dir)? {
                count += 1;
                check_count(count)?;
                let entry = entry?;
                let name = entry.file_name();
                let name = name
                    .to_str()
                    .ok_or_else(|| LibraryError::UnsafePath(entry.path().display().to_string()))?;
                let name = format!("{prefix}{name}");
                check_path(&name)?;
                let path = entry.path();
                let metadata = checked_metadata(&path)?;
                if metadata.is_dir() {
                    pending.push((format!("{name}/"), path));
                } else if metadata.is_file() {
                    let size = metadata.len();
                    check_size(&name, size, &mut total)?;
                    files.insert(name, Entry { size, zip: None });
                } else {
                    return Err(LibraryError::UnsafePath(name));
                }
            }
        }
        Ok(Self {
            source: Source::Directory(root),
            files,
        })
    }

    /// File paths in bytewise order, with '/' as the only separator.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    /// Read one file, refusing size, ratio and integrity failures before exposing its bytes.
    pub fn read(&self, path: &str) -> Result<Vec<u8>, LibraryError> {
        check_path(path)?;
        let entry = self
            .files
            .get(path)
            .ok_or_else(|| LibraryError::MissingEntry(path.into()))?;
        let bytes = match (&self.source, &entry.zip) {
            (Source::Directory(root), None) => {
                let path_on_disk = root.join(path);
                let file = open_regular(&path_on_disk, || directory_entry_metadata(root, path))?;
                read_entry(file, path, entry.size, MAX_ENTRY)?
            }
            (Source::Archive(archive), Some(zip)) => {
                let mut archive = archive.clone();
                let member = archive.by_index(zip.index)?;
                read_zip_entry(
                    member,
                    path,
                    entry.size,
                    zip.compressed.saturating_mul(MAX_RATIO),
                )?
            }
            _ => unreachable!("entries are indexed with their source"),
        };
        Ok(bytes)
    }

    pub fn part(&self, path: &str) -> Result<PartToml, LibraryError> {
        let bytes = self.read(path)?;
        PartToml::parse(utf8(path, &bytes)?).map_err(|error| LibraryError::Schema {
            path: path.into(),
            error,
        })
    }

    pub fn manifest(&self) -> Result<ManifestToml, LibraryError> {
        let path = "manifest.toml";
        let bytes = self.read(path)?;
        ManifestToml::parse(utf8(path, &bytes)?).map_err(|error| LibraryError::Schema {
            path: path.into(),
            error,
        })
    }

    pub fn resolve_part(&self, path: &str) -> Result<ResolvedPart, LibraryError> {
        let part = self.part(path)?;
        let symbols = SymbolLibrary::builtin();
        let symbol = symbols
            .by_name(&part.symbol.builtin)
            .ok_or_else(|| LibraryError::UnknownSymbol(part.symbol.builtin.clone()))?;
        let pins = symbols.get(symbol).pins;
        if pins.len() != part.pins.len()
            || !pins
                .iter()
                .all(|pin| part.pins.iter().any(|decl| decl.number == pin.number))
        {
            return Err(LibraryError::PinMismatch(part.symbol.builtin.clone()));
        }

        let spice = match &part.sim {
            SimToml::None => None,
            SimToml::Primitive { primitive, pins } => {
                let expected = match primitive {
                    PrimitiveKind::Ground | PrimitiveKind::Rail => 1,
                    PrimitiveKind::Potentiometer => 3,
                    PrimitiveKind::Switch
                    | PrimitiveKind::Vcvs
                    | PrimitiveKind::Vccs
                    | PrimitiveKind::Ccvs
                    | PrimitiveKind::Cccs => 4,
                    _ => 2,
                };
                check_terminals(expected, pins.len())?;
                None
            }
            SimToml::Model {
                entry, file, spice, ..
            } => Some(self.extract_spice(
                path,
                entry,
                file.as_deref(),
                spice.as_deref(),
                EntryKind::Model,
            )?),
            SimToml::Subckt {
                entry,
                file,
                spice,
                pins,
                ..
            } => {
                let extracted = self.extract_spice(
                    path,
                    entry,
                    file.as_deref(),
                    spice.as_deref(),
                    EntryKind::Subckt,
                )?;
                if let Some(ports) = &extracted.ports {
                    check_terminals(ports.len(), pins.len())?;
                }
                Some(extracted)
            }
        };
        let content_hash = hash_part(&part, spice.as_ref().map(|deck| deck.text.as_str()));
        Ok(ResolvedPart {
            part,
            spice,
            content_hash,
        })
    }

    fn extract_spice(
        &self,
        part_path: &str,
        entry: &str,
        file: Option<&str>,
        spice: Option<&str>,
        kind: EntryKind,
    ) -> Result<Extraction, LibraryError> {
        if let Some(file) = file {
            check_path(file)?;
            let source = SourceDeck {
                library: self,
                error: Cell::new(None),
            };
            return extract(&source, file, entry, kind)
                .map_err(|error| source.error.take().unwrap_or(LibraryError::Spice(error)));
        }

        let mut source = MemSource::new();
        source.insert(part_path, spice.unwrap_or_default().as_bytes());
        extract(&source, part_path, entry, kind).map_err(LibraryError::Spice)
    }
}

struct SourceDeck<'a> {
    library: &'a LibrarySource,
    error: Cell<Option<LibraryError>>,
}

impl DeckSource for SourceDeck<'_> {
    fn contains(&self, path: &str) -> bool {
        self.library.files.contains_key(path)
    }

    fn open(&self, path: &str) -> io::Result<Box<dyn Read + '_>> {
        match self.library.read(path) {
            Ok(bytes) => Ok(Box::new(Cursor::new(bytes))),
            Err(error) => {
                self.error.set(Some(error));
                Err(io::Error::from(io::ErrorKind::InvalidData))
            }
        }
    }
}

fn check_path(path: &str) -> Result<(), LibraryError> {
    if path.contains(['\\', ':', '\0'])
        || path
            .split('/')
            .any(|component| matches!(component, "" | "." | ".."))
    {
        return Err(LibraryError::UnsafePath(path.into()));
    }
    if path.split('/').any(|component| {
        component.eq_ignore_ascii_case("symbol.svg") || component.eq_ignore_ascii_case("pins.toml")
    }) {
        return Err(LibraryError::CustomSymbolUnsupported(path.into()));
    }
    Ok(())
}

fn checked_metadata(path: &Path) -> Result<Metadata, LibraryError> {
    let metadata = fs::symlink_metadata(path)?;
    if is_link(&metadata) {
        return Err(LibraryError::Link(path.display().to_string()));
    }
    Ok(metadata)
}

fn require_directory(path: &Path) -> Result<(), LibraryError> {
    if !checked_metadata(path)?.is_dir() {
        return Err(LibraryError::UnsafePath(path.display().to_string()));
    }
    Ok(())
}

fn directory_entry_metadata(root: &Path, path: &str) -> Result<Metadata, LibraryError> {
    require_directory(root)?;
    let mut current = root.to_path_buf();
    let mut components = path.split('/').peekable();
    while let Some(component) = components.next() {
        current.push(component);
        if components.peek().is_none() {
            return checked_metadata(&current);
        }
        require_directory(&current)?;
    }
    Err(LibraryError::UnsafePath(path.into()))
}

fn open_regular(
    path: &Path,
    check_metadata: impl Fn() -> Result<Metadata, LibraryError>,
) -> Result<File, LibraryError> {
    if !check_metadata()?.is_file() {
        return Err(LibraryError::UnsafePath(path.display().to_string()));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if is_link(&metadata) {
        return Err(LibraryError::Link(path.display().to_string()));
    }
    if !metadata.is_file() || !check_metadata()?.is_file() {
        return Err(LibraryError::UnsafePath(path.display().to_string()));
    }
    Ok(file)
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & REPARSE_POINT != 0 || metadata.file_type().is_symlink()
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn check_count(count: usize) -> Result<(), LibraryError> {
    if count > MAX_ENTRIES {
        return Err(LibraryError::TooManyEntries);
    }
    Ok(())
}

fn check_size(path: &str, size: u64, total: &mut u64) -> Result<(), LibraryError> {
    if size > MAX_ENTRY {
        return Err(LibraryError::EntryTooLarge(path.into()));
    }
    *total += size;
    if *total > MAX_TOTAL {
        return Err(LibraryError::TotalTooLarge);
    }
    Ok(())
}

fn check_terminals(expected: usize, actual: usize) -> Result<(), LibraryError> {
    if expected != actual {
        return Err(LibraryError::TerminalCount { expected, actual });
    }
    Ok(())
}

fn utf8<'a>(path: &str, bytes: &'a [u8]) -> Result<&'a str, LibraryError> {
    std::str::from_utf8(bytes).map_err(|error| LibraryError::NotUtf8 {
        path: path.into(),
        offset: error.valid_up_to(),
    })
}

fn read_zip_entry(
    reader: impl Read,
    path: &str,
    size: u64,
    ratio_limit: u64,
) -> Result<Vec<u8>, LibraryError> {
    read_entry(reader, path, size, ratio_limit).map_err(|error| match error {
        LibraryError::Io(error) => LibraryError::CorruptEntry {
            path: path.into(),
            error,
        },
        error => error,
    })
}

fn read_entry(
    reader: impl Read,
    path: &str,
    size: u64,
    ratio_limit: u64,
) -> Result<Vec<u8>, LibraryError> {
    // Accepted reads are at most their reservation in the checked 32 MiB index.
    // Probe past that reservation without retaining the extra decoded byte.
    let bytes = read_bounded(reader, size, || {
        if size == MAX_ENTRY {
            LibraryError::EntryTooLarge(path.into())
        } else if size == ratio_limit {
            LibraryError::ExpansionRatio(path.into())
        } else {
            LibraryError::SizeMismatch(path.into())
        }
    })?;
    if bytes.len() as u64 != size {
        return Err(LibraryError::SizeMismatch(path.into()));
    }
    Ok(bytes)
}

fn read_bounded(
    mut reader: impl Read,
    limit: u64,
    overflow: impl FnOnce() -> LibraryError,
) -> Result<Vec<u8>, LibraryError> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let request = chunk.len().min((limit - bytes.len() as u64 + 1) as usize);
        let count = match reader.read(&mut chunk[..request]) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            return Ok(bytes);
        }
        let end = bytes.len() + count;
        if end as u64 > limit {
            return Err(overflow());
        }
        if end > bytes.capacity() {
            let capacity = (2 * bytes.capacity()).max(end).min(limit as usize);
            bytes.reserve_exact(capacity - bytes.len());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

/// Pack a directory containing `part.toml` and return the resolved PK5 hash.
pub fn pack_opart(dir: &Path, out_path: &Path) -> Result<ContentHash, LibraryError> {
    let source = LibrarySource::from_directory(std::path::absolute(dir)?)?;
    let bytes = build_pack(&source)?;
    let archive = LibrarySource::from_zip(bytes.as_slice())?;
    let hash = archive.resolve_part("part.toml")?.content_hash;
    write_pack(out_path, "opart", &bytes)?;
    Ok(hash)
}

/// Pack a directory mirroring the v1 library layout.
pub fn pack_olib(dir: &Path, out_path: &Path) -> Result<PackSummary, LibraryError> {
    let source = LibrarySource::from_directory(std::path::absolute(dir)?)?;
    let bytes = build_pack(&source)?;
    let archive = LibrarySource::from_zip(bytes.as_slice())?;
    let manifest = archive.manifest()?;
    let mut ids = BTreeSet::new();
    for path in archive.paths().filter(|path| part_path(path)) {
        let part = archive.resolve_part(path)?.part;
        if !ids.insert(part.id.as_str().to_owned()) {
            return Err(LibraryError::DuplicatePartId(part.id.to_string()));
        }
    }
    write_pack(out_path, "olib", &bytes)?;
    Ok(PackSummary {
        manifest,
        part_count: ids.len(),
    })
}

fn part_path(path: &str) -> bool {
    let mut components = path.split('/');
    matches!(
        (components.next(), components.next(), components.next(), components.next()),
        (Some("parts"), Some(_), Some(file), None) if file.ends_with(".toml")
    )
}

fn build_pack(source: &LibrarySource) -> Result<Vec<u8>, LibraryError> {
    let mut writer = ZipWriter::new(ArchiveBuffer::default());
    let options = SimpleFileOptions::default()
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    for path in source.paths() {
        let bytes = source.read(path)?;
        let mut file = PreparedZipFile::from_data(
            path,
            options.compression_method(CompressionMethod::Deflated),
            &bytes,
        )?;
        if file.compressed_size() >= file.uncompressed_size()
            || file.uncompressed_size() > file.compressed_size().saturating_mul(MAX_RATIO)
        {
            file = PreparedZipFile::from_data(
                path,
                options.compression_method(CompressionMethod::Stored),
                &bytes,
            )?;
        }
        writer.add_prepared_file(file).map_err(pack_error)?;
    }
    Ok(writer.finish().map_err(pack_error)?.0.into_inner())
}

fn write_pack(out_path: &Path, extension: &str, bytes: &[u8]) -> Result<(), LibraryError> {
    if out_path.extension().and_then(|ext| ext.to_str()) != Some(extension) {
        return Err(LibraryError::InvalidContainer(
            "output extension does not match container type",
        ));
    }
    // Create exclusively so a previous output or a link is never overwritten.
    let mut output = File::create_new(out_path)?;
    if let Err(error) = output.write_all(bytes) {
        drop(output);
        let _ = fs::remove_file(out_path);
        return Err(error.into());
    }
    Ok(())
}

#[derive(Default)]
struct ArchiveBuffer(Cursor<Vec<u8>>);

impl Write for ArchiveBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.position().saturating_add(bytes.len() as u64) > MAX_ARCHIVE {
            return Err(io::Error::from(io::ErrorKind::FileTooLarge));
        }
        self.0.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Seek for ArchiveBuffer {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.0.seek(position)
    }
}

fn pack_error(error: ZipError) -> LibraryError {
    match error {
        ZipError::Io(error) if error.kind() == io::ErrorKind::FileTooLarge => {
            LibraryError::ArchiveTooLarge
        }
        _ => LibraryError::Zip(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    const PART: &str = r#"
schema = 1
id = "opamp-generic"
name = "Op-amp (generic)"
category = "Op-amps"

[symbol]
builtin = "IC-COM-OpAmp"

[[pins]]
number = "3"
name = "positive input"
role = "input"

[[pins]]
number = "2"
name = "negative input"
role = "input"

[[pins]]
number = "8"
name = "positive supply"
role = "power"

[[pins]]
number = "4"
name = "negative supply"
role = "power"

[[pins]]
number = "1"
name = "output"
role = "output"

[sim]
kind = "subckt"
file = "model.lib"
entry = "AMP"
pins = ["3", "2", "8", "4", "1"]

[sim.supply]
pins = ["8", "4"]
implicit = ["15", "-15"]
"#;
    const MODEL: &str = ".include models/helpers.inc\n.subckt AMP inp inn vcc vee out\n+ PARAMS: A=10\n* keep this comment\nE1 out vee inp inn {A*GAIN}\n.ends AMP\n";
    const HELPERS: &[u8] = b".param GAIN=2\n";
    const MANIFEST: &[u8] = b"schema=1\nid='opamps'\nname='Op-amps'\nversion='1'\n";
    const PART_PATH: &str = "parts/opamps/opamp-generic.toml";

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            loop {
                let path = std::env::temp_dir().join(format!(
                    "omnia-p13-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("{error}"),
                }
            }
        }

        fn populate(
            &self,
            dir: &str,
            files: impl IntoIterator<Item = (&'static str, &'static [u8])>,
        ) -> PathBuf {
            let root = self.0.join(dir);
            for (name, bytes) in files {
                let path = root.join(name);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, bytes).unwrap();
            }
            root
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn zip_bytes<'a>(
        files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
        method: CompressionMethod,
        date: DateTime,
    ) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default()
            .compression_method(method)
            .last_modified_time(date);
        for (path, bytes) in files {
            zip.start_file(path, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn stored<'a>(files: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> Vec<u8> {
        zip_bytes(files, CompressionMethod::Stored, DateTime::default())
    }

    #[test]
    fn ptc_opart_roundtrip() {
        let scratch = Scratch::new();
        let dir = scratch.populate(
            "source",
            [
                ("part.toml", PART.as_bytes()),
                ("model.lib", MODEL.as_bytes()),
                ("models/helpers.inc", HELPERS),
                ("LICENSE", &b"CC0-1.0\r\n"[..]),
            ],
        );
        let original = LibrarySource::open(&dir)
            .unwrap()
            .resolve_part("part.toml")
            .unwrap();
        let path = scratch.0.join("part.opart");
        assert_eq!(pack_opart(&dir, &path).unwrap(), original.content_hash);
        let archive = LibrarySource::open(&path).unwrap();
        let packed = archive.resolve_part("part.toml").unwrap();
        assert_eq!(packed.spice, original.spice);
        assert_eq!(packed.content_hash, original.content_hash);
        assert_eq!(archive.read("part.toml").unwrap(), PART.as_bytes());
        assert_eq!(archive.read("LICENSE").unwrap(), b"CC0-1.0\r\n");
    }

    proptest! {
        #[test]
        fn pinv2_archive_and_directory_order_do_not_affect_hash_or_output(
            dir_order in Just(vec![0, 1, 2, 3]).prop_shuffle(),
            zip_order in Just(vec![0, 1, 2, 3]).prop_shuffle(),
            year in 1981u16..=2107,
        ) {
            let files = [
                ("manifest.toml", MANIFEST),
                (PART_PATH, PART.as_bytes()),
                ("model.lib", MODEL.as_bytes()),
                ("models/helpers.inc", HELPERS),
            ];
            let scratch = Scratch::new();
            let ordered = scratch.populate("ordered", files);
            let shuffled = scratch.populate("shuffled", dir_order.into_iter().map(|idx| files[idx]));
            let order_out = scratch.0.join("ordered.olib");
            let shuffle_out = scratch.0.join("shuffled.olib");
            prop_assert_eq!(pack_olib(&ordered, &order_out).unwrap().part_count, 1);
            prop_assert_eq!(pack_olib(&shuffled, &shuffle_out).unwrap().part_count, 1);
            let baseline = fs::read(&order_out).unwrap();
            prop_assert_eq!(&baseline, &fs::read(shuffle_out).unwrap());

            let zip = zip_bytes(
                zip_order.into_iter().map(|idx| files[idx]),
                CompressionMethod::Stored,
                DateTime::from_date_and_time(year, 2, 3, 4, 5, 6).unwrap(),
            );
            let archive = LibrarySource::from_zip(zip.as_slice()).unwrap();
            let dir = LibrarySource::open(&ordered).unwrap();
            prop_assert_eq!(dir.paths().collect::<Vec<_>>(), archive.paths().collect::<Vec<_>>());
            prop_assert_eq!(
                dir.resolve_part(PART_PATH).unwrap().content_hash,
                archive.resolve_part(PART_PATH).unwrap().content_hash
            );
            prop_assert_eq!(baseline, build_pack(&archive).unwrap());
        }
    }

    #[test]
    fn pk5_ignores_provenance_source_format_and_deck_line_endings() {
        let resolve = |part: &str, model: &str| {
            let zip = stored([
                ("part.toml", part.as_bytes()),
                ("model.lib", model.as_bytes()),
                ("models/helpers.inc", HELPERS),
            ]);
            LibrarySource::from_zip(zip.as_slice())
                .unwrap()
                .resolve_part("part.toml")
                .unwrap()
                .content_hash
        };
        let normal = resolve(PART, MODEL);
        let reordered = PART.replace(
            "schema = 1\nid = \"opamp-generic\"",
            "id = 'another-id'\nschema = 1",
        );
        assert_eq!(
            normal,
            resolve(
                &reordered.replace('\n', "\r\n"),
                &MODEL.replace('\n', "\r\n")
            )
        );
        assert_ne!(
            normal,
            resolve(PART, &MODEL.replace("keep this comment", "edited comment"))
        );
        assert_ne!(
            normal,
            resolve(&PART.replace("positive input", "input +"), MODEL)
        );
        assert_eq!(normal.to_string().len(), 64);
        assert!(
            normal
                .to_string()
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        );
    }

    #[test]
    fn ptc_hostile_opart_rejects_unsafe_members_and_spice() {
        for path in [
            "../escape.lib",
            "models/../escape.lib",
            "/escape.lib",
            "C:/escape.lib",
            "x:y",
            "sub\\file",
            "sub//file",
            "sub/./file",
        ] {
            assert!(
                matches!(
                    LibrarySource::from_zip(stored([(path, &b""[..])]).as_slice()),
                    Err(LibraryError::UnsafePath(_))
                ),
                "{path}"
            );
        }
        for path in ["symbol.svg", "sub/pins.toml", "SYMBOL.SVG"] {
            assert!(
                matches!(
                    LibrarySource::from_zip(stored([(path, &b""[..])]).as_slice()),
                    Err(LibraryError::CustomSymbolUnsupported(_))
                ),
                "{path}"
            );
        }
        let invalid_spice = MODEL.replace("* keep this comment", ".control\n.endc");
        let file = stored([
            ("part.toml", PART.as_bytes()),
            ("model.lib", invalid_spice.as_bytes()),
            ("models/helpers.inc", HELPERS),
        ]);
        let archive = LibrarySource::from_zip(file.as_slice()).unwrap();
        assert!(archive.part("part.toml").is_ok());
        assert!(matches!(
            archive.resolve_part("part.toml"),
            Err(LibraryError::Spice(SpiceError::Screen { .. }))
        ));
    }

    #[test]
    fn pinv8_rejects_zip_links_duplicate_paths_and_malformed_zip() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.add_symlink("link", "../outside", SimpleFileOptions::default())
            .unwrap();
        assert!(matches!(
            LibrarySource::from_zip(zip.finish().unwrap().into_inner().as_slice()),
            Err(LibraryError::Link(_))
        ));

        let mut duplicate = stored([("aa", &b""[..]), ("bb", &b""[..])]);
        for (offset, marker) in [(30, b"PK\x03\x04"), (46, b"PK\x01\x02")] {
            let start = duplicate
                .windows(4)
                .rposition(|bytes| bytes == marker)
                .unwrap()
                + offset;
            duplicate[start..start + 2].copy_from_slice(b"aa");
        }
        assert!(matches!(
            LibrarySource::from_zip(duplicate.as_slice()),
            Err(LibraryError::DuplicatePath(_))
        ));

        let mut truncated = stored([("a", &b"data"[..])]);
        truncated.truncate(truncated.len() - 10);
        assert!(matches!(
            LibrarySource::from_zip(truncated.as_slice()),
            Err(LibraryError::Zip(_))
        ));
        assert!(matches!(
            LibrarySource::from_zip(&b"not a ZIP"[..]),
            Err(LibraryError::Zip(_))
        ));
    }

    struct DirectoryLink(PathBuf);

    impl DirectoryLink {
        fn new(link: PathBuf, target: &Path) -> Self {
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, &link).unwrap();
            #[cfg(windows)]
            {
                let output = std::process::Command::new("cmd.exe")
                    .args(["/d", "/c", "mklink", "/J"])
                    .arg(&link)
                    .arg(target)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Self(link)
        }
    }

    impl Drop for DirectoryLink {
        fn drop(&mut self) {
            #[cfg(unix)]
            let _ = fs::remove_file(&self.0);
            #[cfg(windows)]
            let _ = fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn pinv8_accepts_links_above_library_root() {
        let scratch = Scratch::new();
        let original = scratch.populate(
            "parent/source",
            [
                ("part.toml", PART.as_bytes()),
                ("model.lib", MODEL.as_bytes()),
                ("models/helpers.inc", HELPERS),
            ],
        );
        let link = DirectoryLink::new(scratch.0.join("alias"), original.parent().unwrap());
        let root = link.0.join("source");
        let source = LibrarySource::open(&root).unwrap();
        assert_eq!(source.read("models/helpers.inc").unwrap(), HELPERS);

        let output = link.0.join("packed.opart");
        let hash = pack_opart(&root, &output).unwrap();
        let archive = LibrarySource::open(&output).unwrap();
        assert_eq!(
            archive.resolve_part("part.toml").unwrap().content_hash,
            hash
        );
    }

    #[test]
    fn pinv8_rejects_os_links_and_junctions_inside_library_root() {
        let scratch = Scratch::new();
        let root = scratch.populate("source", [("nested/model.lib", MODEL.as_bytes())]);
        let target = scratch.populate("outside", [("model.lib", MODEL.as_bytes())]);
        let source = LibrarySource::open(&root).unwrap();
        let nested = root.join("nested");
        fs::rename(&nested, scratch.0.join("original")).unwrap();
        let link = DirectoryLink::new(nested, &target);

        assert!(matches!(
            LibrarySource::open(&link.0),
            Err(LibraryError::Link(_))
        ));
        assert!(matches!(
            LibrarySource::open(&root),
            Err(LibraryError::Link(_))
        ));
        assert!(matches!(
            source.read("nested/model.lib"),
            Err(LibraryError::Link(_))
        ));
    }

    #[test]
    fn pinv8_corrupt_directory_entry_reports_its_path() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.add_directory("models/", SimpleFileOptions::default())
            .unwrap();
        let mut bytes = zip.finish().unwrap().into_inner();
        let central = bytes
            .windows(4)
            .position(|part| part == b"PK\x01\x02")
            .unwrap();
        bytes[14] = 1;
        bytes[central + 16] = 1;
        assert!(matches!(
            LibrarySource::from_zip(bytes.as_slice()),
            Err(LibraryError::CorruptEntry { path, .. }) if path == "models"
        ));
    }

    #[test]
    fn pinv8_rejects_archive_entry_count_size_and_ratio_overflow() {
        let too_big = vec![0; MAX_ENTRY as usize + 1];
        assert!(matches!(
            LibrarySource::from_zip(stored([("large", &too_big[..])]).as_slice()),
            Err(LibraryError::EntryTooLarge(_))
        ));
        let bomb = zip_bytes(
            [("bomb", &too_big[..100_000])],
            CompressionMethod::Deflated,
            DateTime::default(),
        );
        assert!(matches!(
            LibrarySource::from_zip(bomb.as_slice()),
            Err(LibraryError::ExpansionRatio(_))
        ));
        let paths: Vec<_> = (0..=MAX_ENTRIES).map(|i| format!("entry{i}")).collect();
        let many = stored(paths.iter().map(|path| (path.as_str(), &b""[..])));
        assert!(matches!(
            LibrarySource::from_zip(many.as_slice()),
            Err(LibraryError::TooManyEntries)
        ));
        let too_big = vec![0; MAX_ARCHIVE as usize + 1];
        assert!(matches!(
            LibrarySource::from_zip(too_big.as_slice()),
            Err(LibraryError::ArchiveTooLarge)
        ));
    }

    #[test]
    fn pinv8_rejects_total_overflow_with_each_entry_under_the_ratio_cap() {
        let mut payload = vec![0; MAX_ENTRY as usize];
        let mut random = 1u32;
        for byte in &mut payload[..64 * 1024] {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            *byte = random as u8;
        }
        let paths: Vec<_> = (0..=MAX_TOTAL / MAX_ENTRY)
            .map(|i| format!("entry{i}"))
            .collect();
        let zip = zip_bytes(
            paths.iter().map(|path| (path.as_str(), payload.as_slice())),
            CompressionMethod::Deflated,
            DateTime::default(),
        );
        assert!(matches!(
            LibrarySource::from_zip(zip.as_slice()),
            Err(LibraryError::TotalTooLarge)
        ));
    }

    #[test]
    fn pinv8_stream_checks_forged_uncompressed_sizes() {
        for size in [1u32, 100_000] {
            let payload = vec![b'A'; 80_000];
            let mut archive = zip_bytes(
                [("model.lib", payload.as_slice())],
                CompressionMethod::Deflated,
                DateTime::default(),
            );
            let start = archive
                .windows(4)
                .position(|bytes| bytes == b"PK\x01\x02")
                .unwrap();
            archive[start + 24..start + 28].copy_from_slice(&size.to_le_bytes());
            archive[22..26].copy_from_slice(&size.to_le_bytes());
            let open = LibrarySource::from_zip(archive.as_slice());
            if size == 1 {
                assert!(matches!(
                    open.unwrap().read("model.lib"),
                    Err(LibraryError::CorruptEntry { error, .. })
                        if error.kind() == io::ErrorKind::InvalidData
                ));
            } else {
                assert!(matches!(open, Err(LibraryError::ExpansionRatio(_))));
            }
        }
    }

    #[test]
    fn pd10_refuses_invalid_utf8_in_toml_and_models() {
        let zip = stored([("part.toml", &b"\xff"[..])]);
        assert!(matches!(
            LibrarySource::from_zip(zip.as_slice())
                .unwrap()
                .part("part.toml"),
            Err(LibraryError::NotUtf8 { offset: 0, .. })
        ));
        let zip = stored([("part.toml", PART.as_bytes()), ("model.lib", &b"\xff"[..])]);
        assert!(matches!(
            LibrarySource::from_zip(zip.as_slice())
                .unwrap()
                .resolve_part("part.toml"),
            Err(LibraryError::Spice(SpiceError::NotUtf8 { offset: 0, .. }))
        ));
    }
}
