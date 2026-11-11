//! Resolves a link to a document in the person's selected account into the read the
//! executor serves for it.
//!
//! This is a pure parse with no new dependency. The URL is never fetched: there is no DNS,
//! no HTTP, and nothing is stored or forwarded. The resolver names no connection. The read
//! still runs on the connection the person selected, and the caller decides whether the
//! read is allowed. A link that matches none of the shapes below is `None`, which includes
//! `OneDrive` and `SharePoint` share links: they carry no stable item id.
//!
//! The accepted shapes are all `https`, on an exact host, with no userinfo and no port:
//!
//! - `docs.google.com/{document,spreadsheets,presentation}/d/{id}`, with an optional
//!   `u/{n}` account index before the `d` and an optional `edit`, `view` or `preview` after
//!   the id. Google `drive.file.content` with `fileId` and `exportAs: "text"`.
//! - `drive.google.com/file/d/{id}`, with the same optional `view`, `edit` or `preview`, and
//!   `drive.google.com/open?id={id}`. Google `drive.file.content` with `fileId` only, since a
//!   plain Drive file has no `exportAs`.
//! - `github.com/{owner}/{repo}/blob/{ref}/{path}`. GitHub `contents.get` with `owner`,
//!   `repo`, `path` and `ref`.
//! - `github.com/{owner}/{repo}`. GitHub `readme.get` with `owner` and `repo`.
//! - `notion.so`, `www.notion.so` and `{workspace}.notion.site` page links. Notion
//!   `page.content` with `pageId`, dashed.
//!
//! The parameter names are the executor's, as `drive_read.go` and `service.go` in
//! integration-corev2 read them. Paths are handed over percent-decoded, because the executor
//! escapes each segment itself. The query and the fragment are never copied, except the one
//! Drive `id` of an `open` link.

use serde_json::{Map, Value};

/// Longest URL the resolver reads. A longer one is refused, not cut.
pub const DOCUMENT_URL_MAX_BYTES: usize = 2_048;

/// Longest Google Drive file id, once decoded.
const DRIVE_ID_MAX_BYTES: usize = 128;

/// Longest GitHub owner name, which is GitHub's own limit.
const GITHUB_OWNER_MAX_BYTES: usize = 39;

/// Longest GitHub repository name, which is GitHub's own limit.
const GITHUB_REPO_MAX_BYTES: usize = 100;

/// Longest path or ref segment, once decoded.
const SEGMENT_MAX_BYTES: usize = 255;

/// Hex digits in a Notion page id, with or without dashes.
const NOTION_ID_HEX_LEN: usize = 32;

/// Longest Notion workspace subdomain.
const NOTION_WORKSPACE_MAX_BYTES: usize = 63;

/// One document read the executor serves for a link.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentRead {
    /// The provider the read goes to: `google`, `github` or `notion`.
    pub provider: &'static str,
    /// The operation the executor serves for that provider.
    pub operation: &'static str,
    /// The operation's parameters, under the names the executor reads.
    pub params: Map<String, Value>,
}

/// Resolves a document link to the read that serves it. `None` when the link is not one of the
/// accepted shapes, or when any part of it fails validation.
#[must_use]
pub fn resolve_document_url(url: &str) -> Option<DocumentRead> {
    let url = url.trim();
    let too_long = url.len() > DOCUMENT_URL_MAX_BYTES;
    let has_space_or_control = url
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte == b' ');
    if too_long || has_space_or_control {
        return None;
    }
    let rest = strip_https(url)?;
    let authority_end = rest.find(ends_authority).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let host = authority_host(authority)?;
    let (path, query) = path_and_query(tail);
    match host.as_str() {
        "docs.google.com" => google_docs(path),
        "drive.google.com" => google_drive(path, query),
        "github.com" => github(path),
        "notion.so" | "www.notion.so" => notion_page(path),
        other if notion_site_host(other) => notion_page(path),
        _ => None,
    }
}

/// Whether a character ends the authority of a URL: the path, the query or the fragment starts.
fn ends_authority(c: char) -> bool {
    matches!(c, '/' | '?' | '#')
}

/// The `https://` scheme, matched without regard to case, and what follows it.
fn strip_https(url: &str) -> Option<&str> {
    if url.get(..8)?.eq_ignore_ascii_case("https://") {
        url.get(8..)
    } else {
        None
    }
}

