//! Line-oriented SPICE `.model` / `.subckt` extractor prototype for spike PS1.
//!
//! What it does, in order:
//! 1. Decodes each file as UTF-8 (refusing anything else, `PD10`), drops a leading byte-order
//!    mark and turns CRLF into LF (`PD3`).
//! 2. Screens every physical line of every file it reads for the `PK10` directives.
//! 3. Flattens `.include` / `.inc` and two-argument `.lib <file> <section>` directives that
//!    resolve inside the archive (`PK10` path rules), in source order.
//! 4. Joins `+` continuation lines into logical lines. Blank and `*` comment lines may sit
//!    between a line and its continuations; inline `;`, ` $ ` and ` //` comments are cut for
//!    tokenising only.
//! 5. Builds a scoped definition table: `.subckt` (nested to any depth), `.model`, `.param`,
//!    `.func`, plus `PARAMS:` names on a `.subckt` header as locals of that subcircuit.
//! 6. Extracts one entry with its transitive top-level dependencies. Names resolve through the
//!    enclosing subcircuit scopes first, so a local `.model` shadows a top-level one of the same
//!    name. Every element whose model or subcircuit slot is known positionally (`D`, `J`, `Z`,
//!    `M`, `Q`, `X`) must resolve, or the extraction is refused.
//! 7. Emits the physical lines of every block in flattened source order, byte for byte.
//!
//! Prototype only: P1.2 writes the real `crates/omnia-parts/src/spice.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

/// Deepest `.include` / `.lib` nesting the flattener follows.
pub const MAX_INCLUDE_DEPTH: usize = 16;

/// `PK10` directives refused anywhere (compared without a leading dot, ASCII case-insensitive).
pub const FORBIDDEN: [&str; 8] = [
    "control",
    "endc",
    "csdf",
    "shell",
    "system",
    "osdi",
    "pre_osdi",
    "codemodel",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    /// Archive or library file: archive-relative includes are allowed and flattened.
    Archive,
    /// Embedded text (document or command): no `.include` / `.lib` at all.
    Embedded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DefKind {
    Model,
    Subckt,
    Param,
    Func,
}

impl DefKind {
    pub const ALL: [DefKind; 4] = [
        DefKind::Model,
        DefKind::Subckt,
        DefKind::Param,
        DefKind::Func,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DefKind::Model => "model",
            DefKind::Subckt => "subckt",
            DefKind::Param => "param",
            DefKind::Func => "func",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpiceError {
    NotUtf8 {
        file: String,
    },
    Forbidden {
        file: String,
        line: usize,
        directive: String,
    },
    IncludeInEmbedded {
        file: String,
        line: usize,
    },
    IncludeUnsafePath {
        file: String,
        line: usize,
        path: String,
    },
    IncludeMissing {
        file: String,
        line: usize,
        path: String,
    },
    IncludeCycle {
        file: String,
        line: usize,
        path: String,
    },
    IncludeTooDeep {
        file: String,
        line: usize,
    },
    LibSectionMissing {
        file: String,
        section: String,
    },
    SectionOutsideLibInclude {
        file: String,
        line: usize,
    },
    OrphanContinuation {
        file: String,
        line: usize,
    },
    Malformed {
        file: String,
        line: usize,
        what: &'static str,
    },
    UnterminatedSubckt {
        file: String,
        line: usize,
        name: String,
    },
    UnmatchedEnds {
        file: String,
        line: usize,
    },
    EndsNameMismatch {
        file: String,
        line: usize,
        open: String,
        close: String,
    },
    Duplicate {
        file: String,
        line: usize,
        kind: DefKind,
        name: String,
    },
    EntryNotFound {
        name: String,
        kind: DefKind,
    },
    Unresolved {
        file: String,
        line: usize,
        name: String,
        kind: DefKind,
    },
}

impl fmt::Display for SpiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for SpiceError {}

impl SpiceError {
    /// Short stable code used by the fixtures and the results file.
    pub fn code(&self) -> &'static str {
        match self {
            SpiceError::NotUtf8 { .. } => "not_utf8",
            SpiceError::Forbidden { .. } => "forbidden",
            SpiceError::IncludeInEmbedded { .. } => "include_in_embedded",
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
        }
    }
}

/// Archive-like file set keyed by archive-relative path with `/` separators.
#[derive(Clone, Debug, Default)]
pub struct Vfs {
    files: BTreeMap<String, Vec<u8>>,
}

impl Vfs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: &str, bytes: impl Into<Vec<u8>>) {
        self.files.insert(path.to_owned(), bytes.into());
    }

    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// Loads every regular file under `root`, keyed by its `/`-separated relative path.
    pub fn from_dir(root: &Path) -> std::io::Result<Self> {
        let mut vfs = Vfs::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let mut entries: Vec<_> = std::fs::read_dir(&dir)?.collect::<Result<_, _>>()?;
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let p = e.path();
                let ty = e.file_type()?;
                if ty.is_dir() {
                    stack.push(p);
                } else if ty.is_file() {
                    let rel = p.strip_prefix(root).expect("under root");
                    let key: Vec<String> = rel
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect();
                    vfs.insert(&key.join("/"), std::fs::read(&p)?);
                }
            }
        }
        Ok(vfs)
    }
}

