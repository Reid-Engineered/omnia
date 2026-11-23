//! Root-jailed file access and staged writes (mcp-plan.md section 12.1, AK2, AINV12, ATC-path-01).
//!
//! Every file path the agent surface reads or writes passes through [`Roots`]. Configured root
//! directories are canonicalised up front. Any path that is empty, contains a NUL byte, has a
//! `..` traversal component (in either POSIX `/` or Windows `\` syntax), names a foreign
//! platform prefix (such as a Windows drive or UNC path on POSIX, or a rooted POSIX path
//! outside roots on Windows), or resolves through a symlink to anything outside the configured
//! roots is refused with `path_outside_root` before any file content is read or written.
//!
//! Opened files are bounded by [`MAX_FILE_BYTES`] (AK2) before and during reading and parsed
//! solely through `omnia_doc::Doc::load` (RON v2 test sheets, AQ12). Writes are staged through a
//! sibling temporary file, synced to disk, and renamed into place so a failure mid-write leaves
//! any existing file untouched.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use omnia_doc::Doc;

use crate::{AgentError, ErrorKind, MAX_FILE_BYTES, error::Limit};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Canonicalised root directories that bound every file read and write (`--root DIR`, AINV12).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roots {
    dirs: Vec<PathBuf>,
}

/// A validated, canonicalised write target inside [`Roots`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteTarget {
    /// Canonical path to write to (inside a configured root).
    pub path: PathBuf,
    /// Whether a file already exists at `path` (decides tier 2 vs tier 3, AD7).
    pub exists: bool,
}

impl Roots {
    /// No roots configured: every file read or write is refused with `path_outside_root`.
    pub fn empty() -> Self {
        Self { dirs: Vec::new() }
    }

