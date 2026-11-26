//! SPICE `.model` / `.subckt` extractor with local dependency closure (`P1.2`).
//!
//! A line-oriented extractor with no SPICE parser dependency, built on the `P0.3` spike
//! (`docs/measurements/P0.3.md`, `PS1`) and covering its fourteen grammar points. In order:
//!
//! 1. **Read** every file through a [`DeckSource`] with one byte budget per load
//!    ([`MAX_DECK_BYTES`], the `PK3` 8 MiB cap): every read, including each repeated include of
//!    the same file, counts, so neither one large file nor an include fan-out can exceed it.
//! 2. **Decode** each file as UTF-8 (refused with the byte offset otherwise, `PD10`), drop a
//!    leading byte-order mark and turn CRLF into LF. Nothing else is normalised (`PD3`).
//! 3. **Screen** every physical line of every file read, unselected `.lib` sections included,
//!    with the one `PK10` screen in `omnia-doc` ([`screen_embedded`]); only the include
//!    directives this module follows are exempt. A lone CR, a stray byte-order mark or a NUL
//!    byte is refused there too.
//! 4. **Flatten** `.include` / `.inc` / `.lib <file> <section>` that resolve inside the archive
//!    (`PK10` path rules, cycle and depth checks). Every include path in a file is checked,
//!    selected or not; only selected ones are followed (`P0.3` gap 14).
//! 5. **Join** `+` continuations into logical lines; blank and `*` lines may sit between.
//! 6. **Parse** a scoped definition table: `.subckt` (nested to any depth), `.model`, `.param`,
//!    `.func`, and `PARAMS:` names on a `.subckt` header as locals of that subcircuit.
//! 7. **Extract** an entry with its transitive top-level dependencies, `.param` and `.func`
//!    included (gap 1), resolving each reference from its own scope outwards (gap 2), checking
//!    positional model and subcircuit slots (gap 3) and skipping names by position, never by
//!    value (gap 13).
//! 8. **Emit** the physical lines of every block in flattened source order, byte for byte, and
//!    run [`screen_embedded`] on the result (`PD3`, `PINV9`).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{self, Read};
use std::ops::Range;

use omnia_doc::spice_screen::{ScreenError, screen_embedded, subckt_ports};

/// Deepest `.include` / `.lib` nesting followed, counting the root file.
pub const MAX_INCLUDE_DEPTH: usize = 16;

/// Byte budget for one deck load: the sum of every file read, each time it is read (`PK3`
/// 8 MiB file cap, applied to the reader path).
pub const MAX_DECK_BYTES: u64 = 8 * 1024 * 1024;

/// Largest extracted block: the embedded SPICE text cap (`PK7`, the `PK3` single-entry cap).
pub const MAX_EXTRACTED_BYTES: usize = 2 * 1024 * 1024;

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntryKind {
    Model,
    Subckt,
}

impl EntryKind {
    fn def_kind(self) -> DefKind {
        match self {
            EntryKind::Model => DefKind::Model,
            EntryKind::Subckt => DefKind::Subckt,
        }
    }
}

/// A definition namespace. Names are ASCII case-insensitive within one namespace and scope;
/// the namespaces are separate (`P0.3` gap 11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DefKind {
    Model,
    Subckt,
    Param,
    Func,
}

impl DefKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DefKind::Model => "model",
            DefKind::Subckt => "subckt",
            DefKind::Param => "param",
            DefKind::Func => "func",
        }
    }
}

/// Why a deck could not be read or an entry extracted. `file` is an archive-relative path and
/// `line` a 1-based physical line in that file after decoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpiceError {
    /// The root path breaks the `PK10` path rules.
    UnsafeRoot { path: String },
    /// The root file is not in the source.
    MissingRoot { path: String },
    /// The source failed to read a file.
    Io { file: String, kind: io::ErrorKind },
    /// The bytes read for one load passed [`MAX_DECK_BYTES`].
    TooLarge { file: String, limit: u64 },
    /// A file is not UTF-8 (`PD10`); `offset` is the first invalid byte.
    NotUtf8 { file: String, offset: usize },
    /// The `PK10` screen refused a line.
    Screen { file: String, error: ScreenError },
    /// An include path breaks the `PK10` path rules.
    IncludeUnsafePath {
        file: String,
        line: usize,
        path: String,
    },
    /// An include names a file that is not in the archive.
    IncludeMissing {
        file: String,
        line: usize,
        path: String,
    },
    /// An include names a file already being read.
    IncludeCycle {
        file: String,
        line: usize,
        path: String,
    },
    /// An include would nest deeper than [`MAX_INCLUDE_DEPTH`].
    IncludeTooDeep { file: String, line: usize },
    /// A `.lib <file> <section>` names a section the file does not have.
    LibSectionMissing { file: String, section: String },
    /// A `.lib <section>` / `.endl` outside a file included by section.
    SectionOutsideLibInclude { file: String, line: usize },
    /// A continuation line with no line to continue.
    OrphanContinuation { file: String, line: usize },
    /// A line the extractor cannot read.
    Malformed {
        file: String,
        line: usize,
        what: &'static str,
    },
    /// A `.subckt` with no `.ends`.
    UnterminatedSubckt {
        file: String,
        line: usize,
        name: String,
    },
    /// An `.ends` with no open `.subckt`.
    UnmatchedEnds { file: String, line: usize },
    /// An `.ends <name>` that closes a different subcircuit.
    EndsNameMismatch {
        file: String,
        line: usize,
        open: String,
        close: String,
    },
    /// Two definitions of one name in one scope and namespace.
    Duplicate {
        file: String,
        line: usize,
        kind: DefKind,
        name: String,
    },
    /// No top-level entry of that name and kind.
    EntryNotFound { name: String, kind: EntryKind },
    /// A positional model or subcircuit reference resolves to nothing.
    Unresolved {
        file: String,
        line: usize,
        name: String,
        kind: DefKind,
    },
    /// The extracted block is larger than [`MAX_EXTRACTED_BYTES`].
    OutputTooLarge { entry: String, bytes: usize },
}

impl SpiceError {
    /// A short stable code, as used by the `P0.3` fixtures.
    pub fn code(&self) -> &'static str {
        match self {
            SpiceError::UnsafeRoot { .. } => "unsafe_root",
            SpiceError::MissingRoot { .. } => "missing_root",
            SpiceError::Io { .. } => "io",
            SpiceError::TooLarge { .. } => "too_large",
            SpiceError::NotUtf8 { .. } => "not_utf8",
            SpiceError::Screen { error, .. } => match error {
                ScreenError::Forbidden { .. } => "forbidden",
                ScreenError::Include { .. } => "include_in_embedded",
                ScreenError::ByteOrderMark { .. } => "byte_order_mark",
                ScreenError::CarriageReturn { .. } => "carriage_return",
                ScreenError::Nul { .. } => "nul",
            },
            SpiceError::IncludeUnsafePath { .. } => "include_unsafe_path",
            SpiceError::IncludeMissing { .. } => "include_missing",
            SpiceError::IncludeCycle { .. } => "include_cycle",
            SpiceError::IncludeTooDeep { .. } => "include_too_deep",
            SpiceError::LibSectionMissing { .. } => "lib_section_missing",
            SpiceError::SectionOutsideLibInclude { .. } => "section_outside_lib_include",
            SpiceError::OrphanContinuation { .. } => "orphan_continuation",
            SpiceError::Malformed { .. } => "malformed",
            SpiceError::UnterminatedSubckt { .. } => "unterminated_subckt",
            SpiceError::UnmatchedEnds { .. } => "unmatched_ends",
            SpiceError::EndsNameMismatch { .. } => "ends_name_mismatch",
            SpiceError::Duplicate { .. } => "duplicate",
            SpiceError::EntryNotFound { .. } => "entry_not_found",
            SpiceError::Unresolved { .. } => "unresolved",
            SpiceError::OutputTooLarge { .. } => "output_too_large",
        }
    }
}

impl fmt::Display for SpiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpiceError::UnsafeRoot { path } => write!(f, "unsafe model path `{path}`"),
            SpiceError::MissingRoot { path } => write!(f, "model file `{path}` not found"),
            SpiceError::Io { file, kind } => write!(f, "{file}: read failed ({kind})"),
            SpiceError::TooLarge { file, limit } => {
                write!(f, "{file}: model files read exceed {limit} bytes")
            }
            SpiceError::NotUtf8 { file, offset } => {
                write!(f, "{file}: not UTF-8 at byte offset {offset}")
            }
            SpiceError::Screen { file, error } => write!(f, "{file}: {error}"),
            SpiceError::IncludeUnsafePath { file, line, path } => {
                write!(f, "{file}:{line}: unsafe include path `{path}`")
            }
            SpiceError::IncludeMissing { file, line, path } => {
                write!(
                    f,
                    "{file}:{line}: included file `{path}` not in the archive"
                )
            }
            SpiceError::IncludeCycle { file, line, path } => {
                write!(f, "{file}:{line}: include cycle through `{path}`")
            }
            SpiceError::IncludeTooDeep { file, line } => write!(
                f,
                "{file}:{line}: includes nested deeper than {MAX_INCLUDE_DEPTH}"
            ),
            SpiceError::LibSectionMissing { file, section } => {
                write!(f, "{file}: no `.lib {section}` section")
            }
            SpiceError::SectionOutsideLibInclude { file, line } => write!(
                f,
                "{file}:{line}: `.lib` section marker outside a section include"
            ),
            SpiceError::OrphanContinuation { file, line } => {
                write!(
                    f,
                    "{file}:{line}: continuation line with nothing to continue"
                )
            }
            SpiceError::Malformed { file, line, what } => write!(f, "{file}:{line}: {what}"),
            SpiceError::UnterminatedSubckt { file, line, name } => {
                write!(f, "{file}:{line}: `.subckt {name}` has no `.ends`")
            }
            SpiceError::UnmatchedEnds { file, line } => {
                write!(f, "{file}:{line}: `.ends` with no open `.subckt`")
            }
            SpiceError::EndsNameMismatch {
                file,
                line,
                open,
                close,
            } => write!(f, "{file}:{line}: `.ends {close}` closes `.subckt {open}`"),
            SpiceError::Duplicate {
                file,
                line,
                kind,
                name,
            } => write!(f, "{file}:{line}: duplicate {} `{name}`", kind.as_str()),
            SpiceError::EntryNotFound { name, kind } => {
                write!(f, "no top-level {} `{name}`", kind.def_kind().as_str())
            }
            SpiceError::Unresolved {
                file,
                line,
                name,
                kind,
            } => write!(f, "{file}:{line}: unresolved {} `{name}`", kind.as_str()),
            SpiceError::OutputTooLarge { entry, bytes } => write!(
                f,
                "`{entry}` extracts to {bytes} bytes, over {MAX_EXTRACTED_BYTES}"
            ),
        }
    }
}