/// One physical line after flattening, with where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhysLine {
    pub file: usize,
    pub line: usize,
    pub text: String,
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

/// Cuts an inline comment: `;` anywhere, `$` or `//` after whitespace. Quoted text is skipped.
pub fn code_part(s: &str) -> &str {
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

/// Whitespace tokens with `a = b`, `a= b` and `a =b` merged into `a=b`.
pub fn tokens(code: &str) -> Vec<String> {
    let raw: Vec<&str> = code.split_ascii_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let t = raw[i];
        if t == "=" && !out.is_empty() && i + 1 < raw.len() {
            let last = out.pop().expect("non-empty");
            out.push(format!("{last}={}", raw[i + 1]));
            i += 2;
        } else if t.ends_with('=') && t.len() > 1 && i + 1 < raw.len() {
            out.push(format!("{t}{}", raw[i + 1]));
            i += 2;
        } else if t.starts_with('=') && t.len() > 1 && !out.is_empty() {
            let last = out.pop().expect("non-empty");
            out.push(format!("{last}{t}"));
            i += 1;
        } else {
            out.push(t.to_owned());
            i += 1;
        }
    }
    out
}

fn lc(s: &str) -> String {
    s.to_ascii_lowercase()
}

fn first_token_lc(code: &str) -> Option<String> {
    code.split_ascii_whitespace().next().map(lc)
}

fn decode(file: &str, bytes: &[u8]) -> Result<Vec<String>, SpiceError> {
    let text = std::str::from_utf8(bytes).map_err(|_| SpiceError::NotUtf8 {
        file: file.to_owned(),
    })?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    Ok(body
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_owned())
        .collect())
}

fn screen_line(file: &str, line: usize, raw: &str, mode: Mode) -> Result<(), SpiceError> {
    if classify(raw) != LineClass::Normal {
        return Ok(());
    }
    let Some(first) = first_token_lc(code_part(raw)) else {
        return Ok(());
    };
    let bare = first.trim_start_matches('.');
    if FORBIDDEN.contains(&bare) {
        return Err(SpiceError::Forbidden {
            file: file.to_owned(),
            line,
            directive: first,
        });
    }
    if mode == Mode::Embedded && matches!(first.as_str(), ".include" | ".inc" | ".lib" | ".endl") {
        return Err(SpiceError::IncludeInEmbedded {
            file: file.to_owned(),
            line,
        });
    }
    Ok(())
}

/// Screens embedded text under the `PK10` embedded rules.
pub fn screen_embedded(text: &str) -> Result<(), SpiceError> {
    let lines = decode("<embedded>", text.as_bytes())?;
    for (i, l) in lines.iter().enumerate() {
        screen_line("<embedded>", i + 1, l, Mode::Embedded)?;
    }
    Ok(())
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner;
        }
    }
    s
}

