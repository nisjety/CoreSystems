//! The durable loop's cross-service contract (build plan WS03 S9, static part).
//!
//! The gateway builds the `/continue` request, execution-core validates it, session-core
//! stores the approval it consumes, and the compose file decides which of them is switched
//! on. They are separate crates with separate deploy cadence, so none depends on another
//! and the agreement between them is held here, the way `cross_service_loop_contract.rs`
//! holds the loop constants: by reading the source of record on each side.
//!
//! # What is guarded
//!
//! * the inline chat loop (`model-gateway/src/tool_loop.rs`) has no resume path at all: the
//!   durable loop lives in execution-core and the two loops stay separate (Model Plane
//!   `CLAUDE.md`, "Two tool-dispatch loops");
//! * `/continue` is one route, bound to one handler, and the gateway builds exactly one
//!   `AgentResume`;
//! * the resume the gateway builds is a resume the executor accepts, with the segment number
//!   left to the executor, so an old gateway sending one does not break a new executor;
//! * the executor checks a caller is not zero-retention before it looks at a resume;
//! * the pause site decides `loop` from "not ZDR and not waived", never "not ZDR" alone;
//! * the mode words (`""`, `segment`, `loop`) mean one thing on every plane;
//! * the retention class a store compares in SQL is the one the shared durable gate opens for,
//!   and the one the posture matrix calls persistent;
//! * every durable-loop switch is OFF by default in compose, is set on every service that reads
//!   it, and is read by exactly one parser;
//! * which tests drive each store's OWN gate (`store_proofs()`): the shared durable gate the
//!   posture matrix is held against has no production caller, so the matrix proves the gate and
//!   these tests, plus the wiring guards above, prove the stores. The map is checked: a named
//!   test that is renamed, removed or commented out fails here.
//!
//! # How the guards are written
//!
//! Each guard is a pure function over source text that returns its violations. The real
//! source must produce none, and a deliberately broken copy of the same text (the mutation a
//! careless edit would make) must produce one: a guard that cannot fail is a defect, and
//! `every_guard_goes_red_when_the_code_it_guards_regresses` proves each of them can, without
//! touching a production file. Comments are stripped before anything is matched (a comment
//! that names a forbidden identifier is explaining it, not using it) and `#[cfg(test)]`
//! items are cut out where only production code is meant.
//!
//! If a file moves, a guard fails with its path: re-point it. Do not delete the assertion.

use std::collections::BTreeSet;
use std::path::PathBuf;

use mp_contracts::durable_gate::{
    loop_mode_for_persistent, refuse_unless_persistent_class, DurableStore,
};
use regex::Regex;

/// Relative to this crate root; `services/` holds the three Rust services side by side.
const TOOL_LOOP: &str = "../model-gateway/src/tool_loop.rs";
const GATEWAY_SSE: &str = "../model-gateway/src/sse.rs";
const GATEWAY_ROUTES: &str = "../model-gateway/src/http_routes.rs";
const GATEWAY_POSTURE: &str = "../model-gateway/src/zdr_posture.rs";
const EXECUTOR_GRPC: &str = "src/grpc.rs";
const EXECUTOR_LOOP: &str = "src/runtime_loop/agent.rs";
const EXECUTOR_EFFECT: &str = "src/runtime_loop/effect.rs";
const EXECUTOR_FOLD: &str = "src/runtime_loop/fold.rs";
const SESSION_APPROVALS: &str = "../session-core/src/orchestration_store.rs";
const SESSION_APPROVAL_RPC: &str = "../session-core/src/orchestration_grpc.rs";
const SESSION_CHECKPOINT: &str = "../session-core/src/loop_checkpoint.rs";
const SESSION_LEDGER: &str = "../session-core/src/tool_ledger.rs";
const COMPOSE: &str = "../../../deploy/docker-compose.yml";
const MATRIX: &str = include_str!("fixtures/zdr-posture-matrix.json");

/// Every directory whose production code must not read the continue switch itself.
const SOURCE_TREES: [&str; 5] = [
    "src",
    "../model-gateway/src",
    "../session-core/src",
    "../inference-core/src",
    "../../crates",
];

fn read(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    let text = std::fs::read_to_string(&path).map(|text| text.replace("\r\n", "\n"));
    text.unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}).\n\
             This file holds one half of a cross-service contract. If it moved, re-point the \
             path in this test; do not delete the assertion, or the contract goes back to being \
             held by a comment alone.",
            path.display()
        )
    })
}

/// Every `.rs` file under `relative`, as (path relative to the crate root, text).
fn rust_files(relative: &str) -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                if name != "target" && name != "tests" {
                    walk(&path, out);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    assert!(
        root.is_dir(),
        "{} is not a directory: re-point SOURCE_TREES",
        root.display()
    );
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
                .replace("\r\n", "\n");
            (path.display().to_string(), text)
        })
        .collect()
}

// ---------------------------------------------------------------------------------------
// A small Rust-aware text cleaner: comments out, string contents blanked for brace matching.
// ---------------------------------------------------------------------------------------

