//! Pins the closed error vocabulary of the `/continue` route across the BFF and
//! the SPA, and pins that only the continue handler relays upstream error codes.
//!
//! The vocabulary has three homes that must agree: model-gateway (which says the
//! codes), the BFF's `CONTINUE_RELAYED_ERRORS` (which relays eight of them with
//! their upstream status), and the SPA's `chat-run-continue-contract.ts` (which
//! reads them). A code added in one place and not the others fails silently: the
//! BFF turns an unlisted code into `model_error` and the SPA's guard returns null.
//!
//! Source-scanning, in the style of `tests/sse_relay_no_allowlist.rs`, with the
//! same cross-plane relative-path precedent and the same hard failure when a file
//! moved (a silent skip would make an assertion vacuous exactly when it is needed).
//!
//! Present here:
//!
//! * BFF list = SPA HTTP list, in order;
//! * the BFF list is a subset of model-gateway's `CONTINUE_HTTP_REFUSALS`, with the same
//!   status for every code, and the difference is exactly `continue_incomplete` (the BFF's
//!   own blank check answers first) and `missing_bearer` (a 401, which stays
//!   `unauthorized`);
//! * the SPA's stream-frame tuple = model-gateway's `CONTINUE_FRAME_REFUSALS`, in order;
//! * every `proxy_sse_stream_with_data_plane` caller other than `continue_run` passes `&[]`.
//!
//! The model-gateway source is read across the plane boundary as a TEST only, the way
//! `sse_relay_no_allowlist.rs` reads its `ChatEvent`: nothing in the BFF's production
//! code reads or links it, and a file that moved fails with its path.
//!
//! Whether CI builds the BFF with the Model Plane sources present is UNVERIFIED
//! (the same assumption as `sse_relay_no_allowlist.rs`); the SPA file is inside this
//! app's own checkout.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn gateway_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// Reads a source file with CRLF folded to LF. Hard-fails with the path.
fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| {
            panic!(
                "cannot read {} ({error}). This test pins the closed /continue error \
                 vocabulary; if the file moved, re-point the path, do not delete the assertion.",
                path.display()
            )
        })
        .replace("\r\n", "\n")
}

/// The SPA contract module: `apps/gateway` -> the verevonv3 app root -> `src/`.
fn spa_contract_source() -> String {
    read(&gateway_file(
        "../../src/shared/actions/chat-run-continue-contract.ts",
    ))
}

/// Text between the first `open` after `start_marker` and its matching `close`.
fn delimited<'a>(source: &'a str, start_marker: &str, open: char, close: char) -> &'a str {
    let start = source
        .find(start_marker)
        .unwrap_or_else(|| panic!("`{start_marker}` not found; was it renamed?"));
    let open_at = source[start..]
        .find(open)
        .unwrap_or_else(|| panic!("no `{open}` after `{start_marker}`"))
        + start;
    let mut depth = 0usize;
    for (offset, ch) in source[open_at..].char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return &source[open_at + 1..open_at + offset];
            }
        }
    }
    panic!("unbalanced `{open}` after `{start_marker}`");
}

/// Quoted string literals of a fragment, in order. `quote` is `"` for Rust source
/// and `'` for the SPA's TypeScript.
fn string_literals(fragment: &str, quote: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = fragment;
    while let Some(open) = rest.find(quote) {
        let after = &rest[open + 1..];
        let close = after.find(quote).expect("unterminated string literal");
        out.push(after[..close].to_owned());
        rest = &after[close + 1..];
    }
    out
}