    /// Canonicalises each configured root directory. Every root must exist and be a directory.
    pub fn new<I, P>(dirs: I) -> Result<Self, AgentError>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut out = Vec::new();
        for dir in dirs {
            let canon = fs::canonicalize(dir.as_ref()).map_err(|_| {
                AgentError::with_message(ErrorKind::IoError, "configured root directory not found")
            })?;
            let meta = fs::metadata(&canon).map_err(|_| {
                AgentError::with_message(ErrorKind::IoError, "cannot inspect root directory")
            })?;
            if !meta.is_dir() {
                return Err(AgentError::with_message(
                    ErrorKind::IoError,
                    "configured root path is not a directory",
                ));
            }
            out.push(canon);
        }
        out.sort();
        out.dedup();
        Ok(Self { dirs: out })
    }

    /// The canonicalised root directories, sorted and deduplicated.
    pub fn as_slice(&self) -> &[PathBuf] {
        &self.dirs
    }

    /// Whether no roots are configured.
    pub fn is_empty(&self) -> bool {
        self.dirs.is_empty()
    }

    /// Whether `canon` lies inside (or equals) one of the configured roots.
    pub fn contains(&self, canon: &Path) -> bool {
        self.dirs.iter().any(|root| canon.starts_with(root))
    }

    /// Resolves `raw` for reading, refusing anything outside the configured roots (`AINV12`).
    ///
    /// Relative paths are checked against each root in order; an escaping symlink under any
    /// root is refused immediately with `path_outside_root`. Missing files that stay inside a
    /// root fail with `io_error` rather than `path_outside_root`.
    pub fn resolve_read(&self, raw: &str) -> Result<PathBuf, AgentError> {
        let parsed = parse_user_path(raw)?;
        if self.dirs.is_empty() {
            return Err(outside());
        }
        match parsed {
            ParsedPath::Absolute(abs) => self.resolve_existing_inside(&abs),
            ParsedPath::Relative(rel) => {
                let mut saw_missing = false;
                for root in &self.dirs {
                    let candidate = root.join(&rel);
                    match self.resolve_existing_inside(&candidate) {
                        Ok(canon) => return Ok(canon),
                        Err(e) if e.code() == crate::ErrorCode::PathOutsideRoot => return Err(e),
                        Err(_) => saw_missing = true,
                    }
                }
                if saw_missing {
                    Err(AgentError::with_message(
                        ErrorKind::IoError,
                        "the file does not exist",
                    ))
                } else {
                    Err(outside())
                }
            }
        }
    }

    /// Resolves `raw` for writing, refusing anything outside the configured roots (`AINV12`).
    ///
    /// Relative paths resolve under the first configured root. Both existing and new target
    /// filenames are supported; the parent directory must exist inside a configured root, and
    /// any symlink along the path or at the target that escapes the roots (or is dangling) is
    /// refused with `path_outside_root`.
    pub fn resolve_write(&self, raw: &str) -> Result<WriteTarget, AgentError> {
        let parsed = parse_user_path(raw)?;
        if self.dirs.is_empty() {
            return Err(outside());
        }
        let candidate = match parsed {
            ParsedPath::Absolute(abs) => abs,
            ParsedPath::Relative(rel) => self.dirs[0].join(rel),
        };
        self.resolve_write_candidate(&candidate)
    }

    /// Re-validates an already-stored canonical path before writing (defense against symlink
    /// swaps between `open` and `save`).
    pub fn resolve_write_stored(&self, stored: &Path) -> Result<WriteTarget, AgentError> {
        if self.dirs.is_empty() || !stored.is_absolute() {
            return Err(outside());
        }
        self.resolve_write_candidate(stored)
    }

    /// Validates an existing path for reading: verifies no ancestor or target link escapes
    /// the roots, returning `path_outside_root` even when the target does not exist outside roots.
    fn resolve_existing_inside(&self, candidate: &Path) -> Result<PathBuf, AgentError> {
        self.verify_ancestors_inside(candidate)?;
        match fs::symlink_metadata(candidate) {
            Ok(meta) => {
                let canon = fs::canonicalize(candidate).map_err(|_| {
                    // A dangling symlink or broken reparse point cannot be proved inside roots.
                    if is_link_metadata(&meta) {
                        outside()
                    } else {
                        AgentError::with_message(
                            ErrorKind::IoError,
                            "the file could not be inspected",
                        )
                    }
                })?;
                if !self.contains(&canon) {
                    return Err(outside());
                }
                Ok(canon)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(AgentError::with_message(
                ErrorKind::IoError,
                "the file does not exist",
            )),
            Err(_) => Err(AgentError::with_message(
                ErrorKind::IoError,
                "the file could not be inspected",
            )),
        }
    }

    /// Validates a write candidate whose final component may not exist yet.
    fn resolve_write_candidate(&self, candidate: &Path) -> Result<WriteTarget, AgentError> {
        self.verify_ancestors_inside(candidate)?;
        let Some(name) = candidate.file_name() else {
            return Err(outside());
        };
        let Some(parent) = candidate.parent() else {
            return Err(outside());
        };
        let canon_parent = fs::canonicalize(parent).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                AgentError::with_message(ErrorKind::IoError, "the parent directory does not exist")
            } else {
                AgentError::with_message(
                    ErrorKind::IoError,
                    "the parent directory could not be inspected",
                )
            }
        })?;
        if !self.contains(&canon_parent) {
            return Err(outside());
        }
        if !fs::metadata(&canon_parent).is_ok_and(|m| m.is_dir()) {
            return Err(AgentError::with_message(
                ErrorKind::IoError,
                "the parent path is not a directory",
            ));
        }
        let target = canon_parent.join(name);
        match fs::symlink_metadata(&target) {
            Ok(meta) => {
                // Always canonicalise an existing entry so symlinks and Windows reparse points
                // (junctions/mount points) can never escape the configured roots.
                let canon = fs::canonicalize(&target).map_err(|_| {
                    if is_link_metadata(&meta) {
                        outside()
                    } else {
                        AgentError::with_message(
                            ErrorKind::IoError,
                            "the target file could not be inspected",
                        )
                    }
                })?;
                if !self.contains(&canon) {
                    return Err(outside());
                }
                let target_meta = fs::metadata(&canon).map_err(|_| {
                    AgentError::with_message(
                        ErrorKind::IoError,
                        "the target file could not be inspected",
                    )
                })?;
                if !target_meta.is_file() {
                    return Err(AgentError::with_message(
                        ErrorKind::IoError,
                        "the target path is not a regular file",
                    ));
                }
                Ok(WriteTarget {
                    path: canon,
                    exists: true,
                })
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(WriteTarget {
                path: target,
                exists: false,
            }),
            Err(_) => Err(AgentError::with_message(
                ErrorKind::IoError,
                "the target path could not be inspected",
            )),
        }
    }

    /// Walks every ancestor component of `candidate` top-down (using `symlink_metadata` and
    /// `canonicalize`, so symlinks, Windows NTFS junctions, and dangling links are all caught)
    /// and requires that `candidate` enters a configured root and never leaves it.
    fn verify_ancestors_inside(&self, candidate: &Path) -> Result<(), AgentError> {
        let mut prefix = PathBuf::new();
        let mut saw_existing_inside = false;
        let components: Vec<Component<'_>> = candidate.components().collect();
        let parent_len = components.len().saturating_sub(1);
        for (idx, comp) in components.iter().enumerate() {
            match comp {
                Component::ParentDir => return Err(outside()),
                Component::CurDir => continue,
                Component::Prefix(_) | Component::RootDir => {
                    prefix.push(comp.as_os_str());
                    continue;
                }
                Component::Normal(_) => prefix.push(comp.as_os_str()),
            }
            if idx >= parent_len {
                break;
            }
            match fs::symlink_metadata(&prefix) {
                Ok(meta) => {
                    if saw_existing_inside {
                        // Canonicalise every existing ancestor inside a root so both POSIX/Windows
                        // symlinks and Windows NTFS junctions are checked for escape or dangling.
                        let canon = fs::canonicalize(&prefix).map_err(|_| {
                            if is_link_metadata(&meta) {
                                outside()
                            } else {
                                AgentError::with_message(
                                    ErrorKind::IoError,
                                    "a parent directory could not be inspected",
                                )
                            }
                        })?;
                        if !self.contains(&canon) {
                            return Err(outside());
                        }
                    } else if let Ok(canon) = fs::canonicalize(&prefix)
                        && self.contains(&canon)
                    {
                        saw_existing_inside = true;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    // Once we hit a missing directory, we must already be inside a root;
                    // otherwise the path is outside all roots even though it doesn't exist.
                    return if saw_existing_inside {
                        Ok(())
                    } else {
                        Err(outside())
                    };
                }
                Err(_) => {
                    if saw_existing_inside {
                        return Err(AgentError::with_message(
                            ErrorKind::IoError,
                            "a parent directory could not be inspected",
                        ));
                    }
                }
            }
        }
        if saw_existing_inside {
            Ok(())
        } else {
            Err(outside())
        }
    }
}