impl std::error::Error for SpiceError {}

/// The files an extraction may read: an archive, an unpacked library folder or memory, keyed
/// by archive-relative paths with `/` separators.
pub trait DeckSource {
    /// Whether a file exists at `path`.
    fn contains(&self, path: &str) -> bool;
    /// Opens the file at `path` for reading. The extractor bounds what it reads.
    fn open(&self, path: &str) -> io::Result<Box<dyn Read + '_>>;
}

/// An in-memory [`DeckSource`].
#[derive(Clone, Debug, Default)]
pub struct MemSource {
    files: BTreeMap<String, Vec<u8>>,
}

impl MemSource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces the file at `path`.
    pub fn insert(&mut self, path: impl Into<String>, bytes: impl Into<Vec<u8>>) {
        self.files.insert(path.into(), bytes.into());
    }
}

impl DeckSource for MemSource {
    fn contains(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    fn open(&self, path: &str) -> io::Result<Box<dyn Read + '_>> {
        match self.files.get(path) {
            Some(bytes) => Ok(Box::new(bytes.as_slice())),
            None => Err(io::ErrorKind::NotFound.into()),
        }
    }
}

/// One block of an extraction: a definition and where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: DefKind,
    /// The name as written in the source.
    pub name: String,
    pub file: String,
    pub line_first: usize,
    pub line_last: usize,
}

/// A self-contained, flattened definition ready to embed (`PD3`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extraction {
    /// The entry name as written in the source.
    pub entry: String,
    pub kind: EntryKind,
    /// The `.subckt` port names, in order; `None` for a `.model`.
    pub ports: Option<Vec<String>>,
    /// Every emitted definition, in emitted order.
    pub blocks: Vec<Block>,
    /// The extracted lines, LF-terminated, byte for byte from the source.
    pub text: String,
}

/// Reads `root` from `source` and extracts one top-level entry with its local closure.
pub fn extract(
    source: &dyn DeckSource,
    root: &str,
    entry: &str,
    kind: EntryKind,
) -> Result<Extraction, SpiceError> {
    Deck::load(source, root)?.extract(entry, kind)
}

#[derive(Clone, Debug)]
struct PhysLine {
    file: usize,
    line: usize,
    text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineClass {
    Blank,
    Comment,
    Continuation,
    Normal,
}

fn classify(raw: &str) -> LineClass {
    let t = raw.trim_start();
    if t.starts_with('*') {
        LineClass::Comment
    } else if t.starts_with('+') {
        LineClass::Continuation
    } else if code_part(t).trim().is_empty() {
        LineClass::Blank
    } else {
        LineClass::Normal
    }
}

/// Cuts an inline comment: `;` anywhere, `$` or `//` after whitespace (or at the start), with
/// quoted text skipped (`P0.3` gap 6). The same rule as the `omnia-doc` screen.
fn code_part(s: &str) -> &str {
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

/// Whitespace tokens with `a = b`, `a= b` and `a =b` merged into `a=b` (`P0.3` gap 5).
fn tokens(code: &str) -> Vec<String> {
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

/// Directive arguments: whitespace-separated, with `"…"` or `'…'` taken whole and unquoted.
fn directive_args(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = code.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_ascii_whitespace()).is_some() {}
        let Some(&c) = chars.peek() else {
            return out;
        };
        let mut arg = String::new();
        if c == '"' || c == '\'' {
            chars.next();
            for d in chars.by_ref() {
                if d == c {
                    break;
                }
                arg.push(d);
            }
        } else {
            while let Some(d) = chars.next_if(|d| !d.is_ascii_whitespace()) {
                arg.push(d);
            }
        }
        out.push(arg);
    }
}

fn lc(s: &str) -> String {
    s.to_ascii_lowercase()
}

fn is_include_head(head: &str) -> bool {
    matches!(head, ".include" | ".inc" | ".lib" | ".endl")
}

fn has_params_prefix(token: &str) -> bool {
    token
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("params:"))
}

/// Where the ports of a `.subckt` header (or the nodes and subcircuit of an `X` line) end: at a
/// standalone `PARAMS:` token (any case) or at the first `name=value`. A port named
/// `params:out` is a port (review of #142).
fn is_params_boundary(token: &str) -> bool {
    token.eq_ignore_ascii_case("params:") || token.contains('=')
}

/// Applies the `PK10` path rules to `raw` (already unquoted) and resolves it against the
/// including file's directory: no empty path, NUL, leading `/` or `\`, `:` (drive or other
/// prefix) or `..` component. `\` separates like `/`.
fn resolve_path(including: Option<&str>, raw: &str) -> Option<String> {
    if raw.is_empty()
        || raw.contains('\0')
        || raw.starts_with('/')
        || raw.starts_with('\\')
        || raw.contains(':')
    {
        return None;
    }
    let mut parts: Vec<&str> = match including.and_then(|f| f.rfind('/').map(|i| &f[..i])) {
        Some(dir) => dir.split('/').collect(),
        None => Vec::new(),
    };
    for comp in raw.split(['/', '\\']) {
        match comp {
            "" | "." => {}
            ".." => return None,
            c => parts.push(c),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// What came before a line in the same file, for continuation checks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prev {
    Start,
    Line,
    Directive,
}

struct Flattener<'a> {
    source: &'a dyn DeckSource,
    budget_used: u64,
    files: Vec<String>,
    out: Vec<PhysLine>,
}

impl Flattener<'_> {
    fn file_id(&mut self, path: &str) -> usize {
        if let Some(i) = self.files.iter().position(|f| f == path) {
            return i;
        }
        self.files.push(path.to_owned());
        self.files.len() - 1
    }

    /// Reads one file within the load's byte budget and decodes it (`PD3`, `PD10`).
    fn read(&mut self, path: &str) -> Result<Vec<String>, SpiceError> {
        let io_err = |e: io::Error| SpiceError::Io {
            file: path.to_owned(),
            kind: e.kind(),
        };
        let remaining = MAX_DECK_BYTES - self.budget_used;
        let mut bytes = Vec::new();
        self.source
            .open(path)
            .map_err(io_err)?
            .take(remaining + 1)
            .read_to_end(&mut bytes)
            .map_err(io_err)?;
        let len = bytes.len() as u64;
        if len > remaining {
            return Err(SpiceError::TooLarge {
                file: path.to_owned(),
                limit: MAX_DECK_BYTES,
            });
        }
        self.budget_used += len;
        decode(path, &bytes)
    }

    fn check_include(&self, from: &str, line: usize, raw_path: &str) -> Result<String, SpiceError> {
        let Some(target) = resolve_path(Some(from), raw_path) else {
            return Err(SpiceError::IncludeUnsafePath {
                file: from.to_owned(),
                line,
                path: raw_path.to_owned(),
            });
        };
        if !self.source.contains(&target) {
            return Err(SpiceError::IncludeMissing {
                file: from.to_owned(),
                line,
                path: target,
            });
        }
        Ok(target)
    }

    fn file(
        &mut self,
        stack: &mut Vec<String>,
        path: &str,
        section: Option<&str>,
    ) -> Result<(), SpiceError> {
        let lines = self.read(path)?;
        screen_file(path, &lines)?;
        let fid = self.file_id(path);
        stack.push(path.to_owned());
        let want = section.map(lc);
        // None: outside any section; Some(true): inside the selected section; Some(false):
        // inside another section (skipped, but still screened and path-checked).
        let mut in_section: Option<(bool, usize)> = None;
        let mut found = want.is_none();
        let mut prev = Prev::Start;
        let malformed = |line, what| SpiceError::Malformed {
            file: path.to_owned(),
            line,
            what,
        };
        for (i, raw) in lines.iter().enumerate() {
            let n = i + 1;
            let emitting = want.is_none() || matches!(in_section, Some((true, _)));
            match classify(raw) {
                LineClass::Blank | LineClass::Comment => {
                    if emitting {
                        self.push(fid, n, raw);
                    }
                    continue;
                }
                LineClass::Continuation => {
                    match prev {
                        Prev::Start => {
                            return Err(SpiceError::OrphanContinuation {
                                file: path.to_owned(),
                                line: n,
                            });
                        }
                        Prev::Directive => {
                            return Err(malformed(n, "continued include or section line"));
                        }
                        Prev::Line => {}
                    }
                    if emitting {
                        self.push(fid, n, raw);
                    }
                    continue;
                }
                LineClass::Normal => {}
            }
            let args = directive_args(code_part(raw));
            let head = args.first().map(|h| lc(h)).unwrap_or_default();
            if !is_include_head(&head) {
                if emitting {
                    self.push(fid, n, raw);
                }
                prev = Prev::Line;
                continue;
            }
            prev = Prev::Directive;
            match (head.as_str(), args.len()) {
                (".lib", 2) => {
                    // A section start: only meaningful in a file included by section (gap 4).
                    if want.is_none() {
                        return Err(SpiceError::SectionOutsideLibInclude {
                            file: path.to_owned(),
                            line: n,
                        });
                    }
                    if in_section.is_some() {
                        return Err(malformed(n, "nested .lib section"));
                    }
                    let hit = want.as_deref() == Some(lc(&args[1]).as_str());
                    found |= hit;
                    in_section = Some((hit, n));
                }
                (".endl", 1 | 2) => {
                    if in_section.is_none() {
                        return Err(SpiceError::SectionOutsideLibInclude {
                            file: path.to_owned(),
                            line: n,
                        });
                    }
                    in_section = None;
                }
                (".include" | ".inc", 2) | (".lib", 3) => {
                    // Every include path in the file is checked, selected or not (gap 14);
                    // only selected ones are followed.
                    let target = self.check_include(path, n, &args[1])?;
                    if !emitting {
                        continue;
                    }
                    if stack.contains(&target) {
                        return Err(SpiceError::IncludeCycle {
                            file: path.to_owned(),
                            line: n,
                            path: target,
                        });
                    }
                    if stack.len() >= MAX_INCLUDE_DEPTH {
                        return Err(SpiceError::IncludeTooDeep {
                            file: path.to_owned(),
                            line: n,
                        });
                    }
                    let section = (head == ".lib").then(|| args[2].as_str());
                    self.file(stack, &target, section)?;
                }
                (".endl", _) => return Err(malformed(n, ".endl with extra arguments")),
                (".lib", 1) => return Err(malformed(n, ".lib without a file or section")),
                (".lib", _) => return Err(malformed(n, ".lib with extra arguments")),
                (_, 1) => return Err(malformed(n, "include without a path")),
                _ => return Err(malformed(n, "include with extra arguments")),
            }
        }
        if let Some((_, line)) = in_section {
            return Err(malformed(line, "unterminated .lib section"));
        }
        stack.pop();
        if !found {
            return Err(SpiceError::LibSectionMissing {
                file: path.to_owned(),
                section: section.unwrap_or_default().to_owned(),
            });
        }
        Ok(())
    }

    fn push(&mut self, file: usize, line: usize, text: &str) {
        self.out.push(PhysLine {
            file,
            line,
            text: text.to_owned(),
        });
    }
}