/// The authority as a lowercase host. Only letters, digits, dots and hyphens are allowed, so a
/// userinfo (`@`), a port (`:`) or a bracketed address never reads as a host.
fn authority_host(authority: &str) -> Option<String> {
    let valid = !authority.is_empty()
        && authority
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-');
    valid.then(|| authority.to_ascii_lowercase())
}

/// Splits what follows the authority into the path and the query. The fragment is dropped.
fn path_and_query(tail: &str) -> (&str, Option<&str>) {
    let before_fragment = tail.split_once('#').map_or(tail, |(before, _)| before);
    match before_fragment.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (before_fragment, None),
    }
}

/// The segments of a path, where one trailing slash is allowed. See `path_segments`.
fn page_segments(path: &str) -> Option<Vec<&str>> {
    path_segments(path.strip_suffix('/').unwrap_or(path))
}

/// The segments of an absolute path. `None` for the bare root, and for any empty segment: a
/// `//`, or a trailing slash the caller did not allow for.
fn path_segments(path: &str) -> Option<Vec<&str>> {
    let body = path.strip_prefix('/')?;
    if body.is_empty() {
        return None;
    }
    let segments: Vec<&str> = body.split('/').collect();
    let all_present = segments.iter().all(|segment| !segment.is_empty());
    all_present.then_some(segments)
}

/// Drops the Google account index (`u/{n}`) that some Docs links carry before the `d`.
fn without_account_index<'a, 'b>(segments: &'a [&'b str]) -> &'a [&'b str] {
    match segments {
        ["u", index, rest @ ..] if is_account_index(index) => rest,
        _ => segments,
    }
}

/// A Google account index is one to three digits.
fn is_account_index(segment: &str) -> bool {
    (1..=3).contains(&segment.len()) && segment.bytes().all(|byte| byte.is_ascii_digit())
}

/// Google Docs, Sheets and Slides files, read as text. The executor exports a Docs Editors file
/// as plain text, and a sheet as the CSV of its first sheet.
fn google_docs(path: &str) -> Option<DocumentRead> {
    let segments = page_segments(path)?;
    let (kind, rest) = segments.split_first()?;
    if !matches!(*kind, "document" | "spreadsheets" | "presentation") {
        return None;
    }
    let file_id = match without_account_index(rest) {
        ["d", id] | ["d", id, "edit" | "view" | "preview"] => drive_id(id)?,
        _ => return None,
    };
    Some(DocumentRead {
        provider: "google",
        operation: "drive.file.content",
        params: params([("fileId", file_id), ("exportAs", "text".to_owned())]),
    })
}

/// Google Drive files: `file/d/{id}` (with an optional `view`, `edit` or `preview`) and the
/// `open?id={id}` link. A plain file has no `exportAs`, so it is read as stored.
fn google_drive(path: &str, query: Option<&str>) -> Option<DocumentRead> {
    let file_id = match page_segments(path)?.as_slice() {
        ["file", "d", id] | ["file", "d", id, "view" | "edit" | "preview"] => drive_id(id)?,
        ["open"] => open_id(query?)?,
        _ => return None,
    };
    Some(DocumentRead {
        provider: "google",
        operation: "drive.file.content",
        params: params([("fileId", file_id)]),
    })
}

/// The one `id` of an `open?id=` query. Other keys are not copied, and a repeated `id` is refused
/// rather than chosen between.
fn open_id(query: &str) -> Option<String> {
    let mut found: Option<&str> = None;
    for pair in query.split('&') {
        if let Some(("id", value)) = pair.split_once('=') {
            if found.replace(value).is_some() {
                return None;
            }
        }
    }
    drive_id(found?)
}

/// A GitHub repository's root is its readme. A `blob` path is one file at one ref.
fn github(path: &str) -> Option<DocumentRead> {
    if let [owner, repo] = page_segments(path)?.as_slice() {
        return github_readme(owner, repo);
    }
    match path_segments(path)?.as_slice() {
        [owner, repo, "blob", git_ref, file @ ..] if !file.is_empty() => {
            github_contents(owner, repo, git_ref, file)
        }
        _ => None,
    }
}

/// A repository's readme, through `readme.get`.
fn github_readme(owner: &str, repo: &str) -> Option<DocumentRead> {
    Some(DocumentRead {
        provider: "github",
        operation: "readme.get",
        params: params([
            ("owner", github_owner(owner)?),
            ("repo", github_repo(repo)?),
        ]),
    })
}