/// Whether `meta` represents a symlink or (on Windows) any reparse point such as an NTFS junction.
fn is_link_metadata(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// A lexically validated user path (free of `..`, NULs, and cross-platform prefix escapes).
enum ParsedPath {
    Absolute(PathBuf),
    Relative(PathBuf),
}

fn outside() -> AgentError {
    AgentError::new(ErrorKind::PathOutsideRoot)
}

/// Parses and lexically screens `raw` in both Windows and POSIX path forms (`AINV12`).
///
/// Rejects:
/// - empty strings and NUL bytes;
/// - any `..` segment across either `/` or `\` separators;
/// - cross-platform absolute / prefixed forms that cannot reside inside a root on this host
///   (e.g. `C:\...`, `C:rel`, `\\server\share` on POSIX; rooted `/foo` or `\\?\...` outside roots
///   on Windows);
/// - colons inside relative path segments (blocks Windows alternate data streams and
///   drive-relative paths like `C:foo.ron` on both OSes).
fn parse_user_path(raw: &str) -> Result<ParsedPath, AgentError> {
    if raw.is_empty() || raw.contains('\0') {
        return Err(outside());
    }

    // Detect UNC / extended-length device prefixes (`\\...` or `//...`) in both OS forms.
    let starts_double_slash = raw.starts_with("\\\\") || raw.starts_with("//");
    // Detect Windows drive prefix (`X:` where X is an ASCII letter).
    let bytes = raw.as_bytes();
    let has_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';

    // Check every lexical segment split on BOTH `/` and `\` for `..`.
    let tail_after_prefix = if has_drive {
        &raw[2..]
    } else if starts_double_slash {
        raw.trim_start_matches(['\\', '/'])
    } else {
        raw
    };
    for seg in tail_after_prefix.split(['/', '\\']) {
        if seg == ".." {
            return Err(outside());
        }
        // Reject colons inside segments (Windows alternate data streams / drive-relative tricks).
        if seg.contains(':') {
            return Err(outside());
        }
    }

    if cfg!(windows) {
        if starts_double_slash {
            // Allow `\\?\...` or UNC only if Windows considers it absolute and it later resolves
            // inside a configured root; reject bare `\\` or `//`.
            if tail_after_prefix.is_empty() {
                return Err(outside());
            }
            return Ok(ParsedPath::Absolute(PathBuf::from(raw)));
        }
        if has_drive {
            // `C:foo` (drive-relative, no separator after `:`) is ambiguous and refused.
            let rest = &raw[2..];
            if !rest.starts_with('\\') && !rest.starts_with('/') {
                return Err(outside());
            }
            return Ok(ParsedPath::Absolute(PathBuf::from(raw)));
        }
        // A leading `/` or `\` on Windows is rooted on the current drive (not relative to a root).
        if raw.starts_with('/') || raw.starts_with('\\') {
            return Err(outside());
        }
    } else {
        // On POSIX, Windows drive letters and UNC prefixes are never valid inside a POSIX root.
        if has_drive || starts_double_slash {
            return Err(outside());
        }
        if raw.starts_with('/') {
            return Ok(ParsedPath::Absolute(PathBuf::from(raw)));
        }
        // A leading `\` on POSIX looks like a Windows rooted path; refuse it.
        if raw.starts_with('\\') {
            return Err(outside());
        }
    }

    // Relative path: build from clean segments (splitting on both `/` and `\`, skipping `.` and
    // repeated separators).
    let mut rel = PathBuf::new();
    for seg in raw.split(['/', '\\']) {
        if seg.is_empty() || seg == "." {
            continue;
        }
        debug_assert_ne!(seg, "..");
        rel.push(seg);
    }
    if rel.as_os_str().is_empty() {
        return Err(outside());
    }
    Ok(ParsedPath::Relative(rel))
}

/// Formats a canonical path for wire responses, stripping Windows' `\\?\` verbatim prefix when
/// possible so returned paths stay clean and round-trip through [`Roots`].
pub fn display_path(canon: &Path) -> String {
    let text = canon.to_string_lossy();
    if cfg!(windows) {
        if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{unc}");
        }
        if let Some(stripped) = text.strip_prefix(r"\\?\") {
            return stripped.to_owned();
        }
    }
    text.into_owned()
}

