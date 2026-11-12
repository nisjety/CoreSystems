//! Pins the closed error vocabulary of the `/continue` route across the BFF and
//! the SPA, and pins that only the continue handler relays upstream error codes.
//!
//! The vocabulary has three homes that must agree: model-gateway (which says the
//! codes), the BFF's `CONTINUE_RELAYED_ERRORS` (which relays fourteen of them with
//! their upstream status: the approval and park forms' nine and the question form's five), and the SPA's `chat-run-continue-contract.ts` (which
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
//! * the SPA's stream-frame tuple = model-gateway's `CONTINUE_FRAME_REFUSALS`, in order, then
//!   the tokens only the question form's `CONTINUE_QUESTION_FRAME_REFUSALS` adds (WS14 S5);
//! * the SPA's run-end list (`CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES`, the three frames that
//!   follow the executor's answer and are final after a pre-consume) = model-gateway's
//!   `CONTINUE_SPENT_RUN_END_FRAMES`, and neither refusal list holds them;
//! * the SPA's subscription list (`CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES`, the seven
//!   typed ChatGPT-subscription failures a spent run can end on, final after a pre-consume)
//!   = model-gateway's `CONTINUE_SPENT_SUBSCRIPTION_FRAMES`, disjoint from the run-end list
//!   and from both refusal lists;
//! * every relay caller (`proxy_sse_stream_with_data_plane` and `proxy_sse_stream_with_browser`) passes
//!   `&[]`, except `continue_run`'s two calls (CONTINUE_RELAYED_ERRORS), the space live route's one call
//!   (its own named RELAYED list) and the forwarder in upstream.rs, which passes on its caller's list;
//! * the frame of a failed PARK continue (WS03 S8-A): model-gateway's `park_refusal_event`
//!   writes `park_continue: true` and never `approval_spent`, the SPA's park reader reads that
//!   marker, the tokens such a frame carries are in the closed frame lists, and the BFF reads
//!   and writes neither marker.
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
        14,
        "the BFF relays exactly fourteen closed codes; parsed {bff:?}"
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