/// UTF-8 decode, leading byte-order mark dropped, CRLF → LF; nothing else (`PD3`, `PD10`).
fn decode(file: &str, bytes: &[u8]) -> Result<Vec<String>, SpiceError> {
    let text = std::str::from_utf8(bytes).map_err(|e| SpiceError::NotUtf8 {
        file: file.to_owned(),
        offset: e.valid_up_to(),
    })?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let text = text.replace("\r\n", "\n");
    let body = text.strip_suffix('\n').unwrap_or(&text);
    Ok(body.split('\n').map(str::to_owned).collect())
}

/// Screens a whole decoded file with the one `PK10` screen. The include and section lines this
/// module follows are blanked first (keeping line numbers); everything else, unselected `.lib`
/// sections included, goes through the embedded rules unchanged. A line carrying a CR, NUL or
/// byte-order mark is never blanked, so the stored-form rules hold on every line, include
/// lines included (review of #142).
fn screen_file(path: &str, lines: &[String]) -> Result<(), SpiceError> {
    let mut text = String::with_capacity(lines.iter().map(|l| l.len() + 1).sum());
    for raw in lines {
        let blank = !raw.contains(['\r', '\0', '\u{feff}'])
            && classify(raw) == LineClass::Normal
            && directive_args(code_part(raw))
                .first()
                .is_some_and(|h| is_include_head(&lc(h)));
        if !blank {
            text.push_str(raw);
        }
        text.push('\n');
    }
    screen_embedded(&text).map_err(|error| SpiceError::Screen {
        file: path.to_owned(),
        error,
    })
}

/// A joined logical line: one normal line plus its continuations.
#[derive(Clone, Debug)]
struct Logical {
    first: usize,
    last: usize,
    code: String,
    scope: Option<usize>,
}

/// One definition in the scoped table.
#[derive(Clone, Debug)]
struct Def {
    kind: DefKind,
    name: String,
    scope: Option<usize>,
    /// Physical line span (inclusive) in the flattened deck.
    first: usize,
    last: usize,
    /// Logical line span (inclusive).
    lfirst: usize,
    llast: usize,
    ports: Vec<String>,
    /// A `PARAMS:` name on a subcircuit header: a local, never emitted on its own.
    header_local: bool,
}

/// A read, screened, flattened and parsed deck.
#[derive(Clone, Debug)]
pub struct Deck {
    files: Vec<String>,
    lines: Vec<PhysLine>,
    logical: Vec<Logical>,
    defs: Vec<Def>,
    index: BTreeMap<(Option<usize>, DefKind, String), usize>,
}

impl Deck {
    /// Reads `root` (an archive-relative path) and everything it includes from `source`.
    pub fn load(source: &dyn DeckSource, root: &str) -> Result<Deck, SpiceError> {
        let Some(root_path) = resolve_path(None, root) else {
            return Err(SpiceError::UnsafeRoot {
                path: root.to_owned(),
            });
        };
        if !source.contains(&root_path) {
            return Err(SpiceError::MissingRoot { path: root_path });
        }
        let mut fl = Flattener {
            source,
            budget_used: 0,
            files: Vec::new(),
            out: Vec::new(),
        };
        fl.file(&mut Vec::new(), &root_path, None)?;
        let mut deck = Deck {
            files: fl.files,
            lines: fl.out,
            logical: Vec::new(),
            defs: Vec::new(),
            index: BTreeMap::new(),
        };
        deck.join()?;
        deck.parse()?;
        Ok(deck)
    }

    /// Top-level definition names of `kind`, as written, in source order.
    pub fn top_level(&self, kind: DefKind) -> Vec<&str> {
        self.defs
            .iter()
            .filter(|d| d.scope.is_none() && d.kind == kind && !d.header_local)
            .map(|d| d.name.as_str())
            .collect()
    }

    fn at(&self, phys: usize) -> (String, usize) {
        let l = &self.lines[phys];
        (self.files[l.file].clone(), l.line)
    }

    fn join(&mut self) -> Result<(), SpiceError> {
        let mut cur: Option<Logical> = None;
        for (i, l) in self.lines.iter().enumerate() {
            match classify(&l.text) {
                LineClass::Blank | LineClass::Comment => {}
                LineClass::Continuation => {
                    let Some(c) = cur.as_mut() else {
                        return Err(SpiceError::OrphanContinuation {
                            file: self.files[l.file].clone(),
                            line: l.line,
                        });
                    };
                    let body = l.text.trim_start();
                    let body = body.strip_prefix('+').unwrap_or(body);
                    c.code.push(' ');
                    c.code.push_str(code_part(body).trim());
                    c.last = i;
                }
                LineClass::Normal => {
                    if let Some(c) = cur.take() {
                        self.logical.push(c);
                    }
                    cur = Some(Logical {
                        first: i,
                        last: i,
                        code: code_part(l.text.trim_start()).trim().to_owned(),
                        scope: None,
                    });
                }
            }
        }
        self.logical.extend(cur);
        Ok(())
    }

    fn define(&mut self, def: Def) -> Result<usize, SpiceError> {
        let key = (def.scope, def.kind, lc(&def.name));
        if self.index.contains_key(&key) {
            let (file, line) = self.at(def.first);
            return Err(SpiceError::Duplicate {
                file,
                line,
                kind: def.kind,
                name: def.name,
            });
        }
        self.defs.push(def);
        let id = self.defs.len() - 1;
        self.index.insert(key, id);
        Ok(id)
    }

    fn parse(&mut self) -> Result<(), SpiceError> {
        let mut stack: Vec<usize> = Vec::new();
        for li in 0..self.logical.len() {
            let (first, last) = (self.logical[li].first, self.logical[li].last);
            let code = self.logical[li].code.clone();
            let toks = tokens(&code);
            let head = lc(&toks[0]);
            let scope = stack.last().copied();
            self.logical[li].scope = scope;
            let def = |kind, name: String, scope, header_local| Def {
                kind,
                name,
                scope,
                first,
                last,
                lfirst: li,
                llast: li,
                ports: Vec::new(),
                header_local,
            };
            let malformed = |what| {
                let (file, line) = self.at(first);
                SpiceError::Malformed { file, line, what }
            };
            match head.as_str() {
                ".subckt" => {
                    let name = toks
                        .get(1)
                        .ok_or_else(|| malformed(".subckt without a name"))?
                        .clone();
                    let (ports, params) = split_header(&toks[2..]);
                    let mut d = def(DefKind::Subckt, name, scope, false);
                    d.ports = ports;
                    let id = self.define(d)?;
                    for p in params {
                        self.define(def(DefKind::Param, p, Some(id), true))?;
                    }
                    stack.push(id);
                    self.logical[li].scope = Some(id);
                }
                ".ends" => {
                    let Some(id) = stack.pop() else {
                        let (file, line) = self.at(first);
                        return Err(SpiceError::UnmatchedEnds { file, line });
                    };
                    if let Some(close) = toks.get(1)
                        && lc(close) != lc(&self.defs[id].name)
                    {
                        let (file, line) = self.at(first);
                        return Err(SpiceError::EndsNameMismatch {
                            file,
                            line,
                            open: self.defs[id].name.clone(),
                            close: close.clone(),
                        });
                    }
                    self.defs[id].last = last;
                    self.defs[id].llast = li;
                    self.logical[li].scope = Some(id);
                }
                ".model" | ".func" => {
                    let raw = toks
                        .get(1)
                        .ok_or_else(|| malformed("definition without a name"))?;
                    let name = raw.split('(').next().unwrap_or_default();
                    if name.is_empty() {
                        return Err(malformed("definition without a name"));
                    }
                    let kind = if head == ".model" {
                        DefKind::Model
                    } else {
                        DefKind::Func
                    };
                    self.define(def(kind, name.to_owned(), scope, false))?;
                }
                ".param" | ".params" => {
                    let rest = after_head(&code);
                    for span in assignment_names(rest) {
                        self.define(def(DefKind::Param, rest[span].to_owned(), scope, false))?;
                    }
                }
                _ => {}
            }
        }
        if let Some(&open) = stack.last() {
            let (file, line) = self.at(self.defs[open].first);
            return Err(SpiceError::UnterminatedSubckt {
                file,
                line,
                name: self.defs[open].name.clone(),
            });
        }
        Ok(())
    }