/// Opens and parses a RON v2 test sheet at canonical `path`, enforcing [`MAX_FILE_BYTES`] (AK2).
pub fn open_doc(path: &Path) -> Result<Doc, AgentError> {
    let file = File::open(path)
        .map_err(|_| AgentError::with_message(ErrorKind::IoError, "the file could not be read"))?;
    let meta = file.metadata().map_err(|_| {
        AgentError::with_message(ErrorKind::IoError, "the file could not be inspected")
    })?;
    if !meta.is_file() {
        return Err(AgentError::with_message(
            ErrorKind::IoError,
            "the path is not a regular file",
        ));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(AgentError::new(ErrorKind::LimitExceeded {
            limit: Limit::FileBytes,
            value: meta.len(),
        }));
    }
    let mut buf = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    let mut limited = file.take(MAX_FILE_BYTES + 1);
    limited
        .read_to_end(&mut buf)
        .map_err(|_| AgentError::with_message(ErrorKind::IoError, "the file could not be read"))?;
    let len = u64::try_from(buf.len()).unwrap_or(u64::MAX);
    if len > MAX_FILE_BYTES {
        return Err(AgentError::new(ErrorKind::LimitExceeded {
            limit: Limit::FileBytes,
            value: len,
        }));
    }
    Doc::load(buf.as_slice()).map_err(map_load_error)
}

