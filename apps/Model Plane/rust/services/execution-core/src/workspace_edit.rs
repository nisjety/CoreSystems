//! Workspace file operations as pure functions over one canonical root (WS24-S2).
//!
//! `read`, `grep`, `glob`, `list`, `edit` and `put` touch the local filesystem
//! under a [`WorkspaceRoot`] and nothing else: no network, no gRPC, no sandbox
//! lease and no tool bridge. Every file is named by a root-relative path, and
//! escapes, symlinks and reserved names are refused. An edit must present the
//! version token (`sha256:<hex>`, the digest the Space CAS stores) of the bytes
//! it was based on, so a stale edit is refused rather than applied over newer
//! content. Callers hold the run's [`WorkspaceLock`] around each call: read for
//! the read-only operations, write for `edit` and `put`.

use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::workspace_hydrate::{content_version, would_redact};

/// One run's lock over its workspace. Reads share it; edits and puts take it exclusively.
pub type WorkspaceLock = Arc<tokio::sync::RwLock<()>>;

/// Largest file an operation reads or writes, and the largest edit result.
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
const READ_DEFAULT_LINES: usize = 200;
const READ_MAX_LINES: usize = 800;
const READ_MAX_BYTES: usize = 64 * 1024;
const LINE_MAX_CHARS: usize = 2000;
const CONTEXT_MAX_LINES: usize = 3;
const GREP_MAX_RESULTS: usize = 100;
const GLOB_MAX_RESULTS: usize = 500;
const LIST_MAX_RESULTS: usize = 500;
const LIST_MAX_DEPTH: usize = 3;
/// Entries a walk may visit before a glob or grep stops; a glob past it is refused.
const WALK_MAX_ENTRIES: usize = 5000;
const DIFF_PREVIEW_LINES: usize = 40;
const AMBIGUOUS_LINES_REPORTED: usize = 20;
const BOM: &str = "\u{feff}";
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Every way a workspace operation can refuse. [`EditError::code`] is the closed
/// machine-readable code a tool result carries; the message is for the model.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("the path leaves the workspace root")]
    OutsideRoot,
    #[error("symbolic links are not followed inside the workspace")]
    SymlinkRefused,
    #[error("the file is binary or not UTF-8 text")]
    NotUtf8,
    #[error("the file or the result is larger than the 1 MiB limit")]
    TooLarge,
    #[error("the file changed since it was read; read it again and retry")]
    StaleVersion { current: Version },
    #[error("read the file first and pass the version token from that read")]
    ReadFirst,
    #[error("old_string was not found; copy the text exactly as a read returned it")]
    NoMatch,
    #[error("old_string matches more than once; add surrounding context or set replace_all")]
    AmbiguousMatch { count: usize, lines: Vec<usize> },
    #[error("new text matches a secret pattern; use a placeholder instead of the value")]
    SecretPattern,
    #[error("the pattern scans too many entries; start it from a literal directory")]
    GlobTooBroad,
    #[error("the file already exists; pass its version token to overwrite it")]
    Exists,
    #[error("the file does not exist")]
    NotFound,
    #[error("the path uses a reserved name")]
    ReservedPath,
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),
    #[error("filesystem error: {0}")]
    Io(String),
}

impl EditError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::OutsideRoot => "outside_root",
            Self::SymlinkRefused => "symlink_refused",
            Self::NotUtf8 => "not_utf8",
            Self::TooLarge => "too_large",
            Self::StaleVersion { .. } => "stale_version",
            Self::ReadFirst => "read_first",
            Self::NoMatch => "no_match",
            Self::AmbiguousMatch { .. } => "ambiguous_match",
            Self::SecretPattern => "secret_pattern",
            Self::GlobTooBroad => "glob_too_broad",
            Self::Exists => "exists",
            Self::NotFound => "not_found",
            Self::ReservedPath => "reserved_path",
            Self::InvalidArgument(_) => "invalid_argument",
            Self::Io(_) => "io_error",
        }
    }
}

/// The `sha256:<hex>` version token of one file's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version(String);