/// The gateway's (code, status) pairs from `CONTINUE_QUESTION_HTTP_REFUSALS` (WS14 S5), the
/// question form's own list.
fn gateway_question_http_refusals() -> Vec<(String, u16)> {
    let list = gateway_const_list(&model_gateway_sse_source(), "CONTINUE_QUESTION_HTTP_REFUSALS");
    let mut pairs = Vec::new();
    let mut rest = list.as_str();
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
            "GONE" => 410,
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
    // The codes the gateway sends outside CONTINUE_HTTP_REFUSALS, each pinned to the source that
    // writes it: the question form's own list (WS14 S5), the answer route's two typed refusals
    // (the question form relays them from the same Session Core statuses), and the org pause.
    let mut sent = gateway.clone();
    sent.extend(gateway_question_http_refusals());
    let routes = read(&gateway_file(
        "../../../../Model Plane/rust/services/model-gateway/src/http_routes.rs",
    ));
    assert!(
        routes.contains("StatusCode::CONFLICT, \"question_already_answered\""),
        "http_routes.rs no longer sends question_already_answered with 409"
    );
    assert!(
        routes.contains("StatusCode::UNPROCESSABLE_ENTITY,\n                \"invalid_answer\""),
        "http_routes.rs no longer sends invalid_answer with 422"
    );
    sent.push(("question_already_answered".to_owned(), 409));
    sent.push(("invalid_answer".to_owned(), 422));
    let policy = read(&gateway_file(
        "../../../../Model Plane/rust/services/model-gateway/src/org_agent_policy.rs",
    ));
    assert!(
        policy.contains("pub const AGENTS_PAUSED_CODE: &str = \"org_agents_paused\";"),
        "org_agent_policy.rs no longer names org_agents_paused"
    );
    sent.push(("org_agents_paused".to_owned(), 403));
    for (code, status) in &bff {
        match sent.iter().find(|(known, _)| known == code) {
            None => panic!(
                "the BFF relays `{code}`, which model-gateway's refusal lists do not list: the \
                 gateway never sends it, so the entry is dead or the gateway lost a code"
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
        26,
        "model-gateway's CONTINUE_FRAME_REFUSALS has 26 tokens; parsed {gateway:?}"
    );
    no_duplicates("model-gateway's CONTINUE_FRAME_REFUSALS", &gateway);
    no_duplicates("the SPA's CHAT_RUN_CONTINUE_FRAME_REFUSALS", &spa);
    // The question form (WS14 S5) writes frames from its own list; the tokens it adds to the base
    // list are the SPA's too, after the base tokens and in the question list's order.
    let question = string_literals(
        &gateway_const_list(&model_gateway_sse_source(), "CONTINUE_QUESTION_FRAME_REFUSALS"),
        '"',
    );
    no_duplicates("model-gateway's CONTINUE_QUESTION_FRAME_REFUSALS", &question);
    let question_only: Vec<String> = question
        .iter()
        .filter(|token| !gateway.contains(token))
        .cloned()
        .collect();
    assert_eq!(
        question_only,
        [
            "question_answer_released",
            "question_resume_unavailable",
            "question_not_answered",
            "question_expired",
            "question_not_found",
            "question_not_resumable",
        ],
        "the question form adds exactly six frame tokens to the base list; parsed {question_only:?}"
    );
    // The question form's authority refusals are base tokens (the executor's refusals of the same
    // check, which the approval also carries), so they sit in the base list and the SPA's tuple, and
    // the question list must still name them: `question_refusal_event` debug-asserts that.
    for token in [
        "authority_widened",
        "authority_space_mismatch",
        "authority_route_mismatch",
        "continue_space_scope_unsupported",
    ] {
        assert!(
            question.iter().any(|known| known == token),
            "the question form refuses with `{token}`, which CONTINUE_QUESTION_FRAME_REFUSALS does not list"
        );
    }
    let expected: Vec<String> = gateway.iter().cloned().chain(question_only).collect();
    assert_eq!(
        spa, expected,
        "the SPA's CHAT_RUN_CONTINUE_FRAME_REFUSALS must be model-gateway's \
         CONTINUE_FRAME_REFUSALS, in order, followed by the tokens only \
         CONTINUE_QUESTION_FRAME_REFUSALS adds. A token only the gateway knows is a frame the \
         SPA's guards read as null."
    );
}

/// The SPA's run-end frame codes, from `CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES`.
fn spa_run_end_frames() -> Vec<String> {
    let source = spa_contract_source();
    let list = delimited(
        &source,
        "export const CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES",
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

/// The frames that follow the executor's ANSWER (the run ended failed, in a status the gateway
/// does not know, or completed with no answer to read) are final and say `approval_spent`
/// after a gateway pre-consume. The SPA reads them only with the flag
/// (`chatRunContinueSpentRunEndOf`), so the SPA's list is the gateway's list: a code only the
/// gateway makes final is a frame the SPA would read as an ordinary retryable failure of a click
/// that is gone, and a code only the SPA lists is one the gateway never sends final. They are
/// NOT refusal-vocabulary codes: neither list may hold them (an unspent continuation's frame of
/// this code is the committed one, read by nobody here).
#[test]
fn the_spa_run_end_frames_are_the_gateways_and_stay_out_of_the_refusal_lists() {
    let gateway = string_literals(
        &gateway_const_list(&model_gateway_sse_source(), "CONTINUE_SPENT_RUN_END_FRAMES"),
        '"',
    );
    let spa = spa_run_end_frames();
    assert_eq!(
        gateway,
        [
            "agent_run_failed",
            "agent_run_state_unknown",
            "agent_run_output_unavailable"
        ],
        "model-gateway's CONTINUE_SPENT_RUN_END_FRAMES is the closed trio; parsed {gateway:?}"
    );
    assert_eq!(
        spa, gateway,
        "the SPA's CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES and model-gateway's \
         CONTINUE_SPENT_RUN_END_FRAMES must list the same codes in the same order"
    );
    let gateway_refusals = string_literals(
        &gateway_const_list(&model_gateway_sse_source(), "CONTINUE_FRAME_REFUSALS"),
        '"',
    );
    let spa_refusals = spa_frame_refusals();
    for code in &gateway {
        assert!(
            !gateway_refusals.contains(code) && !spa_refusals.contains(code),
            "`{code}` is a run-end frame, not a refusal: it would be read as an executor refusal \
         (spend unknown) when the gateway did not flag it"
        );
    }
}

/// The SPA's typed subscription failure codes, from `CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES`.
fn spa_subscription_frames() -> Vec<String> {
    let source = spa_contract_source();
    let list = delimited(
        &source,
        "export const CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES",
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

/// A run pinned to the ChatGPT subscription that ended on a typed refusal says why (a spent usage
/// limit, an expired sign-in) in a frame of its own code. After a gateway pre-consume that frame
/// keeps its code and text and becomes final (`retryable: false`, the run named,
/// `approval_spent: true`). The SPA reads it through the closed list
/// (`chatRunContinueSpentSubscriptionFailureOf`), not by shape: so the list is the gateway's,
/// code for code and in order, and it shares no code with the run-end list or either refusal
/// list (one guard per vocabulary).
#[test]
fn the_spa_subscription_frames_are_the_gateways_and_stay_out_of_every_other_list() {
    let gateway = string_literals(
        &gateway_const_list(
            &model_gateway_sse_source(),
            "CONTINUE_SPENT_SUBSCRIPTION_FRAMES",
        ),
        '"',
    );
    let spa = spa_subscription_frames();
    assert_eq!(
        gateway.len(),
        7,
        "model-gateway's CONTINUE_SPENT_SUBSCRIPTION_FRAMES has seven codes; parsed {gateway:?}"
    );
    no_duplicates("model-gateway's CONTINUE_SPENT_SUBSCRIPTION_FRAMES", &gateway);
    no_duplicates("the SPA's CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES", &spa);
    assert_eq!(
        spa, gateway,
        "the SPA's CHAT_RUN_CONTINUE_SPENT_SUBSCRIPTION_FRAMES and model-gateway's \
         CONTINUE_SPENT_SUBSCRIPTION_FRAMES must list the same codes in the same order: a code \
         only the gateway makes final is a spent run the SPA would not recognise, and a code \
         only the SPA lists is one the gateway never sends final"
    );
    let others: Vec<(&str, Vec<String>)> = vec![
        (
            "model-gateway's CONTINUE_FRAME_REFUSALS",
            string_literals(
                &gateway_const_list(&model_gateway_sse_source(), "CONTINUE_FRAME_REFUSALS"),
                '"',
            ),
        ),
        ("the SPA's CHAT_RUN_CONTINUE_FRAME_REFUSALS", spa_frame_refusals()),
        (
            "model-gateway's CONTINUE_SPENT_RUN_END_FRAMES",
            string_literals(
                &gateway_const_list(&model_gateway_sse_source(), "CONTINUE_SPENT_RUN_END_FRAMES"),
                '"',
            ),
        ),
        ("the SPA's CHAT_RUN_CONTINUE_SPENT_RUN_END_FRAMES", spa_run_end_frames()),
    ];
    for code in &gateway {
        for (name, list) in &others {
            assert!(
                !list.contains(code),
                "`{code}` is a typed subscription failure, and {name} lists it too: a frame must be \
         read by one guard"
            );
        }
    }
}

/// Every relay call that passes a non-empty list is one of the named ones, and no other route's error
/// body is ever read or relayed by code: `continue_run`'s two calls (CONTINUE_RELAYED_ERRORS), the space
/// live route's call (its own RELAYED list, the second and separately named relay) and upstream.rs's
/// forwarder of the list its caller gave it.
#[test]
fn only_the_continue_handler_relays_upstream_error_codes() {
    /// Every call of the relay `needle` (`name(`) in `source`: its last argument, and the text before
    /// it. A name inside a string literal is not a call (a test's `matches("name(")` quotes it).
    fn calls(source: &str, needle: &str) -> Vec<(String, String)> {
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
        let mut found = Vec::new();
        let mut from = 0;
        while let Some(at) = source[from..].find(needle) {
            let at = at + from;
            from = at + needle.len();
            let before = &source[..at];
            if before.trim_end().ends_with("fn") || before.ends_with('"') {
                continue; // the definition, or the name quoted in a string
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

    /// The name of the innermost `fn` before `before` ends: the last `fn <name>` in it, as a whole word.
    fn enclosing_fn(before: &str) -> String {
        let at = before
            .rmatch_indices("fn ")
            .map(|(at, _)| at)
            .find(|&at| match before[..at].chars().next_back() {
                None => true,
                Some(c) => !(c.is_alphanumeric() || c == '_'),
            })
            .expect("a relay call sits inside a function");
        before[at + "fn ".len()..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect()
    }

    // (path under src/, last argument, enclosing fn) of every call that passes a non-empty list.
    let mut relaying: Vec<(String, String, String)> = Vec::new();
    let mut total = 0usize;
    let src_root = gateway_file("src");
    for path in files {
        let normalized = path.to_string_lossy().replace('\\', "/");
        let relative = path
            .strip_prefix(&src_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let source = read(&path);
        // upstream.rs holds the relay itself and the unit tests of its error
        // branch; only the production text (before the first test module) is a
        // caller scan.
        let scanned = if normalized.ends_with("src/upstream.rs") {
            let end = source
                .find("#[cfg(test)]")
                .expect("upstream.rs has test modules");
            &source[..end]
        } else {
            &source[..]
        };
        for needle in ["proxy_sse_stream_with_data_plane(", "proxy_sse_stream_with_browser("] {
            for (last, before) in calls(scanned, needle) {
                total += 1;
                if last != "&[]" {
                    relaying.push((relative.clone(), last, enclosing_fn(&before)));
                }
            }
        }
    }
    assert!(
        total >= 5,
        "found only {total} relay callers; the scan is vacuous"
    );

    // The closed set: continue_run's two calls (the skip-site browser path and the plain relay) pass
    // CONTINUE_RELAYED_ERRORS; the space live route passes its own named list, RELAYED; and upstream.rs's
    // proxy_sse_stream_with_data_plane forwards the relayed_errors its caller gave it. Nothing else.
    let mut actual = relaying.clone();
    actual.sort();
    let mut expected: Vec<(String, String, String)> = [
        ("domains/orchestration.rs", "CONTINUE_RELAYED_ERRORS", "continue_run"),
        ("domains/orchestration.rs", "CONTINUE_RELAYED_ERRORS", "continue_run"),
        ("domains/spaces/run_live.rs", "RELAYED", "space_run_live"),
        ("upstream.rs", "relayed_errors", "proxy_sse_stream_with_data_plane"),
    ]
    .map(|(file, list, handler)| (file.to_owned(), list.to_owned(), handler.to_owned()))
    .into_iter()
    .collect();
    expected.sort();
    assert_eq!(
        actual, expected,
        "the calls that pass a non-empty relay list are not the closed set (file, list, enclosing fn); \
         any other caller must pass &[]"
    );
}

/// The body of `fn name(` in `source`, with whole-line comments dropped.
fn gateway_fn_body(source: &str, name: &str) -> String {
    delimited(source, &format!("fn {name}("), '{', '}')
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The third argument of every production call of `park_refusal_event(` in the gateway's
/// `sse.rs` (a production call passes `&request_id`; the unit tests pass literals), with
/// its whitespace folded away.
fn gateway_park_refusal_tokens(source: &str) -> Vec<String> {
    let needle = "park_refusal_event(";
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = source[from..].find(needle) {
        let at = at + from;
        from = at + needle.len();
        if source[..at].trim_end().ends_with("fn") {
            continue; // the definition
        }
        let args = delimited(&source[at..], needle, '(', ')');
        let mut depth = 0i32;
        let mut parts = Vec::new();
        let mut current = String::new();
        for ch in args.chars() {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
            if ch == ',' && depth == 0 {
                parts.push(current.trim().to_owned());
                current.clear();
            } else {
                current.push(ch);
            }
        }
        if !current.trim().is_empty() {
            parts.push(current.trim().to_owned());
        }
        if parts.first().is_some_and(|first| first == "&request_id") {
            assert_eq!(parts.len(), 3, "park_refusal_event takes three arguments: {args}");
            found.push(parts[2].split_whitespace().collect::<Vec<_>>().join(""));
        }
    }
    found
}

/// WS03 S8-A parity row: the frame of a failed PARK continue. model-gateway's
/// `park_refusal_event` writes the committed refusal frame plus `run_id` and the marker
/// `park_continue: true`, and never `approval_spent`; the SPA's park reader
/// (`chatRunContinueParkFailureOf`) reads exactly that marker, refuses a frame that also says
/// `approval_spent`, and the tokens such a frame can carry are the closed frame list the SPA
/// already has. The BFF relays the stream byte for byte: it neither reads nor writes the marker.
#[test]
fn the_park_frame_the_gateway_writes_is_the_one_the_spa_park_reader_reads() {
    let gateway = model_gateway_sse_source();
    let spa = spa_contract_source();

    // The gateway's frame: the marker and the run, and no spend.
    let writer = gateway_fn_body(&gateway, "park_refusal_event");
    assert!(
        writer.contains(r#"payload["park_continue"] = json!(true);"#),
        "park_refusal_event no longer sets park_continue: true\n{writer}"
    );
    assert!(
        writer.contains(r#"payload["run_id"] = json!(run_id);"#),
        "park_refusal_event no longer names the run\n{writer}"
    );
    assert!(
        !writer.contains("approval_spent") && !writer.contains("ApprovalSpend"),
        "a park frame must never say approval_spent\n{writer}"
    );

    // The SPA's reader: the same marker (an exact `true`), and no `approval_spent` key at all.
    assert!(
        spa.contains("return record.park_continue === true"),
        "the SPA's park marker is no longer `park_continue === true`"
    );
    let reader = delimited(&spa, "export function chatRunContinueParkRefusalOf(", '{', '}');
    assert!(
        reader.contains("isParkMarked(record)") && reader.contains("'approval_spent' in record"),
        "the SPA's park reader must require the marker and refuse any approval_spent key\n{reader}"
    );

    // The tokens a park frame can carry: the construction refusal, whatever the executor refused
    // (a token of the closed list), an unreachable executor and an unknown outcome.
    let frame_list = string_literals(&gateway_const_list(&gateway, "CONTINUE_FRAME_REFUSALS"), '"');
    let spa_list = spa_frame_refusals();
    let mut written = gateway_park_refusal_tokens(&gateway);
    written.sort();
    assert_eq!(
        written,
        vec!["\"continue_refused\"".to_owned(), "park_frame_token(&failure)".to_owned()],
        "model-gateway's production park frames are the construction refusal and the dispatch \
         failure, nothing else; parsed {written:?}"
    );
    let mapped = string_literals(&gateway_fn_body(&gateway, "park_frame_token"), '"');
    assert_eq!(mapped, ["continue_unreachable", "continue_outcome_unknown"]);
    for token in mapped.iter().map(String::as_str).chain(["continue_refused"]) {
        assert!(
            frame_list.iter().any(|known| known == token) && spa_list.iter().any(|known| known == token),
            "a park frame can say `{token}`, which is not in both frame lists"
        );
    }
    // The three tokens only a park continue sends are in the lists too.
    for token in ["park_not_restorable", "park_route_unavailable", "park_not_due"] {
        assert!(
            frame_list.iter().any(|known| known == token) && spa_list.iter().any(|known| known == token),
            "`{token}` is not in both frame lists"
        );
    }

    // The SPA's `runStaysParked` buckets name only tokens a park frame can carry: a closed frame
    // token, or the bare `agent_dispatch_unavailable`, which is NOT in the frame list (it is the
    // committed unmarked frame of a deadline or a join error).
    let bucket = |name: &str| -> Vec<String> {
        string_literals(
            &delimited(&spa, &format!("export const {name}"), '[', ']')
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n"),
            '\'',
        )
    };
    let not_parked = bucket("CHAT_RUN_CONTINUE_PARK_NOT_PARKED_CODES");
    let unknown = bucket("CHAT_RUN_CONTINUE_PARK_UNKNOWN_CODES");
    assert_eq!(not_parked, ["not_resumable", "park_not_restorable"]);
    assert_eq!(
        unknown,
        ["continue_outcome_unknown", "continue_in_progress", "agent_dispatch_unavailable"]
    );
    for code in not_parked.iter().chain(unknown.iter()) {
        if code == "agent_dispatch_unavailable" {
            assert!(
                !frame_list.contains(code) && !spa_list.contains(code),
                "agent_dispatch_unavailable is the bare frame, not a refusal token"
            );
            assert!(
                gateway_fn_body(&gateway, "agent_dispatch_unavailable_event").contains("\"agent_dispatch_unavailable\""),
                "the gateway no longer writes agent_dispatch_unavailable"
            );
        } else if code == "not_resumable" {
            // A 409 before the stream opens (CONTINUE_HTTP_REFUSALS), read through the park
            // reader only when a frame names it; the BFF relays it by its closed code.
            assert!(
                bff_relayed().iter().any(|(known, _)| known == code),
                "the BFF no longer relays `{code}`"
            );
        } else {
            assert!(
                frame_list.contains(code) && spa_list.contains(code),
                "`{code}` is in a runStaysParked bucket but not in both frame lists"
            );
        }
    }

    // The BFF relays the stream untouched: its continue handler neither reads nor writes
    // the park marker or the spend flag.
    let orchestration = read(&gateway_file("src/domains/orchestration.rs"));
    let production = orchestration
        .split("#[cfg(test)]\nmod tests")
        .next()
        .expect("orchestration.rs production text")
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for word in ["park_continue", "approval_spent"] {
        assert!(
            !production.contains(word),
            "the BFF's continue handler must not read or write `{word}`: the stream is relayed byte for byte"
        );
    }
}