fn map_load_error(err: io::Error) -> AgentError {
    if err.kind() == io::ErrorKind::InvalidData
        && err.to_string().contains("unsupported test-sheet version")
    {
        return AgentError::new(ErrorKind::UnsupportedFileVersion);
    }
    // AD14: do not echo arbitrary file text into error messages.
    AgentError::with_message(
        ErrorKind::IoError,
        "the file is not a valid version 2 test sheet",
    )
}

/// Serialises `doc` as RON v2 and writes it atomically to `target` via a sibling temporary file.
pub fn save_doc(doc: &Doc, target: &Path) -> Result<(), AgentError> {
    let mut bytes = Vec::new();
    doc.save(&mut bytes).map_err(|_| {
        AgentError::with_message(ErrorKind::IoError, "the document could not be serialised")
    })?;
    write_staged_with(target, |file| file.write_all(&bytes))
}

/// Stages a write into a unique sibling temporary file, syncs it, and renames it over `target`.
///
/// If `write_fn`, `sync_all`, or `rename` fails, the temporary file is removed and any existing
/// file at `target` is left untouched (mcp-plan.md section 12.1).
pub(crate) fn write_staged_with(
    target: &Path,
    write_fn: impl FnOnce(&mut File) -> io::Result<()>,
) -> Result<(), AgentError> {
    let parent = target.parent().ok_or_else(|| {
        AgentError::with_message(ErrorKind::IoError, "target path has no parent directory")
    })?;
    let (mut tmp_file, tmp_path) = create_temp_file(parent)?;
    let write_res = write_fn(&mut tmp_file).and_then(|()| tmp_file.sync_all());
    drop(tmp_file);
    if write_res.is_err() {
        let _ = fs::remove_file(&tmp_path);
        return Err(AgentError::with_message(
            ErrorKind::IoError,
            "the file could not be written",
        ));
    }
    if fs::rename(&tmp_path, target).is_err() {
        let _ = fs::remove_file(&tmp_path);
        return Err(AgentError::with_message(
            ErrorKind::IoError,
            "the file could not be written",
        ));
    }
    Ok(())
}