/// The BFF's (code, status) pairs, from the source of `CONTINUE_RELAYED_ERRORS`.
fn bff_relayed() -> Vec<(String, u16)> {
    let source = read(&gateway_file("src/domains/orchestration.rs"));
    let marker = source
        .find("pub(crate) const CONTINUE_RELAYED_ERRORS")
        .expect("CONTINUE_RELAYED_ERRORS not found in src/domains/orchestration.rs");
    // The list literal is the bracket after `= &`; the first bracket belongs to the
    // type `&[(&str, u16, &str)]`.
    let literal = &source[source[marker..].find("= &[").expect("the list literal") + marker + 3..];
    let list = delimited(literal, "[", '[', ']');
    let mut pairs = Vec::new();
    let mut rest = list;
    // Each entry is `( "code", status, "message" )`, possibly wrapped over lines.
    while let Some(open) = rest.find('(') {
        let entry = delimited(&rest[open..], "(", '(', ')');
        let code = string_literals(entry, '"')
            .into_iter()
            .next()
            .expect("an entry names its code first");
        let status: u16 = entry
            .split(',')
            .map(str::trim)
            .find_map(|part| part.parse().ok())
            .expect("an entry gives its status as an integer");
        pairs.push((code, status));
        rest = &rest[open + 1 + entry.len() + 1..];
    }
    pairs
}

/// The SPA's (code, status) pairs, from `CHAT_RUN_CONTINUE_HTTP_REFUSALS`.
fn spa_http_refusals() -> Vec<(String, u16)> {
    let source = spa_contract_source();
    let list = delimited(
        &source,
        "export const CHAT_RUN_CONTINUE_HTTP_REFUSALS",
        '[',
        ']',
    );
    let mut pairs = Vec::new();
    for line in list.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let code = string_literals(line, '\'')
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no code on `{line}`"));
        let status_at = line
            .find("status:")
            .unwrap_or_else(|| panic!("no status on `{line}`"));
        let status: u16 = line[status_at + "status:".len()..]
            .trim()
            .trim_end_matches(|c: char| !c.is_ascii_digit())
            .parse()
            .unwrap_or_else(|_| panic!("status is not an integer on `{line}`"));
        pairs.push((code, status));
    }
    pairs
}

#[test]
fn the_bff_relay_list_is_the_spa_http_list() {
    let bff = bff_relayed();
    let spa = spa_http_refusals();
    assert_eq!(
        bff.len(),
        8,
        "the BFF relays exactly eight closed codes; parsed {bff:?}"
    );
    assert_eq!(
        bff, spa,
        "the BFF's CONTINUE_RELAYED_ERRORS and the SPA's CHAT_RUN_CONTINUE_HTTP_REFUSALS \
         must list the same (code, status) pairs in the same order. A code only one side \
         knows is `model_error` to the browser or null to the guard."
    );
}

/// The model-gateway source that says the closed vocabulary. `apps/gateway` -> the
/// verevonv3 app root -> `Frontend Plane` -> `apps/` -> the Model Plane's gateway. A test
/// reads it; production code never does.
fn model_gateway_sse_source() -> String {
    read(&gateway_file(
        "../../../../Model Plane/rust/services/model-gateway/src/sse.rs",
    ))
}