/// One file at one ref, through `contents.get`. Each path segment is decoded on its own.
fn github_contents(owner: &str, repo: &str, git_ref: &str, file: &[&str]) -> Option<DocumentRead> {
    let segments = file
        .iter()
        .map(|segment| decoded_segment(segment))
        .collect::<Option<Vec<String>>>()?;
    Some(DocumentRead {
        provider: "github",
        operation: "contents.get",
        params: params([
            ("owner", github_owner(owner)?),
            ("repo", github_repo(repo)?),
            ("path", segments.join("/")),
            ("ref", decoded_segment(git_ref)?),
        ]),
    })
}

/// A Notion page, through `page.content`. The page id is the end of the last path segment.
fn notion_page(path: &str) -> Option<DocumentRead> {
    let segments = page_segments(path)?;
    let page_id = notion_page_id(segments.last()?)?;
    Some(DocumentRead {
        provider: "notion",
        operation: "page.content",
        params: params([("pageId", page_id)]),
    })
}

/// The page id a Notion segment ends with: `Title-<32 hex>`, or a bare `<32 hex>`. Returned in
/// lowercase and dashed as 8-4-4-4-12.
fn notion_page_id(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let start = bytes.len().checked_sub(NOTION_ID_HEX_LEN)?;
    let digits = &bytes[start..];
    let dash_before = start == 0 || bytes[start - 1] == b'-';
    if !dash_before || !digits.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let digits = std::str::from_utf8(digits).ok()?.to_ascii_lowercase();
    let dashed = [
        &digits[..8],
        &digits[8..12],
        &digits[12..16],
        &digits[16..20],
        &digits[20..],
    ]
    .join("-");
    Some(dashed)
}

/// Whether a host is a Notion workspace site: exactly one label under `notion.site`.
fn notion_site_host(host: &str) -> bool {
    host.strip_suffix(".notion.site").is_some_and(|workspace| {
        !workspace.is_empty()
            && workspace.len() <= NOTION_WORKSPACE_MAX_BYTES
            && workspace
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

/// A Google Drive file id, once decoded: `[A-Za-z0-9_-]`, at most `DRIVE_ID_MAX_BYTES`.
fn drive_id(raw: &str) -> Option<String> {
    let id = percent_decode(raw)?;
    let valid = !id.is_empty()
        && id.len() <= DRIVE_ID_MAX_BYTES
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    valid.then_some(id)
}

/// A GitHub owner, once decoded: letters, digits and hyphens, at most `GITHUB_OWNER_MAX_BYTES`.
fn github_owner(raw: &str) -> Option<String> {
    let owner = percent_decode(raw)?;
    let valid = !owner.is_empty()
        && owner.len() <= GITHUB_OWNER_MAX_BYTES
        && owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    valid.then_some(owner)
}

/// A GitHub repository, once decoded: letters, digits, dots, hyphens and underscores, at most
/// `GITHUB_REPO_MAX_BYTES`. `.` and `..` are refused, since a dot segment walks out of the path.
fn github_repo(raw: &str) -> Option<String> {
    let repo = percent_decode(raw)?;
    let valid = !repo.is_empty()
        && repo.len() <= GITHUB_REPO_MAX_BYTES
        && repo != "."
        && repo != ".."
        && repo
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
    valid.then_some(repo)
}

/// A path or ref segment, once decoded: not empty and not a dot segment, with no slash,
/// backslash, query or fragment character and no control character, at most `SEGMENT_MAX_BYTES`.
fn decoded_segment(raw: &str) -> Option<String> {
    let segment = percent_decode(raw)?;
    let valid = !segment.is_empty()
        && segment.len() <= SEGMENT_MAX_BYTES
        && segment != "."
        && segment != ".."
        && !segment
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | '?' | '#'));
    valid.then_some(segment)
}

/// Decodes the percent-escapes of one URL component. `None` when an escape is not two hex
/// digits, or when the decoded bytes are not UTF-8.
fn percent_decode(raw: &str) -> Option<String> {
    let mut bytes = raw.bytes();
    let mut out = Vec::with_capacity(raw.len());
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = hex_value(bytes.next()?)?;
            let low = hex_value(bytes.next()?)?;
            out.push((high << 4) | low);
        } else {
            out.push(byte);
        }
    }
    String::from_utf8(out).ok()
}

/// The value of one ASCII hex digit.
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// The parameter map from `(name, value)` pairs, under the names the executor reads.
fn params<const N: usize>(entries: [(&str, String); N]) -> Map<String, Value> {
    entries
        .into_iter()
        .map(|(name, value)| (name.to_owned(), Value::String(value)))
        .collect()
}
