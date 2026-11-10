//! Public-URL rewrite table (Q4-E4.3-056b1): a pure mapping from the URL of a
//! publicly shared Google Doc, Sheet or Slides deck, or a GitHub `blob` page,
//! to the plain-content URL that serves the same document without the app
//! shell (a Docs export, or `raw.githubusercontent.com`).
//!
//! The module does no I/O and nothing calls it yet. The wiring (Q4-E4.3-056b2)
//! must run the SSRF guard, robots and the deny list again on the rewritten
//! host before fetching it: a rewrite never skips a gate, it only names a
//! different public URL.
//!
//! The table is closed. Only `https`, only the exact hosts below, no explicit
//! port, no userinfo. A rewrite never targets `googleapis.com`,
//! `api.github.com` or any other authenticated or API host, and the output URL
//! carries no credentials. A private document still answers with the login
//! page, which the normal pipeline handles as it does any login wall.
//!
//! Switch: [`ENV`] (`QUARRY_RUNTIME__URL_REWRITE`), default off.

use url::Url;

/// Environment variable that turns the public-URL rewrite on. Default off.
pub const ENV: &str = "QUARRY_RUNTIME__URL_REWRITE";

/// Rule name of the Google Docs export rule.
pub const RULE_GOOGLE_DOCS: &str = "google_docs_txt";
/// Rule name of the Google Sheets export rule.
pub const RULE_GOOGLE_SHEETS: &str = "google_sheets_csv";
/// Rule name of the Google Slides export rule.
pub const RULE_GOOGLE_SLIDES: &str = "google_slides_txt";
/// Rule name of the GitHub blob-to-raw rule.
pub const RULE_GITHUB_BLOB: &str = "github_blob_raw";

const GOOGLE_HOST: &str = "docs.google.com";
const GITHUB_HOST: &str = "github.com";
const GITHUB_RAW_HOST: &str = "raw.githubusercontent.com";

/// Longest accepted digit run of a Sheets `gid`.
const MAX_GID_DIGITS: usize = 20;
/// Most path segments after the repository (the `blob` marker, the ref and the
/// file path together) that a GitHub rewrite accepts.
const MAX_GITHUB_SEGMENTS: usize = 64;

/// True when the switch value turns the rewrite on. Only `on`, `1` and `true`
/// (any case, surrounding space ignored) enable it; unset, empty or anything
/// else is off.
pub fn enabled_from(value: Option<&str>) -> bool {
    match value {
        Some(v) => {
            let v = v.trim();
            v.eq_ignore_ascii_case("on") || v == "1" || v.eq_ignore_ascii_case("true")
        }
        None => false,
    }
}

/// True when [`ENV`] turns the rewrite on.
pub fn enabled() -> bool {
    enabled_from(std::env::var(ENV).ok().as_deref())
}

/// One rewrite: the URL to fetch in place of the requested one, and the name of
/// the rule that produced it (for the trace and metrics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    /// The plain-content URL to fetch.
    pub fetched: Url,
    /// The rule that matched; one of the `RULE_*` constants.
    pub rule: &'static str,
}

/// Rewrite `url` through the table, or `None` when no rule matches (any other
/// scheme, host, port or path shape, or any value outside the closed classes).
pub fn rewrite(url: &Url) -> Option<Rewrite> {
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    if url.port().is_some() {
        return None;
    }
    match url.host_str()? {
        GOOGLE_HOST => rewrite_google(url),
        GITHUB_HOST => rewrite_github(url),
        _ => None,
    }
}