    /// Resolves `name` of `kind` from `scope` outwards; the innermost definition wins (gap 2).
    fn resolve(&self, name: &str, kind: DefKind, scope: Option<usize>) -> Option<usize> {
        let key = lc(name);
        let mut s = scope;
        loop {
            if let Some(&d) = self.index.get(&(s, kind, key.clone())) {
                return Some(d);
            }
            s = self.defs[s?].scope;
        }
    }

    /// Extracts a top-level `.model` or `.subckt` with its transitive top-level dependencies
    /// (`.model`, `.subckt`, `.param`, `.func`), screened under the embedded rules (`PD3`,
    /// `PINV9`). The entry name is ASCII case-insensitive.
    pub fn extract(&self, entry: &str, kind: EntryKind) -> Result<Extraction, SpiceError> {
        let root = self.resolve(entry, kind.def_kind(), None).ok_or_else(|| {
            SpiceError::EntryNotFound {
                name: entry.to_owned(),
                kind,
            }
        })?;
        let mut included: BTreeSet<usize> = BTreeSet::from([root]);
        let mut queue = vec![root];
        while let Some(d) = queue.pop() {
            for l in &self.logical[self.defs[d].lfirst..=self.defs[d].llast] {
                self.check_slots(l)?;
                for (t, k) in references(&l.code) {
                    if let Some(x) = self.resolve(&t, k, l.scope)
                        && self.defs[x].scope.is_none()
                        && included.insert(x)
                    {
                        queue.push(x);
                    }
                }
            }
        }
        // One block per physical span, in flattened source order. A span lists every
        // top-level definition on it (one `.param` line may define several names; the
        // closure above already followed every right-hand side on that line).
        let emitted: BTreeSet<(usize, usize)> = included
            .iter()
            .map(|&d| (self.defs[d].first, self.defs[d].last))
            .collect();
        let mut spans: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
        for (d, def) in self.defs.iter().enumerate() {
            let span = (def.first, def.last);
            if def.scope.is_none() && emitted.contains(&span) {
                spans.entry(span).or_default().push(d);
            }
        }
        let mut text = String::new();
        let mut blocks = Vec::new();
        for (&(first, last), ds) in &spans {
            for l in &self.lines[first..=last] {
                text.push_str(&l.text);
                text.push('\n');
            }
            let (file, line_first) = self.at(first);
            for &d in ds {
                blocks.push(Block {
                    kind: self.defs[d].kind,
                    name: self.defs[d].name.clone(),
                    file: file.clone(),
                    line_first,
                    line_last: self.lines[last].line,
                });
            }
        }
        let name = self.defs[root].name.clone();
        if text.len() > MAX_EXTRACTED_BYTES {
            return Err(SpiceError::OutputTooLarge {
                entry: name,
                bytes: text.len(),
            });
        }
        screen_embedded(&text).map_err(|error| SpiceError::Screen {
            file: format!("<extracted {name}>"),
            error,
        })?;
        let ports = (kind == EntryKind::Subckt).then(|| self.defs[root].ports.clone());
        debug_assert_eq!(
            ports.as_ref().map(Vec::len),
            ports.as_ref().and_then(|_| subckt_ports(&text, &name)),
            "extractor and omnia-doc port counts agree"
        );
        Ok(Extraction {
            entry: name,
            kind,
            ports,
            blocks,
            text,
        })
    }

    /// Positional model and subcircuit slots that must resolve (gap 3): `D` 4th token, `J`/`Z`
    /// 5th, `M` 6th, `Q` 5th else 6th, and the subcircuit of `X` (the last token before
    /// `PARAMS:` or the first `name=value`).
    fn check_slots(&self, l: &Logical) -> Result<(), SpiceError> {
        let toks = tokens(&l.code);
        let Some(c) = toks[0].chars().next().map(|c| c.to_ascii_lowercase()) else {
            return Ok(());
        };
        let model_slots: &[usize] = match c {
            'd' => &[3],
            'j' | 'z' => &[4],
            'm' => &[5],
            'q' => &[4, 5],
            'x' => {
                let end = toks
                    .iter()
                    .position(|t| is_params_boundary(t))
                    .unwrap_or(toks.len());
                if end < 2 {
                    return Err(self.unresolved(l, "", DefKind::Subckt));
                }
                let name = &toks[end - 1];
                if self.resolve(name, DefKind::Subckt, l.scope).is_none() {
                    return Err(self.unresolved(l, name, DefKind::Subckt));
                }
                return Ok(());
            }
            _ => return Ok(()),
        };
        let mut tried = None;
        for &s in model_slots {
            if let Some(t) = toks.get(s) {
                let name = t.split('(').next().unwrap_or_default();
                if self.resolve(name, DefKind::Model, l.scope).is_some() {
                    return Ok(());
                }
                tried.get_or_insert_with(|| name.to_owned());
            }
        }
        Err(self.unresolved(l, tried.as_deref().unwrap_or(""), DefKind::Model))
    }

    fn unresolved(&self, l: &Logical, name: &str, kind: DefKind) -> SpiceError {
        let (file, line) = self.at(l.first);
        SpiceError::Unresolved {
            file,
            line,
            name: name.to_owned(),
            kind,
        }
    }
}

/// Splits `.subckt` header tokens after the name into ports and `PARAMS:` names. Ports end at
/// a standalone `PARAMS:` token (any case) or at the first `name=value` (gap 5).
fn split_header(rest: &[String]) -> (Vec<String>, Vec<String>) {
    let boundary = rest
        .iter()
        .position(|t| is_params_boundary(t))
        .unwrap_or(rest.len());
    let ports = rest[..boundary].to_vec();
    let tail = rest[boundary..].join(" ");
    let tail = if has_params_prefix(&tail) {
        &tail[7..]
    } else {
        &tail[..]
    };
    let params = assignment_names(tail)
        .into_iter()
        .map(|r| tail[r].to_owned())
        .collect();
    (ports, params)
}

/// The code after a line's first token.
fn after_head(code: &str) -> &str {
    let t = code.trim_start();
    t.find(|c: char| c.is_ascii_whitespace())
        .map_or("", |i| &t[i..])
}

fn is_name_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'$' | b'#' | b'@')
}

/// Byte ranges of the names assigned by `name=value` pairs. An `=` is an assignment only at
/// nesting depth 0 (outside `{}`, `()`, `[]` and quotes) and never part of `==`, `!=`, `<=` or
/// `>=`, so `.param GAIN={ LIMIT>=1 }` assigns `GAIN` and leaves `LIMIT` a reference (gap 13).
fn assignment_names(text: &str) -> Vec<Range<usize>> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    for i in 0..b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            b'"' | b'\'' => quote = Some(c),
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => depth -= 1,
            b'=' if depth == 0 => {
                let comparison = (i > 0 && matches!(b[i - 1], b'<' | b'>' | b'!' | b'='))
                    || b.get(i + 1) == Some(&b'=');
                if comparison {
                    continue;
                }
                let mut end = i;
                while end > 0 && b[end - 1].is_ascii_whitespace() {
                    end -= 1;
                }
                let mut start = end;
                while start > 0 && is_name_byte(b[start - 1]) {
                    start -= 1;
                }
                if start < end {
                    out.push(start..end);
                }
            }
            _ => {}
        }
    }
    out
}

/// Namespaces an expression may name: parameters and functions.
const EXPR: &[DefKind] = &[DefKind::Param, DefKind::Func];
/// Namespaces an element or directive operand may name: a model (any slot, so `R`/`C`/`L`/`S`/`W`
/// models and `AKO:` bases are included when defined, gap 3) or an expression. Over-including a
/// model or parameter adds a harmless block.
const OPERAND: &[DefKind] = &[DefKind::Model, DefKind::Param, DefKind::Func];

/// The `name=value` text with every depth-0 assigned name blanked, so only right-hand sides
/// remain (gap 13).
fn blank_assignments(text: &str) -> String {
    let mut blanked = text.to_owned();
    for r in assignment_names(text) {
        let blank = " ".repeat(r.len());
        blanked.replace_range(r, &blank);
    }
    blanked
}

/// Blanks left-hand parameter names on a `.model` tail (`<type> [(] k1=v1 ... [)]`), unwrapping
/// an outer parenthesized parameter list before [`blank_assignments`] so parameter names inside
/// `D(IS=1e-12)` are ignored while right-hand expressions like `D(IS={IS_VAL})` remain.
fn blank_model_assignments(rest: &str) -> String {
    let mut chars = rest.as_bytes().to_vec();
    let mut quote: Option<u8> = None;
    let mut saw_eq = false;
    let mut open_idx = None;
    for (i, &c) in chars.iter().enumerate() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            b'"' | b'\'' => quote = Some(c),
            b'=' => saw_eq = true,
            b'(' if !saw_eq => {
                open_idx = Some(i);
                break;
            }
            _ => {}
        }
    }
    if let Some(open) = open_idx {
        let mut depth = 0i32;
        let mut close_idx = None;
        let mut quote: Option<u8> = None;
        for (i, &c) in chars.iter().enumerate().skip(open) {
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
                continue;
            }
            match c {
                b'"' | b'\'' => quote = Some(c),
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        close_idx = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        chars[open] = b' ';
        if let Some(close) = close_idx {
            chars[close] = b' ';
        }
    }
    let unwrapped = std::str::from_utf8(&chars).expect("ASCII edits preserve UTF-8");
    blank_assignments(unwrapped)
}

