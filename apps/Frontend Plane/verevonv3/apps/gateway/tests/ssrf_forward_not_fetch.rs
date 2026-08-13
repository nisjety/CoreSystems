//! Regression test for the gateway's "forward, don't fetch" SSRF invariant.
//!
//! `src/public_url.rs`'s `normalize_public_http_url` is a **pre-filter**: it
//! rejects obviously-bad input by string/literal-IP inspection, but it never
//! resolves DNS, so it cannot see a hostname that *resolves* to a
//! private/loopback/metadata address (DNS rebinding, a wildcard DNS record,
//! a CNAME to an internal name, ...). That gap is safe today only because
//! every caller hands the normalized URL to Quarry-v2 as request-body/query
//! *content*, for Quarry to fetch with its own DNS-pinning guard
//! (`Quarry-v2/crates/quarry-runtime/src/dns_guard.rs`'s `ResolvedTarget` /
//! `PinnedDnsResolver` / `TlsDnsPin`), instead of the gateway dialing the
//! normalized URL itself.
//!
//! That invariant is currently *incidental* — nothing stops a future call
//! site from doing `state.client.get(&normalized_url).send()` directly. This
//! test pins it down: it scans every gateway source file for a direct
//! outbound-dial call (the shared `state.client` HTTP methods, and the
//! generic "dial an arbitrary URL and relay the response" plumbing helpers
//! like `src/upstream.rs`'s `proxy_bearer_json`/`proxy_sse_stream`) and
//! asserts that the dial target is anchored to one of `AppState`'s fixed,
//! operator-configured `..._url` fields (`src/config.rs` — `quarry_edge_url`,
//! `auth_core_url`, ...) rather than built straight from caller/request
//! input.
//!
//! This is a source-scanning test, not a full data-flow prover: it looks for
//! `state.<ident ending in _url>` textually within each dial call's
//! enclosing function (or, for a generic forwarding helper that just
//! receives a pre-built URL string as a parameter, at the caller's
//! responsibility instead — see `takes_external_url_param` below). It cannot
//! see through several layers of indirection, and a sufficiently contrived
//! rename could dodge it. That is an accepted tradeoff for a cheap,
//! dependency-free tripwire — see QM's own `test/egress-proxy-config.test.ts`,
//! which takes the same static-assertion approach for an analogous invariant.
//!
//! ## If this test fails
//!
//! Do not delete or loosen it. It means a new (or changed) call site dials
//! an outbound HTTP request whose target this scan could not prove comes
//! from a fixed `AppState` base URL. Two honest fixes:
//!   1. The dial is fine but doesn't match the scan's pattern (e.g. a new
//!      naming convention) — widen `has_configured_url_anchor` or
//!      `DIAL_MARKERS` to recognize it, and add a comment saying why.
//!   2. The dial is a real regression — a caller-supplied or normalized URL
//!      is being fetched directly instead of forwarded. Route it through a
//!      plane client anchored to a fixed `AppState` `..._url` field instead
//!      (the way every existing call site does), or forward it to Quarry-v2
//!      for Quarry to fetch with its DNS-pinning guard.

use std::fs;
use std::path::{Path, PathBuf};

/// Direct outbound-dial call shapes this test cares about: the gateway's
/// shared reqwest client, and the generic "dial an arbitrary URL and relay
/// the response" plumbing helpers (`src/upstream.rs`'s `proxy_bearer_json`
/// / `proxy_sse_stream`, and the domain-local wrappers built on them). The
/// six `state.client.*` verbs cover every `reqwest::Client` builder method;
/// only `get`/`post`/`request` are used today, the rest are included so a
/// future `put`/`patch`/`delete` call site is checked too instead of
/// silently skipped.
const DIAL_MARKERS: &[&str] = &[
    "state.client.get(",
    "state.client.post(",
    "state.client.put(",
    "state.client.patch(",
    "state.client.delete(",
    "state.client.request(",
    "proxy_bearer_json(",
    "proxy_sse_stream(",
];

/// Every internal-service base URL on `AppState` (`src/config.rs`) is named
/// `<service>_url` and is set once, at startup, from an operator-controlled
/// env var with an internal-hostname default (`quarry_edge_url`,
/// `auth_core_url`, `user_core_url`, ...) — never from request input. A dial
/// whose target is built from `state.<something>_url` is therefore anchored
/// to fixed, trusted configuration; a dial built from anything else (a bare
/// local variable, a caller-supplied field) is not.
fn has_configured_url_anchor(window: &str) -> bool {
    for (idx, _) in window.match_indices("state.") {
        let rest = &window[idx + "state.".len()..];
        let ident_len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        if rest[..ident_len].ends_with("_url") {
            return true;
        }
    }
    false
}