fn rewrite_google(url: &Url) -> Option<Rewrite> {
    let segs: Vec<&str> = url.path_segments()?.collect();
    // /{kind}/d/{id}[/edit|/view|/preview][/]
    if segs.len() < 3 || segs[1] != "d" || !is_google_id(segs[2]) {
        return None;
    }
    let tail = &segs[3..];
    let tail_ok = match tail {
        [] | [""] => true,
        [one] | [one, ""] => matches!(*one, "edit" | "view" | "preview"),
        _ => false,
    };
    if !tail_ok {
        return None;
    }
    let id = segs[2];
    let (rule, kind, query) = match segs[0] {
        "document" => (RULE_GOOGLE_DOCS, "document", "format=txt".to_string()),
        "presentation" => (RULE_GOOGLE_SLIDES, "presentation", "format=txt".to_string()),
        "spreadsheets" => {
            let mut q = "format=csv".to_string();
            match sheet_gid(url) {
                Gid::Absent => {}
                Gid::Valid(gid) => {
                    q.push_str("&gid=");
                    q.push_str(&gid);
                }
                // A tab id that is present but not a number would export the
                // wrong sheet: leave the URL to the normal pipeline.
                Gid::Invalid => return None,
            }
            (RULE_GOOGLE_SHEETS, "spreadsheets", q)
        }
        _ => return None,
    };
    let fetched = Url::parse(&format!(
        "https://{GOOGLE_HOST}/{kind}/d/{id}/export?{query}"
    ))
    .ok()?;
    Some(Rewrite { fetched, rule })
}

enum Gid {
    Absent,
    Valid(String),
    Invalid,
}

/// The Sheets tab id: `gid` from the query, else from the fragment
/// (`#gid=123`, possibly followed by `&range=...`).
fn sheet_gid(url: &Url) -> Gid {
    let from_query = url.query().and_then(|q| gid_in(q));
    let found = from_query.or_else(|| url.fragment().and_then(|f| gid_in(f)));
    match found {
        None => Gid::Absent,
        Some(v) if is_gid(v) => Gid::Valid(v.to_string()),
        Some(_) => Gid::Invalid,
    }
}

fn gid_in(pairs: &str) -> Option<&str> {
    pairs.split('&').find_map(|pair| pair.strip_prefix("gid="))
}

fn is_gid(v: &str) -> bool {
    !v.is_empty() && v.len() <= MAX_GID_DIGITS && v.bytes().all(|b| b.is_ascii_digit())
}