impl Version {
    /// The token for `bytes`, which is the same digest the CAS upload uses.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(content_version(bytes))
    }

    /// Parses a token a caller sent back: `sha256:` and 64 lowercase hex digits.
    pub fn parse(token: &str) -> Result<Self, EditError> {
        let well_formed = token.strip_prefix("sha256:").is_some_and(|hex| {
            hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        });
        if well_formed {
            Ok(Self(token.to_owned()))
        } else {
            Err(EditError::InvalidArgument("expected_version must be a sha256 version token"))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A canonical workspace directory. Every path the operations take is resolved
/// beneath it, and a symlink at any existing component is refused.
#[derive(Debug, Clone)]
pub struct WorkspaceRoot {
    root: PathBuf,
}

impl WorkspaceRoot {
    /// Canonicalises `root` once, so each later containment check compares
    /// against the real directory and not against one spelling of it.
    pub fn new(root: &Path) -> Result<Self, EditError> {
        let root = fs::canonicalize(root).map_err(io_error)?;
        if !root.is_dir() {
            return Err(EditError::NotFound);
        }
        Ok(Self { root })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.root
    }

    /// Resolves a non-empty root-relative path that must exist as far as it goes.
    fn resolve(&self, rel: &str) -> Result<PathBuf, EditError> {
        let mut abs = self.root.clone();
        for segment in segments(rel)? {
            abs.push(segment);
            match fs::symlink_metadata(&abs) {
                Ok(meta) if meta.file_type().is_symlink() => return Err(EditError::SymlinkRefused),
                Ok(_) => {}
                // A missing tail is fine: `put` creates it.
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(error)),
            }
        }
        Ok(abs)
    }

    /// Like [`Self::resolve`] for the root itself too, which is the start of a walk.
    fn locate(&self, rel: &str) -> Result<PathBuf, EditError> {
        if rel.is_empty() {
            return Ok(self.root.clone());
        }
        self.resolve(rel)
    }

    fn resolve_write(&self, rel: &str) -> Result<PathBuf, EditError> {
        if segments(rel)?.iter().any(|segment| is_reserved(segment)) {
            return Err(EditError::ReservedPath);
        }
        self.resolve(rel)
    }
}

/// Splits a root-relative path. Absolute paths, drive or stream syntax,
/// backslashes and `..` leave the root; empty and `.` segments are malformed.
fn segments(rel: &str) -> Result<Vec<&str>, EditError> {
    if rel.starts_with('/') || rel.contains(['\\', ':', '\0']) || rel.split('/').any(|segment| segment == "..") {
        return Err(EditError::OutsideRoot);
    }
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.iter().any(|segment| segment.is_empty() || *segment == ".") {
        return Err(EditError::InvalidArgument("the path must have no empty or . segments"));
    }
    Ok(parts)
}

/// Names the platform reserves for uploads and for the staging files of writes.
fn is_reserved(segment: &str) -> bool {
    segment == "attachments" || segment.starts_with(".verevon")
}

fn io_error(error: io::Error) -> EditError {
    match error.kind() {
        io::ErrorKind::NotFound => EditError::NotFound,
        _ => EditError::Io(error.to_string()),
    }
}

/// A file's raw bytes and metadata, refused when it is not a file or is over the limit.
fn load(path: &Path) -> Result<(Vec<u8>, fs::Metadata), EditError> {
    let meta = fs::metadata(path).map_err(io_error)?;
    if !meta.is_file() {
        return Err(EditError::InvalidArgument("the path is not a file"));
    }
    if meta.len() > MAX_FILE_BYTES as u64 {
        return Err(EditError::TooLarge);
    }
    let raw = fs::read(path).map_err(io_error)?;
    Ok((raw, meta))
}

/// Splits a file into its BOM flag and text. A NUL byte or invalid UTF-8 is binary.
fn decode(raw: &[u8]) -> Result<(bool, &str), EditError> {
    if raw.contains(&0) {
        return Err(EditError::NotUtf8);
    }
    let text = std::str::from_utf8(raw).map_err(|_| EditError::NotUtf8)?;
    Ok(match text.strip_prefix(BOM) {
        Some(body) => (true, body),
        None => (false, text),
    })
}

fn clip(line: &str) -> String {
    line.chars().take(LINE_MAX_CHARS).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadOutcome {
    pub version: Version,
    pub total_lines: usize,
    pub start_line: usize,
    pub lines: Vec<String>,
    /// The 1-based line to ask for next when the window stopped before the end of the file.
    pub next_line: Option<usize>,
}

/// Reads a window of lines from one file. `start_line` is 1-based.
pub fn read(
    root: &WorkspaceRoot,
    rel: &str,
    start_line: usize,
    max_lines: Option<usize>,
) -> Result<ReadOutcome, EditError> {
    if start_line == 0 {
        return Err(EditError::InvalidArgument("start_line is 1-based"));
    }
    let path = root.resolve(rel)?;
    let (raw, _) = load(&path)?;
    let (_, body) = decode(&raw)?;
    let wanted = max_lines.unwrap_or(READ_DEFAULT_LINES).clamp(1, READ_MAX_LINES);
    let mut lines = Vec::new();
    let mut used = 0;
    let mut next_line = None;
    for (index, line) in body.lines().enumerate().skip(start_line - 1) {
        let clipped = clip(line);
        if lines.len() == wanted || used + clipped.len() > READ_MAX_BYTES {
            next_line = Some(index + 1);
            break;
        }
        used += clipped.len() + 1;
        lines.push(clipped);
    }
    Ok(ReadOutcome {
        version: Version::of(&raw),
        total_lines: body.lines().count(),
        start_line,
        lines,
        next_line,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepMatch {
    pub path: String,
    pub line: usize,
    pub text: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepOutcome {
    pub matches: Vec<GrepMatch>,
    pub truncated: bool,
}

/// Searches text files under `rel` (or the whole root). Binary files, files over
/// the size limit and symlinks are skipped; the walk and the result are capped.
pub fn grep(
    root: &WorkspaceRoot,
    pattern: &str,
    rel: Option<&str>,
    context: usize,
    max_results: usize,
) -> Result<GrepOutcome, EditError> {
    let regex = regex::Regex::new(pattern)
        .map_err(|_| EditError::InvalidArgument("the pattern is not a valid regular expression"))?;
    let context = context.min(CONTEXT_MAX_LINES);
    let max_results = max_results.clamp(1, GREP_MAX_RESULTS);
    let start = root.locate(rel.unwrap_or(""))?;
    let (files, complete) = files_under(&start)?;
    let mut matches = Vec::new();
    let mut truncated = !complete;
    'files: for file in files {
        let Ok((raw, _)) = load(&file) else { continue };
        let Ok((_, body)) = decode(&raw) else { continue };
        let lines: Vec<&str> = body.lines().collect();
        for (index, &line) in lines.iter().enumerate() {
            if !regex.is_match(line) {
                continue;
            }
            if matches.len() == max_results {
                truncated = true;
                break 'files;
            }
            matches.push(GrepMatch {
                path: rel_string(root.path(), &file),
                line: index + 1,
                text: clip(line),
                before: lines[index.saturating_sub(context)..index].iter().map(|l| clip(l)).collect(),
                after: lines[index + 1..(index + 1 + context).min(lines.len())].iter().map(|l| clip(l)).collect(),
            });
        }
    }
    Ok(GrepOutcome { matches, truncated })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobOutcome {
    pub paths: Vec<String>,
    pub truncated: bool,
}

/// Lists paths matching a glob over root-relative segments: `*` and `?` within a
/// segment, `**` across segments. A pattern that scans more than
/// `WALK_MAX_ENTRIES` entries is refused, so start it from a literal directory.
pub fn glob(root: &WorkspaceRoot, pattern: &str) -> Result<GlobOutcome, EditError> {
    let parts = segments(pattern)?;
    let literal = parts.iter().take_while(|segment| !segment.contains(['*', '?'])).count();
    let base = if literal == 0 {
        root.path().to_path_buf()
    } else {
        root.resolve(&parts[..literal].join("/"))?
    };
    let mut candidates = Vec::new();
    if base.is_dir() {
        let (entries, complete) = walk(&base, usize::MAX, WALK_MAX_ENTRIES)?;
        if !complete {
            return Err(EditError::GlobTooBroad);
        }
        candidates.push(base);
        candidates.extend(entries.into_iter().map(|entry| entry.path));
    } else if base.is_file() {
        candidates.push(base);
    }
    let mut paths: Vec<String> = candidates
        .iter()
        .map(|path| rel_string(root.path(), path))
        .filter(|rel| !rel.is_empty() && glob_match(&parts, &rel.split('/').collect::<Vec<_>>()))
        .collect();
    paths.sort();
    let truncated = paths.len() > GLOB_MAX_RESULTS;
    paths.truncate(GLOB_MAX_RESULTS);
    Ok(GlobOutcome { paths, truncated })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntry {
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListOutcome {
    pub entries: Vec<ListEntry>,
    pub truncated: bool,
}

/// Lists entries under `rel` (or the root) down to `depth` levels, at most 500.
pub fn list(root: &WorkspaceRoot, rel: Option<&str>, depth: usize) -> Result<ListOutcome, EditError> {
    let start = root.locate(rel.unwrap_or(""))?;
    if start.is_file() {
        return Err(EditError::InvalidArgument("the path is not a directory"));
    }
    let (entries, complete) = walk(&start, depth.clamp(1, LIST_MAX_DEPTH), LIST_MAX_RESULTS)?;
    let mut listed: Vec<ListEntry> = entries
        .into_iter()
        .map(|entry| ListEntry {
            path: rel_string(root.path(), &entry.path),
            is_dir: entry.is_dir,
            size: (!entry.is_dir).then_some(entry.len),
        })
        .collect();
    listed.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ListOutcome { entries: listed, truncated: !complete })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditOutcome {
    pub version_before: Version,
    pub version_after: Version,
    pub replacements: usize,
    /// 1-based line of the first replaced text in the file as it was before the edit.
    pub first_changed_line: usize,
    /// The first replacement as `-`/`+` lines, at most 40.
    pub diff_preview: Vec<String>,
}

/// Replaces `old_string` in one file. The match is tried exactly, then with
/// CRLF and LF treated alike, then with trailing whitespace ignored, and the
/// first step that matches wins. `expected` is the version the caller read.
pub fn edit(
    root: &WorkspaceRoot,
    rel: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
    expected: Option<&Version>,
) -> Result<EditOutcome, EditError> {
    let path = root.resolve_write(rel)?;
    if old_string.is_empty() {
        return Err(EditError::InvalidArgument("old_string must not be empty"));
    }
    let (raw, meta) = load(&path)?;
    let (bom, body) = decode(&raw)?;
    let current = Version::of(&raw);
    let expected = expected.ok_or(EditError::ReadFirst)?;
    if expected != &current {
        return Err(EditError::StaleVersion { current });
    }
    if would_redact(new_string.as_bytes()) {
        return Err(EditError::SecretPattern);
    }
    let (spans, ladder) = match_spans(body, old_string)?;
    if spans.len() > 1 && !replace_all {
        return Err(EditError::AmbiguousMatch {
            count: spans.len(),
            lines: spans.iter().take(AMBIGUOUS_LINES_REPORTED).map(|(start, _)| line_of(body, *start)).collect(),
        });
    }
    let eol = dominant_eol(body);
    let mut replacement = new_string.replace("\r\n", "\n").replace('\n', eol);
    // A whole-line match consumed its line break, so the replacement keeps one.
    if ladder == Ladder::Tolerant && old_string.ends_with('\n') && !replacement.is_empty() && !replacement.ends_with('\n') {
        replacement.push_str(eol);
    }
    let mut edited = String::with_capacity(body.len() + replacement.len());
    let mut cursor = 0;
    for &(start, end) in &spans {
        edited.push_str(&body[cursor..start]);
        edited.push_str(&replacement);
        cursor = end;
    }
    edited.push_str(&body[cursor..]);
    let mut bytes = Vec::with_capacity(edited.len() + BOM.len());
    if bom {
        bytes.extend_from_slice(BOM.as_bytes());
    }
    bytes.extend_from_slice(edited.as_bytes());
    if bytes.len() > MAX_FILE_BYTES {
        return Err(EditError::TooLarge);
    }
    let version_after = Version::of(&bytes);
    write_atomic(&path, &bytes, Some(meta.permissions()))?;
    let (first, _) = spans[0];
    let old_text = &body[first..spans[0].1];
    let diff_preview = old_text
        .lines()
        .map(|line| format!("-{line}"))
        .chain(replacement.lines().map(|line| format!("+{line}")))
        .take(DIFF_PREVIEW_LINES)
        .collect();
    Ok(EditOutcome {
        version_before: current,
        version_after,
        replacements: spans.len(),
        first_changed_line: line_of(body, first),
        diff_preview,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutOutcome {
    pub version_before: Option<Version>,
    pub version_after: Version,
}

/// Creates or overwrites one file. With no `expected` the file must not exist;
/// with one, the file must exist and still hold that version.
pub fn put(
    root: &WorkspaceRoot,
    rel: &str,
    content: &str,
    expected: Option<&Version>,
) -> Result<PutOutcome, EditError> {
    let path = root.resolve_write(rel)?;
    if content.len() > MAX_FILE_BYTES {
        return Err(EditError::TooLarge);
    }
    if would_redact(content.as_bytes()) {
        return Err(EditError::SecretPattern);
    }
    let existing = match fs::metadata(&path) {
        Ok(meta) if meta.is_dir() => return Err(EditError::InvalidArgument("the path is a directory")),
        Ok(_) => Some(load(&path)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(io_error(error)),
    };
    let version_before = match (&existing, expected) {
        (None, None) => None,
        (None, Some(_)) => return Err(EditError::NotFound),
        (Some(_), None) => return Err(EditError::Exists),
        (Some((raw, _)), Some(expected)) => {
            let current = Version::of(raw);
            if expected != &current {
                return Err(EditError::StaleVersion { current });
            }
            Some(current)
        }
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let perms = existing.map(|(_, meta)| meta.permissions());
    write_atomic(&path, content.as_bytes(), perms)?;
    Ok(PutOutcome { version_before, version_after: Version::of(content.as_bytes()) })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ladder {
    Exact,
    Eol,
    Tolerant,
}

/// The byte spans `old_string` matches, and the ladder step that found them.
fn match_spans(body: &str, old: &str) -> Result<(Vec<(usize, usize)>, Ladder), EditError> {
    let exact: Vec<(usize, usize)> = body.match_indices(old).map(|(i, found)| (i, i + found.len())).collect();
    if !exact.is_empty() {
        return Ok((exact, Ladder::Exact));
    }
    let (normalised, origin) = normalised_eol(body);
    let needle = old.replace("\r\n", "\n");
    let eol: Vec<(usize, usize)> = normalised
        .match_indices(needle.as_str())
        .map(|(i, found)| (origin[i], origin[i + found.len()]))
        .collect();
    if !eol.is_empty() {
        return Ok((eol, Ladder::Eol));
    }
    let tolerant = tolerant_spans(body, old);
    if !tolerant.is_empty() {
        return Ok((tolerant, Ladder::Tolerant));
    }
    Err(EditError::NoMatch)
}

/// `body` with each CRLF read as one LF, and for every byte of the result the
/// byte offset in `body` it came from. A match is mapped back through the table.
fn normalised_eol(body: &str) -> (String, Vec<usize>) {
    let mut normalised = String::with_capacity(body.len());
    let mut origin = Vec::with_capacity(body.len() + 1);
    let mut chars = body.char_indices();
    while let Some((index, ch)) = chars.next() {
        let (out, from) = if ch == '\r' && body[index + 1..].starts_with('\n') {
            chars.next();
            ('\n', index)
        } else {
            (ch, index)
        };
        normalised.push(out);
        for _ in 0..out.len_utf8() {
            origin.push(from);
        }
    }
    origin.push(body.len());
    (normalised, origin)
}

/// Matches whole lines of `old` against whole lines of `body`, ignoring trailing
/// whitespace. Returns spans that run from the first line's start to the last line's end.
fn tolerant_spans(body: &str, old: &str) -> Vec<(usize, usize)> {
    let normalised = old.replace("\r\n", "\n");
    let trailing = normalised.ends_with('\n');
    let needle: Vec<&str> = normalised
        .strip_suffix('\n')
        .unwrap_or(&normalised)
        .split('\n')
        .map(str::trim_end)
        .collect();
    let lines = line_bounds(body);
    let mut spans = Vec::new();
    let mut i = 0;
    while i + needle.len() <= lines.len() {
        let last = lines[i + needle.len() - 1];
        let matched = needle.iter().enumerate().all(|(j, want)| {
            let line = lines[i + j];
            body[line.start..line.content_end].trim_end() == *want
        });
        if matched && (!trailing || last.end > last.content_end) {
            let end = if trailing { last.end } else { last.content_end };
            spans.push((lines[i].start, end));
            i += needle.len();
        } else {
            i += 1;
        }
    }
    spans
}

#[derive(Debug, Clone, Copy)]
struct LineBound {
    start: usize,
    content_end: usize,
    end: usize,
}

/// Every line of `body` with its content span (no line break) and its full span (with it).
fn line_bounds(body: &str) -> Vec<LineBound> {
    let mut lines = Vec::new();
    let mut start = 0;
    while start < body.len() {
        match body[start..].find('\n') {
            Some(offset) => {
                let newline = start + offset;
                let content_end = if newline > start && body.as_bytes()[newline - 1] == b'\r' {
                    newline - 1
                } else {
                    newline
                };
                lines.push(LineBound { start, content_end, end: newline + 1 });
                start = newline + 1;
            }
            None => {
                lines.push(LineBound { start, content_end: body.len(), end: body.len() });
                break;
            }
        }
    }
    lines
}

/// The file's line break: CRLF when most of its breaks are CRLF, else LF.
fn dominant_eol(body: &str) -> &'static str {
    let lf = body.matches('\n').count();
    let crlf = body.matches("\r\n").count();
    if crlf > lf - crlf {
        "\r\n"
    } else {
        "\n"
    }
}

/// The 1-based line that contains byte `index` of `body`.
fn line_of(body: &str, index: usize) -> usize {
    body[..index].matches('\n').count() + 1
}

/// Writes `bytes` to a sibling staging file and renames it over `path`, so a
/// reader sees either the old content or the new content, never a mix.
fn write_atomic(path: &Path, bytes: &[u8], perms: Option<fs::Permissions>) -> Result<(), EditError> {
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let staging = path.with_file_name(format!(".verevon-tmp-{}-{seq}", std::process::id()));
    match write_then_rename(&staging, path, bytes, perms) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&staging);
            Err(io_error(error))
        }
    }
}

fn write_then_rename(staging: &Path, path: &Path, bytes: &[u8], perms: Option<fs::Permissions>) -> io::Result<()> {
    {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(staging)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    if let Some(perms) = perms {
        fs::set_permissions(staging, perms)?;
    }
    fs::rename(staging, path)
}

struct Entry {
    path: PathBuf,
    is_dir: bool,
    len: u64,
}

/// Entries below `start`, descending at most `max_depth` levels (1 is the
/// direct children) and visiting at most `cap` entries. Symlinks are never
/// followed or reported and staging files are skipped. The flag is false when
/// the cap cut the walk short.
fn walk(start: &Path, max_depth: usize, cap: usize) -> Result<(Vec<Entry>, bool), EditError> {
    let mut found = Vec::new();
    let mut pending = vec![(start.to_path_buf(), 1_usize)];
    while let Some((dir, depth)) = pending.pop() {
        for item in fs::read_dir(&dir).map_err(io_error)? {
            let item = item.map_err(io_error)?;
            if item.file_name().to_string_lossy().starts_with(".verevon") {
                continue;
            }
            let kind = item.file_type().map_err(io_error)?;
            if kind.is_symlink() {
                continue;
            }
            if found.len() == cap {
                return Ok((found, false));
            }
            let is_dir = kind.is_dir();
            let path = item.path();
            let len = if is_dir { 0 } else { item.metadata().map_err(io_error)?.len() };
            if is_dir && depth < max_depth {
                pending.push((path.clone(), depth + 1));
            }
            found.push(Entry { path, is_dir, len });
        }
    }
    Ok((found, true))
}

/// The regular files at or under `start`, in path order, and whether the walk saw them all.
fn files_under(start: &Path) -> Result<(Vec<PathBuf>, bool), EditError> {
    if start.is_file() {
        return Ok((vec![start.to_path_buf()], true));
    }
    let (entries, complete) = walk(start, usize::MAX, WALK_MAX_ENTRIES)?;
    let mut files: Vec<PathBuf> = entries.into_iter().filter(|entry| !entry.is_dir).map(|entry| entry.path).collect();
    files.sort();
    Ok((files, complete))
}

/// `path` as a `/`-separated string relative to `root`.
fn rel_string(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Matches root-relative segments against a glob. `**` spans any number of segments.
fn glob_match(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((segment, rest)) if *segment == "**" => (0..=path.len()).any(|skip| glob_match(rest, &path[skip..])),
        Some((segment, rest)) => match path.split_first() {
            Some((name, path_rest)) => segment_match(segment, name) && glob_match(rest, path_rest),
            None => false,
        },
    }
}

/// Matches one segment: `*` is any run of characters, `?` is one character.
fn segment_match(pattern: &str, name: &str) -> bool {
    fn matches(pattern: &[char], name: &[char]) -> bool {
        match pattern.split_first() {
            None => name.is_empty(),
            Some(('*', rest)) => (0..=name.len()).any(|skip| matches(rest, &name[skip..])),
            Some(('?', rest)) => !name.is_empty() && matches(rest, &name[1..]),
            Some((ch, rest)) => name.first() == Some(ch) && matches(rest, &name[1..]),
        }
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    matches(&pattern, &name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory under the OS temp dir, removed on drop.
    struct Scratch {
        dir: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos());
            let dir = std::env::temp_dir().join(format!("verevon-ws-edit-{name}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&dir).expect("scratch dir");
            Self { dir }
        }

        fn root(&self) -> WorkspaceRoot {
            WorkspaceRoot::new(&self.dir).expect("root")
        }

        fn write(&self, rel: &str, bytes: &[u8]) {
            let path = self.dir.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("parent dir");
            }
            fs::write(path, bytes).expect("write fixture");
        }

        fn bytes(&self, rel: &str) -> Vec<u8> {
            fs::read(self.dir.join(rel)).expect("read back")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn token(bytes: &[u8]) -> Version {
        Version::of(bytes)
    }

    #[test]
    fn exact_edit_replaces_the_match_and_reports_both_versions() {
        let scratch = Scratch::new("exact");
        let before = b"one\ntwo\nthree\n";
        scratch.write("a.txt", before);
        let root = scratch.root();
        let outcome = edit(&root, "a.txt", "two", "2", false, Some(&token(before))).expect("edit");
        assert_eq!(scratch.bytes("a.txt"), b"one\n2\nthree\n");
        assert_eq!(outcome.version_before, token(before));
        assert_eq!(outcome.version_after, token(b"one\n2\nthree\n"));
        assert_eq!(outcome.replacements, 1);
        assert_eq!(outcome.first_changed_line, 2);
        assert_eq!(outcome.diff_preview, vec!["-two".to_owned(), "+2".to_owned()]);
    }

    #[test]
    fn an_ambiguous_match_names_the_count_and_the_lines() {
        let scratch = Scratch::new("ambiguous");
        let before = b"x\nfoo\ny\nfoo\n";
        scratch.write("a.txt", before);
        let root = scratch.root();
        let error = edit(&root, "a.txt", "foo", "bar", false, Some(&token(before))).expect_err("ambiguous");
        assert_eq!(error, EditError::AmbiguousMatch { count: 2, lines: vec![2, 4] });
        assert_eq!(scratch.bytes("a.txt"), before, "a refused edit leaves the file alone");
    }

    #[test]
    fn replace_all_replaces_every_match() {
        let scratch = Scratch::new("replace-all");
        let before = b"foo foo\nfoo\n";
        scratch.write("a.txt", before);
        let root = scratch.root();
        let outcome = edit(&root, "a.txt", "foo", "bar", true, Some(&token(before))).expect("edit");
        assert_eq!(scratch.bytes("a.txt"), b"bar bar\nbar\n");
        assert_eq!(outcome.replacements, 3);
    }

    #[test]
    fn a_missing_old_string_is_no_match() {
        let scratch = Scratch::new("no-match");
        let before = b"alpha\n";
        scratch.write("a.txt", before);
        let root = scratch.root();
        assert_eq!(edit(&root, "a.txt", "beta", "gamma", false, Some(&token(before))), Err(EditError::NoMatch));
    }

    #[test]
    fn an_edit_without_a_version_is_read_first() {
        let scratch = Scratch::new("read-first");
        scratch.write("a.txt", b"alpha\n");
        let root = scratch.root();
        assert_eq!(edit(&root, "a.txt", "alpha", "beta", false, None), Err(EditError::ReadFirst));
    }

    #[test]
    fn a_stale_version_is_refused_with_the_current_token() {
        let scratch = Scratch::new("stale");
        let old = b"alpha\n";
        let new = b"alpha changed\n";
        scratch.write("a.txt", new);
        let root = scratch.root();
        let error = edit(&root, "a.txt", "alpha", "beta", false, Some(&token(old))).expect_err("stale");
        assert_eq!(error, EditError::StaleVersion { current: token(new) });
        assert_eq!(error.code(), "stale_version");
    }

    #[test]
    fn lf_text_matches_a_crlf_file_and_the_file_keeps_crlf() {
        let scratch = Scratch::new("eol");
        let before = b"one\r\ntwo\r\nthree\r\n";
        scratch.write("a.txt", before);
        let root = scratch.root();
        edit(&root, "a.txt", "one\ntwo", "1\n2\nX", false, Some(&token(before))).expect("edit");
        assert_eq!(scratch.bytes("a.txt"), b"1\r\n2\r\nX\r\nthree\r\n");
    }

    #[test]
    fn trailing_whitespace_is_ignored_and_the_line_break_is_kept() {
        let scratch = Scratch::new("tolerant");
        let before = b"fn main() {   \n    x();\t\n}\n";
        scratch.write("a.txt", before);
        let root = scratch.root();
        edit(&root, "a.txt", "fn main() {\n    x();\n", "fn main() {\n    y();\n", false, Some(&token(before)))
            .expect("edit");
        assert_eq!(scratch.bytes("a.txt"), b"fn main() {\n    y();\n}\n");
    }

    #[test]
    fn a_byte_order_mark_survives_an_edit() {
        let scratch = Scratch::new("bom");
        let before = "\u{feff}hello world\n".as_bytes().to_vec();
        scratch.write("a.txt", &before);
        let root = scratch.root();
        edit(&root, "a.txt", "hello", "bye", false, Some(&token(&before))).expect("edit");
        assert_eq!(scratch.bytes("a.txt"), "\u{feff}bye world\n".as_bytes());
    }

    #[test]
    fn new_text_that_matches_a_secret_pattern_is_refused() {
        let scratch = Scratch::new("secret");
        let before = b"config: here\n";
        scratch.write("a.txt", before);
        let root = scratch.root();
        let error = edit(
            &root,
            "a.txt",
            "config",
            "DB_PASSWORD=hunter2hunter2",
            false,
            Some(&token(before)),
        )
        .expect_err("secret");
        assert_eq!(error, EditError::SecretPattern);
    }

    #[test]
    fn binary_and_non_utf8_files_are_not_utf8() {
        let scratch = Scratch::new("binary");
        scratch.write("bin.dat", &[0x00, 0x01, 0x02]);
        scratch.write("latin.txt", &[0xFF, 0xFE, 0x41]);
        let root = scratch.root();
        assert_eq!(read(&root, "bin.dat", 1, None), Err(EditError::NotUtf8));
        assert_eq!(read(&root, "latin.txt", 1, None), Err(EditError::NotUtf8));
    }

    #[test]
    fn a_file_over_the_limit_is_too_large() {
        let scratch = Scratch::new("too-large");
        scratch.write("big.txt", &vec![b'a'; MAX_FILE_BYTES + 1]);
        let root = scratch.root();
        assert_eq!(read(&root, "big.txt", 1, None), Err(EditError::TooLarge));
        assert_eq!(put(&root, "huge.txt", &"a".repeat(MAX_FILE_BYTES + 1), None), Err(EditError::TooLarge));
    }

    #[test]
    fn escaping_and_malformed_paths_are_refused() {
        let scratch = Scratch::new("escape");
        let root = scratch.root();
        assert_eq!(put(&root, "../x.txt", "a", None), Err(EditError::OutsideRoot));
        assert_eq!(put(&root, "/abs.txt", "a", None), Err(EditError::OutsideRoot));
        assert_eq!(put(&root, "a\\b.txt", "a", None), Err(EditError::OutsideRoot));
        assert_eq!(put(&root, "C:x.txt", "a", None), Err(EditError::OutsideRoot));
        assert_eq!(put(&root, "a//b.txt", "a", None).map_err(|e| e.code()), Err("invalid_argument"));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_component_is_refused() {
        let scratch = Scratch::new("symlink");
        scratch.write("real/a.txt", b"alpha\n");
        std::os::unix::fs::symlink(scratch.dir.join("real"), scratch.dir.join("link")).expect("symlink");
        let root = scratch.root();
        assert_eq!(read(&root, "link/a.txt", 1, None), Err(EditError::SymlinkRefused));
    }

    #[test]
    fn put_creates_then_refuses_an_overwrite_without_a_version() {
        let scratch = Scratch::new("put-create");
        let root = scratch.root();
        let created = put(&root, "notes/todo.md", "one\n", None).expect("create");
        assert_eq!(created.version_before, None);
        assert_eq!(put(&root, "notes/todo.md", "two\n", None), Err(EditError::Exists));
        let replaced = put(&root, "notes/todo.md", "two\n", Some(&created.version_after)).expect("overwrite");
        assert_eq!(replaced.version_before, Some(created.version_after));
        assert_eq!(scratch.bytes("notes/todo.md"), b"two\n");
    }

    #[test]
    fn put_with_a_version_needs_an_existing_file_at_that_version() {
        let scratch = Scratch::new("put-version");
        let root = scratch.root();
        assert_eq!(put(&root, "missing.txt", "a", Some(&token(b"a"))), Err(EditError::NotFound));
        scratch.write("kept.txt", b"kept\n");
        let stale = put(&root, "kept.txt", "a", Some(&token(b"other\n"))).expect_err("stale");
        assert_eq!(stale.code(), "stale_version");
    }

    #[test]
    fn writes_refuse_reserved_names() {
        let scratch = Scratch::new("reserved");
        let root = scratch.root();
        assert_eq!(put(&root, "attachments/a.txt", "a", None), Err(EditError::ReservedPath));
        assert_eq!(put(&root, "docs/.verevon-x", "a", None), Err(EditError::ReservedPath));
    }

    #[test]
    fn read_windows_the_file_and_names_the_next_line() {
        let scratch = Scratch::new("read-window");
        let text: String = (1..=5).map(|n| format!("line {n}\n")).collect();
        scratch.write("a.txt", text.as_bytes());
        let root = scratch.root();
        let window = read(&root, "a.txt", 2, Some(2)).expect("read");
        assert_eq!(window.lines, vec!["line 2".to_owned(), "line 3".to_owned()]);
        assert_eq!(window.total_lines, 5);
        assert_eq!(window.next_line, Some(4));
        assert_eq!(window.version, token(text.as_bytes()));
        assert_eq!(read(&root, "a.txt", 0, None), Err(EditError::InvalidArgument("start_line is 1-based")));
    }

    #[test]
    fn grep_skips_binary_files_and_caps_the_results() {
        let scratch = Scratch::new("grep");
        scratch.write("a.txt", b"hit one\nhit two\nhit three\n");
        scratch.write("bin.dat", b"hit\x00binary");
        let root = scratch.root();
        let capped = grep(&root, "hit", None, 0, 2).expect("grep");
        assert_eq!(capped.matches.len(), 2);
        assert!(capped.truncated);
        let all = grep(&root, "hit", None, 1, 100).expect("grep");
        assert_eq!(all.matches.len(), 3);
        assert_eq!(all.matches[1].before, vec!["hit one".to_owned()]);
        assert!(all.matches.iter().all(|m| m.path == "a.txt"));
    }

    #[test]
    fn glob_matches_across_directories_and_refuses_an_unscoped_scan_over_the_cap() {
        let scratch = Scratch::new("glob");
        scratch.write("src/lib/mod.rs", b"a");
        scratch.write("src/main.rs", b"b");
        scratch.write("docs/readme.md", b"c");
        let root = scratch.root();
        assert_eq!(glob(&root, "src/**/*.rs").expect("glob").paths, vec!["src/lib/mod.rs", "src/main.rs"]);
        assert_eq!(glob(&root, "src/main.rs").expect("glob").paths, vec!["src/main.rs"]);
        for n in 0..=WALK_MAX_ENTRIES {
            scratch.write(&format!("bulk/f{n}.txt"), b"");
        }
        assert_eq!(glob(&root, "**/*.txt"), Err(EditError::GlobTooBroad));
    }

    #[test]
    fn list_stops_at_the_depth_limit() {
        let scratch = Scratch::new("list");
        scratch.write("a/b/c/d/deep.txt", b"x");
        let root = scratch.root();
        let shallow = list(&root, None, 1).expect("list");
        assert_eq!(shallow.entries.len(), 1);
        assert!(shallow.entries[0].is_dir);
        let three = list(&root, None, 3).expect("list");
        assert!(three.entries.iter().all(|entry| entry.path != "a/b/c/d/deep.txt"));
        assert!(three.entries.iter().any(|entry| entry.path == "a/b/c"));
    }

    #[test]
    fn segment_match_handles_star_and_question_mark() {
        assert!(segment_match("*.rs", "main.rs"));
        assert!(segment_match("m?in.rs", "main.rs"));
        assert!(!segment_match("*.rs", "main.py"));
        assert!(!segment_match("m?n.rs", "main.rs"));
    }

    #[test]
    fn version_parse_accepts_only_well_formed_tokens() {
        assert!(Version::parse(&format!("sha256:{}", "a".repeat(64))).is_ok());
        assert!(Version::parse(&format!("sha256:{}", "A".repeat(64))).is_err());
        assert!(Version::parse("sha256:abc").is_err());
    }
}