fn gateway_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read directory {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// True when `line`, trimmed, looks like a function signature line (`fn
/// foo(` or `...async fn foo(`). Used both to bound the backward walk to
/// "this function" and to recognize (and skip) a `fn` line that happens to
/// textually contain a dial marker, e.g. `pub(crate) async fn
/// proxy_bearer_json(` contains the literal substring `proxy_bearer_json(`.
fn is_fn_signature_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("fn ") || trimmed.contains(" fn ")
}

/// Walk back from `call_idx` to the nearest enclosing `fn` line, so the
/// anchor search below covers "this function" rather than unrelated code
/// earlier in a long file (and never crosses into a different function).
fn enclosing_fn_start(lines: &[&str], call_idx: usize) -> usize {
    let mut i = call_idx;
    loop {
        if is_fn_signature_line(lines[i]) {
            return i;
        }
        if i == 0 {
            return 0;
        }
        i -= 1;
    }
}

/// Build the text this test inspects for a dial call at `call_idx` whose
/// argument list starts with `marker`: everything from the enclosing
/// function's `fn` line through the call line (covering a `let url =
/// format!(...)` binding written just above the call, the common shape in
/// this crate), PLUS the rest of the call expression itself, tracked by
/// paren balance from the marker's own opening `(` forward across
/// subsequent lines. The forward extension matters because rustfmt commonly
/// puts one argument per line, so a call like
/// `proxy_bearer_json(state, Method::GET, &format!("{}{}", state.foo_url,
/// path), ...)` has its anchor on a line *after* the marker, not before it.
fn call_expression_window(
    lines: &[&str],
    fn_start: usize,
    call_idx: usize,
    marker: &str,
) -> String {
    let mut text = lines[fn_start..=call_idx].join("\n");

    let Some(marker_pos) = lines[call_idx].find(marker) else {
        return text;
    };
    let mut depth = 0i32;
    let mut seen_open = false;
    for ch in lines[call_idx][marker_pos..].chars() {
        match ch {
            '(' => {
                depth += 1;
                seen_open = true;
            }
            ')' => depth -= 1,
            _ => {}
        }
    }

    let mut i = call_idx + 1;
    while seen_open && depth > 0 && i < lines.len() && i - call_idx < 60 {
        text.push('\n');
        text.push_str(lines[i]);
        for ch in lines[i].chars() {
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
        }
        i += 1;
    }
    text
}

/// True when the enclosing function's own signature declares a parameter
/// named `<...>url` of type `&str`/`&String` — this crate's shape for a
/// generic "take an already-built URL, dial it" forwarding helper
/// (`upstream::proxy_bearer_json`, `upstream::proxy_auth_with_headers`,
/// `integrations::shared::proxy_sse_for_user`, ...). Such a function cannot
/// build its own anchor: it never constructs the URL, it only relays one
/// its caller already built. The anchor discipline for these is enforced at
/// their call sites instead, which this scan also visits (every call site
/// of a marker in `DIAL_MARKERS`, including calls to these helpers
/// themselves).
fn takes_external_url_param(lines: &[&str], fn_start: usize) -> bool {
    let mut end = fn_start;
    while end < lines.len() && !lines[end].contains('{') {
        end += 1;
        if end - fn_start > 40 {
            break;
        }
    }
    let end = end.min(lines.len().saturating_sub(1));
    let signature: String = lines[fn_start..=end]
        .iter()
        .flat_map(|line| line.chars())
        .filter(|c| !c.is_whitespace())
        .collect();

    for type_pat in [":&str", ":&String"] {
        let mut search_from = 0;
        while let Some(rel) = signature[search_from..].find(type_pat) {
            let pos = search_from + rel;
            let bytes = signature.as_bytes();
            let mut name_start = pos;
            while name_start > 0
                && (bytes[name_start - 1].is_ascii_alphanumeric() || bytes[name_start - 1] == b'_')
            {
                name_start -= 1;
            }
            if signature[name_start..pos].ends_with("url") {
                return true;
            }
            search_from = pos + type_pat.len();
        }
    }
    false
}