/// Resolves an include path against the including file's directory under the `PK10` rules.
fn resolve_include(including: &str, raw: &str) -> Option<String> {
    let p = unquote(raw);
    if p.is_empty()
        || p.contains('\0')
        || p.starts_with('/')
        || p.starts_with('\\')
        || p.contains(':')
    {
        return None;
    }
    let mut parts: Vec<&str> = match including.rfind('/') {
        Some(i) => including[..i].split('/').collect(),
        None => Vec::new(),
    };
    for comp in p.split(['/', '\\']) {
        match comp {
            "" | "." => {}
            ".." => return None,
            c => parts.push(c),
        }
    }
    Some(parts.join("/"))
}

struct Flattener<'a> {
    vfs: &'a Vfs,
    mode: Mode,
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

    fn include(
        &mut self,
        stack: &mut Vec<String>,
        from: &str,
        line: usize,
        raw_path: &str,
        section: Option<&str>,
    ) -> Result<(), SpiceError> {
        if self.mode == Mode::Embedded {
            return Err(SpiceError::IncludeInEmbedded {
                file: from.to_owned(),
                line,
            });
        }
        let Some(target) = resolve_include(from, raw_path) else {
            return Err(SpiceError::IncludeUnsafePath {
                file: from.to_owned(),
                line,
                path: raw_path.to_owned(),
            });
        };
        if stack.contains(&target) {
            return Err(SpiceError::IncludeCycle {
                file: from.to_owned(),
                line,
                path: target,
            });
        }
        if stack.len() >= MAX_INCLUDE_DEPTH {
            return Err(SpiceError::IncludeTooDeep {
                file: from.to_owned(),
                line,
            });
        }
        if self.vfs.get(&target).is_none() {
            return Err(SpiceError::IncludeMissing {
                file: from.to_owned(),
                line,
                path: target,
            });
        }
        self.file(stack, &target, section)
    }

    fn file(
        &mut self,
        stack: &mut Vec<String>,
        path: &str,
        section: Option<&str>,
    ) -> Result<(), SpiceError> {
        let bytes = self
            .vfs
            .get(path)
            .ok_or_else(|| SpiceError::IncludeMissing {
                file: path.to_owned(),
                line: 0,
                path: path.to_owned(),
            })?;
        let lines = decode(path, bytes)?;
        // Screen the whole file, including sections that are not selected.
        for (i, l) in lines.iter().enumerate() {
            screen_line(path, i + 1, l, self.mode)?;
        }
        let fid = self.file_id(path);
        stack.push(path.to_owned());
        let want = section.map(lc);
        // Section state: None = top of file; Some(true) = inside the selected section;
        // Some(false) = inside another section (skipped).
        let mut in_section: Option<bool> = None;
        let mut found = want.is_none();
        let mut last_was_directive = false;
        for (i, raw) in lines.iter().enumerate() {
            let n = i + 1;
            let class = classify(raw);
            if class == LineClass::Continuation && last_was_directive {
                return Err(SpiceError::Malformed {
                    file: path.to_owned(),
                    line: n,
                    what: "continued include",
                });
            }
            if class != LineClass::Normal {
                if Self::emitting(&want, in_section) {
                    self.out.push(PhysLine {
                        file: fid,
                        line: n,
                        text: raw.clone(),
                    });
                }
                continue;
            }
            last_was_directive = false;
            let toks = tokens(code_part(raw));
            let first = lc(&toks[0]);
            match first.as_str() {
                ".lib" if toks.len() == 2 => {
                    // Section start marker.
                    if want.is_none() {
                        return Err(SpiceError::SectionOutsideLibInclude {
                            file: path.to_owned(),
                            line: n,
                        });
                    }
                    if in_section.is_some() {
                        return Err(SpiceError::Malformed {
                            file: path.to_owned(),
                            line: n,
                            what: "nested .lib section",
                        });
                    }
                    let hit = want.as_deref() == Some(lc(&toks[1]).as_str());
                    found |= hit;
                    in_section = Some(hit);
                    last_was_directive = true;
                }
                ".endl" => {
                    if in_section.is_none() {
                        return Err(SpiceError::SectionOutsideLibInclude {
                            file: path.to_owned(),
                            line: n,
                        });
                    }
                    in_section = None;
                    last_was_directive = true;
                }
                ".include" | ".inc" | ".lib" => {
                    last_was_directive = true;
                    if !Self::emitting(&want, in_section) {
                        continue;
                    }
                    let arg = toks.get(1).ok_or(SpiceError::Malformed {
                        file: path.to_owned(),
                        line: n,
                        what: "include without a path",
                    })?;
                    let sec = if first == ".lib" {
                        toks.get(2).map(String::as_str)
                    } else {
                        None
                    };
                    if first == ".lib" && sec.is_none() {
                        return Err(SpiceError::Malformed {
                            file: path.to_owned(),
                            line: n,
                            what: ".lib without a section",
                        });
                    }
                    self.include(stack, path, n, arg, sec)?;
                }
                _ => {
                    if Self::emitting(&want, in_section) {
                        self.out.push(PhysLine {
                            file: fid,
                            line: n,
                            text: raw.clone(),
                        });
                    }
                }
            }
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

    fn emitting(want: &Option<String>, in_section: Option<bool>) -> bool {
        match want {
            None => true,
            Some(_) => in_section == Some(true),
        }
    }
}

/// A joined logical line: one normal line plus its continuations.
#[derive(Clone, Debug)]
pub struct Logical {
    pub first: usize,
    pub last: usize,
    pub code: String,
    pub scope: Option<usize>,
}

/// One definition in the scoped table.
#[derive(Clone, Debug)]
pub struct Def {
    pub kind: DefKind,
    pub name: String,
    pub scope: Option<usize>,
    /// Physical line span (inclusive) in the flattened deck.
    pub first: usize,
    pub last: usize,
    /// Logical line span (inclusive).
    pub lfirst: usize,
    pub llast: usize,
    pub ports: Vec<String>,
    /// `PARAMS:` names on a subcircuit header: locals, never emitted on their own.
    pub header_local: bool,
}

/// A parsed, flattened deck.
#[derive(Clone, Debug)]
pub struct Deck {
    pub files: Vec<String>,
    pub lines: Vec<PhysLine>,
    pub logical: Vec<Logical>,
    pub defs: Vec<Def>,
    index: BTreeMap<(Option<usize>, DefKind, String), usize>,
    /// Dot directives seen that the extractor neither defines nor screens (for the report).
    pub other_directives: BTreeSet<String>,
}

/// One block of an extraction, for the report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: DefKind,
    pub name: String,
    pub file: String,
    pub line_first: usize,
    pub line_last: usize,
}