fn create_temp_file(parent: &Path) -> Result<(File, PathBuf), AgentError> {
    let pid = std::process::id();
    for _ in 0..128 {
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".omnia-save-{pid}-{seq}.tmp"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((file, candidate)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => {
                return Err(AgentError::with_message(
                    ErrorKind::IoError,
                    "the temporary file could not be created",
                ));
            }
        }
    }
    Err(AgentError::with_message(
        ErrorKind::IoError,
        "could not allocate a unique temporary file",
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use omnia_doc::{Command, Label, Pt};
    use proptest::prelude::*;

    use super::*;
    use crate::ErrorCode;

    static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let n = DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "omnia-agent-files-{tag}-{}-{n}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn make_symlink_file(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_file(target, link).is_ok()
        }
    }

    fn make_symlink_dir(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_dir(target, link).is_ok() {
                return true;
            }
            // Unprivileged Windows fallback: NTFS directory junction (`mklink /J`).
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .is_ok_and(|out| out.status.success())
        }
    }

    fn write_sample_sheet(path: &Path, label: &str) -> Doc {
        let mut doc = Doc::new();
        doc.apply(Command::AddLabel(Label {
            at: Pt::ZERO,
            text: label.into(),
        }))
        .unwrap();
        save_doc(&doc, path).unwrap();
        doc
    }

    #[test]
    fn atc_path_01_refuses_traversal_outside_absolute_and_symlink_escapes() {
        let base = TempDir::new("atc-path-01");
        let root = base.path().join("root");
        let outside_dir = base.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside_dir).unwrap();
        let outside_file = outside_dir.join("x.ron");
        write_sample_sheet(&outside_file, "SECRET");

        let roots = Roots::new([&root]).unwrap();

        // 1. Relative traversal in both POSIX and Windows forms.
        for bad in [
            "../x.ron",
            "..\\x.ron",
            "../outside/x.ron",
            "..\\outside\\x.ron",
            "sub/../../outside/x.ron",
            "sub\\..\\..\\outside\\x.ron",
            "sub/../inside.ron",
            "sub\\..\\inside.ron",
        ] {
            assert_eq!(
                roots.resolve_read(bad).unwrap_err().code(),
                ErrorCode::PathOutsideRoot,
                "read {bad}"
            );
            assert_eq!(
                roots.resolve_write(bad).unwrap_err().code(),
                ErrorCode::PathOutsideRoot,
                "write {bad}"
            );
        }

        // 2. Absolute paths outside roots in both host and foreign forms.
        let abs_outside = outside_file.to_string_lossy().into_owned();
        for bad in [
            abs_outside.as_str(),
            "/etc/passwd",
            "/nonexistent/outside.ron",
            r"C:\Windows\System32\drivers\etc\hosts",
            r"C:/nonexistent/outside.ron",
            r"C:relative.ron",
            r"\\server\share\x.ron",
            "//server/share/x.ron",
            "file.ron:ads",
            "",
            "bad\0name.ron",
        ] {
            assert_eq!(
                roots.resolve_read(bad).unwrap_err().code(),
                ErrorCode::PathOutsideRoot,
                "read {bad:?}"
            );
            assert_eq!(
                roots.resolve_write(bad).unwrap_err().code(),
                ErrorCode::PathOutsideRoot,
                "write {bad:?}"
            );
        }

        // 3. Symlink/junction escapes out of the root (dir link always succeeds on Unix & Windows).
        let link_file = root.join("escape.ron");
        if make_symlink_file(&outside_file, &link_file) {
            assert_eq!(
                roots.resolve_read("escape.ron").unwrap_err().code(),
                ErrorCode::PathOutsideRoot
            );
            assert_eq!(
                roots.resolve_write("escape.ron").unwrap_err().code(),
                ErrorCode::PathOutsideRoot
            );
        }
        let link_dir = root.join("escape_dir");
        assert!(
            make_symlink_dir(&outside_dir, &link_dir),
            "directory symlink or NTFS junction must be creatable for ATC-path-01"
        );
        for bad in [
            "escape_dir/x.ron",
            "escape_dir\\x.ron",
            "escape_dir/new.ron",
            "escape_dir\\new.ron",
            "escape_dir/missing/new.ron",
        ] {
            assert_eq!(
                roots.resolve_read(bad).unwrap_err().code(),
                ErrorCode::PathOutsideRoot,
                "read {bad}"
            );
            assert_eq!(
                roots.resolve_write(bad).unwrap_err().code(),
                ErrorCode::PathOutsideRoot,
                "write {bad}"
            );
        }
        // Dangling directory link pointing outside root.
        let missing_outside = base.path().join("does_not_exist_outside");
        let dangling_dir = root.join("dangling_dir");
        if make_symlink_dir(&missing_outside, &dangling_dir) {
            assert_eq!(
                roots.resolve_read("dangling_dir/x.ron").unwrap_err().code(),
                ErrorCode::PathOutsideRoot
            );
            assert_eq!(
                roots
                    .resolve_write("dangling_dir/x.ron")
                    .unwrap_err()
                    .code(),
                ErrorCode::PathOutsideRoot
            );
        }
        // Dangling file symlink pointing outside root (Unix / Windows Developer Mode).
        let dangling = root.join("dangling.ron");
        if make_symlink_file(&outside_dir.join("missing.ron"), &dangling) {
            assert_eq!(
                roots.resolve_read("dangling.ron").unwrap_err().code(),
                ErrorCode::PathOutsideRoot
            );
            assert_eq!(
                roots.resolve_write("dangling.ron").unwrap_err().code(),
                ErrorCode::PathOutsideRoot
            );
        }
    }

    #[test]
    fn open_enforces_ak2_file_size_and_version_check() {
        let base = TempDir::new("ak2");
        let roots = Roots::new([base.path()]).unwrap();

        // Unsupported version (v3).
        let v3 = base.path().join("v3.ron");
        fs::write(&v3, "(version:3,revision:0,parts:[],wires:[],labels:[])").unwrap();
        let canon = roots.resolve_read("v3.ron").unwrap();
        assert_eq!(
            open_doc(&canon).unwrap_err().code(),
            ErrorCode::UnsupportedFileVersion
        );

        // Corrupt RON does not leak file content (AD14).
        let bad = base.path().join("corrupt.ron");
        fs::write(&bad, "SECRET_MARKER_TEXT_12345").unwrap();
        let canon = roots.resolve_read("corrupt.ron").unwrap();
        let err = open_doc(&canon).unwrap_err();
        assert_eq!(err.code(), ErrorCode::IoError);
        assert!(!err.message.contains("SECRET_MARKER_TEXT_12345"));

        // Over AK2 (8 MiB + 1 byte).
        let big = base.path().join("big.ron");
        let file = File::create(&big).unwrap();
        file.set_len(MAX_FILE_BYTES + 1).unwrap();
        let canon = roots.resolve_read("big.ron").unwrap();
        assert_eq!(
            open_doc(&canon).unwrap_err().kind,
            ErrorKind::LimitExceeded {
                limit: Limit::FileBytes,
                value: MAX_FILE_BYTES + 1
            }
        );
    }

    #[test]
    fn staged_write_failure_halfway_keeps_old_file_and_cleans_temp() {
        let base = TempDir::new("staged-fail");
        let target = base.path().join("sheet.ron");
        let original = write_sample_sheet(&target, "ORIGINAL");
        let original_bytes = fs::read(&target).unwrap();

        let err = write_staged_with(&target, |file| {
            file.write_all(b"(version:2,revision:999,")?;
            file.flush()?;
            Err(io::Error::other("simulated disk full"))
        })
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::IoError);

        // Old file is untouched byte for byte.
        assert_eq!(fs::read(&target).unwrap(), original_bytes);
        assert_eq!(open_doc(&target).unwrap(), original);

        // No temporary file left behind.
        let entries: Vec<_> = fs::read_dir(base.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec!["sheet.ron"]);
    }

    #[test]
    fn prefix_boundary_sibling_directory_is_outside_root() {
        let base = TempDir::new("prefix");
        let root = base.path().join("work");
        let sibling = base.path().join("work-evil");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        let secret = sibling.join("x.ron");
        write_sample_sheet(&secret, "NO");

        let roots = Roots::new([&root]).unwrap();
        let secret_str = secret.to_string_lossy().into_owned();
        assert_eq!(
            roots.resolve_read(&secret_str).unwrap_err().code(),
            ErrorCode::PathOutsideRoot
        );
        assert_eq!(
            roots.resolve_write(&secret_str).unwrap_err().code(),
            ErrorCode::PathOutsideRoot
        );
    }

    fn escaping_path_strategy() -> impl Strategy<Value = String> {
        let seg = "[a-zA-Z0-9_-]{1,8}";
        prop_oneof![
            // Traversal in POSIX and Windows forms.
            (seg, seg).prop_map(|(a, b)| format!("../{a}/{b}.ron")),
            (seg, seg).prop_map(|(a, b)| format!("..\\{a}\\{b}.ron")),
            (seg, seg).prop_map(|(a, b)| format!("{a}/../../{b}.ron")),
            (seg, seg).prop_map(|(a, b)| format!("{a}\\..\\..\\{b}.ron")),
            (seg, seg).prop_map(|(a, b)| format!("{a}/../{b}.ron")),
            (seg, seg).prop_map(|(a, b)| format!("{a}\\..\\{b}.ron")),
            // POSIX absolute paths outside root.
            (seg, seg).prop_map(|(a, b)| format!("/{a}/{b}.ron")),
            // Windows drive absolute & drive-relative & UNC paths.
            (b'A'..=b'Z', seg, seg).prop_map(|(d, a, b)| format!("{}:\\{a}\\{b}.ron", d as char)),
            (b'a'..=b'z', seg, seg).prop_map(|(d, a, b)| format!("{}:/{a}/{b}.ron", d as char)),
            (b'C'..=b'F', seg).prop_map(|(d, a)| format!("{}:{a}.ron", d as char)),
            (seg, seg, seg).prop_map(|(h, s, f)| format!("\\\\{h}\\{s}\\{f}.ron")),
            (seg, seg, seg).prop_map(|(h, s, f)| format!("//{h}/{s}/{f}.ron")),
            // Alternate data streams / NUL bytes.
            (seg, seg).prop_map(|(a, b)| format!("{a}.ron:{b}")),
            seg.prop_map(|a| format!("{a}\0.ron")),
            // Symlink/junction escapes in both POSIX and Windows forms.
            seg.prop_map(|a| format!("escape_dir/{a}.ron")),
            seg.prop_map(|a| format!("escape_dir\\{a}.ron")),
            (seg, seg).prop_map(|(a, b)| format!("escape_dir/{a}/{b}.ron")),
            (seg, seg).prop_map(|(a, b)| format!("escape_dir\\{a}\\{b}.ron")),
            seg.prop_map(|a| format!("dangling_dir/{a}.ron")),
            seg.prop_map(|a| format!("dangling_dir\\{a}.ron")),
        ]
    }

    #[test]
    fn valid_paths_inside_roots_and_multi_root_resolution_work() {
        let base = TempDir::new("valid-roots");
        let root_a = base.path().join("root_a");
        let root_b = base.path().join("root_b");
        let sub = root_a.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::create_dir_all(&root_b).unwrap();

        let in_a = sub.join("a.ron");
        let in_b = root_b.join("b.ron");
        let doc_a = write_sample_sheet(&in_a, "A");
        let doc_b = write_sample_sheet(&in_b, "B");

        let roots = Roots::new([&root_a, &root_b]).unwrap();
        assert_eq!(roots.as_slice().len(), 2);

        // Relative POSIX and Windows separators, plus `./` and absolute path inside root.
        let ca_posix = roots.resolve_read("sub/a.ron").unwrap();
        let ca_win = roots.resolve_read(r"sub\a.ron").unwrap();
        let ca_dot = roots.resolve_read("./sub/a.ron").unwrap();
        let ca_abs = roots.resolve_read(&display_path(&ca_posix)).unwrap();
        assert_eq!(ca_posix, ca_win);
        assert_eq!(ca_posix, ca_dot);
        assert_eq!(ca_posix, ca_abs);
        assert_eq!(open_doc(&ca_posix).unwrap(), doc_a);

        // Second root is searched on relative `open`.
        let cb = roots.resolve_read("b.ron").unwrap();
        assert_eq!(open_doc(&cb).unwrap(), doc_b);

        // Missing file inside a root is `io_error`, not `path_outside_root`.
        assert_eq!(
            roots.resolve_read("missing.ron").unwrap_err().code(),
            ErrorCode::IoError
        );

        // New write target inside root reports `exists == false`; existing reports `exists == true`.
        let new_target = roots.resolve_write("sub/new.ron").unwrap();
        assert!(!new_target.exists);
        let existing_target = roots.resolve_write("sub/a.ron").unwrap();
        assert!(existing_target.exists);
        assert_eq!(existing_target.path, ca_posix);

        // Internal link staying inside roots is allowed.
        let inner_link = root_a.join("inner_link");
        if make_symlink_dir(&root_b, &inner_link) {
            let resolved = roots.resolve_read("inner_link/b.ron").unwrap();
            assert_eq!(resolved, cb);
        }
    }

    #[test]
    fn ainv12_escaping_paths_are_refused_in_windows_and_posix_forms() {
        let base = TempDir::new("ainv12");
        let root = base.path().join("root");
        let outside_dir = base.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside_dir).unwrap();
        let escape_dir = root.join("escape_dir");
        assert!(make_symlink_dir(&outside_dir, &escape_dir));
        let dangling_dir = root.join("dangling_dir");
        assert!(make_symlink_dir(
            &base.path().join("missing_outside"),
            &dangling_dir
        ));
        let roots = Roots::new([&root]).unwrap();

        proptest!(ProptestConfig::with_cases(128), |(bad in escaping_path_strategy())| {
            prop_assert_eq!(
                roots.resolve_read(&bad).unwrap_err().code(),
                ErrorCode::PathOutsideRoot
            );
            prop_assert_eq!(
                roots.resolve_write(&bad).unwrap_err().code(),
                ErrorCode::PathOutsideRoot
            );
        });
    }
}