/// Candidate references on a logical line, each with the namespace its position allows.
///
/// A subcircuit is referenced only from the subcircuit slot of an `X` line, so a port or node
/// named like an unrelated `.subckt` never pulls it in (review of #142). Names are skipped by
/// *position*, never by value (gap 13): the directive keyword or element name (first token),
/// the name a `.model`/`.subckt`/`.func` line defines (second token), `.subckt` ports and `X`
/// nodes, and the left-hand side of each depth-0 `name=value` (on `.param` lines, `.subckt`,
/// `X` and `.model` parameter lists and element instance parameters). Any other occurrence of the
/// same text is still a reference, so `D1 a k D1` with a top-level `.model D1` keeps its
/// dependency. Operands are split coarsely (whitespace, brackets, commas, quotes and `=`) and
/// again on expression operators.
fn references(code: &str) -> BTreeSet<(String, DefKind)> {
    let toks = tokens(code);
    let head = lc(&toks[0]);
    let mut out = BTreeSet::new();
    let mut fragments: Vec<(String, &[DefKind])> = Vec::new();
    let params_tail = |rest: &[String]| {
        let tail = rest.join(" ");
        let tail = if has_params_prefix(&tail) {
            tail[7..].to_owned()
        } else {
            tail
        };
        blank_assignments(&tail)
    };
    match head.as_str() {
        ".ends" => {}
        ".param" | ".params" => fragments.push((blank_assignments(after_head(code)), EXPR)),
        ".subckt" => {
            let rest = toks.get(2..).unwrap_or_default();
            let boundary = rest
                .iter()
                .position(|t| is_params_boundary(t))
                .unwrap_or(rest.len());
            fragments.push((params_tail(&rest[boundary..]), EXPR));
        }
        ".model" => {
            fragments.push((
                blank_model_assignments(after_head(after_head(code))),
                OPERAND,
            ));
        }
        ".func" => {
            for (i, t) in toks.iter().enumerate().skip(1) {
                let t = if i == 1 {
                    &t[t.split('(').next().unwrap_or_default().len()..]
                } else {
                    t.as_str()
                };
                fragments.push((t.to_owned(), OPERAND));
            }
        }
        h if h.starts_with('.') => fragments.push((after_head(code).to_owned(), OPERAND)),
        h if h.starts_with('x') => {
            let end = toks
                .iter()
                .position(|t| is_params_boundary(t))
                .unwrap_or(toks.len());
            if end >= 2 {
                out.insert((lc(&toks[end - 1]), DefKind::Subckt));
            }
            fragments.push((params_tail(&toks[end..]), EXPR));
        }
        _ => fragments.push((blank_assignments(after_head(code)), OPERAND)),
    }
    let coarse = |c: char| c.is_ascii_whitespace() || "(){}[],=\"'".contains(c);
    let fine = |c: char| "*/+-^<>!?:&|%".contains(c);
    for (frag, kinds) in fragments {
        for t in frag.split(coarse).filter(|t| !t.is_empty()) {
            for f in std::iter::once(t).chain(t.split(fine).filter(|f| !f.is_empty())) {
                for &k in kinds {
                    out.insert((lc(f), k));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    // ---- helpers ----

    /// A source from `(path, text)` pairs; the first file is the root.
    fn src(files: &[(&str, &str)]) -> MemSource {
        let mut s = MemSource::new();
        for (p, t) in files {
            s.insert(*p, t.as_bytes());
        }
        s
    }

    fn run(files: &[(&str, &str)], entry: &str, kind: EntryKind) -> Result<Extraction, SpiceError> {
        extract(&src(files), files[0].0, entry, kind)
    }

    fn ok(files: &[(&str, &str)], entry: &str, kind: EntryKind) -> Extraction {
        run(files, entry, kind).unwrap_or_else(|e| panic!("{entry}: {e}"))
    }

    fn code(r: Result<Extraction, SpiceError>) -> &'static str {
        match r {
            Ok(x) => panic!("expected a refusal, extracted:\n{}", x.text),
            Err(e) => e.code(),
        }
    }

    fn names(x: &Extraction) -> Vec<String> {
        x.blocks
            .iter()
            .map(|b| format!("{}:{}", b.kind.as_str(), b.name))
            .collect()
    }

    /// Re-extracting from the output alone gives the output (`P0.3` gap 12), the output is in
    /// stored form and passes the embedded screen, and port counts agree with `omnia-doc`.
    fn assert_self_contained(x: &Extraction) {
        assert!(!x.text.contains('\r') && !x.text.contains('\u{feff}'));
        assert_eq!(screen_embedded(&x.text), Ok(()));
        let again = ok(&[("out.lib", &x.text)], &x.entry, x.kind);
        assert_eq!(again.text, x.text, "extraction is a fixpoint");
        assert_eq!(
            x.ports.as_ref().map(Vec::len),
            subckt_ports(&x.text, &x.entry).filter(|_| x.kind == EntryKind::Subckt)
        );
    }

    const M: EntryKind = EntryKind::Model;
    const S: EntryKind = EntryKind::Subckt;

    // ---- the P0.3 spike decks and hostile set ----

    fn p03() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/measurements/P0.3")
    }

    fn dir_source(root: &Path) -> MemSource {
        fn walk(root: &Path, dir: &Path, out: &mut MemSource) {
            let mut entries: Vec<_> = std::fs::read_dir(dir)
                .expect("read fixture dir")
                .map(|e| e.expect("dir entry").path())
                .collect();
            entries.sort();
            for p in entries {
                if p.is_dir() {
                    walk(root, &p, out);
                } else {
                    let rel = p.strip_prefix(root).expect("under root");
                    let key: Vec<String> = rel
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect();
                    out.insert(key.join("/"), std::fs::read(&p).expect("read fixture"));
                }
            }
        }
        let mut out = MemSource::new();
        walk(root, root, &mut out);
        out
    }

    fn expect_toml() -> toml::Table {
        std::fs::read_to_string(p03().join("decks/expect.toml"))
            .expect("expect.toml")
            .parse()
            .expect("expect.toml parses")
    }

    fn kind_of(s: &str) -> EntryKind {
        match s {
            "model" => M,
            "subckt" => S,
            other => panic!("unknown kind {other}"),
        }
    }

    /// `PS1` parity: all 24 extractions of the 20 spike decks match the expectations and are
    /// byte-identical to the committed spike campaign (`results/extracted/`).
    #[test]
    fn ps1_twenty_decks_match_the_spike_campaign() {
        let decks = dir_source(&p03().join("decks"));
        let expect = expect_toml();
        let mut count = 0;
        for deck in expect["deck"].as_array().expect("decks") {
            let file = deck["file"].as_str().expect("file");
            for e in deck["entry"].as_array().expect("entries") {
                let name = e["name"].as_str().expect("name");
                let x = extract(
                    &decks,
                    file,
                    name,
                    kind_of(e["kind"].as_str().expect("kind")),
                )
                .unwrap_or_else(|err| panic!("{file} {name}: {err}"));
                let want_blocks: Vec<String> = e["blocks"]
                    .as_array()
                    .expect("blocks")
                    .iter()
                    .map(|b| b.as_str().expect("block").to_owned())
                    .collect();
                assert_eq!(names(&x), want_blocks, "{file} {name}");
                assert_eq!(
                    x.text.lines().count() as i64,
                    e["lines"].as_integer().expect("lines"),
                    "{file} {name}"
                );
                let want_ports = e.get("ports").map(|p| {
                    p.as_array()
                        .expect("ports")
                        .iter()
                        .map(|s| s.as_str().expect("port").to_owned())
                        .collect::<Vec<_>>()
                });
                assert_eq!(x.ports, want_ports, "{file} {name}");
                let stem = file.trim_end_matches(".lib");
                let campaign =
                    std::fs::read(p03().join(format!("results/extracted/{stem}--{name}.lib")))
                        .expect("campaign output");
                // `results/` is a text checkout (no `-text` attribute), so a Windows checkout
                // may have turned its LF into CRLF; the committed files are LF.
                let campaign = String::from_utf8(campaign)
                    .expect("UTF-8 campaign output")
                    .replace("\r\n", "\n");
                assert_eq!(x.text, campaign, "{file} {name}");
                assert_self_contained(&x);
                count += 1;
            }
        }
        assert_eq!(count, 24, "PS1: 24 extractions over 20 decks");
    }

    /// `PS1` hostile set: all 16 fixtures are refused with the spike's error code.
    #[test]
    fn ps1_hostile_fixtures_are_refused() {
        let hostile = dir_source(&p03().join("hostile"));
        let expect = expect_toml();
        let mut count = 0;
        for h in expect["hostile"].as_array().expect("hostile") {
            let file = h["file"].as_str().expect("file");
            let want = h["code"].as_str().expect("code");
            let got = match (Deck::load(&hostile, file), h.get("entry")) {
                (Err(e), _) => e.code(),
                (Ok(deck), Some(entry)) => code(deck.extract(entry.as_str().expect("entry"), S)),
                (Ok(_), None) => panic!("{file} loaded"),
            };
            assert_eq!(got, want, "{file}");
            count += 1;
        }
        assert_eq!(count, 16);
    }

    #[test]
    fn pd10_not_utf8_names_the_file_and_byte_offset() {
        let hostile = dir_source(&p03().join("hostile"));
        match Deck::load(&hostile, "h05-latin1.lib") {
            Err(SpiceError::NotUtf8 { file, offset }) => {
                assert_eq!(file, "h05-latin1.lib");
                let bytes = std::fs::read(p03().join("hostile/h05-latin1.lib")).expect("h05");
                assert_eq!(bytes[offset], 0xB5);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn pinv9_raw_deck_with_include_fails_the_embedded_screen() {
        let raw =
            std::fs::read_to_string(p03().join("decks/17-shunt-ref-include.lib")).expect("deck 17");
        assert!(matches!(
            screen_embedded(&raw),
            Err(ScreenError::Include { .. })
        ));
    }

    // ---- grammar ----

    #[test]
    fn continuations_with_comments_and_blank_lines_between_are_kept() {
        let deck = "* family\n\
                    .model NPN_A NPN(IS=1e-14\n\
                    * comment between continuations\n\
                    \n\
                    + BF=200 ; inline\n\
                    +\tVAF=100 $ dollar comment\n\
                    + )\n\
                    .model UNUSED NPN\n";
        let x = ok(&[("a.lib", deck)], "npn_a", M);
        assert_eq!(
            x.text,
            ".model NPN_A NPN(IS=1e-14\n\
             * comment between continuations\n\
             \n\
             + BF=200 ; inline\n\
             +\tVAF=100 $ dollar comment\n\
             + )\n"
        );
        assert_eq!(names(&x), ["model:NPN_A"]);
        assert_self_contained(&x);
    }

    #[test]
    fn pd3_comments_case_whitespace_kept_crlf_and_bom_normalised_only() {
        let deck = "\u{feff}* Header\r\n.MoDeL\tLed_Red\tD  IS = 93.2e-21 ; Keep ME\r\n+ N=2\r\n";
        let x = ok(&[("a.lib", deck)], "LED_RED", M);
        assert_eq!(x.entry, "Led_Red");
        assert_eq!(
            x.text,
            ".MoDeL\tLed_Red\tD  IS = 93.2e-21 ; Keep ME\n+ N=2\n"
        );
        // A file with no final newline still ends its last line with LF.
        let x = ok(&[("a.lib", ".model D1 D")], "d1", M);
        assert_eq!(x.text, ".model D1 D\n");
    }

    #[test]
    fn nested_subcircuits_and_local_models_stay_inside_the_block() {
        let deck = ".model QX NPN(BF=50)\n\
                    .subckt OUTER a b c\n\
                    XIN a b c INNER\n\
                    .subckt INNER x y z\n\
                    Q1 x y z QX\n\
                    .model QX NPN(BF=400) ; local, shadows the top-level QX\n\
                    .ends INNER\n\
                    .ends OUTER\n";
        let x = ok(&[("a.lib", deck)], "OUTER", S);
        assert_eq!(
            names(&x),
            ["subckt:OUTER"],
            "the local QX shadows the top-level one"
        );
        assert_eq!(
            x.ports.as_deref(),
            Some(&["a", "b", "c"].map(String::from)[..])
        );
        assert_self_contained(&x);
        // Without the local model, the top-level one is a dependency (gap 2).
        let deck = deck.replace(
            ".model QX NPN(BF=400) ; local, shadows the top-level QX\n",
            "",
        );
        let x = ok(&[("a.lib", &deck)], "OUTER", S);
        assert_eq!(names(&x), ["model:QX", "subckt:OUTER"]);
        // Nested names are not top-level entries.
        assert_eq!(
            code(run(&[("a.lib", &deck)], "INNER", S)),
            "entry_not_found"
        );
    }

    #[test]
    fn params_header_forms_give_ports_and_locals() {
        let deck = ".param GAIN=5\n\
                    .subckt A in out\n\
                    + PARAMS: GAIN=2\n\
                    E1 out 0 in 0 {GAIN}\n\
                    .ends A\n\
                    .subckt B in out params: gain = 3\n\
                    E1 out 0 in 0 {gain}\n\
                    .ends\n\
                    .subckt C in out gain=4 ; no PARAMS: keyword\n\
                    E1 out 0 in 0 {GAIN}\n\
                    .ends C\n\
                    .subckt D in out\n\
                    E1 out 0 in 0 {GAIN}\n\
                    .ends D\n";
        for entry in ["A", "B", "C"] {
            let x = ok(&[("a.lib", deck)], entry, S);
            assert_eq!(x.ports.as_ref().map(Vec::len), Some(2), "{entry}");
            assert_eq!(names(&x), [format!("subckt:{entry}")], "local GAIN shadows");
            assert_self_contained(&x);
        }
        let x = ok(&[("a.lib", deck)], "D", S);
        assert_eq!(names(&x), ["param:GAIN", "subckt:D"]);
    }

    #[test]
    fn transitive_closure_takes_models_subckts_params_and_funcs_in_source_order() {
        let deck = ".param VT=0.7 UNUSED=1\n\
                    .func clamp(x,lo,hi) {min(max(x,lo),hi)}\n\
                    .func unusedf(x) {2*x}\n\
                    .model DX D(IS=1e-14 VJ={VT})\n\
                    .model DUNUSED D\n\
                    .subckt STAGE a b\n\
                    D1 a b DX\n\
                    .ends STAGE\n\
                    .subckt TOP in out\n\
                    X1 in mid STAGE\n\
                    B1 mid out V={clamp(V(in),0,1)}\n\
                    .ends TOP\n";
        let x = ok(&[("a.lib", deck)], "top", S);
        assert_eq!(
            names(&x),
            [
                "param:VT",
                "param:UNUSED",
                "func:clamp",
                "model:DX",
                "subckt:STAGE",
                "subckt:TOP"
            ]
        );
        assert!(!x.text.contains("unusedf") && !x.text.contains("DUNUSED"));
        assert_self_contained(&x);
    }

    #[test]
    fn positional_slots_must_resolve() {
        let base = ".model MOD D\n.subckt SUB a b\n.ends SUB\n";
        for (element, ok_case) in [
            ("D1 a k MOD", true),
            ("D1 a k NOPE", false),
            ("J1 d g s MOD", true),
            ("J1 d g s NOPE", false),
            ("Z1 d g s MOD", true),
            ("Z1 d g s NOPE", false),
            ("M1 d g s b MOD L=1u", true),
            ("M1 d g s b NOPE L=1u", false),
            ("Q1 c b e MOD", true),
            ("Q1 c b e sub MOD", true),
            ("Q1 c b e sub NOPE", false),
            ("X1 a b SUB", true),
            ("X1 a b SUB PARAMS: g=1", true),
            ("X1 a b SUB g = 1", true),
            ("X1 a b NOPE", false),
            ("X1", false),
            ("R1 a b NOPE", true), // R/C/L/S/W models are not checked (gap 3)
        ] {
            let deck = format!("{base}.subckt T a b\n{element}\n.ends T\n");
            let r = run(&[("a.lib", &deck)], "T", S);
            assert_eq!(r.is_ok(), ok_case, "{element}: {r:?}");
            if !ok_case {
                assert_eq!(code(r), "unresolved", "{element}");
            }
        }
    }

    /// Gap 13: names are skipped by position, never by value.
    #[test]
    fn same_name_element_or_definition_keeps_its_dependency() {
        let x = ok(
            &[("a.lib", ".model D1 D\n.subckt T a k\nD1 a k D1\n.ends T\n")],
            "T",
            S,
        );
        assert_eq!(names(&x), ["model:D1", "subckt:T"]);
        let x = ok(
            &[(
                "a.lib",
                ".subckt XAMP i o\n.ends XAMP\n.subckt T i o\nXAMP i o XAMP\n.ends T\n",
            )],
            "T",
            S,
        );
        assert_eq!(names(&x), ["subckt:XAMP", "subckt:T"]);
        let x = ok(
            &[("a.lib", ".param VT=0.7\n.model VT NMOS(VTO={VT})\n")],
            "VT",
            M,
        );
        assert_eq!(names(&x), ["param:VT", "model:VT"]);
    }

    /// Gap 13: `==`, `!=`, `<=`, `>=` are comparisons, and `=` inside `{}` is not an
    /// assignment, on `.param` lines and on `.subckt` headers.
    #[test]
    fn comparisons_are_references_not_assignments() {
        for op in ["==", "!=", "<=", ">=", "=="] {
            let deck = format!(
                ".param LIMIT=1\n.param GAIN={{ LIMIT{op}1 ? 2 : 1 }}\n.model DX D(N={{GAIN}})\n"
            );
            let x = ok(&[("a.lib", &deck)], "DX", M);
            assert_eq!(names(&x), ["param:LIMIT", "param:GAIN", "model:DX"], "{op}");
            let deck = format!(
                ".param LIMIT=1\n.subckt T a b PARAMS: G={{LIMIT{op}1}}\nR1 a b {{G}}\n.ends T\n"
            );
            let x = ok(&[("a.lib", &deck)], "T", S);
            assert_eq!(names(&x), ["param:LIMIT", "subckt:T"], "{op}");
            assert_eq!(x.ports.as_ref().map(Vec::len), Some(2));
        }
        // Depth-0 comparisons without braces define nothing either.
        let x = ok(
            &[(
                "a.lib",
                ".param A=1\n.param B=2 C=A>=1\n.model DX D(N={C})\n",
            )],
            "DX",
            M,
        );
        assert_eq!(names(&x), ["param:A", "param:B", "param:C", "model:DX"]);
    }

    #[test]
    fn port_counts_agree_with_the_omnia_doc_screen() {
        let deck = ".subckt XTAL\n\
                    * ports on continuations\n\
                    + p1\n\
                    \n\
                    + p2 PARAMS: C0=5p\n\
                    C0 p1 p2 {C0}\n\
                    .ends XTAL\n\
                    .subckt OPAMP inp inn vcc vee out\n\
                    .ends\n";
        for (entry, n) in [("XTAL", 2), ("opamp", 5)] {
            let x = ok(&[("a.lib", deck)], entry, S);
            assert_eq!(x.ports.as_ref().map(Vec::len), Some(n));
            assert_eq!(subckt_ports(&x.text, entry), Some(n));
        }
    }

    #[test]
    fn structural_errors() {
        for (deck, want) in [
            (".subckt A a\n", "unterminated_subckt"),
            (".ends\n", "unmatched_ends"),
            (".subckt A a\n.ends B\n", "ends_name_mismatch"),
            (".model A D\n.MODEL a D\n", "duplicate"),
            (".param X=1\n.param x=2\n", "duplicate"),
            ("+ IS=1\n.model A D\n", "orphan_continuation"),
            ("* only a comment first\n+ IS=1\n", "orphan_continuation"),
            (".model\n", "malformed"),
        ] {
            assert_eq!(code(run(&[("a.lib", deck)], "A", M)), want, "{deck:?}");
        }
        // Same name in different namespaces or scopes is fine.
        let x = ok(
            &[(
                "a.lib",
                ".model A D\n.subckt A p\nD1 p 0 A\n.model A D\n.ends A\n",
            )],
            "A",
            S,
        );
        assert_eq!(names(&x), ["subckt:A"]);
        assert_eq!(
            code(run(&[("a.lib", ".model A D\n")], "A", S)),
            "entry_not_found"
        );
    }

    // ---- includes ----

    #[test]
    fn includes_flatten_in_place_relative_to_the_including_file() {
        let files = [
            (
                "models/top.lib",
                "* top\n.include \"common/base.inc\"\n.subckt T a b\nX1 a b CORE\n.ends T\n",
            ),
            (
                "models/common/base.inc",
                ".inc 'more lib.inc'\n.subckt CORE a b\nD1 a b DB\n.ends CORE\n",
            ),
            ("models/common/more lib.inc", ".model DB D\n"),
        ];
        let x = ok(&files, "T", S);
        assert_eq!(names(&x), ["model:DB", "subckt:CORE", "subckt:T"]);
        assert_eq!(x.blocks[0].file, "models/common/more lib.inc");
        assert!(!x.text.contains(".inc"));
        assert_self_contained(&x);
        // Backslash separators resolve the same way.
        let files = [
            ("a.lib", ".include sub\\b.inc\n"),
            ("sub/b.inc", ".model DB D\n"),
        ];
        assert_eq!(names(&ok(&files, "DB", M)), ["model:DB"]);
    }

    #[test]
    fn lib_sections_select_one_section_and_check_every_path() {
        let corners = "* corners\n\
                       .lib TT\n.param TRIM=0\n.model DREF D(IS=1e-14)\n.endl TT\n\
                       .lib FF\n.param TRIM=0.01\n.model DREF D(IS=2e-14)\n.endl\n";
        let top =
            ".lib \"inc/corners.lib\" tt\n.subckt REF o g\nV1 o n {TRIM}\nD1 n g DREF\n.ends REF\n";
        let x = ok(&[("top.lib", top), ("inc/corners.lib", corners)], "REF", S);
        assert_eq!(names(&x), ["param:TRIM", "model:DREF", "subckt:REF"]);
        assert!(x.text.contains("IS=1e-14") && !x.text.contains("IS=2e-14"));
        assert_self_contained(&x);

        let missing = top.replace(" tt\n", " SS\n");
        assert_eq!(
            code(run(
                &[("top.lib", &missing), ("inc/corners.lib", corners)],
                "REF",
                S
            )),
            "lib_section_missing"
        );
        // Gap 14: an unsafe or missing include in an unselected section is still refused.
        for (bad, want) in [
            (".include \"../../escape.lib\"", "include_unsafe_path"),
            (".lib /abs.lib TT", "include_unsafe_path"),
            (".include not-there.lib", "include_missing"),
        ] {
            let hostile = corners.replace(".param TRIM=0.01\n", &format!("{bad}\n"));
            assert_eq!(
                code(run(
                    &[("top.lib", top), ("inc/corners.lib", &hostile)],
                    "REF",
                    S
                )),
                want,
                "{bad}"
            );
        }
        // Section markers outside a section include, nesting and unterminated sections.
        for (root, want) in [
            (
                ".lib TT\n.model A D\n.endl\n",
                "section_outside_lib_include",
            ),
            (".endl\n", "section_outside_lib_include"),
            (
                ".lib only-one-arg-is-a-section\n",
                "section_outside_lib_include",
            ),
            (".lib a b c d\n", "malformed"),
            (".include\n", "malformed"),
            (".include a b\n", "malformed"),
        ] {
            assert_eq!(code(run(&[("a.lib", root)], "A", M)), want, "{root:?}");
        }
        for (lib, want) in [
            (".lib TT\n.lib FF\n.endl\n.endl\n", "malformed"),
            (".lib TT\n.model A D\n", "malformed"),
        ] {
            assert_eq!(
                code(run(&[("a.lib", ".lib b.lib TT\n"), ("b.lib", lib)], "A", M)),
                want,
                "{lib:?}"
            );
        }
    }

    #[test]
    fn pk10_unsafe_paths_are_refused() {
        for path in [
            "../x.lib",
            "a/../x.lib",
            "/x.lib",
            "\\x.lib",
            "\\\\server\\share\\x.lib",
            "C:/x.lib",
            "c:x.lib",
            "file:x.lib",
            "\"\"",
            ".",
        ] {
            let root = format!(".include {path}\n");
            let mut s = src(&[("a.lib", &root)]);
            s.insert("x.lib", ".model A D\n");
            assert_eq!(
                Deck::load(&s, "a.lib").map(|_| ()).map_err(|e| e.code()),
                Err("include_unsafe_path"),
                "{path:?}"
            );
        }
        // A NUL in a path is refused by the stored-form rule before the path rules run.
        let mut s = src(&[("a.lib", ".include x\0.lib\n")]);
        s.insert("x.lib", ".model A D\n");
        assert_eq!(Deck::load(&s, "a.lib").err().map(|e| e.code()), Some("nul"));
        assert_eq!(resolve_path(Some("a.lib"), "x\0.lib"), None);
        for root in ["../a.lib", "/a.lib", "C:\\a.lib", ""] {
            assert_eq!(
                Deck::load(&src(&[("a.lib", "")]), root)
                    .map(|_| ())
                    .map_err(|e| e.code()),
                Err("unsafe_root"),
                "{root:?}"
            );
        }
        assert_eq!(
            Deck::load(&src(&[("a.lib", "")]), "b.lib")
                .map(|_| ())
                .map_err(|e| e.code()),
            Err("missing_root")
        );
    }

    #[test]
    fn include_cycles_and_depth_are_bounded() {
        let files = [("a.lib", ".include b.lib\n"), ("b.lib", ".include a.lib\n")];
        assert_eq!(
            Deck::load(&src(&files), "a.lib").err().map(|e| e.code()),
            Some("include_cycle")
        );
        // A chain of MAX_INCLUDE_DEPTH files loads; one more is refused.
        let chain = |n: usize| {
            let mut s = MemSource::new();
            for i in 0..n {
                let body = if i + 1 < n {
                    format!(".include f{}.lib\n", i + 1)
                } else {
                    ".model LEAF D\n".to_owned()
                };
                s.insert(format!("f{i}.lib"), body);
            }
            s
        };
        let x = extract(&chain(MAX_INCLUDE_DEPTH), "f0.lib", "LEAF", M).expect("16 deep");
        assert_eq!(x.blocks[0].file, format!("f{}.lib", MAX_INCLUDE_DEPTH - 1));
        assert_eq!(
            Deck::load(&chain(MAX_INCLUDE_DEPTH + 1), "f0.lib")
                .err()
                .map(|e| e.code()),
            Some("include_too_deep")
        );
        // A continued include line and a continuation at the start of an included file.
        let files = [
            ("a.lib", ".include b.lib\n+ more\n"),
            ("b.lib", ".model A D\n"),
        ];
        assert_eq!(code(run(&files, "A", M)), "malformed");
        let files = [
            ("a.lib", ".model A D\n.include b.lib\n"),
            ("b.lib", "+ IS=1\n"),
        ];
        assert_eq!(code(run(&files, "A", M)), "orphan_continuation");
    }

    /// `PK3` 8 MiB on the reader path: one large file, or many reads of a smaller one.
    #[test]
    fn pk3_byte_budget_bounds_every_read() {
        let budget = MAX_DECK_BYTES as usize;
        let model = ".model A D\n";
        let mut exact = String::with_capacity(budget);
        exact.push_str(model);
        let filler = "* filler comment line, padded to sixty-four bytes in total.....\n";
        while exact.len() + filler.len() <= budget {
            exact.push_str(filler);
        }
        while exact.len() < budget {
            exact.push('*');
        }
        assert_eq!(exact.len(), budget);
        assert_eq!(ok(&[("a.lib", &exact)], "A", M).text, model);
        let over = format!("{exact}*");
        assert_eq!(code(run(&[("a.lib", &over)], "A", M)), "too_large");

        // An include fan-out: 20 reads of a 512 KiB file pass the budget although each is small.
        let half_mib = "*".repeat(512 * 1024 - 1) + "\n";
        let root = ".include big.inc\n".repeat(20) + model;
        assert_eq!(
            code(run(&[("a.lib", &root), ("big.inc", &half_mib)], "A", M)),
            "too_large"
        );
        let root = ".include big.inc\n".repeat(10) + model;
        assert_eq!(
            ok(&[("a.lib", &root), ("big.inc", &half_mib)], "A", M).text,
            model
        );
    }

    #[test]
    fn pk7_extracted_text_is_capped() {
        let mut deck = String::from(".model BIG D(IS=1e-14\n");
        let cont = "+ N=1 * padding padding padding padding padding padding padding\n";
        while deck.len() <= MAX_EXTRACTED_BYTES {
            deck.push_str(cont);
        }
        deck.push_str("+ )\n");
        assert_eq!(code(run(&[("a.lib", &deck)], "BIG", M)), "output_too_large");
    }

    // ---- PK10 / PINV9 ----

    const DIRECTIVES: [&str; 8] = [
        ".control",
        ".endc",
        "shell",
        "system",
        "osdi",
        "pre_osdi",
        "codemodel",
        ".csdf",
    ];

    /// `PINV9`: every `PK10` directive is refused in archive form wherever it sits (root,
    /// included file, unselected `.lib` section, inside a `.subckt`, on a continuation) and in
    /// embedded form.
    #[test]
    fn pinv9_every_pk10_directive_is_refused_in_archive_and_embedded_form() {
        for d in DIRECTIVES {
            for written in [d.to_owned(), d.to_ascii_uppercase()] {
                let line = format!("{written} arg");
                // On a continuation only the dot form starts a directive; a bare token there
                // continues the line (review of #142).
                let dotted = if written.starts_with('.') {
                    written.clone()
                } else {
                    format!(".{written}")
                };
                let cases: Vec<(Vec<(&str, String)>, &str)> = vec![
                    (vec![("a.lib", format!(".model A D\n{line}\n"))], &written),
                    (
                        vec![("a.lib", format!(".subckt A p\n  {line}\n.ends A\n"))],
                        &written,
                    ),
                    (
                        vec![("a.lib", format!(".model A D(IS=1\n+ {dotted} arg\n+ )\n"))],
                        &dotted,
                    ),
                    (
                        vec![
                            ("a.lib", ".include b.inc\n.model A D\n".to_owned()),
                            ("b.inc", format!("* included\n{line}\n")),
                        ],
                        &written,
                    ),
                    (
                        vec![
                            ("a.lib", ".lib b.lib TT\n".to_owned()),
                            (
                                "b.lib",
                                format!(".lib TT\n.model A D\n.endl\n.lib FF\n{line}\n.endl\n"),
                            ),
                        ],
                        &written,
                    ),
                ];
                for (files, want) in cases {
                    let refs: Vec<(&str, &str)> =
                        files.iter().map(|(p, t)| (*p, t.as_str())).collect();
                    match run(&refs, "A", M) {
                        Err(SpiceError::Screen {
                            error: ScreenError::Forbidden { directive, .. },
                            ..
                        }) => assert_eq!(directive, want),
                        other => panic!("{written} in {files:?}: {other:?}"),
                    }
                }
                let embedded = format!(".model A D\n{line}\n");
                assert!(
                    matches!(
                        screen_embedded(&embedded),
                        Err(ScreenError::Forbidden { line: 2, .. })
                    ),
                    "{written} embedded"
                );
            }
        }
        // `PTC-hostile-opart` shape: `.control / shell rm -rf` in a library.
        let deck = ".subckt EVIL a b\nR1 a b 1k\n.ends EVIL\n.control\nshell rm -rf /\n.endc\n";
        assert_eq!(code(run(&[("a.lib", deck)], "EVIL", S)), "forbidden");
    }

    #[test]
    fn pinv9_stored_form_violations_are_refused_in_archives() {
        for (deck, want) in [
            (".model A D\r.control\n", "carriage_return"),
            (".model A D\n* \u{feff}\n", "byte_order_mark"),
            (".model A D\n*\0\n", "nul"),
            (".includex b.lib\n.model A D\n", "include_in_embedded"),
        ] {
            assert_eq!(code(run(&[("a.lib", deck)], "A", M)), want, "{deck:?}");
        }
        let mut s = MemSource::new();
        s.insert("a.lib", b".model A D\n* \xB5\n".to_vec());
        assert_eq!(
            extract(&s, "a.lib", "A", M),
            Err(SpiceError::NotUtf8 {
                file: "a.lib".to_owned(),
                offset: 13
            })
        );
    }

    #[test]
    fn extraction_is_independent_of_source_insertion_order() {
        let files = [
            (
                "top.lib",
                ".include b.inc\n.include c.inc\n.subckt T a\nX1 a B\nX2 a C\n.ends T\n",
            ),
            ("b.inc", ".subckt B a\nD1 a 0 DM\n.ends B\n"),
            ("c.inc", ".model DM D\n.subckt C a\n.ends C\n"),
        ];
        let forward = ok(&files, "T", S);
        let mut s = MemSource::new();
        for (p, t) in files.iter().rev() {
            s.insert(*p, t.as_bytes());
        }
        assert_eq!(extract(&s, "top.lib", "T", S), Ok(forward));
    }

    // ---- review of #142 ----

    /// A bare `+ SYSTEM` continues the line (a port or model type); it is not a command.
    #[test]
    fn bare_continuation_tokens_are_not_directives() {
        let x = ok(
            &[(
                "a.lib",
                ".subckt AMP in\n+ SYSTEM\nR1 in SYSTEM 1k\n.ends AMP\n",
            )],
            "amp",
            S,
        );
        assert_eq!(
            x.ports.as_deref(),
            Some(&["in", "SYSTEM"].map(String::from)[..])
        );
        assert_self_contained(&x);
        let x = ok(&[("a.lib", ".model DSYS\n+ SYSTEM D\n")], "DSYS", M);
        assert_eq!(x.text, ".model DSYS\n+ SYSTEM D\n");
        assert_self_contained(&x);
    }

    /// `PARAMS:` is a boundary only as a standalone token: `params:out` is a port.
    #[test]
    fn a_port_named_like_params_is_a_port() {
        let deck = ".subckt A in params:out PARAMS: g=1\nR1 in params:out {g}\n.ends A\n\
                    .subckt B x\nXA x y A g=2\n.ends B\n";
        let x = ok(&[("a.lib", deck)], "A", S);
        assert_eq!(
            x.ports.as_deref(),
            Some(&["in", "params:out"].map(String::from)[..])
        );
        assert_eq!(subckt_ports(&x.text, "A"), Some(2));
        let x = ok(&[("a.lib", deck)], "B", S);
        assert_eq!(names(&x), ["subckt:A", "subckt:B"]);
        // An `X` line whose last node is `params:out` still names its subcircuit last.
        let deck = ".subckt A p q\n.ends A\n.subckt T i\nX1 i params:out A\n.ends T\n";
        assert_eq!(
            names(&ok(&[("a.lib", deck)], "T", S)),
            ["subckt:A", "subckt:T"]
        );
    }

    /// A port or node named like an unrelated `.subckt` does not pull that subcircuit in: only
    /// the subcircuit slot of an `X` line references the subcircuit namespace.
    #[test]
    fn subcircuits_are_referenced_only_from_x_slots() {
        let deck = ".subckt REQ in OUT\n\
                    R1 in OUT 1k\n\
                    E1 OUT 0 VALUE={V(in)}\n\
                    .ends REQ\n\
                    .subckt OUT a\n\
                    D1 a 0 DMISSING\n\
                    .ends OUT\n";
        let x = ok(&[("a.lib", deck)], "REQ", S);
        assert_eq!(names(&x), ["subckt:REQ"]);
        assert_self_contained(&x);
        assert_eq!(code(run(&[("a.lib", deck)], "OUT", S)), "unresolved");
        // The same name in the subcircuit slot is a reference.
        let deck = ".subckt OUT a\n.ends OUT\n.subckt T OUT\nX1 OUT OUT\n.ends T\n";
        assert_eq!(
            names(&ok(&[("a.lib", deck)], "T", S)),
            ["subckt:OUT", "subckt:T"]
        );
    }

    /// The stored-form rules hold on include lines too: a lone CR after an otherwise valid
    /// `.include` is refused before the line is exempted from the directive screen.
    #[test]
    fn lone_cr_on_an_include_line_is_refused() {
        for (root, want) in [
            (".include b.inc\r .control\n.model A D\n", "carriage_return"),
            (".include b.inc\r\r\n.model A D\n", "carriage_return"),
            (".lib b.inc TT\r x\n.model A D\n", "carriage_return"),
            (".include b.inc \0\n.model A D\n", "nul"),
            (".include b.inc \u{feff}\n.model A D\n", "byte_order_mark"),
        ] {
            let files = [("a.lib", root), ("b.inc", "* empty\n")];
            assert_eq!(code(run(&files, "A", M)), want, "{root:?}");
        }
        let files = [
            ("a.lib", ".include b.inc\r\n.model A D\r\n"),
            ("b.inc", "* empty\n"),
        ];
        assert_eq!(ok(&files, "A", M).text, ".model A D\n");
    }

    /// `.model` parameter lists ignore left-hand parameter names while keeping right-hand
    /// expressions, `AKO:` bases and same-name right-hand references.
    #[test]
    fn model_parameters_only_inspect_right_hand_sides() {
        // Parenthesized parameters do not pull unrelated `.param` names matching LHS keys.
        let deck = ".param IS=7\n\
                    .param N=2\n\
                    .model DIO D(IS=1e-12 N=1.5)\n";
        let x = ok(&[("a.lib", deck)], "DIO", M);
        assert_eq!(names(&x), ["model:DIO"]);
        assert_self_contained(&x);

        // Right-hand expressions ARE pulled in while LHS parameter names are still ignored.
        let deck = ".param IS=7\n\
                    .param IS_VAL=1e-12\n\
                    .model DIO D(IS={IS_VAL} N=1.5)\n";
        assert_eq!(
            names(&ok(&[("a.lib", deck)], "DIO", M)),
            ["param:IS_VAL", "model:DIO"]
        );

        // Same name on the right-hand side still resolves as a reference.
        let deck = ".param IS=1e-12\n\
                    .model DIO D(IS={IS})\n";
        assert_eq!(
            names(&ok(&[("a.lib", deck)], "DIO", M)),
            ["param:IS", "model:DIO"]
        );

        // Unparenthesized model parameter lists also ignore LHS keys.
        let deck = ".param IS=7\n\
                    .model DIO D IS=1e-12\n";
        assert_eq!(names(&ok(&[("a.lib", deck)], "DIO", M)), ["model:DIO"]);

        // `AKO:` base models still pull in the base while ignoring LHS parameter keys.
        let deck = ".param IS=7\n\
                    .model D1 D\n\
                    .model D2 AKO:D1 (IS=(2*A))\n\
                    .param A=1e-12\n";
        assert_eq!(
            names(&ok(&[("a.lib", deck)], "D2", M)),
            ["model:D1", "model:D2", "param:A"]
        );
    }
}