#[derive(Clone, Debug)]
pub struct Extraction {
    pub entry: String,
    pub kind: DefKind,
    pub ports: Option<Vec<String>>,
    pub blocks: Vec<Block>,
    pub text: String,
}

impl Deck {
    /// Reads `root` from `vfs`, screening and flattening it.
    pub fn load(vfs: &Vfs, root: &str, mode: Mode) -> Result<Deck, SpiceError> {
        let mut fl = Flattener {
            vfs,
            mode,
            files: Vec::new(),
            out: Vec::new(),
        };
        let mut stack = Vec::new();
        fl.file(&mut stack, root, None)?;
        let Flattener { files, out, .. } = fl;
        let mut deck = Deck {
            files,
            lines: out,
            logical: Vec::new(),
            defs: Vec::new(),
            index: BTreeMap::new(),
            other_directives: BTreeSet::new(),
        };
        deck.join()?;
        deck.parse()?;
        Ok(deck)
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
                    let body = l.text.trim_start().strip_prefix('+').expect("continuation");
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
                        code: code_part(&l.text).trim().to_owned(),
                        scope: None,
                    });
                }
            }
        }
        if let Some(c) = cur.take() {
            self.logical.push(c);
        }
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
            let (first, last, code) = {
                let l = &self.logical[li];
                (l.first, l.last, l.code.clone())
            };
            let toks = tokens(&code);
            let head = lc(&toks[0]);
            let scope = stack.last().copied();
            self.logical[li].scope = scope;
            let malformed = |what| {
                let l = &self.lines[first];
                SpiceError::Malformed {
                    file: self.files[l.file].clone(),
                    line: l.line,
                    what,
                }
            };
            match head.as_str() {
                ".subckt" => {
                    let name = toks
                        .get(1)
                        .ok_or_else(|| malformed(".subckt without a name"))?
                        .clone();
                    let (ports, params) = split_header(&toks[2..]);
                    let id = self.define(Def {
                        kind: DefKind::Subckt,
                        name,
                        scope,
                        first,
                        last,
                        lfirst: li,
                        llast: li,
                        ports,
                        header_local: false,
                    })?;
                    for p in params {
                        self.define(Def {
                            kind: DefKind::Param,
                            name: p,
                            scope: Some(id),
                            first,
                            last,
                            lfirst: li,
                            llast: li,
                            ports: Vec::new(),
                            header_local: true,
                        })?;
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
                ".model" => {
                    let name = toks
                        .get(1)
                        .ok_or_else(|| malformed(".model without a name"))?;
                    let name = name.split('(').next().unwrap_or_default().to_owned();
                    self.define(Def {
                        kind: DefKind::Model,
                        name,
                        scope,
                        first,
                        last,
                        lfirst: li,
                        llast: li,
                        ports: Vec::new(),
                        header_local: false,
                    })?;
                }
                ".param" | ".params" => {
                    for t in &toks[1..] {
                        if let Some((n, _)) = t.split_once('=') {
                            self.define(Def {
                                kind: DefKind::Param,
                                name: n.to_owned(),
                                scope,
                                first,
                                last,
                                lfirst: li,
                                llast: li,
                                ports: Vec::new(),
                                header_local: false,
                            })?;
                        }
                    }
                }
                ".func" => {
                    let raw = toks
                        .get(1)
                        .ok_or_else(|| malformed(".func without a name"))?;
                    let name = raw.split('(').next().unwrap_or_default().to_owned();
                    self.define(Def {
                        kind: DefKind::Func,
                        name,
                        scope,
                        first,
                        last,
                        lfirst: li,
                        llast: li,
                        ports: Vec::new(),
                        header_local: false,
                    })?;
                }
                h if h.starts_with('.') => {
                    self.other_directives.insert(h.to_owned());
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

    /// Resolves `name` of `kind` from `scope` outwards; the innermost definition wins.
    pub fn resolve(&self, name: &str, kind: DefKind, scope: Option<usize>) -> Option<usize> {
        let key = lc(name);
        let mut s = scope;
        loop {
            if let Some(&d) = self.index.get(&(s, kind, key.clone())) {
                return Some(d);
            }
            s = self.defs[s?].scope;
        }
    }

    /// Top-level definition names of `kind`, in source order.
    pub fn top_level(&self, kind: DefKind) -> Vec<&str> {
        self.defs
            .iter()
            .filter(|d| d.scope.is_none() && d.kind == kind && !d.header_local)
            .map(|d| d.name.as_str())
            .collect()
    }

    /// Port names of a top-level subcircuit (the `subckt_ports` count is their length).
    pub fn subckt_ports(&self, name: &str) -> Option<&[String]> {
        self.resolve(name, DefKind::Subckt, None)
            .map(|d| self.defs[d].ports.as_slice())
    }

    /// Extracts a top-level `.model` or `.subckt` with its transitive top-level dependencies.
    pub fn extract(&self, entry: &str, kind: DefKind) -> Result<Extraction, SpiceError> {
        let root = self
            .resolve(entry, kind, None)
            .filter(|&d| self.defs[d].scope.is_none())
            .ok_or_else(|| SpiceError::EntryNotFound {
                name: entry.to_owned(),
                kind,
            })?;
        let mut included: BTreeSet<usize> = BTreeSet::from([root]);
        let mut queue = vec![root];
        while let Some(d) = queue.pop() {
            let (lf, ll) = (self.defs[d].lfirst, self.defs[d].llast);
            for li in lf..=ll {
                let l = &self.logical[li];
                self.check_slots(l)?;
                for t in reference_tokens(&l.code) {
                    for k in DefKind::ALL {
                        if let Some(x) = self.resolve(&t, k, l.scope)
                            && self.defs[x].scope.is_none()
                            && included.insert(x)
                        {
                            queue.push(x);
                        }
                    }
                }
            }
        }
        // One block per physical span, in flattened source order.
        let mut spans: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
        for &d in &included {
            spans
                .entry((self.defs[d].first, self.defs[d].last))
                .or_default()
                .push(d);
        }
        let mut text = String::new();
        let mut blocks = Vec::new();
        for ((first, last), ds) in &spans {
            for l in &self.lines[*first..=*last] {
                text.push_str(&l.text);
                text.push('\n');
            }
            for &d in ds {
                let (file, line_first) = self.at(*first);
                blocks.push(Block {
                    kind: self.defs[d].kind,
                    name: self.defs[d].name.clone(),
                    file,
                    line_first,
                    line_last: self.lines[*last].line,
                });
            }
        }
        let ports = (kind == DefKind::Subckt).then(|| self.defs[root].ports.clone());
        Ok(Extraction {
            entry: self.defs[root].name.clone(),
            kind,
            ports,
            blocks,
            text,
        })
    }

    /// Positional model / subcircuit slots that must resolve.
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
                    .position(|t| lc(t).starts_with("params:") || t.contains('='))
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

/// Splits `.subckt` header tokens after the name into ports and `PARAMS:` names.
fn split_header(rest: &[String]) -> (Vec<String>, Vec<String>) {
    let mut ports = Vec::new();
    let mut params = Vec::new();
    let mut in_params = false;
    for t in rest {
        let l = lc(t);
        if let Some(after) = l.strip_prefix("params:") {
            in_params = true;
            if !after.is_empty() {
                let orig = &t[t.len() - after.len()..];
                if let Some((n, _)) = orig.split_once('=') {
                    params.push(n.to_owned());
                }
            }
            continue;
        }
        if in_params || t.contains('=') {
            in_params = true;
            if let Some((n, _)) = t.split_once('=') {
                params.push(n.to_owned());
            }
            continue;
        }
        ports.push(t.clone());
    }
    (ports, params)
}

/// Candidate reference names on a logical line: coarse tokens (split on whitespace,
/// brackets, commas, quotes and `=`) plus a finer split on expression operators, minus the
/// directive keyword, the element name and any name the line itself defines. Over-inclusion is
/// harmless (an extra block); under-inclusion is not.
fn reference_tokens(code: &str) -> BTreeSet<String> {
    let toks = tokens(code);
    let head = lc(&toks[0]);
    let mut skip: BTreeSet<String> = BTreeSet::new();
    skip.insert(head.clone());
    match head.as_str() {
        ".model" | ".subckt" | ".func" => {
            if let Some(n) = toks.get(1) {
                skip.insert(lc(n.split('(').next().unwrap_or_default()));
            }
        }
        ".param" | ".params" => {
            for t in &toks[1..] {
                if let Some((n, _)) = t.split_once('=') {
                    skip.insert(lc(n));
                }
            }
        }
        ".ends" => return BTreeSet::new(),
        _ => {}
    }
    let mut out = BTreeSet::new();
    let coarse = |c: char| c.is_ascii_whitespace() || "(){}[],=\"'".contains(c);
    let fine = |c: char| "*/+-^<>!?:&|%".contains(c);
    for t in code.split(coarse).filter(|t| !t.is_empty()) {
        out.insert(lc(t));
        for f in t.split(fine).filter(|f| !f.is_empty()) {
            out.insert(lc(f));
        }
    }
    out.retain(|t| !skip.contains(t));
    out
}