fn is_google_id(id: &str) -> bool {
    (10..=200).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn rewrite_github(url: &Url) -> Option<Rewrite> {
    let segs: Vec<&str> = url.path_segments()?.collect();
    // /{owner}/{repo}/blob/{ref}/{path...}
    if segs.len() < 5 || segs.len() > 2 + MAX_GITHUB_SEGMENTS {
        return None;
    }
    let (owner, repo) = (segs[0], segs[1]);
    if !is_github_name(owner) || !is_github_name(repo) || repo.ends_with(".git") {
        return None;
    }
    if segs[2] != "blob" {
        return None;
    }
    // The ref is the one segment after `blob`; a branch name with a slash is
    // ambiguous in the URL, so it is left to the normal pipeline rather than
    // guessed. Every ref and path segment must be safe as written.
    if !segs[3..].iter().all(|s| is_safe_segment(s)) {
        return None;
    }
    let rest = segs[3..].join("/");
    let fetched = Url::parse(&format!("https://{GITHUB_RAW_HOST}/{owner}/{repo}/{rest}")).ok()?;
    Some(Rewrite {
        fetched,
        rule: RULE_GITHUB_BLOB,
    })
}

fn is_github_name(s: &str) -> bool {
    (1..=100).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        // "." and ".." (and any all-dot name) are path traversal, not names.
        && s.bytes().any(|b| b != b'.')
}

/// A ref or path segment that can be copied verbatim into the raw URL: not
/// empty, not `.` or `..` (also when percent-encoded), no encoded or raw
/// slash or backslash, no control byte, and only well-formed percent escapes.
fn is_safe_segment(seg: &str) -> bool {
    if seg.is_empty() || seg.len() > 255 {
        return false;
    }
    let bytes = seg.as_bytes();
    let mut decoded: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            let (Some(h), Some(l)) = (
                bytes.get(i + 1).and_then(|c| hex_val(*c)),
                bytes.get(i + 2).and_then(|c| hex_val(*c)),
            ) else {
                return false;
            };
            decoded.push(h * 16 + l);
            i += 3;
        } else {
            // Reject anything the URL parser would have encoded and a raw
            // delimiter that could change the path shape.
            if !b.is_ascii() || b <= 0x20 || b == 0x7f || matches!(b, b'\\' | b'/' | b'?' | b'#') {
                return false;
            }
            decoded.push(b);
            i += 1;
        }
    }
    if decoded == b"." || decoded == b".." {
        return false;
    }
    !decoded
        .iter()
        .any(|&b| b < 0x20 || b == 0x7f || b == b'/' || b == b'\\')
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "1AbCdEfGhIjKlMnOpQrStUvWxYz_0123-456";

    fn rw(s: &str) -> Option<Rewrite> {
        rewrite(&Url::parse(s).expect("test url"))
    }

    fn fetched(s: &str) -> String {
        rw(s).expect("rewritten").fetched.to_string()
    }

    #[test]
    fn switch_is_on_only_for_on_one_true() {
        assert!(enabled_from(Some("on")));
        assert!(enabled_from(Some("ON")));
        assert!(enabled_from(Some("1")));
        assert!(enabled_from(Some(" true ")));
        assert!(!enabled_from(None));
        assert!(!enabled_from(Some("")));
        assert!(!enabled_from(Some("0")));
        assert!(!enabled_from(Some("yes")));
        assert!(!enabled_from(Some("off")));
        assert!(!enabled_from(Some("false")));
    }

    #[test]
    fn docs_edit_view_preview_and_bare_rewrite_to_txt_export() {
        let want = format!("https://docs.google.com/document/d/{ID}/export?format=txt");
        for tail in ["", "/", "/edit", "/view", "/preview", "/edit/"] {
            let u = format!("https://docs.google.com/document/d/{ID}{tail}");
            let r = rw(&u).unwrap_or_else(|| panic!("no rewrite for {u}"));
            assert_eq!(r.fetched.as_str(), want);
            assert_eq!(r.rule, RULE_GOOGLE_DOCS);
        }
        // Query and fragment on a Doc are dropped.
        assert_eq!(
            fetched(&format!(
                "https://docs.google.com/document/d/{ID}/edit?usp=sharing#heading=h.abc"
            )),
            want
        );
    }

    #[test]
    fn slides_rewrite_to_txt_export() {
        let r = rw(&format!(
            "https://docs.google.com/presentation/d/{ID}/edit#slide=id.p"
        ))
        .unwrap();
        assert_eq!(
            r.fetched.as_str(),
            format!("https://docs.google.com/presentation/d/{ID}/export?format=txt")
        );
        assert_eq!(r.rule, RULE_GOOGLE_SLIDES);
    }

    #[test]
    fn sheets_rewrite_to_csv_and_carry_a_numeric_gid() {
        let base = format!("https://docs.google.com/spreadsheets/d/{ID}/export?format=csv");
        let r = rw(&format!("https://docs.google.com/spreadsheets/d/{ID}/edit")).unwrap();
        assert_eq!(r.fetched.as_str(), base);
        assert_eq!(r.rule, RULE_GOOGLE_SHEETS);
        assert_eq!(
            fetched(&format!(
                "https://docs.google.com/spreadsheets/d/{ID}/edit?gid=42"
            )),
            format!("{base}&gid=42")
        );
        assert_eq!(
            fetched(&format!(
                "https://docs.google.com/spreadsheets/d/{ID}/edit#gid=123"
            )),
            format!("{base}&gid=123")
        );
        assert_eq!(
            fetched(&format!(
                "https://docs.google.com/spreadsheets/d/{ID}/edit#gid=7&range=A1"
            )),
            format!("{base}&gid=7")
        );
        // The query wins over the fragment.
        assert_eq!(
            fetched(&format!(
                "https://docs.google.com/spreadsheets/d/{ID}/edit?gid=1#gid=2"
            )),
            format!("{base}&gid=1")
        );
        assert_eq!(
            fetched(&format!(
                "https://docs.google.com/spreadsheets/d/{ID}/edit?usp=sharing&gid=0"
            )),
            format!("{base}&gid=0")
        );
    }

    #[test]
    fn sheets_with_a_non_numeric_gid_are_not_rewritten() {
        for g in ["abc", "", "1;drop", "12x", "-1", "123456789012345678901"] {
            let u = format!("https://docs.google.com/spreadsheets/d/{ID}/edit#gid={g}");
            assert!(rw(&u).is_none(), "{u}");
        }
    }

    #[test]
    fn google_rejects_bad_ids_and_shapes() {
        // Id too short, too long, or outside the class.
        for id in ["short", "has.dot.in.the.id.xx", "bad%20id%20here%20xx"] {
            assert!(
                rw(&format!("https://docs.google.com/document/d/{id}/edit")).is_none(),
                "{id}"
            );
        }
        let long = "a".repeat(201);
        assert!(rw(&format!("https://docs.google.com/document/d/{long}/edit")).is_none());
        let max = "a".repeat(200);
        assert!(rw(&format!("https://docs.google.com/document/d/{max}/edit")).is_some());
        // Published-to-web ids ("e/2PACX...") and other shapes.
        for p in [
            "document/d/e/2PACX-1vSabcdefghijklmnop/pub",
            "document/u/0/d/1AbCdEfGhIjKlMnOpQrStUvWxYz/edit",
            "forms/d/1AbCdEfGhIjKlMnOpQrStUvWxYz/edit",
            "drive/folders/1AbCdEfGhIjKlMnOpQrStUvWxYz",
            "document/d",
            "document",
            "",
        ] {
            assert!(rw(&format!("https://docs.google.com/{p}")).is_none(), "{p}");
        }
        // Tails outside the closed set, including an export we would loop on.
        for tail in [
            "export",
            "export?format=pdf",
            "edit/extra",
            "copy",
            "comments",
        ] {
            let u = format!("https://docs.google.com/document/d/{ID}/{tail}");
            assert!(rw(&u).is_none(), "{u}");
        }
    }

    #[test]
    fn github_blob_rewrites_to_raw() {
        let r = rw("https://github.com/rust-lang/rust/blob/master/src/lib.rs").unwrap();
        assert_eq!(
            r.fetched.as_str(),
            "https://raw.githubusercontent.com/rust-lang/rust/master/src/lib.rs"
        );
        assert_eq!(r.rule, RULE_GITHUB_BLOB);
        // Query and fragment (line anchors, ?plain=1) are dropped.
        assert_eq!(
            fetched("https://github.com/o/r.x/blob/v1.2.3/docs/a%20b.md?plain=1#L10-L20"),
            "https://raw.githubusercontent.com/o/r.x/v1.2.3/docs/a%20b.md"
        );
        // A commit sha as the ref.
        assert_eq!(
            fetched("https://github.com/o/r/blob/0123456789abcdef0123456789abcdef01234567/README.md"),
            "https://raw.githubusercontent.com/o/r/0123456789abcdef0123456789abcdef01234567/README.md"
        );
    }

    #[test]
    fn github_rejects_other_pages_and_unsafe_segments() {
        for u in [
            "https://github.com/o/r",
            "https://github.com/o/r/tree/main/src",
            "https://github.com/o/r/blob/main",
            "https://github.com/o/r/blob/main/",
            "https://github.com/o/r/blob/main/a//b.rs",
            "https://github.com/o/r/blob/main/a%2Fb.rs",
            "https://github.com/o/r/blob/main/a%2fb.rs",
            "https://github.com/o/r/blob/main/a%5Cb.rs",
            "https://github.com/o/r/blob/main/a%00b.rs",
            "https://github.com/o/r/blob/main/a%zzb.rs",
            "https://github.com/o/r/blob/main/a%2",
            "https://github.com/o/r.git/blob/main/a.rs",
            "https://github.com/../r/blob/main/a.rs",
            "https://github.com/o/../blob/main/a.rs",
            "https://github.com/o%2fx/r/blob/main/a.rs",
            "https://github.com/o/r/raw/main/a.rs",
            "https://github.com/login",
        ] {
            assert!(rw(u).is_none(), "{u}");
        }
        let too_long_owner = "a".repeat(101);
        assert!(rw(&format!(
            "https://github.com/{too_long_owner}/r/blob/main/a.rs"
        ))
        .is_none());
        let deep = "d/".repeat(MAX_GITHUB_SEGMENTS + 1);
        assert!(rw(&format!("https://github.com/o/r/blob/main/{deep}f.rs")).is_none());
    }

    #[test]
    fn github_dot_segments_that_the_parser_normalises_never_escape_the_repo() {
        // A literal `..` is resolved by the URL parser before the table sees
        // it, so the rewrite stays inside the owner/repo it names.
        for u in [
            "https://github.com/o/r/blob/main/a/../b.rs",
            "https://github.com/o/r/blob/main/a/%2e%2e/b.rs",
            "https://github.com/o/r/blob/main/a/%2E%2E/b.rs",
            "https://github.com/o/r/blob/main/a/../../../etc/passwd",
        ] {
            if let Some(r) = rw(u) {
                assert!(
                    r.fetched
                        .as_str()
                        .starts_with("https://raw.githubusercontent.com/o/r/main/"),
                    "{u}"
                );
                assert!(!r.fetched.as_str().to_ascii_lowercase().contains("%2e"));
                assert!(!r.fetched.as_str().contains(".."));
            }
        }
    }

    #[test]
    fn scheme_host_port_and_userinfo_are_closed() {
        let p = format!("document/d/{ID}/edit");
        for u in [
            format!("http://docs.google.com/{p}"),
            format!("https://docs.google.com:8443/{p}"),
            format!("https://user@docs.google.com/{p}"),
            format!("https://user:pw@docs.google.com/{p}"),
            format!("https://www.docs.google.com/{p}"),
            format!("https://docs.google.com.evil.example/{p}"),
            format!("https://evil.example/docs.google.com/{p}"),
            format!("https://drive.google.com/{p}"),
            format!("https://sheets.googleapis.com/{p}"),
            format!("ftp://docs.google.com/{p}"),
            "http://github.com/o/r/blob/main/a.rs".to_string(),
            "https://github.com:444/o/r/blob/main/a.rs".to_string(),
            "https://user@github.com/o/r/blob/main/a.rs".to_string(),
            "https://www.github.com/o/r/blob/main/a.rs".to_string(),
            "https://gist.github.com/o/r/blob/main/a.rs".to_string(),
            "https://raw.githubusercontent.com/o/r/main/a.rs".to_string(),
            "https://api.github.com/repos/o/r/contents/a.rs".to_string(),
        ] {
            assert!(rw(&u).is_none(), "{u}");
        }
    }

    #[test]
    fn output_is_https_on_a_public_host_without_credentials() {
        let inputs = [
            format!("https://docs.google.com/document/d/{ID}/edit"),
            format!("https://docs.google.com/spreadsheets/d/{ID}/edit#gid=5"),
            format!("https://docs.google.com/presentation/d/{ID}/edit"),
            "https://github.com/o/r/blob/main/a.rs".to_string(),
        ];
        for i in inputs {
            let r = rw(&i).expect("rewritten");
            assert_eq!(r.fetched.scheme(), "https");
            assert!(matches!(
                r.fetched.host_str(),
                Some("docs.google.com") | Some("raw.githubusercontent.com")
            ));
            assert!(r.fetched.username().is_empty());
            assert!(r.fetched.password().is_none());
            assert!(r.fetched.port().is_none());
            assert!(r.fetched.fragment().is_none());
            let host = r.fetched.host_str().unwrap();
            assert!(!host.contains("googleapis") && !host.starts_with("api."));
        }
    }
}