/// Inclusive `(start, end)` line-index ranges covered by a `#[cfg(test)]
/// mod ... { ... }` block, found by brace-matching forward from the `mod`
/// line. This crate's unit tests (see `upstream.rs`'s own `mod tests`)
/// legitimately call the generic proxy helpers with synthetic URLs —
/// wiremock URIs, deliberately-unreachable loopback literals to exercise an
/// error path, ... — that have no `state.*_url` anchor because the anchor
/// isn't what's under test there. That is not the SSRF regression this test
/// looks for, so those ranges are excluded from the scan entirely.
fn test_module_ranges(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() != "#[cfg(test)]" {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < lines.len() && j - i < 5 && !lines[j].contains("mod ") {
            j += 1;
        }
        if j >= lines.len() || !lines[j].contains("mod ") {
            i += 1;
            continue;
        }
        let mut depth = 0i32;
        let mut started = false;
        let mut k = j;
        while k < lines.len() {
            for ch in lines[k].chars() {
                match ch {
                    '{' => {
                        depth += 1;
                        started = true;
                    }
                    '}' => depth -= 1,
                    _ => {}
                }
            }
            if started && depth <= 0 {
                break;
            }
            k += 1;
        }
        let end = k.min(lines.len() - 1);
        ranges.push((i, end));
        i = end + 1;
    }
    ranges
}

#[test]
fn every_direct_dial_is_anchored_to_a_configured_base_url_not_caller_input() {
    let src_dir = gateway_src_dir();
    let mut files = Vec::new();
    collect_rs_files(&src_dir, &mut files);
    assert!(
        files.len() > 20,
        "expected to scan dozens of gateway source files under {}, only found {} — \
         this test's path resolution is almost certainly broken (fix the scan, \
         don't assume the invariant is fine just because nothing was flagged)",
        src_dir.display(),
        files.len()
    );

    // Canary: if every gateway call site of `normalize_public_http_url` were
    // ever removed (or this constant typo'd), the scan below would still
    // "pass" vacuously. Confirm the guard is still actually in real use.
    let guarded_file_hits = files
        .iter()
        .filter(|path| {
            fs::read_to_string(path)
                .map(|contents| contents.contains("normalize_public_http_url("))
                .unwrap_or(false)
        })
        .count();
    assert!(
        guarded_file_hits >= 5,
        "expected several gateway source files to call `normalize_public_http_url` \
         (monitoring.rs, browser.rs, knowledge/quarry.rs, knowledge/products.rs, \
         ingestions/shared.rs, ...); only found {guarded_file_hits}. Either the SSRF \
         pre-filter was removed from real call sites, or this test's scan is broken — \
         investigate before assuming the invariant holds."
    );

    let mut violations = Vec::new();
    for path in &files {
        let contents = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let lines: Vec<&str> = contents.lines().collect();
        let test_ranges = test_module_ranges(&lines);

        for (idx, line) in lines.iter().enumerate() {
            if test_ranges
                .iter()
                .any(|&(start, end)| idx >= start && idx <= end)
            {
                continue;
            }
            if is_fn_signature_line(line) {
                // A `fn` line that happens to textually contain a marker
                // (e.g. `async fn proxy_bearer_json(`) is a definition, not
                // a call — its own body is checked separately, line by
                // line, like everything else.
                continue;
            }
            let Some(marker) = DIAL_MARKERS.iter().find(|marker| line.contains(*marker)) else {
                continue;
            };

            let fn_start = enclosing_fn_start(&lines, idx);
            if takes_external_url_param(&lines, fn_start) {
                // This function only relays a URL string it received as a
                // parameter — it never builds one, so it cannot carry its
                // own anchor. Its callers are checked instead (they also
                // match a DIAL_MARKERS entry, since calling one of these
                // helpers IS one of the marker patterns).
                continue;
            }

            let window = call_expression_window(&lines, fn_start, idx, marker);
            if !has_configured_url_anchor(&window) {
                violations.push(format!(
                    "{}:{}: `{}` — no `state.<..._url>` anchor found in its enclosing \
                     function (scanned from line {}):\n    {}",
                    path.display(),
                    idx + 1,
                    marker,
                    fn_start + 1,
                    line.trim(),
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "\n\n\
         SSRF regression: found a direct outbound HTTP dial that this scan could not \
         prove is anchored to one of AppState's fixed, operator-configured `..._url` \
         fields (src/config.rs).\n\n\
         WHY THIS MATTERS: `public_url::normalize_public_http_url` (src/public_url.rs) \
         is a pre-filter only — it never resolves DNS, so it cannot catch DNS \
         rebinding. It is safe today solely because every caller forwards the \
         normalized URL to Quarry-v2 as request content, letting Quarry's own \
         DNS-pinning guard (dns_guard.rs) do the real vetting. A direct dial of a \
         caller-supplied or normalized URL from this crate would make this weak \
         pre-filter the ENTIRE defense.\n\n\
         WHAT TO DO INSTEAD: route the request through a plane client the way every \
         existing call site does — build the dial's URL from a fixed `state.*_url` \
         field (e.g. `state.quarry_edge_url`) and put the caller's URL in the \
         JSON body/query as data, never as the dial target itself.\n\n\
         Violations:\n{}",
        violations.join("\n\n")
    );
}