/// The text inside the list literal of `const NAME ... = &[ .. ];` in `source`, with
/// whole-line comments dropped (a comment may quote a code without listing it).
fn gateway_const_list(source: &str, name: &str) -> String {
    let at = source
        .find(&format!("const {name}"))
        .unwrap_or_else(|| panic!("`const {name}` not found in model-gateway sse.rs; was it renamed?"));
    // The list literal is the bracket after `= &`; the first bracket belongs to the type.
    let literal = &source[source[at..]
        .find("= &[")
        .unwrap_or_else(|| panic!("no list literal after `const {name}`"))
        + at
        + 3..];
    delimited(literal, "[", '[', ']')
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The gateway's (code, status) pairs from `CONTINUE_HTTP_REFUSALS`.
fn gateway_http_refusals() -> Vec<(String, u16)> {
    let list = gateway_const_list(&model_gateway_sse_source(), "CONTINUE_HTTP_REFUSALS");
    let mut pairs = Vec::new();
    let mut rest = list.as_str();
    // Each entry is `( "code", StatusCode::NAME )`.
    while let Some(open) = rest.find('(') {
        let entry = delimited(&rest[open..], "(", '(', ')');
        let code = string_literals(entry, '"')
            .into_iter()
            .next()
            .expect("an entry names its code first");
        let name = entry
            .split("StatusCode::")
            .nth(1)
            .unwrap_or_else(|| panic!("no StatusCode on `{entry}`"))
            .trim();
        let status = match name {
            "BAD_REQUEST" => 400,
            "UNAUTHORIZED" => 401,
            "NOT_FOUND" => 404,
            "CONFLICT" => 409,
            "SERVICE_UNAVAILABLE" => 503,
            other => panic!(
                "StatusCode::{other} is not in this test's table: add its number, do not skip the entry"
            ),
        };
        pairs.push((code, status));
        rest = &rest[open + 1 + entry.len() + 1..];
    }
    pairs
}

/// The SPA's stream-frame refusal codes, from `CHAT_RUN_CONTINUE_FRAME_REFUSALS`.
fn spa_frame_refusals() -> Vec<String> {
    let source = spa_contract_source();
    let list = delimited(
        &source,
        "export const CHAT_RUN_CONTINUE_FRAME_REFUSALS",
        '[',
        ']',
    );
    let code_lines = list
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    string_literals(&code_lines, '\'')
}

fn no_duplicates<T: Ord + Clone + std::fmt::Debug>(what: &str, items: &[T]) {
    let unique: BTreeSet<&T> = items.iter().collect();
    assert_eq!(
        unique.len(),
        items.len(),
        "{what} lists a code twice: {items:?}"
    );
}

/// The BFF relays a subset of the gateway's HTTP refusals, under the status the gateway
/// sends them with, and what it leaves out is exactly what it must leave out.
#[test]
fn the_bff_relay_list_is_the_gateways_http_list_less_the_two_codes_it_never_relays() {
    let gateway = gateway_http_refusals();
    assert_eq!(
        gateway.len(),
        10,
        "model-gateway's CONTINUE_HTTP_REFUSALS has ten rows; parsed {gateway:?}"
    );
    no_duplicates(
        "model-gateway's CONTINUE_HTTP_REFUSALS",
        &gateway.iter().map(|(code, _)| code.clone()).collect::<Vec<_>>(),
    );
    let bff = bff_relayed();
    for (code, status) in &bff {
        match gateway.iter().find(|(known, _)| known == code) {
            None => panic!(
                "the BFF relays `{code}`, which model-gateway's CONTINUE_HTTP_REFUSALS does not \
                 list: the gateway never sends it, so the entry is dead or the gateway lost a code"
            ),
            Some((_, upstream)) => assert_eq!(
                upstream, status,
                "the BFF relays `{code}` under {status} but the gateway sends it with {upstream}"
            ),
        }
    }
    let relayed: BTreeSet<&str> = bff.iter().map(|(code, _)| code.as_str()).collect();
    let left_out: BTreeSet<&str> = gateway
        .iter()
        .map(|(code, _)| code.as_str())
        .filter(|code| !relayed.contains(code))
        .collect();
    assert_eq!(
        left_out,
        BTreeSet::from(["continue_incomplete", "missing_bearer"]),
        "the BFF leaves out `continue_incomplete` (its own blank check answers first) and \
         `missing_bearer` (a 401 stays `unauthorized`) and nothing else: a gateway code the BFF \
         does not relay reaches the browser as `model_error`"
    );
}

/// The SPA reads the frames the gateway writes: the tuple is the gateway's list, so a
/// token the gateway can put in an `error` frame is a token the guard recognises.
#[test]
fn the_spa_frame_tuple_is_the_gateways_frame_list() {
    let gateway = gateway_const_list(&model_gateway_sse_source(), "CONTINUE_FRAME_REFUSALS");
    let gateway = string_literals(&gateway, '"');
    let spa = spa_frame_refusals();
    assert_eq!(
        gateway.len(),
        22,
        "model-gateway's CONTINUE_FRAME_REFUSALS has 22 tokens; parsed {gateway:?}"
    );
    no_duplicates("model-gateway's CONTINUE_FRAME_REFUSALS", &gateway);
    no_duplicates("the SPA's CHAT_RUN_CONTINUE_FRAME_REFUSALS", &spa);
    assert_eq!(
        spa, gateway,
        "the SPA's CHAT_RUN_CONTINUE_FRAME_REFUSALS and model-gateway's CONTINUE_FRAME_REFUSALS \
         must list the same tokens in the same order. A token only the gateway knows is a frame \
         the SPA's guards read as null."
    );
}

/// Every call of the relay other than `continue_run`'s passes the empty list, so no
/// other route's error body is ever read or relayed by code.
#[test]
fn only_the_continue_handler_relays_upstream_error_codes() {
    /// Every call of the relay in `source`: its last argument, and the text before it.
    fn calls(source: &str) -> Vec<(String, String)> {
        // Whole-line comments never carry a call.
        let source: String = source
            .lines()
            .map(|line| {
                if line.trim_start().starts_with("//") {
                    ""
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let needle = "proxy_sse_stream_with_data_plane(";
        let mut found = Vec::new();
        let mut from = 0;
        while let Some(at) = source[from..].find(needle) {
            let at = at + from;
            from = at + needle.len();
            let before = &source[..at];
            if before.trim_end().ends_with("fn") {
                continue; // the definition
            }
            let args = delimited(&source[at..], needle, '(', ')');
            // Split at top-level commas and take the last non-empty argument.
            let mut depth = 0i32;
            let mut last = String::new();
            let mut current = String::new();
            for ch in args.chars() {
                match ch {
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' => depth -= 1,
                    _ => {}
                }
                if ch == ',' && depth == 0 {
                    last = current.trim().to_owned();
                    current.clear();
                } else {
                    current.push(ch);
                }
            }
            if !current.trim().is_empty() {
                last = current.trim().to_owned();
            }
            found.push((last, before.to_owned()));
        }
        found
    }

    let mut files = Vec::new();
    let mut stack = vec![gateway_file("src")];
    while let Some(dir) = stack.pop() {
        for entry in
            fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot list {} ({e})", dir.display()))
        {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    assert!(
        files.len() > 20,
        "found only {} source files; the scan is not looking at the crate",
        files.len()
    );

    let mut relaying = Vec::new();
    let mut total = 0usize;
    for path in files {
        let normalized = path.to_string_lossy().replace('\\', "/");
        let source = read(&path);
        // upstream.rs holds the relay itself and the unit tests of its error
        // branch; only the production text (before the first test module) is a
        // caller scan, and its two wrappers must pass the empty list.
        let scanned = if normalized.ends_with("src/upstream.rs") {
            let end = source
                .find("#[cfg(test)]")
                .expect("upstream.rs has test modules");
            &source[..end]
        } else {
            &source[..]
        };
        for (last, before) in calls(scanned) {
            total += 1;
            if last != "&[]" {
                relaying.push((normalized.clone(), last, before));
            }
        }
    }
    assert!(
        total >= 5,
        "found only {total} relay callers; the scan is vacuous"
    );

    // The one non-empty caller is `continue_run`, passing CONTINUE_RELAYED_ERRORS.
    assert_eq!(
        relaying.len(),
        1,
        "exactly one caller may pass a relay list; found {:?}",
        relaying
            .iter()
            .map(|(file, last, _)| (file, last))
            .collect::<Vec<_>>()
    );
    let (file, last, before) = &relaying[0];
    assert!(file.ends_with("src/domains/orchestration.rs"), "{file}");
    assert_eq!(last, "CONTINUE_RELAYED_ERRORS");
    let handler = before
        .rfind("pub(crate) async fn ")
        .map(|at| &before[at..])
        .expect("an enclosing function");
    assert!(
        handler.starts_with("pub(crate) async fn continue_run("),
        "the relay list is passed from `{}`, not from continue_run",
        handler.lines().next().unwrap_or("")
    );
}