/// Source with comments removed. `text` keeps string and char literals; `skel` is the same
/// length with their contents blanked, so braces and keywords inside a string are never
/// mistaken for code. Both are pure ASCII (a non-ASCII character becomes `?`), so a byte
/// index into one is a byte index into the other.
#[derive(Clone)]
struct Code {
    text: String,
    skel: String,
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

impl Code {
    fn new(source: &str) -> Self {
        let c: Vec<char> = source
            .chars()
            .map(|ch| if ch.is_ascii() { ch } else { '?' })
            .collect();
        let mut text = String::with_capacity(c.len());
        let mut skel = String::with_capacity(c.len());
        let code = |text: &mut String, skel: &mut String, ch: char| {
            text.push(ch);
            skel.push(ch);
        };
        let blank = |text: &mut String, skel: &mut String, ch: char| {
            text.push(ch);
            skel.push(if ch == '\n' { '\n' } else { ' ' });
        };
        let mut i = 0;
        while i < c.len() {
            let ch = c[i];
            let next = c.get(i + 1).copied();
            if ch == '/' && next == Some('/') {
                while i < c.len() && c[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            if ch == '/' && next == Some('*') {
                let mut depth = 1;
                i += 2;
                while i < c.len() && depth > 0 {
                    if c[i] == '/' && c.get(i + 1) == Some(&'*') {
                        depth += 1;
                        i += 2;
                    } else if c[i] == '*' && c.get(i + 1) == Some(&'/') {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            // A raw string: r"..", r#".."#, br"..".
            let previous_is_ident = text.chars().last().is_some_and(is_ident);
            if (ch == 'r' || (ch == 'b' && next == Some('r'))) && !previous_is_ident {
                let mut j = i + if ch == 'b' { 2 } else { 1 };
                let mut hashes = 0;
                while c.get(j) == Some(&'#') {
                    hashes += 1;
                    j += 1;
                }
                if c.get(j) == Some(&'"') {
                    for &k in &c[i..=j] {
                        code(&mut text, &mut skel, k);
                    }
                    j += 1;
                    while j < c.len() {
                        if c[j] == '"' && (0..hashes).all(|k| c.get(j + 1 + k) == Some(&'#')) {
                            for &k in &c[j..j + 1 + hashes] {
                                code(&mut text, &mut skel, k);
                            }
                            j += 1 + hashes;
                            break;
                        }
                        blank(&mut text, &mut skel, c[j]);
                        j += 1;
                    }
                    i = j;
                    continue;
                }
            }
            if ch == '"' {
                code(&mut text, &mut skel, '"');
                i += 1;
                while i < c.len() {
                    if c[i] == '\\' {
                        blank(&mut text, &mut skel, c[i]);
                        if let Some(&escaped) = c.get(i + 1) {
                            blank(&mut text, &mut skel, escaped);
                        }
                        i += 2;
                        continue;
                    }
                    if c[i] == '"' {
                        code(&mut text, &mut skel, '"');
                        i += 1;
                        break;
                    }
                    blank(&mut text, &mut skel, c[i]);
                    i += 1;
                }
                continue;
            }
            if ch == '\'' {
                if next == Some('\\') {
                    code(&mut text, &mut skel, '\'');
                    i += 1;
                    while i < c.len() && c[i] != '\'' {
                        if c[i] == '\\' {
                            blank(&mut text, &mut skel, c[i]);
                            i += 1;
                        }
                        if i < c.len() {
                            blank(&mut text, &mut skel, c[i]);
                            i += 1;
                        }
                    }
                    if i < c.len() {
                        code(&mut text, &mut skel, '\'');
                        i += 1;
                    }
                    continue;
                }
                if c.get(i + 2) == Some(&'\'') && next != Some('\'') {
                    code(&mut text, &mut skel, '\'');
                    blank(&mut text, &mut skel, c[i + 1]);
                    code(&mut text, &mut skel, '\'');
                    i += 3;
                    continue;
                }
                // Otherwise a lifetime: ordinary code.
            }
            code(&mut text, &mut skel, ch);
            i += 1;
        }
        assert_eq!(text.len(), skel.len());
        Self { text, skel }
    }

    /// The production code: every item behind `#[cfg(test)]` removed.
    fn without_tests(&self) -> Self {
        let mut text = self.text.clone();
        let mut skel = self.skel.clone();
        while let Some(start) = skel.find("#[cfg(test)]") {
            let after = start + "#[cfg(test)]".len();
            let end = match skel[after..].find(['{', ';']) {
                None => skel.len(),
                Some(offset) if skel.as_bytes()[after + offset] == b';' => after + offset + 1,
                Some(offset) => {
                    matching_brace(&skel, after + offset).map_or(skel.len(), |close| close + 1)
                }
            };
            text.replace_range(start..end, "");
            skel.replace_range(start..end, "");
        }
        Self { text, skel }
    }

    /// The body of the first `fn name`, between its braces.
    fn fn_body(&self, name: &str) -> Option<String> {
        let pattern = Regex::new(&format!(r"\bfn\s+{}\s*[<(]", regex::escape(name))).ok()?;
        for found in pattern.find_iter(&self.skel) {
            let rest = &self.skel[found.end()..];
            let Some(offset) = rest.find(['{', ';']) else {
                continue;
            };
            if rest.as_bytes()[offset] == b';' {
                continue;
            }
            let open = found.end() + offset;
            let close = matching_brace(&self.skel, open)?;
            return Some(self.text[open + 1..close].to_owned());
        }
        None
    }
}

/// The index of the brace that closes the one at `open`, over blanked source.
fn matching_brace(skel: &str, open: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (offset, byte) in skel.as_bytes()[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn word_count(haystack: &str, word: &str) -> usize {
    Regex::new(&format!(r"\b{}\b", regex::escape(word)))
        .expect("a word is a valid pattern")
        .find_iter(haystack)
        .count()
}

// ---------------------------------------------------------------------------------------
// Guards: each returns its violations (empty is green).
// ---------------------------------------------------------------------------------------

/// What the inline chat loop must never mention: the durable loop's resume path.
const FORBIDDEN_IN_THE_INLINE_LOOP: [&str; 13] = [
    "AgentResume",
    "ConsumeApproval",
    "consume_approval",
    "SaveLoopCheckpoint",
    "ClaimLoopCheckpoint",
    "CloseLoopCheckpoint",
    "LoopFoldV1",
    "loop_fold",
    "continue_run",
    "RunAgentRequest",
    "continuation_mode",
    "run_agent(",
    "resume:",
];

fn inline_loop_violations(tool_loop: &str) -> Vec<String> {
    let code = Code::new(tool_loop);
    let mut violations: Vec<String> = FORBIDDEN_IN_THE_INLINE_LOOP
        .iter()
        .filter(|token| code.text.contains(**token))
        .map(|token| format!("tool_loop.rs mentions `{token}`: the inline loop has a resume path"))
        .collect();
    // The scan is only meaningful on the file it was written for.
    if !code.text.contains("dispatch_tool") {
        violations
            .push("tool_loop.rs no longer defines dispatch_tool: wrong file, re-point".into());
    }
    violations
}

/// `/continue` is one route, bound to `sse::continue_run`, in the whole gateway.
fn continue_route_violations(gateway_sources: &[(String, String)]) -> Vec<String> {
    let mut violations = Vec::new();
    let mut routes = Vec::new();
    for (path, source) in gateway_sources {
        let code = Code::new(source).without_tests();
        for found in Regex::new(r#""[^"]*/continue""#)
            .expect("pattern")
            .find_iter(&code.text)
        {
            routes.push((
                path.clone(),
                code.text[found.end()..].to_owned(),
                found.as_str().to_owned(),
            ));
        }
    }
    if routes.len() != 1 {
        violations.push(format!(
            "the gateway has {} /continue routes in production code, expected exactly one: {:?}",
            routes.len(),
            routes
                .iter()
                .map(|(path, _, literal)| format!("{path} {literal}"))
                .collect::<Vec<_>>()
        ));
    }
    for (path, after, literal) in &routes {
        let bound = Regex::new(r"^\s*,\s*post\(\s*crate::sse::continue_run\s*\)")
            .expect("pattern")
            .is_match(after);
        if !bound {
            violations.push(format!(
                "{path}: {literal} is not bound to post(crate::sse::continue_run)"
            ));
        }
        if !literal.contains("/runs/:run_id/continue") {
            violations.push(format!("{path}: {literal} is not the run continue route"));
        }
    }
    violations
}

/// The gateway builds one `AgentResume`, in `continue_resume`, and nowhere builds one
/// another way (`AgentResume::default()`, a second literal).
fn single_resume_violations(gateway_sources: &[(String, String)]) -> Vec<String> {
    let literal = Regex::new(r"\bAgentResume\s*\{").expect("pattern");
    let path_call = Regex::new(r"\bAgentResume\s*::").expect("pattern");
    let mut literals = Vec::new();
    let mut violations = Vec::new();
    for (path, source) in gateway_sources {
        let code = Code::new(source).without_tests();
        for found in literal.find_iter(&code.text) {
            // `-> mp_contracts::..::AgentResume {` is a return type followed by the body, not a literal.
            let before = code.text[..found.start()]
                .trim_end_matches(|c: char| is_ident(c) || c == ':')
                .trim_end();
            if !before.ends_with("->") {
                literals.push(path.clone());
            }
        }
        if path_call.is_match(&code.text) {
            violations.push(format!(
                "{path} builds an AgentResume through a path call, not the one literal"
            ));
        }
    }
    if literals.len() != 1 {
        violations.push(format!(
            "expected exactly one production AgentResume literal in the gateway, found {}: {literals:?}",
            literals.len()
        ));
    } else if !literals[0].ends_with("sse.rs") {
        violations.push(format!(
            "the one AgentResume literal is in {}, not sse.rs",
            literals[0]
        ));
    }
    violations
}

/// A quoted word list: `"a" | "b"` or `"a"`, as the strings in it.
fn quoted(list: &str) -> BTreeSet<String> {
    Regex::new(r#""([a-z0-9_]*)""#)
        .expect("pattern")
        .captures_iter(list)
        .map(|captured| captured[1].to_owned())
        .collect()
}

/// The resume the gateway builds is one the executor accepts, and the segment number is the
/// executor's to derive: nothing on either side reads or demands a client-supplied one.
fn builder_and_validator_violations(
    gateway_sse: &str,
    executor_grpc: &str,
    executor_loop: &str,
) -> Vec<String> {
    let mut violations = Vec::new();
    let sse = Code::new(gateway_sse).without_tests();
    let grpc = Code::new(executor_grpc).without_tests();
    let agent = Code::new(executor_loop).without_tests();

    let Some(builder) = sse.fn_body("continue_resume") else {
        return vec!["sse.rs has no continue_resume builder".into()];
    };
    let Some(validator) = grpc.fn_body("enforce_resume") else {
        return vec!["grpc.rs has no enforce_resume validator".into()];
    };
    let built_mode = Regex::new(r#"\bmode\s*:\s*"([a-z0-9_]+)"\s*\.to_owned\(\)"#)
        .expect("pattern")
        .captures(&builder)
        .map(|captured| captured[1].to_owned());
    let accepted = Regex::new(r"matches!\(\s*resume\.mode\.as_str\(\)\s*,([^)]*)\)")
        .expect("pattern")
        .captures(&validator)
        .map(|captured| quoted(&captured[1]))
        .unwrap_or_default();
    match built_mode {
        None => violations.push("continue_resume does not build a literal mode".into()),
        Some(mode) if !accepted.contains(&mode) => violations.push(format!(
            "the gateway builds resume mode `{mode}` but the executor accepts only {accepted:?}"
        )),
        Some(_) => {}
    }
    for required in ["segment", "loop"] {
        if !accepted.contains(required) {
            violations.push(format!(
                "enforce_resume no longer accepts the `{required}` stage"
            ));
        }
    }
    // The validator still demands what it demanded: both ids, and the switch.
    for needle in [
        "resume.approval_id.is_empty()",
        "resume.claim_token.is_empty()",
        "continue_disabled",
        "resume_incomplete",
    ] {
        if !validator.contains(needle) {
            violations.push(format!("enforce_resume lost `{needle}`"));
        }
    }
    // The executor owns segment numbering: the gateway sends 0 and nobody refuses or reads another.
    let sent = Regex::new(r"\bsegment_no\s*:\s*(\d+)")
        .expect("pattern")
        .captures_iter(&builder)
        .map(|captured| captured[1].to_owned())
        .collect::<Vec<_>>();
    if sent != ["0"] {
        violations.push(format!(
            "continue_resume must send segment_no: 0 exactly once, sends {sent:?}"
        ));
    }
    if word_count(&sse.text, "segment_no") != 1 {
        violations.push(format!(
            "sse.rs names segment_no {} times; the builder's one literal is the only use",
            word_count(&sse.text, "segment_no")
        ));
    }
    if validator.contains("segment_no") {
        violations.push("enforce_resume reads segment_no: a gateway that sends none, or an old one that sends 2, would be refused".into());
    }
    for (name, code) in [("grpc.rs", &grpc), ("agent.rs", &agent)] {
        if code.text.contains(".segment_no") {
            violations.push(format!(
                "{name} reads a client-supplied .segment_no; the executor derives it from the paused step"
            ));
        }
    }
    violations
}

/// The executor refuses a zero-retention caller before it looks at a resume.
fn run_agent_order_violations(executor_grpc: &str) -> Vec<String> {
    let grpc = Code::new(executor_grpc).without_tests();
    let Some(body) = grpc.fn_body("run_agent") else {
        return vec!["grpc.rs has no run_agent".into()];
    };
    let free = body.find(r#"enforce_persistence_free_execution(&caller, req.zdr, "agent_run")"#);
    let resume = body.find("enforce_resume(");
    match (free, resume) {
        (Some(free), Some(resume)) if free < resume => Vec::new(),
        (Some(_), Some(_)) => vec!["run_agent checks the resume before the zero-retention refusal".into()],
        (None, _) => vec!["run_agent no longer refuses a zero-retention caller with enforce_persistence_free_execution(.., \"agent_run\")".into()],
        (_, None) => vec!["run_agent no longer calls enforce_resume".into()],
    }
}

/// The pause site decides `loop` from "not ZDR and not waived". "Not ZDR" alone let a waived
/// run (whose `zdr` flag is false) mint a loop approval it must never have.
fn pause_site_violations(executor_loop: &str, every_source: &[(String, String)]) -> Vec<String> {
    let mut violations = Vec::new();
    let agent = Code::new(executor_loop).without_tests();
    let call = Regex::new(r"\brun_pause_mode\s*\(").expect("pattern");
    let calls: Vec<_> = call.find_iter(&agent.skel).collect();
    if calls.len() != 1 {
        violations.push(format!(
            "agent.rs calls run_pause_mode {} times, expected the one pause site",
            calls.len()
        ));
    }
    for found in calls {
        // The first argument: up to the first top-level comma.
        let mut depth = 0_i32;
        let mut first = String::new();
        for ch in agent.text[found.end()..].chars() {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => break,
                _ => {}
            }
            if depth < 0 {
                break;
            }
            first.push(ch);
        }
        if !(first.contains("zdr") && first.contains("is_waived()")) {
            violations.push(format!(
                "the pause site passes `{}` as persistent: it must say not-ZDR AND not-waived",
                first.trim()
            ));
        }
    }
    if Regex::new(r"loop_mode_for_persistent\s*\(\s*!?\s*\w+\.zdr\s*,")
        .expect("pattern")
        .is_match(&agent.text)
    {
        violations.push("agent.rs mints a loop mode from the zdr flag alone".into());
    }
    // The old name took `zdr` as its first argument; a stale caller must not exist anywhere.
    for (path, source) in every_source {
        if Code::new(source).text.contains("continuation_mode_for") {
            violations.push(format!("{path} still names continuation_mode_for"));
        }
    }
    violations
}

/// The mode words mean one thing on every plane.
fn mode_word_violations(gateway_sse: &str, session_rpc: &str, executor_grpc: &str) -> Vec<String> {
    let mut violations = Vec::new();
    // Minted by the shared gate, over every input.
    let minted: BTreeSet<&str> = [(true, true), (true, false), (false, true), (false, false)]
        .into_iter()
        .map(|(persistent, enabled)| loop_mode_for_persistent(persistent, enabled))
        .collect();
    if minted != BTreeSet::from(["", "loop"]) {
        violations.push(format!(
            "the durable gate mints {minted:?}, expected only the cold mode and loop"
        ));
    }
    // Stored by session-core.
    let rpc = Code::new(session_rpc).without_tests();
    let stored = Regex::new(r"matches!\(\s*req\.continuation_mode\.as_str\(\)\s*,([^)]*)\)")
        .expect("pattern")
        .captures(&rpc.text)
        .map(|captured| quoted(&captured[1]))
        .unwrap_or_default();
    for word in &minted {
        if !stored.contains(*word) {
            violations.push(format!(
                "session-core refuses the mode `{word}` the gate mints; it accepts {stored:?}"
            ));
        }
    }
    // Required by the gateway preflight.
    let sse = Code::new(gateway_sse).without_tests();
    let required = sse.fn_body("continue_preflight").and_then(|body| {
        Regex::new(r#"continuation_mode\s*!=\s*"([a-z0-9_]+)""#)
            .expect("pattern")
            .captures(&body)
            .map(|captured| captured[1].to_owned())
    });
    match required {
        None => violations.push("continue_preflight no longer requires a continuation mode".into()),
        Some(word) if !minted.contains(word.as_str()) || word.is_empty() => violations.push(
            format!("the gateway requires mode `{word}`, which nothing mints"),
        ),
        Some(_) => {}
    }
    // Accepted by the executor: the loop stage, minted words that are not cold.
    let grpc = Code::new(executor_grpc).without_tests();
    let accepted = grpc
        .fn_body("enforce_resume")
        .and_then(|body| {
            Regex::new(r"matches!\(\s*resume\.mode\.as_str\(\)\s*,([^)]*)\)")
                .expect("pattern")
                .captures(&body)
                .map(|captured| quoted(&captured[1]))
        })
        .unwrap_or_default();
    if !accepted.contains("loop") {
        violations.push("the executor does not accept the loop stage".into());
    }
    if accepted.contains("") {
        violations.push("the executor accepts an empty resume mode".into());
    }
    violations
}

/// The retention class a store compares in SQL is the class the shared gate opens for, and the
/// class the posture matrix calls persistent.
fn retention_predicate_violations(approvals: &str, checkpoint: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let vocabulary: serde_json::Value = serde_json::from_str(MATRIX).expect("the matrix is JSON");
    let golden = vocabulary["vocabulary"]["retention_classes"]["persistent"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let approvals = Code::new(approvals).without_tests();
    let consume_sql = Regex::new(r#"const CONSUME_APPROVAL_SQL: &str = (?s)"(.*?)";"#)
        .expect("pattern")
        .captures(&approvals.text)
        .map(|captured| captured[1].to_owned());
    match consume_sql {
        None => violations.push("orchestration_store.rs has no CONSUME_APPROVAL_SQL".into()),
        Some(sql) => {
            let classes: Vec<String> = Regex::new(r"retention_class\s*=\s*'([a-z0-9_]+)'")
                .expect("pattern")
                .captures_iter(&sql)
                .map(|captured| captured[1].to_owned())
                .collect();
            if classes != [golden.clone()] {
                violations.push(format!(
                    "the consume SQL compares retention_class with {classes:?}; the matrix's persistent class is `{golden}`"
                ));
            }
            for class in &classes {
                if refuse_unless_persistent_class(DurableStore::Consume, class).is_err() {
                    violations.push(format!(
                        "the consume SQL opens for `{class}`, which the durable gate refuses"
                    ));
                }
            }
            if !sql.contains("r.status NOT IN ('completed', 'failed', 'cancelled')") {
                violations.push("the consume SQL no longer refuses a run that has ended".into());
            }
            if !sql.contains("consumed_at IS NULL") {
                violations.push("the consume SQL no longer refuses a consumed approval".into());
            }
        }
    }
    let checkpoint = Code::new(checkpoint).without_tests();
    match checkpoint.fn_body("run_owner_tx") {
        None => violations.push("loop_checkpoint.rs has no run_owner_tx".into()),
        Some(body) => {
            let refused: Vec<String> = Regex::new(r#"retention\s*!=\s*"([a-z0-9_]+)""#)
                .expect("pattern")
                .captures_iter(&body)
                .map(|captured| captured[1].to_owned())
                .collect();
            if refused != [golden.clone()] {
                violations.push(format!(
                    "the fold store refuses every class but {refused:?}; the matrix's persistent class is `{golden}`"
                ));
            }
        }
    }
    violations
}

// ---- compose ---------------------------------------------------------------------------

/// The `environment:` entries of one service of the compose file, as (key, value) in order.
fn service_env(compose: &str, service: &str) -> Vec<(String, String)> {
    let mut env = Vec::new();
    let mut in_service = false;
    let mut in_env = false;
    for raw in compose.lines() {
        let line = raw.trim_end();
        let indent = line.len() - line.trim_start().len();
        let content = line.trim_start();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        if indent <= 2 {
            in_service = indent == 2 && content == format!("{service}:");
            in_env = false;
            continue;
        }
        if !in_service {
            continue;
        }
        if indent == 4 {
            in_env = content == "environment:";
            continue;
        }
        if in_env && indent == 6 {
            if let Some((key, value)) = content.split_once(':') {
                env.push((
                    key.trim().to_owned(),
                    value.trim().trim_matches('"').to_owned(),
                ));
            }
        }
    }
    env
}

/// A durable-loop switch: named for the loop or the ledger, and an `_ENABLED` flag.
fn is_loop_switch(key: &str) -> bool {
    key.ends_with("_ENABLED")
        && [
            "LOOP_",
            "CONTINUE_AFTER_APPROVAL",
            "EFFECT_LEDGER",
            "LEDGER_SWEEP",
        ]
        .iter()
        .any(|marker| key.contains(marker))
}

/// Which service reads which switch, and the const that names it in source.
fn switch_table() -> Vec<(&'static str, String, &'static str)> {
    // (service in compose, env name, why it is required there)
    vec![
        (
            "model-gateway",
            mp_contracts::loop_fold::CONTINUE_AFTER_APPROVAL_ENV.to_owned(),
            "the /continue route",
        ),
        (
            "execution-core",
            mp_contracts::loop_fold::CONTINUE_AFTER_APPROVAL_ENV.to_owned(),
            "the executor's resume and the pause site",
        ),
        (
            "execution-core",
            const_str(&read(EXECUTOR_EFFECT), "LEDGER_ENV"),
            "the effect ledger",
        ),
        (
            "execution-core",
            const_str(&read(EXECUTOR_FOLD), "CHECKPOINT_ENV"),
            "the fold writer",
        ),
        (
            "execution-core",
            mp_contracts::loop_fold::LOOP_RESUME_ENV.to_owned(),
            "the in-place resume",
        ),
        (
            "model-gateway",
            mp_contracts::loop_fold::LOOP_RESUME_ENV.to_owned(),
            "the /continue pre-consume and the loop stage (S7c)",
        ),
        (
            "session-core",
            const_str(&read(SESSION_CHECKPOINT), "ENABLED_ENV"),
            "the fold store",
        ),
        (
            "session-core",
            const_str(&read(SESSION_LEDGER), "ENABLED_ENV"),
            "the ledger sweep",
        ),
    ]
}

/// `const NAME: &str = "VALUE";` out of source.
fn const_str(source: &str, name: &str) -> String {
    Regex::new(&format!(
        r#"\bconst\s+{name}\s*:\s*&str\s*=\s*"([A-Z0-9_]+)"\s*;"#
    ))
    .expect("pattern")
    .captures(&Code::new(source).text)
    .map(|captured| captured[1].to_owned())
    .unwrap_or_else(|| panic!("no `const {name}: &str = \"..\";` found; re-point the switch table"))
}

fn compose_violations(
    compose: &str,
    table: &[(&'static str, String, &'static str)],
) -> Vec<String> {
    let mut violations = Vec::new();
    for (service, key, why) in table {
        let env = service_env(compose, service);
        let entries: Vec<&(String, String)> = env.iter().filter(|(name, _)| name == key).collect();
        let want = format!("${{{key}:-false}}");
        match entries.as_slice() {
            [] => violations.push(format!("compose: {service} does not set {key} ({why}), so it cannot be switched with the others")),
            [(_, value)] if *value == want => {}
            [(_, value)] => violations.push(format!("compose: {service} sets {key}: {value}, expected {want}: it must default to OFF")),
            more => violations.push(format!("compose: {service} sets {key} {} times", more.len())),
        }
    }
    violations
}

/// Every loop switch in every compose file defaults OFF, wherever it appears.
fn every_switch_defaults_off_violations(compose_files: &[(String, String)]) -> Vec<String> {
    let mut violations = Vec::new();
    let line = Regex::new(r"(?m)^\s+([A-Z][A-Z0-9_]*)\s*:\s*(.*?)\s*$").expect("pattern");
    for (name, compose) in compose_files {
        for captured in line.captures_iter(compose) {
            let (key, value) = (&captured[1], captured[2].trim_matches('"'));
            if is_loop_switch(key) && value != format!("${{{key}:-false}}") {
                violations.push(format!(
                    "{name}: {key} is `{value}`, a durable-loop switch must be `${{{key}:-false}}`"
                ));
            }
        }
    }
    violations
}

/// The fold writer is off for a ZDR run and for a waived run, and its switch is read by the one
/// strict parser. The writer's gate is its own pure value (`fold::WriterGate`), fed at the one
/// place the loop builds a writer; both halves are read here, because a fold holds conversation
/// text and a waived run's `zdr` flag is false.
fn fold_gate_violations(executor_loop: &str, executor_fold: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let agent = Code::new(executor_loop).without_tests();
    let fold = Code::new(executor_fold).without_tests();

    let literals: Vec<_> = Regex::new(r"\bWriterGate\s*\{")
        .expect("pattern")
        .find_iter(&agent.skel)
        .collect();
    if literals.len() == 1 {
        let open = literals[0].end() - 1;
        let body = matching_brace(&agent.skel, open).map_or("", |close| &agent.text[open..=close]);
        if !Regex::new(r"\bwaived\s*:\s*ctx\.route\.is_waived\(\)")
            .expect("pattern")
            .is_match(body)
        {
            violations.push(
                "the loop builds its fold writer without `waived: ctx.route.is_waived()`".into(),
            );
        }
        if !Regex::new(r"\bzdr\b").expect("pattern").is_match(body) {
            violations.push("the loop builds its fold writer without the run's zdr flag".into());
        }
    } else {
        violations.push(format!(
            "agent.rs builds a fold WriterGate {} times, expected the one place the loop builds its writer",
            literals.len()
        ));
    }

    match fold.fn_body("decide") {
        None => violations.push("fold.rs has no WriterGate::decide".into()),
        Some(body) => {
            for needle in [
                "self.switch",
                "self.ledger",
                "self.zdr",
                "self.waived",
                "OffReason::Zdr",
                "OffReason::Waived",
            ] {
                if !body.contains(needle) {
                    violations.push(format!("WriterGate::decide no longer reads `{needle}`"));
                }
            }
        }
    }
    match fold.fn_body("checkpoint_enabled") {
        None => violations.push("fold.rs has no checkpoint_enabled".into()),
        Some(body) if !body.contains("switch_from(") => {
            violations.push(
                "checkpoint_enabled does not use the shared strict parser switch_from".into(),
            );
        }
        Some(_) => {}
    }
    violations
}

/// The switch has one parser: no production code outside mp-contracts names the env var.
fn one_parser_violations(sources: &[(String, String)]) -> Vec<String> {
    parser_violations(
        mp_contracts::loop_fold::CONTINUE_AFTER_APPROVAL_ENV,
        "continue_after_approval_enabled",
        sources,
    )
}

/// The in-place resume switch has one parser too (S7b): `loop_resume_enabled`, strict (`true`
/// and nothing else). A second reader of the variable would be a second opinion on when the
/// resume is on, and the gateway and the executor must never disagree.
fn one_resume_parser_violations(sources: &[(String, String)]) -> Vec<String> {
    parser_violations(
        mp_contracts::loop_fold::LOOP_RESUME_ENV,
        "loop_resume_enabled",
        sources,
    )
}

fn parser_violations(name: &str, parser: &str, sources: &[(String, String)]) -> Vec<String> {
    sources
        .iter()
        .filter(|(path, _)| !path.replace('\\', "/").contains("crates/mp-contracts/"))
        .filter(|(_, source)| Code::new(source).without_tests().text.contains(name))
        .map(|(path, _)| format!("{path} names {name} itself: use mp_contracts::loop_fold::{parser}"))
        .collect()
}

/// The executor builds ONE `ConsumeApprovalRequest`, and it always wants the call back.
///
/// The gateway's pre-consume is spend-only (`omit_call`, S7c), and then the executor's own
/// consume is a same-claim replay that hands the approved call back. The executor needs that
/// descriptor (it is the only source of what an approval authorised), so it never asks for the
/// call to be left out: `omit_call: true` anywhere in execution-core would starve the resume
/// of the call it runs. And one literal means one place to read what the executor asks for.
fn consume_request_violations(executor_sources: &[(String, String)]) -> Vec<String> {
    let literal = Regex::new(r"\bConsumeApprovalRequest\s*\{").expect("pattern");
    let omits = Regex::new(r"\bomit_call\s*:\s*true\b").expect("pattern");
    let mut violations = Vec::new();
    let mut literals = 0;
    for (path, source) in executor_sources {
        let code = Code::new(source).without_tests();
        literals += literal.find_iter(&code.text).count();
        if omits.is_match(&code.text) {
            violations.push(format!(
                "{path} sets `omit_call: true`: the executor needs the approved call back and never omits it"
            ));
        }
    }
    if literals != 1 {
        violations.push(format!(
            "execution-core builds {literals} ConsumeApprovalRequest literals; there must be exactly one (the executor's consume)"
        ));
    }
    violations
}

/// `resume.rs` holds `restore_in_place`, which is handed the client's whole `AgentResume`.
/// The executor derives the segment from the step that paused and NEVER reads the client's
/// `segment_no` (the gateway sends 0 and an old gateway sends 2: both must work). The fold's
/// own `segment_no` and the claim response's are the only `.segment_no` it may read, so any
/// other receiver (`resume.`, `r.`, ...) is a read of the client's.
fn resume_module_segment_violations(executor_resume: &str) -> Vec<String> {
    let code = Code::new(executor_resume).without_tests();
    let read = Regex::new(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\.\s*segment_no\b").expect("pattern");
    let mut violations = Vec::new();
    let mut allowed = 0;
    for captured in read.captures_iter(&code.text) {
        match &captured[1] {
            "fold" | "response" => allowed += 1,
            receiver => violations.push(format!(
                "resume.rs reads `{receiver}.segment_no`: only the fold's own and the claim response's may be read; the executor derives the segment from the paused step and never reads the client-supplied AgentResume.segment_no"
            )),
        }
    }
    if allowed == 0 {
        violations.push("the scan found no fold or response `.segment_no` in resume.rs: it is not reading the file the guard exists for".into());
    }
    violations
}

/// One durable store's real gate in production, and the tests that drive it.
///
/// `mp_contracts::durable_gate::refuse_unless_persistent` is the SPECIFICATION of when a store
/// opens, and `tests/loop_fold_zdr_matrix.rs` holds it against every row of the posture matrix.
/// No production code calls it: each store decides persistence its own way (the fold writer's
/// `WriterGate`, the consume's SQL predicate, the gateway's `continue_persistent_gate`, the pause
/// site's `!req.zdr && !ctx.route.is_waived()`), and those gates are not visible to an
/// integration test. So the matrix does NOT prove any store. What proves a store is the tests
/// listed here, which drive that store's own gate with a zero-retention run and a waived run, and
/// the source guards in this file, which hold the wiring (what each gate is fed from, which class
/// the SQL compares). This table is the one place that says which is which, and
/// [`store_proof_violations`] fails when a named test is renamed, deleted, commented out or no
/// longer a test, so the claim cannot rot into a comment.
struct StoreProof {
    store: Option<DurableStore>,
    /// The store, in words, and its production gate.
    gate: &'static str,
    /// (file, test fn). A test marked `#[ignore]` needs a database and is run by the disposable
    /// Postgres harness, not by `cargo test`.
    tests: &'static [(&'static str, &'static str)],
}

const EXECUTOR_RESUME: &str = "src/runtime_loop/resume.rs";

fn store_proofs() -> Vec<StoreProof> {
    vec![
        StoreProof {
            store: Some(DurableStore::Fold),
            gate: "fold writer: fold::WriterGate::decide, fed zdr and ctx.route.is_waived() (execution-core); \
                   session-core refuses a fold for a run whose stored class is not persistent",
            tests: &[
                (
                    EXECUTOR_FOLD,
                    "the_writer_is_off_unless_switch_ledger_and_a_persistent_root_run_allow_it",
                ),
                (
                    EXECUTOR_LOOP,
                    "loop_fold_zdr_space_and_resumed_runs_never_save",
                ),
                (EXECUTOR_LOOP, "loop_fold_a_waived_run_never_saves"),
                (SESSION_CHECKPOINT, "a_zdr_or_waived_run_never_keeps_a_fold"),
            ],
        },
        StoreProof {
            store: Some(DurableStore::Consume),
            gate: "approval consume: CONSUME_APPROVAL_SQL joins runs.retention_class = 'persistent' (session-core)",
            tests: &[(
                SESSION_APPROVALS,
                "approval_consume_refuses_a_waived_run_and_a_zdr_run",
            )],
        },
        StoreProof {
            store: Some(DurableStore::Continue),
            gate: "/continue: continue_persistent_gate and the preflight, decided with the plain route (model-gateway)",
            tests: &[
                (
                    GATEWAY_SSE,
                    "only_a_persistent_credential_is_a_persistent_continue_turn",
                ),
                (
                    GATEWAY_SSE,
                    "a_non_persistent_turn_is_refused_at_the_handler_boundary",
                ),
                (
                    GATEWAY_SSE,
                    "continue_run_calls_the_persistent_gate_before_it_reads_anything",
                ),
            ],
        },
        StoreProof {
            store: None,
            gate: "loop approval minted at the pause site: run_pause_mode fed !req.zdr && !ctx.route.is_waived() (execution-core)",
            tests: &[
                (EXECUTOR_LOOP, "a_waived_run_never_offers_a_loop_approval"),
                (
                    EXECUTOR_LOOP,
                    "a_persistent_run_offers_a_loop_approval_only_with_the_switch_on",
                ),
                (
                    EXECUTOR_RESUME,
                    "a_pause_is_loop_owned_only_for_a_run_a_continue_can_reproduce",
                ),
            ],
        },
        StoreProof {
            store: Some(DurableStore::Park),
            gate: "park: NOT BUILT (WS03 S8). When it is, list the tests that drive its real gate here",
            tests: &[],
        },
    ]
}

/// The production files allowed to call the shared durable gate. Empty today: the matrix proves
/// the gate, and each store's own gate is proved by its tests in [`store_proofs`]. A store that
/// starts calling the shared gate is guarded by the matrix directly, which is good: move it out of
/// the doc claim above and add the file here.
const PRODUCTION_CALLERS_OF_THE_SHARED_GATE: [&str; 0] = [];

/// The tests of a store's gate exist, are tests, and are not commented out; every store the shared
/// gate knows has an entry; and no production code calls the shared gate without this list being
/// told, so the header of `loop_fold_zdr_matrix.rs` stays true.
fn store_proof_violations(
    proofs: &[StoreProof],
    file: &dyn Fn(&str) -> String,
    production: &[(String, String)],
) -> Vec<String> {
    let mut violations = Vec::new();
    // Every store the shared gate knows has an entry. The match makes a new variant a compile
    // error here, so it cannot be forgotten.
    for store in [
        DurableStore::Fold,
        DurableStore::Consume,
        DurableStore::Continue,
        DurableStore::Park,
    ] {
        match store {
            DurableStore::Fold
            | DurableStore::Consume
            | DurableStore::Continue
            | DurableStore::Park => {}
        }
        if !proofs.iter().any(|proof| proof.store == Some(store)) {
            violations.push(format!("{store:?} has no entry in store_proofs()"));
        }
    }
    for proof in proofs {
        if proof.tests.is_empty() && proof.store != Some(DurableStore::Park) {
            violations.push(format!("{} names no test that drives it", proof.gate));
        }
        for (path, name) in proof.tests {
            let source = Code::new(&file(path)).text;
            let attribute = format!(
                r"#\[(?:tokio::|sqlx::)?test(?:\([^\]]*\))?\]\s*(?:#\[[^\]]*\]\s*)*(?:pub\s+)?(?:async\s+)?fn\s+{}\s*[(<]",
                regex::escape(name)
            );
            let is_a_test = Regex::new(&attribute)
                .expect("a test name makes a valid pattern")
                .is_match(&source);
            if !is_a_test {
                violations.push(format!(
                    "{path} has no test `{name}` (renamed, removed, commented out or no longer a #[test]): it is one of the tests that drive {}",
                    proof.gate
                ));
            }
        }
    }
    let allowed: BTreeSet<&str> = PRODUCTION_CALLERS_OF_THE_SHARED_GATE.into_iter().collect();
    for (path, source) in production {
        let normal = path.replace('\\', "/");
        if normal.contains("crates/mp-contracts/src/durable_gate.rs")
            || allowed.iter().any(|ok| normal.ends_with(ok))
        {
            continue;
        }
        let text = Code::new(source).without_tests().text;
        if [
            "refuse_unless_persistent",
            "refuse_unless_persistent_class",
            "DurableStore",
        ]
        .iter()
        .any(|word| word_count(&text, word) > 0)
        {
            violations.push(format!(
                "{path} calls the shared durable gate in production: the matrix now guards it directly. \
                 Add the file to PRODUCTION_CALLERS_OF_THE_SHARED_GATE and correct the header of \
                 tests/loop_fold_zdr_matrix.rs"
            ));
        }
    }
    violations
}

fn gateway_sources() -> Vec<(String, String)> {
    rust_files("../model-gateway/src")
}

// ---------------------------------------------------------------------------------------
// Tests on the real tree.
// ---------------------------------------------------------------------------------------

fn assert_green(violations: &[String]) {
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn the_inline_chat_loop_has_no_resume_path() {
    assert_green(&inline_loop_violations(&read(TOOL_LOOP)));
}

#[test]
fn continue_is_one_route_bound_to_one_handler_and_the_gateway_builds_one_resume() {
    let sources = gateway_sources();
    assert_green(&continue_route_violations(&sources));
    assert_green(&single_resume_violations(&sources));
    // The route file is the one that binds it, and is among the files scanned.
    assert!(sources
        .iter()
        .any(|(path, _)| path.ends_with("http_routes.rs")));
    assert!(read(GATEWAY_ROUTES).contains("crate::sse::continue_run"));
}

#[test]
fn the_resume_the_gateway_builds_is_one_the_executor_accepts_and_the_executor_owns_the_segment_number(
) {
    assert_green(&builder_and_validator_violations(
        &read(GATEWAY_SSE),
        &read(EXECUTOR_GRPC),
        &read(EXECUTOR_LOOP),
    ));
}

#[test]
fn the_executor_refuses_a_zero_retention_caller_before_it_looks_at_a_resume() {
    assert_green(&run_agent_order_violations(&read(EXECUTOR_GRPC)));
}

#[test]
fn the_pause_site_mints_loop_for_not_zdr_and_not_waived_and_the_old_name_is_gone() {
    let mut every = Vec::new();
    for tree in SOURCE_TREES {
        every.extend(rust_files(tree));
    }
    assert!(
        every.len() > 100,
        "the source trees were not found: {}",
        every.len()
    );
    assert_green(&pause_site_violations(&read(EXECUTOR_LOOP), &every));
}

#[test]
fn the_mode_words_mean_one_thing_on_every_plane() {
    assert_green(&mode_word_violations(
        &read(GATEWAY_SSE),
        &read(SESSION_APPROVAL_RPC),
        &read(EXECUTOR_GRPC),
    ));
}

#[test]
fn the_sql_that_stores_the_class_agrees_with_the_durable_gate_and_the_matrix() {
    assert_green(&retention_predicate_violations(
        &read(SESSION_APPROVALS),
        &read(SESSION_CHECKPOINT),
    ));
}

#[test]
fn the_gateway_continue_route_is_in_the_durable_mutation_inventory() {
    // The N7 inventory (`every_durable_mutation_refuses_a_zdr_or_waived_token_before_it_runs`)
    // is a test inside the gateway; it cannot name a route it does not list. The list is test
    // code, so it is read with comments stripped but tests kept.
    let posture = Code::new(&read(GATEWAY_POSTURE)).text;
    assert!(
        Regex::new(r#"\(\s*"sse\.rs"\s*,\s*"continue_run"\s*\)"#)
            .expect("pattern")
            .is_match(&posture),
        "zdr_posture.rs DURABLE_MUTATIONS does not name (\"sse.rs\", \"continue_run\")"
    );
}

#[test]
fn every_durable_loop_switch_is_set_on_the_services_that_read_it_and_defaults_off() {
    let compose = read(COMPOSE);
    assert_green(&compose_violations(&compose, &switch_table()));
    let mut files = vec![("docker-compose.yml".to_owned(), compose)];
    for sibling in [
        "docker-compose.override.yml",
        "docker-compose.production.yml",
        "docker-compose.release.yml",
        "docker-compose.bridges.yml",
    ] {
        files.push((
            sibling.to_owned(),
            read(&format!("../../../deploy/{sibling}")),
        ));
    }
    assert_green(&every_switch_defaults_off_violations(&files));
}

#[test]
fn the_fold_writer_is_off_for_a_zdr_or_waived_run_and_reads_its_switch_strictly() {
    assert_green(&fold_gate_violations(
        &read(EXECUTOR_LOOP),
        &read(EXECUTOR_FOLD),
    ));
}

/// The honest scope of the posture matrix: it proves the shared gate, and each store's own gate is
/// proved by the tests named in `store_proofs()`. This fails if one of those tests goes away, or if
/// production starts calling the shared gate without the claim being updated.
#[test]
fn every_durable_store_names_the_tests_that_drive_its_real_gate() {
    let mut every = Vec::new();
    for tree in SOURCE_TREES {
        every.extend(rust_files(tree));
    }
    assert!(
        every.len() > 100,
        "the source trees were not found: {}",
        every.len()
    );
    assert_green(&store_proof_violations(
        &store_proofs(),
        &|path| read(path),
        &every,
    ));
}

#[test]
fn the_resume_switch_has_one_parser() {
    let mut every = Vec::new();
    for tree in SOURCE_TREES {
        every.extend(rust_files(tree));
    }
    assert_green(&one_resume_parser_violations(&every));
    // The shared parser exists where the const says it does, and is the strict one.
    let loop_fold = Code::new(&read("../../crates/mp-contracts/src/loop_fold.rs")).text;
    assert!(loop_fold.contains("pub fn loop_resume_enabled()"));
    assert!(
        Regex::new(r"pub fn loop_resume_enabled\(\)[^}]*switch_from\(")
            .expect("pattern")
            .is_match(&loop_fold),
        "the resume switch is parsed by the strict `switch_from`, not the lenient continue parser"
    );
}

#[test]
fn the_executor_consumes_through_one_literal_and_never_omits_the_call() {
    assert_green(&consume_request_violations(&rust_files("src")));
}

/// The executor never reads the client's `segment_no`, in the file that is handed the whole
/// `AgentResume` as much as in the two files the older guard watches.
#[test]
fn the_resume_module_never_reads_the_clients_segment_no() {
    assert_green(&resume_module_segment_violations(&read(EXECUTOR_RESUME)));
}

#[test]
fn the_continue_switch_has_one_parser() {
    let mut every = Vec::new();
    for tree in SOURCE_TREES {
        every.extend(rust_files(tree));
    }
    assert_green(&one_parser_violations(&every));
    // The shared parser exists where the const says it does.
    let loop_fold = read("../../crates/mp-contracts/src/loop_fold.rs");
    assert!(Code::new(&loop_fold)
        .text
        .contains("pub fn continue_after_approval_enabled()"));
}

// ---------------------------------------------------------------------------------------
// The guards can fail: each goes red on the regression it exists for.
// ---------------------------------------------------------------------------------------

/// `text` with the first `from` replaced by `to`; fails the test if `from` is not there, so
/// a refactor that renames the line makes the mutation fail loudly instead of passing vacuously.
fn mutate(text: &str, from: &str, to: &str) -> String {
    assert!(
        text.contains(from),
        "the mutation target `{from}` is gone: re-point this test"
    );
    text.replacen(from, to, 1)
}

/// As [`mutate`], but the first `from` after `anchor` (a name unique to the item under test).
fn mutate_in(text: &str, anchor: &str, from: &str, to: &str) -> String {
    let start = text
        .find(anchor)
        .unwrap_or_else(|| panic!("the anchor `{anchor}` is gone: re-point this test"));
    let at = text[start..].find(from).unwrap_or_else(|| {
        panic!("the mutation target `{from}` is gone after `{anchor}`: re-point this test")
    }) + start;
    format!("{}{}{}", &text[..at], to, &text[at + from.len()..])
}

fn must_flag(violations: Vec<String>, containing: &str) {
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(containing)),
        "expected a violation containing {containing:?}, got {violations:?}"
    );
}

#[test]
fn the_cleaner_reads_rust_the_way_the_guards_assume() {
    let source = r##"
// AgentResume in a line comment
/* AgentResume in /* a nested */ block comment */
fn real() { let s = "AgentResume { in a string } // not a comment"; let _c = '{'; let _r = r#"} }"#; }
#[cfg(test)]
mod tests { fn only_in_tests() { AgentResume {}; } }
fn after<'a>(x: &'a str) -> &'a str { x }
"##;
    let code = Code::new(source);
    assert!(!code.text.contains("line comment") && !code.text.contains("nested"));
    assert!(
        code.text.contains("in a string"),
        "string contents are kept in text"
    );
    assert!(
        !code.skel.contains("in a string"),
        "and blanked in the skeleton"
    );
    let prod = code.without_tests();
    assert!(
        !prod.text.contains("only_in_tests"),
        "a cfg(test) module is cut out"
    );
    assert!(
        prod.text.contains("fn after"),
        "and nothing after it is lost to brace confusion"
    );
    assert_eq!(
        prod.fn_body("real").map(|body| body.contains("let _c")),
        Some(true),
        "braces inside strings and chars do not end a body early"
    );
    assert!(prod.fn_body("only_in_tests").is_none());
}

#[test]
fn every_guard_goes_red_when_the_code_it_guards_regresses() {
    let tool_loop = read(TOOL_LOOP);
    let sse = read(GATEWAY_SSE);
    let grpc = read(EXECUTOR_GRPC);
    let agent = read(EXECUTOR_LOOP);
    let compose = read(COMPOSE);
    let routes = read(GATEWAY_ROUTES);
    let approvals = read(SESSION_APPROVALS);
    let checkpoint = read(SESSION_CHECKPOINT);
    let session_rpc = read(SESSION_APPROVAL_RPC);

    // The inline loop grows a resume path. A commented mention is not one.
    must_flag(
        inline_loop_violations(&format!(
            "{tool_loop}\nfn sneaky() {{ let _ = AgentResume::default(); }}"
        )),
        "AgentResume",
    );
    must_flag(
        inline_loop_violations(&format!(
            "{tool_loop}\nfn sneaky() {{ consume_approval(); }}"
        )),
        "consume_approval",
    );
    assert_green(&inline_loop_violations(&format!(
        "{tool_loop}\n// AgentResume: not a use\n"
    )));

    // A second /continue route, or the route bound to something else.
    let mut gateway = gateway_sources();
    let at = gateway
        .iter()
        .position(|(path, _)| path.ends_with("http_routes.rs"))
        .expect("routes file");
    let mut doubled = gateway.clone();
    doubled[at].1 = mutate(
        &routes,
        "post(crate::sse::continue_run)",
        "post(crate::sse::continue_run)).route(\"/v1/x/continue\", post(crate::sse::continue_run)",
    );
    must_flag(continue_route_violations(&doubled), "expected exactly one");
    let mut rebound = gateway.clone();
    rebound[at].1 = mutate(
        &routes,
        "post(crate::sse::continue_run)",
        "post(crate::sse::resume_stream)",
    );
    must_flag(
        continue_route_violations(&rebound),
        "not bound to post(crate::sse::continue_run)",
    );

    // A second AgentResume built another way.
    let sse_at = gateway
        .iter()
        .position(|(path, _)| path.ends_with("sse.rs") && !path.ends_with("_sse.rs"))
        .expect("sse file");
    gateway[sse_at].1 = format!(
        "{}\nfn rogue() -> AgentResume {{ AgentResume::default() }}",
        gateway[sse_at].1
    );
    must_flag(single_resume_violations(&gateway), "path call");

    // Builder and validator drift apart.
    must_flag(
        builder_and_validator_violations(
            &mutate(
                &sse,
                "mode: \"segment\".to_owned()",
                "mode: \"resume\".to_owned()",
            ),
            &grpc,
            &agent,
        ),
        "executor accepts only",
    );
    must_flag(
        builder_and_validator_violations(
            &mutate(&sse, "segment_no: 0,", "segment_no: 2,"),
            &grpc,
            &agent,
        ),
        "must send segment_no: 0",
    );
    must_flag(
        builder_and_validator_violations(
            &sse,
            &mutate(
                &grpc,
                "resume.approval_id.is_empty()",
                "resume.approval_id.is_empty() || resume.segment_no != 0",
            ),
            &agent,
        ),
        "enforce_resume reads segment_no",
    );
    must_flag(
        builder_and_validator_violations(
            &sse,
            &mutate(&grpc, "\"segment\" | \"loop\"", "\"loop\""),
            &agent,
        ),
        "executor accepts only",
    );
    must_flag(
        builder_and_validator_violations(
            &sse,
            &grpc,
            &format!("{agent}\nfn rogue(r: &pb::AgentResume) -> u32 {{ r.segment_no }}"),
        ),
        "reads a client-supplied .segment_no",
    );

    // The executor looks at a resume before it refuses a zero-retention caller.
    let free = "enforce_persistence_free_execution(&caller, req.zdr, \"agent_run\")?;";
    let resume_line =
        "enforce_resume(&req, mp_contracts::loop_fold::continue_after_approval_enabled())?;";
    let swapped = mutate(
        &mutate(&grpc, free, ""),
        resume_line,
        &format!("{resume_line}\n        {free}"),
    );
    must_flag(
        run_agent_order_violations(&swapped),
        "before the zero-retention refusal",
    );

    // The pause site goes back to "not ZDR" alone.
    let mut every = rust_files("src");
    every.push(("synthetic".to_owned(), String::new()));
    must_flag(
        pause_site_violations(
            &mutate(&agent, "!req.zdr && !ctx.route.is_waived()", "!req.zdr"),
            &every,
        ),
        "not-ZDR AND not-waived",
    );
    let last = every.len() - 1;
    every[last].1 = "fn old() { let _ = continuation_mode_for(true, true); }".to_owned();
    must_flag(
        pause_site_violations(&agent, &every),
        "continuation_mode_for",
    );

    // A mode the gateway requires that nothing mints, and one session-core would refuse.
    must_flag(
        mode_word_violations(
            &mutate(
                &sse,
                "continuation_mode != \"loop\"",
                "continuation_mode != \"loop2\"",
            ),
            &session_rpc,
            &grpc,
        ),
        "which nothing mints",
    );
    must_flag(
        mode_word_violations(
            &sse,
            &mutate(
                &session_rpc,
                "\"\" | \"segment\" | \"loop\"",
                "\"\" | \"segment\"",
            ),
            &grpc,
        ),
        "session-core refuses the mode",
    );

    // The SQL opens for a class the gate refuses, or stops refusing an ended run or a consumed approval.
    must_flag(
        retention_predicate_violations(
            &mutate_in(
                &approvals,
                "const CONSUME_APPROVAL_SQL",
                "r.retention_class = 'persistent'",
                "r.retention_class = 'zdr_waived'",
            ),
            &checkpoint,
        ),
        "the durable gate refuses",
    );
    must_flag(
        retention_predicate_violations(
            &mutate_in(
                &approvals,
                "const CONSUME_APPROVAL_SQL",
                "r.status NOT IN ('completed', 'failed', 'cancelled')",
                "r.status <> 'x'",
            ),
            &checkpoint,
        ),
        "no longer refuses a run that has ended",
    );
    must_flag(
        retention_predicate_violations(
            &approvals,
            &mutate(
                &checkpoint,
                "retention != \"persistent\"",
                "retention == \"zdr\"",
            ),
        ),
        "the fold store refuses every class but",
    );

    // Compose: a switch missing from a service, defaulting on, or a new switch defaulting on.
    let table = switch_table();
    let gateway_line = "      AGENT_CONTINUE_AFTER_APPROVAL_ENABLED: ${AGENT_CONTINUE_AFTER_APPROVAL_ENABLED:-false}\n";
    assert!(
        compose.contains(gateway_line),
        "the compose switch line moved: re-point this test"
    );
    // The first occurrence is model-gateway's (it is declared before execution-core's).
    must_flag(
        compose_violations(&compose.replacen(gateway_line, "", 1), &table),
        "model-gateway does not set",
    );
    must_flag(
        compose_violations(&compose.replacen(":-false}", ":-true}", usize::MAX), &table),
        "must default to OFF",
    );
    // The in-place resume switch (WS03 S7b) is a row of its own on BOTH services that read it:
    // each line is `${EXECUTION_LOOP_RESUME_ENABLED:-false}`, and dropping either one, or a
    // literal `true` on either, is flagged. (The first occurrence is model-gateway's.)
    let resume_key = mp_contracts::loop_fold::LOOP_RESUME_ENV;
    let resume_line = format!("      {resume_key}: ${{{resume_key}:-false}}\n");
    assert_eq!(
        compose.matches(&resume_line).count(),
        2,
        "the compose resume switch lines moved: re-point this test"
    );
    let first_at = compose.find(&resume_line).expect("model-gateway line");
    let second_at = first_at
        + resume_line.len()
        + compose[first_at + resume_line.len()..]
            .find(&resume_line)
            .expect("execution-core line");
    let without_gateway = compose.replacen(&resume_line, "", 1);
    let without_executor = format!(
        "{}{}",
        &compose[..second_at],
        &compose[second_at + resume_line.len()..]
    );
    must_flag(
        compose_violations(&without_gateway, &table),
        &format!("model-gateway does not set {resume_key}"),
    );
    must_flag(
        compose_violations(&without_executor, &table),
        &format!("execution-core does not set {resume_key}"),
    );
    let literal_on = format!("      {resume_key}: true\n");
    must_flag(
        compose_violations(&compose.replacen(&resume_line, &literal_on, 1), &table),
        &format!("model-gateway sets {resume_key}: true"),
    );
    must_flag(
        every_switch_defaults_off_violations(&[(
            "x.yml".to_owned(),
            format!("{compose}\n      {resume_key}: true\n"),
        )]),
        resume_key,
    );
    must_flag(
        every_switch_defaults_off_violations(&[(
            "x.yml".to_owned(),
            format!("{compose}\n      EXECUTION_LOOP_NEW_ENABLED: ${{EXECUTION_LOOP_NEW_ENABLED:-true}}\n"),
        )]),
        "EXECUTION_LOOP_NEW_ENABLED",
    );
    assert_green(&every_switch_defaults_off_violations(&[(
        "x.yml".to_owned(),
        compose.clone(),
    )]));

    // The fold writer stops being off for a waived run, stops looking at zdr, or parses loosely.
    let fold = read(EXECUTOR_FOLD);
    assert_green(&fold_gate_violations(&agent, &fold));
    must_flag(
        fold_gate_violations(
            &mutate(&agent, "waived: ctx.route.is_waived(),", "waived: false,"),
            &fold,
        ),
        "without `waived: ctx.route.is_waived()`",
    );
    must_flag(
        fold_gate_violations(
            &agent,
            &mutate(&fold, "} else if self.waived {", "} else if false {"),
        ),
        "no longer reads `self.waived`",
    );
    must_flag(
        fold_gate_violations(
            &agent,
            &mutate(&fold, "} else if self.zdr {", "} else if false {"),
        ),
        "no longer reads `self.zdr`",
    );
    must_flag(
        fold_gate_violations(
            &agent,
            &mutate(
                &fold,
                "lf::switch_from(std::env::var(CHECKPOINT_ENV).ok().as_deref())",
                "std::env::var(CHECKPOINT_ENV).is_ok()",
            ),
        ),
        "shared strict parser",
    );

    // A store's own-gate test is renamed away, commented out, stops being a test, or a store
    // loses its entry; production starts calling the shared gate behind the header's back.
    let proofs = store_proofs();
    let real = |path: &str| read(path);
    assert_green(&store_proof_violations(&proofs, &real, &[]));
    let continue_test =
        "#[test]\n    fn only_a_persistent_credential_is_a_persistent_continue_turn";
    let sse_with = |replacement: &str| {
        let broken = mutate(&sse, continue_test, replacement);
        move |path: &str| {
            if path == GATEWAY_SSE {
                broken.clone()
            } else {
                read(path)
            }
        }
    };
    must_flag(
        store_proof_violations(
            &proofs,
            &sse_with(
                "#[test]\n    fn only_a_persistent_credential_is_a_persistent_continue_turn_x",
            ),
            &[],
        ),
        "has no test `only_a_persistent_credential_is_a_persistent_continue_turn`",
    );
    must_flag(
        store_proof_violations(
            &proofs,
            &sse_with("    fn only_a_persistent_credential_is_a_persistent_continue_turn"),
            &[],
        ),
        "no longer a #[test]",
    );
    must_flag(
        store_proof_violations(
            &proofs,
            &sse_with(
                "// #[test]\n    // fn only_a_persistent_credential_is_a_persistent_continue_turn",
            ),
            &[],
        ),
        "commented out",
    );
    let fold_only: Vec<StoreProof> = store_proofs()
        .into_iter()
        .filter(|proof| proof.store != Some(DurableStore::Consume))
        .collect();
    must_flag(
        store_proof_violations(&fold_only, &real, &[]),
        "Consume has no entry",
    );
    let mut emptied = store_proofs();
    emptied[0].tests = &[];
    must_flag(
        store_proof_violations(&emptied, &real, &[]),
        "names no test that drives it",
    );
    must_flag(
        store_proof_violations(
            &proofs,
            &real,
            &[(
                "services/x/src/lib.rs".to_owned(),
                "fn f(r: &Resolution) { let _ = refuse_unless_persistent(DurableStore::Fold, r); }"
                    .to_owned(),
            )],
        ),
        "calls the shared durable gate in production",
    );
    must_flag(
        store_proof_violations(
            &proofs,
            &real,
            &[(
                "services/x/src/lib.rs".to_owned(),
                "fn f(c: &str) { let _ = refuse_unless_persistent_class(store, c); }".to_owned(),
            )],
        ),
        "calls the shared durable gate in production",
    );
    // A mention in a comment or in a test module is not a production call.
    assert_green(&store_proof_violations(
        &proofs,
        &real,
        &[(
            "services/x/src/lib.rs".to_owned(),
            "// refuse_unless_persistent\n#[cfg(test)]\nmod t { fn f() { DurableStore::Fold; } }"
                .to_owned(),
        )],
    ));

    // A second parser of the switch.
    must_flag(
        one_parser_violations(&[(
            "services/x/src/lib.rs".to_owned(),
            "fn f() { std::env::var(\"AGENT_CONTINUE_AFTER_APPROVAL_ENABLED\"); }".to_owned(),
        )]),
        "names AGENT_CONTINUE_AFTER_APPROVAL_ENABLED itself",
    );

    // A second parser of the in-place resume switch, in production code or behind a macro or a
    // string; a mention in a comment or in a test module is not a read.
    let literal = format!(
        "fn f() {{ std::env::var(\"{}\"); }}",
        mp_contracts::loop_fold::LOOP_RESUME_ENV
    );
    must_flag(
        one_resume_parser_violations(&[("services/x/src/lib.rs".to_owned(), literal.clone())]),
        "names EXECUTION_LOOP_RESUME_ENABLED itself",
    );
    assert_green(&one_resume_parser_violations(&[
        ("../../crates/mp-contracts/src/loop_fold.rs".to_owned(), literal.clone()),
        (
            "services/x/src/lib.rs".to_owned(),
            format!("// {}\n#[cfg(test)]\nmod t {{ {literal} }}", mp_contracts::loop_fold::LOOP_RESUME_ENV),
        ),
    ]));

    // The executor builds a second ConsumeApprovalRequest, or asks for the call to be omitted.
    let mut executor_sources = rust_files("src");
    assert_green(&consume_request_violations(&executor_sources));
    let mut second = executor_sources.clone();
    second.push((
        "src/rogue.rs".to_owned(),
        "fn rogue() { let _ = pb::ConsumeApprovalRequest { omit_call: false, ..Default::default() }; }"
            .to_owned(),
    ));
    must_flag(consume_request_violations(&second), "2 ConsumeApprovalRequest literals");
    let mut omitting = executor_sources.clone();
    omitting.push((
        "src/rogue.rs".to_owned(),
        "fn rogue() { let _ = X { omit_call:  true }; }".to_owned(),
    ));
    must_flag(consume_request_violations(&omitting), "sets `omit_call: true`");
    // None at all is as wrong as two: the guard cannot go quiet by reading nothing.
    executor_sources.clear();
    must_flag(consume_request_violations(&executor_sources), "0 ConsumeApprovalRequest literals");
    // A literal in a test module, or named in a comment, is not production code.
    let mut quiet = rust_files("src");
    quiet.push((
        "src/rogue.rs".to_owned(),
        "// ConsumeApprovalRequest { omit_call: true }\n#[cfg(test)]\nmod t { fn f() { pb::ConsumeApprovalRequest { omit_call: true }; } }".to_owned(),
    ));
    assert_green(&consume_request_violations(&quiet));

    // resume.rs reads the client's segment_no: through the argument it is handed, or through
    // any other name for the resume. The fold's own and the claim response's are not reads of it.
    let resume_rs = read(EXECUTOR_RESUME);
    assert_green(&resume_module_segment_violations(&resume_rs));
    for rogue in [
        "fn rogue(resume: &pb::AgentResume) -> u32 { resume.segment_no }",
        "fn rogue(r: &pb::AgentResume) -> u32 { r.segment_no.max(2) }",
        "fn rogue(resume: &pb::AgentResume) -> u32 { resume\n    .segment_no }",
    ] {
        must_flag(
            resume_module_segment_violations(&format!("{resume_rs}\n{rogue}")),
            ".segment_no`: only the fold's own",
        );
    }
    must_flag(
        resume_module_segment_violations(&format!(
            "{resume_rs}\nfn rogue(resume: &pb::AgentResume) -> u32 {{ resume.segment_no }}"
        )),
        "resume.segment_no",
    );
    // A scan that reads nothing is itself a violation, so the guard cannot go quiet.
    must_flag(
        resume_module_segment_violations("fn only() {}"),
        "not reading the file",
    );
}
