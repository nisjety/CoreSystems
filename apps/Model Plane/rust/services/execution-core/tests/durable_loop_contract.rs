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
//!   test that is renamed, removed or commented out fails here;
//! * the gateway's pre-consume (WS03 S7c-B): one `ConsumeApprovalRequest`, spend only
//!   (`omit_call: true`), called in one place inside its bound, one detached task for the
//!   consume and the launch (`trait ApprovalConsumer: Send + Sync`), one read of the resume
//!   switch through `loop_resume_enabled()`, and no attestation field (`consumed_by_gateway`)
//!   anywhere: the executor's own consume is a same-claim replay it accepts only through the
//!   effect-ledger fence (`replay_is_fenced`), which is why the switches flip together;
//! * the frames of a continuation are gated on the gateway's own spend knowledge
//!   (`ApprovalSpend`), the five dispatch arms of `agentic_run_stream` keep their order, and
//!   every refusal the executor can send is a word of the gateway's closed vocabulary;
//! * a spent continuation's attempt is closed on the run subject by one attempt-level event
//!   (`RUN_CONTINUE_REFUSED`, for an executor refusal; unreachable and unknown keep
//!   `RUN_DISPATCH_DEGRADED`) that ends nothing, and every error frame after the executor's
//!   answer (the three run-end codes and the seven typed subscription failures) is final once
//!   the click is spent, with the lists the SPA mirrors pinned to what the stream sends;
//! * a delegated child never writes a fold: `run_rounds` hands the writer gate `ctx.depth`.
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
const GATEWAY_SUBSCRIPTION: &str = "../model-gateway/src/subscription_failure.rs";
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
const SESSION_TERMINALIZATION: &str = "../session-core/src/terminalization.rs";
const GATEWAY_PRECONSUME_TESTS: &str = "../model-gateway/tests/continue_preconsume_test.rs";
const COMPOSE: &str = "../../../deploy/docker-compose.yml";
/// The protobuf sources, `apps/Model Plane/proto`, scanned for an attestation field.
const PROTO_DIR: &str = "../../../proto";
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
        self.fn_span(name).map(|(open, close)| self.text[open + 1..close].to_owned())
    }

    /// As [`Code::fn_body`], over the blanked source: string contents are spaces, so a word
    /// inside a log message or an error text is never mistaken for a call.
    fn fn_body_skel(&self, name: &str) -> Option<String> {
        self.fn_span(name).map(|(open, close)| self.skel[open + 1..close].to_owned())
    }

    /// The indices of the braces of the first `fn name` that has a body.
    fn fn_span(&self, name: &str) -> Option<(usize, usize)> {
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
            return Some((open, close));
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
    let Some(stage_picker) = sse.fn_body("resume_mode_for") else {
        return vec!["sse.rs has no resume_mode_for: the one place the gateway names a stage".into()];
    };
    let Some(validator) = grpc.fn_body("enforce_resume") else {
        return vec!["grpc.rs has no enforce_resume validator".into()];
    };
    let accepted = Regex::new(r"matches!\(\s*resume\.mode\.as_str\(\)\s*,([^)]*)\)")
        .expect("pattern")
        .captures(&validator)
        .map(|captured| quoted(&captured[1]))
        .unwrap_or_default();
    // The builder asks for the stage it is GIVEN; the stages themselves are named in
    // `resume_mode_for` alone, and every one of them is a stage the executor accepts.
    if !Regex::new(r"\bmode\s*:\s*mode\s*\.to_owned\(\)")
        .expect("pattern")
        .is_match(&builder)
    {
        violations.push("continue_resume does not build its mode from its argument".into());
    }
    if quoted(&builder).iter().any(|word| !word.is_empty()) {
        violations.push(
            "continue_resume names a stage itself: only resume_mode_for names them".into(),
        );
    }
    let stated: BTreeSet<String> = quoted(&stage_picker)
        .into_iter()
        .filter(|word| !word.is_empty())
        .collect();
    for stage in &stated {
        if !accepted.contains(stage) {
            violations.push(format!(
                "the gateway asks for resume mode `{stage}` but the executor accepts only {accepted:?}"
            ));
        }
    }
    for required in ["segment", "loop"] {
        if !stated.contains(required) {
            violations.push(format!(
                "resume_mode_for no longer names the `{required}` stage"
            ));
        }
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
            "the /continue pre-consume and the loop stage (read once in sse.rs continue_run through loop_resume_enabled; gateway_resume_reader_violations ties this row to that reader)",
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

    // A literal, not the `-> WriterGate {` of the one fn that builds it.
    let literals: Vec<_> = Regex::new(r"(?:->\s*(?:\w+\s*::\s*)*)?\bWriterGate\s*\{")
        .expect("pattern")
        .find_iter(&agent.skel)
        .filter(|found| !found.as_str().starts_with("->"))
        .collect();
    if literals.len() == 1 {
        let open = literals[0].end() - 1;
        let body = matching_brace(&agent.skel, open).map_or("", |close| &agent.text[open..=close]);
        if !Regex::new(r"\bwaived\s*:\s*(?:ctx\.)?route\.is_waived\(\)")
            .expect("pattern")
            .is_match(body)
        {
            violations.push(
                "the loop builds its fold writer without `waived: route.is_waived()`".into(),
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
/// call to be left out: an `omit_call` that is anything but the literal `false` anywhere in
/// execution-core (`omit_call: flag`, `omit_call: !x`, the shorthand `omit_call,`, an assignment)
/// could starve the resume of the call it runs, and no test of a value can see a value that is
/// computed. So EVERY mention of `omit_call` in production code must be exactly `omit_call:
/// false` followed by the end of the field. And one literal means one place to read what the
/// executor asks for.
fn consume_request_violations(executor_sources: &[(String, String)]) -> Vec<String> {
    let literal = Regex::new(r"\bConsumeApprovalRequest\s*\{").expect("pattern");
    let mention = Regex::new(r"\bomit_call\b").expect("pattern");
    let only_false = Regex::new(r"^omit_call\s*:\s*false\s*[,}]").expect("pattern");
    let mut violations = Vec::new();
    let mut literals = 0;
    for (path, source) in executor_sources {
        let code = Code::new(source).without_tests();
        literals += literal.find_iter(&code.text).count();
        for found in mention.find_iter(&code.text) {
            if !only_false.is_match(&code.text[found.start()..]) {
                violations.push(format!(
                    "{path} sets `omit_call` to something other than the literal `false`: the executor needs the approved call back and never omits it"
                ));
            }
        }
    }
    if literals != 1 {
        violations.push(format!(
            "execution-core builds {literals} ConsumeApprovalRequest literals; there must be exactly one (the executor's consume)"
        ));
    }
    violations
}

/// The in-place resume's order and its one writer (WS03 S7b).
///
/// What makes the resume safe is the ORDER of three things inside `run_rounds`: the fence (the
/// seeded writer's one `pre_effect` save at the claimed seq) comes first, the release's
/// pre-reserve second, and the phase-2 dispatch (`join_all`) last. Two claimants of one pause hold
/// the same seq, and the fence's compare-and-set is what lets exactly one through; the
/// pre-reserve is what lets exactly one attempt of the approval dispatch. Neither is visible to a
/// test that does not race, so the order is read from the source:
///
/// * `FoldWriter::seeded(` is called once in the loop's production code, inside `run_rounds`
///   (a second seeded writer is a second opinion on the seq the fence expects);
/// * the writer's gate is built by the one `writer_gate` fn, whose `resumed` flag reads
///   `!in_place` (an in-place resume is not a stage-1 segment), is handed `in_place` by
///   `run_rounds` and `true, true` by the pre-claim decision (`restore_continuation`), and the
///   two places that decide the seeded writer's gate force the flag false themselves
///   (`FoldWriter::seeded`, `resume::in_place_writer_off`), so they cannot disagree;
/// * `save_fence(` is called once, before `reserve_release(`, which is called once, before the
///   phase-2 `join_all(`, all inside `run_rounds`;
/// * no production file outside mp-contracts names the resume switch's env var.
fn in_place_writer_violations(
    executor_sources: &[(String, String)],
    executor_loop: &str,
    executor_fold: &str,
    executor_resume: &str,
) -> Vec<String> {
    let mut violations = Vec::new();
    let agent = Code::new(executor_loop).without_tests();
    let fold = Code::new(executor_fold).without_tests();
    let resume = Code::new(executor_resume).without_tests();
    let count = |haystack: &str, pattern: &str| {
        Regex::new(pattern)
            .expect("pattern")
            .find_iter(haystack)
            .count()
    };

    let Some(rounds) = agent.fn_body_skel("run_rounds") else {
        violations.push("agent.rs has no `fn run_rounds`: re-point this guard".into());
        return violations;
    };

    // One seeded writer, built by the loop.
    let seeded = r"\bFoldWriter\s*::\s*seeded\s*\(";
    let seeded_total = count(&agent.skel, seeded);
    let seeded_in_rounds = count(&rounds, seeded);
    if seeded_total != 1 || seeded_in_rounds != 1 {
        violations.push(format!(
            "agent.rs calls `FoldWriter::seeded(` {seeded_total} times ({seeded_in_rounds} inside run_rounds); the in-place resume has exactly one seeded writer, built by run_rounds"
        ));
    }

    // The gate: one fn builds it, `resumed` reads `!in_place`.
    match agent.fn_body("writer_gate") {
        None => violations.push("agent.rs has no `fn writer_gate`".into()),
        Some(body) => {
            if !Regex::new(r"\bresumed\s*:[^,}]*!\s*in_place\b")
                .expect("pattern")
                .is_match(&body)
            {
                violations.push(
                    "the fold writer's gate does not read `!in_place` for `resumed`: an in-place resume is not a stage-1 segment, and its seeded writer would be off"
                        .into(),
                );
            }
        }
    }
    if !Regex::new(r"\bwriter_gate\s*\([^;]*\bin_place\s*,?\s*\)\s*;")
        .expect("pattern")
        .is_match(&rounds)
    {
        violations.push(
            "run_rounds does not build its writer's gate with `in_place` as the last argument of writer_gate(..)"
                .into(),
        );
    }
    match agent.fn_body_skel("restore_continuation") {
        None => violations.push("agent.rs has no `fn restore_continuation`".into()),
        Some(body) => {
            if !Regex::new(r"\bwriter_gate\s*\([^;]*\btrue\s*,\s*true\s*,?\s*\)")
                .expect("pattern")
                .is_match(&body)
            {
                violations.push(
                    "the pre-claim writer decision does not go through `writer_gate(.., true, true)`: it must be the gate the loop builds the seeded writer from"
                        .into(),
                );
            }
        }
    }
    // Both deciders force the stage-1 flag off themselves.
    if !Regex::new(r"WriterGate\s*\{\s*resumed\s*:\s*false\s*,\s*\.\.\s*gate\s*,?\s*\}")
        .expect("pattern")
        .is_match(&fold.skel)
    {
        violations.push(
            "FoldWriter::seeded no longer forces `resumed: false` on the gate it is given"
                .into(),
        );
    }
    match resume.fn_body_skel("in_place_writer_off") {
        Some(body)
            if Regex::new(r"WriterGate\s*\{\s*resumed\s*:\s*false\s*,\s*\.\.\s*gate\s*,?\s*\}")
                .expect("pattern")
                .is_match(&body) => {}
        _ => violations.push(
            "resume::in_place_writer_off no longer forces `resumed: false`: it and the seeded writer must be one decision"
                .into(),
        ),
    }

    // The order: fence, then the release's pre-reserve, then the dispatch.
    let fence = r"\bsave_fence\s*\(";
    let reserve = r"\breserve_release\s*\(";
    let join = r"\bjoin_all\s*\(";
    let fences = count(&rounds, fence);
    let fences_total = count(&agent.skel, fence);
    let reserves = count(&rounds, reserve);
    let reserves_total =
        count(&agent.skel, reserve).saturating_sub(count(&agent.skel, r"\bfn\s+reserve_release\b"));
    let joins = count(&rounds, join);
    if fences != 1 || fences_total != 1 {
        violations.push(format!(
            "agent.rs calls `save_fence(` {fences_total} times ({fences} inside run_rounds); the in-place resume writes exactly one fence, in run_rounds"
        ));
    }
    if reserves != 1 || reserves_total != 1 {
        violations.push(format!(
            "agent.rs calls `reserve_release(` {reserves_total} times ({reserves} inside run_rounds); the release is pre-reserved exactly once, in run_rounds"
        ));
    }
    if joins != 1 {
        violations.push(format!(
            "run_rounds has {joins} `join_all(` calls; the guard reads the one phase-2 dispatch"
        ));
    }
    let at = |pattern: &str| {
        Regex::new(pattern)
            .expect("pattern")
            .find(&rounds)
            .map(|found| found.start())
    };
    if let (Some(fence_at), Some(reserve_at), Some(join_at)) = (at(fence), at(reserve), at(join)) {
        if fence_at > reserve_at {
            violations.push(
                "`reserve_release(` comes before `save_fence(` in run_rounds: a claimant that has not won the fence must not take the release"
                    .into(),
            );
        }
        if reserve_at > join_at {
            violations.push(
                "the phase-2 `join_all(` comes before `reserve_release(` in run_rounds: a call would be dispatched before the release is reserved"
                    .into(),
            );
        }
    }

    // The switch has one parser.
    violations.extend(one_resume_parser_violations(executor_sources));
    violations
}

/// `resume.rs` holds `restore_in_place`, which is handed the client's whole `AgentResume`.
/// The executor derives the segment from the step that paused and NEVER reads the client's
/// `segment_no` (the gateway sends 0 and an old gateway sends 2: both must work).
///
/// The reads the file is allowed are an EXACT whitelist, by expression and by count, not by the
/// name a receiver happens to have: `fold.segment_no` twice (the fold the claim handed back, in
/// `check_fold`) and `response.segment_no` once (the claim response, in `restore_in_place_with`).
/// A client's resume renamed to `fold` or `response` is a fourth or third read and fails the
/// count. Everything else that names `segment_no` is refused: another receiver, a bare
/// `segment_no` (destructuring, a binding), an `AgentResume { .. }` pattern or literal, and any
/// field of the client's resume other than the two the claim needs (`claim_token`,
/// `action_fingerprint`). `resumed_step_id`'s own parameter is the one bare use.
fn resume_module_segment_violations(executor_resume: &str) -> Vec<String> {
    let code = Code::new(executor_resume).without_tests();
    let mut violations = Vec::new();

    // Taking the client's resume apart (or building one) by pattern reads a field with no
    // `.segment_no` to find.
    let pattern = Regex::new(r"\bAgentResume\s*\{").expect("pattern");
    if pattern.is_match(&code.text) {
        violations.push("resume.rs destructures or builds an AgentResume by pattern: it reads the client-supplied segment_no (or could) without a `.segment_no` to find; take only `resume.claim_token` and `resume.action_fingerprint`".into());
    }

    // Every field read of an argument named `resume` (the one name the client's resume has).
    let field = Regex::new(r"\bresume\s*\.\s*([A-Za-z_][A-Za-z0-9_]*)").expect("pattern");
    for captured in field.captures_iter(&code.text) {
        if !matches!(&captured[1], "claim_token" | "action_fingerprint") {
            violations.push(format!(
                "resume.rs reads `resume.{}`: only `claim_token` and `action_fingerprint` are read from the client's resume; the executor derives everything else from the consume",
                &captured[1]
            ));
        }
    }

    // Every `segment_no` there is, with the receiver it is read from, if any.
    let token = Regex::new(r"(?:\b([A-Za-z_][A-Za-z0-9_]*)\s*\.\s*)?\bsegment_no\b")
        .expect("pattern");
    let (mut folds, mut responses) = (0, 0);
    for captured in token.captures_iter(&code.text) {
        let whole = captured.get(0).expect("the whole match");
        match captured.get(1).map(|receiver| receiver.as_str()) {
            Some("fold") => folds += 1,
            Some("response") => responses += 1,
            Some(receiver) => violations.push(format!(
                "resume.rs reads `{receiver}.segment_no`: only the fold's own and the claim response's may be read; the executor derives the segment from the paused step and never reads the client-supplied AgentResume.segment_no"
            )),
            None => {
                // A bare `segment_no`: only `resumed_step_id`'s parameter and its `{segment_no}`.
                let before = code.text[..whole.start()].trim_end();
                let after = code.text[whole.end()..].trim_start();
                let parameter = after.starts_with(": u32") && (before.ends_with('(') || before.ends_with(','));
                let formatted = before.ends_with('{') && after.starts_with('}');
                if !(parameter || formatted) {
                    violations.push("resume.rs names a bare `segment_no` (a destructuring, a binding, a method on a reader): it must not read the client-supplied AgentResume.segment_no; only `resumed_step_id`'s own parameter is allowed".into());
                }
            }
        }
    }
    if folds + responses == 0 {
        violations.push("the scan found no fold or response `.segment_no` in resume.rs: it is not reading the file the guard exists for".into());
    } else if (folds, responses) != (2, 1) {
        violations.push(format!(
            "resume.rs reads `fold.segment_no` {folds} times and `response.segment_no` {responses} times; the whitelist is exactly 2 and 1 (check_fold's two reads of the claimed fold, and the claim response in restore_in_place_with): a new read is a read of a client-supplied `.segment_no` until this guard says otherwise"
        ));
    }
    violations
}

/// The in-place resume switch is parsed by the strict `switch_from` and by nothing else: the
/// one function in mp-contracts is a single call to it, with no lenient `*_from` parser beside
/// it in the body (the continue switch's `1`, `on` and `yes` would otherwise flip the resume).
fn resume_parser_strictness_violations(loop_fold_source: &str) -> Vec<String> {
    let code = Code::new(loop_fold_source).without_tests();
    let Some(body) = code.fn_body("loop_resume_enabled") else {
        return vec!["mp-contracts has no `fn loop_resume_enabled`".into()];
    };
    let calls: Vec<String> = Regex::new(r"\b([a-z_][a-z0-9_]*_from)\s*\(")
        .expect("pattern")
        .captures_iter(&body)
        .map(|captured| captured[1].to_owned())
        .collect();
    if calls == ["switch_from"] {
        Vec::new()
    } else {
        vec![format!(
            "loop_resume_enabled must parse with the strict `switch_from` alone (`true` and nothing else), not {calls:?}"
        )]
    }
}

// ---- WS03 S7c-B2: the gateway's pre-consume, the frames of a continuation, the coupled flip --

/// The words of the list literal of `const NAME: .. = &[ .. ];`, from comment-free source.
fn const_words(code: &Code, name: &str) -> Vec<String> {
    Regex::new(&format!(r"\bconst\s+{name}\s*:[^=]*=\s*&\[([^\]]*)\]"))
        .expect("pattern")
        .captures(&code.text)
        .map(|captured| {
            Regex::new(r#""([a-z0-9_]*)""#)
                .expect("pattern")
                .captures_iter(&captured[1])
                .map(|word| word[1].to_owned())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default()
}

/// The arguments of the call whose `(` is at `open` in the blanked source, as text, split at
/// the commas of its own level.
fn call_arguments(code: &Code, open: usize) -> Vec<String> {
    let bytes = code.skel.as_bytes();
    let mut depth = 0_i32;
    let mut from = open + 1;
    let mut arguments = Vec::new();
    for (index, byte) in bytes.iter().enumerate().skip(open) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    let last = code.text[from..index].trim();
                    if !last.is_empty() {
                        arguments.push(last.to_owned());
                    }
                    return arguments;
                }
            }
            b',' if depth == 1 => {
                arguments.push(code.text[from..index].trim().to_owned());
                from = index + 1;
            }
            _ => {}
        }
    }
    arguments
}

/// The braces of the first block whose header (a regex over the blanked source that ends in
/// the opening `{`) matches: an `impl`, a `trait`, an `enum`.
fn block_span(code: &Code, header: &str) -> Option<(usize, usize)> {
    let found = Regex::new(header).ok()?.find(&code.skel)?;
    let open = found.end().checked_sub(1)?;
    Some((open, matching_brace(&code.skel, open)?))
}

/// How many times `pattern` matches in `haystack`.
fn matches_of(haystack: &str, pattern: &str) -> usize {
    Regex::new(pattern)
        .expect("pattern")
        .find_iter(haystack)
        .count()
}

/// The innermost `if` whose CONDITION contains the byte `at`: the condition text, the text of
/// its braced body, and the body's brace span. The condition runs from the `if` to the first
/// `{` outside parentheses and brackets. `None` when `at` is in no `if` condition.
fn enclosing_if(code: &Code, at: usize) -> Option<(String, String, (usize, usize))> {
    let bytes = code.skel.as_bytes();
    let mut found = None;
    for keyword in Regex::new(r"\bif\b").expect("pattern").find_iter(&code.skel) {
        if keyword.end() > at {
            break;
        }
        let mut depth = 0_i32;
        let mut open = None;
        for (index, byte) in bytes.iter().enumerate().skip(keyword.end()) {
            match byte {
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth -= 1,
                b'{' if depth == 0 => {
                    open = Some(index);
                    break;
                }
                _ => {}
            }
        }
        let Some(open) = open else {
            continue;
        };
        if open <= at {
            continue;
        }
        let Some(close) = matching_brace(&code.skel, open) else {
            continue;
        };
        found = Some((
            code.text[keyword.end()..open].trim().to_owned(),
            code.text[open..=close].to_owned(),
            (open, close),
        ));
    }
    found
}

/// What the gateway's `/continue` may do with the approval it spends itself (WS03 S7c).
///
/// With the resume switch on, `continue_run` consumes the approval BEFORE it opens the stream,
/// and the order, the count and the detach of that flow are what make it safe, none of them
/// visible to a test that does not race. So they are read from the source:
///
/// * the gateway builds ONE `ConsumeApprovalRequest`, in `preconsume_request`, and it carries
///   `omit_call: true` (the gateway spends the click; it never asks session-core to decrypt
///   the approved call, and the executor's own consume is the one that gets the call back);
///   EVERY mention of `omit_call` in the gateway is that literal, and the one literal states
///   it itself (a field dropped from the literal while `omit_call: true` survives elsewhere
///   is a full consume);
/// * the consume RPC is called in one place, `impl ApprovalConsumer for GrpcApprovalConsumer`,
///   inside the 10 s bound (`consume_within_bound`), and the only thing read from its answer is
///   `.consumed`; nothing in the flow reads the call, the step, the authority or the replay
///   flags;
/// * the consume and the launch are ONE detached task: the pre-consume flow has exactly one
///   `tokio::spawn`, in `spawn_consume_then_launch`, and none in `continue_run`, so a client
///   that disconnects mid-consume cannot spend a click without the run being sent. The
///   `tokio::spawn` count is the textual half; the compile half is the next bullet;
/// * `trait ApprovalConsumer: Send + Sync`. The spawned task holds `&C` across an await, so
///   its future is `Send` only if `C: Sync`. Dropping `Sync` from the trait is a COMPILE
///   mutation (`spawn_consume_then_launch` stops building, E0277 on the `tokio::spawn`); the
///   in-memory mutation here only proves the guard names the supertraits;
/// * `continue_run` reads the resume switch ONCE (`loop_resume_enabled()`), builds the one
///   launch closure (the single `agentic_run_stream(` call), returns `Ok(launch())` with the
///   switch off, and only then reaches `spawn_consume_then_launch(`. The design's order
///   `continue_preflight( < spawn_consume_then_launch( < agentic_run_stream(` cannot hold with
///   one closure that serves both the off and the on path: the closure is defined first;
/// * the gateway attests nothing to the executor: the one `AgentResume` literal names exactly
///   the fields `mode`, `approval_id`, `claim_token`, `brief` and `segment_no`.
fn gateway_consume_violations(gateway_sources: &[(String, String)]) -> Vec<String> {
    let mut violations = Vec::new();
    let Some((sse_path, sse_source)) = gateway_sources
        .iter()
        .find(|(path, _)| path.replace('\\', "/").ends_with("/sse.rs"))
    else {
        return vec!["the gateway sources have no sse.rs: re-point this guard".into()];
    };
    let sse = Code::new(sse_source).without_tests();

    // One ConsumeApprovalRequest literal in the whole gateway, in preconsume_request.
    let literal = Regex::new(r"\bConsumeApprovalRequest\s*\{").expect("pattern");
    let consume_call = Regex::new(r"\.\s*consume_approval\s*\(").expect("pattern");
    let mut literals = Vec::new();
    let mut consume_calls = Vec::new();
    let mut omit_mentions = 0;
    for (path, source) in gateway_sources {
        let is_sse = path == sse_path;
        let owned;
        let code = if is_sse {
            &sse
        } else {
            owned = Code::new(source).without_tests();
            &owned
        };
        for found in literal.find_iter(&code.text) {
            // `-> .. ::ConsumeApprovalRequest {` is a return type followed by the body.
            let before = code.text[..found.start()]
                .trim_end_matches(|c: char| is_ident(c) || c == ':')
                .trim_end();
            if !before.ends_with("->") {
                literals.push((is_sse, found.start(), found.end() - 1));
            }
        }
        for found in consume_call.find_iter(&code.text) {
            consume_calls.push((is_sse, found.start()));
        }
        for found in Regex::new(r"\bomit_call\b")
            .expect("pattern")
            .find_iter(&code.text)
        {
            omit_mentions += 1;
            if !Regex::new(r"^omit_call\s*:\s*true\s*[,}]")
                .expect("pattern")
                .is_match(&code.text[found.start()..])
            {
                violations.push(format!(
                    "{path} sets `omit_call` to something other than the literal `true`: the gateway consumes for the spend only and never takes the approved call"
                ));
            }
        }
    }
    match literals.as_slice() {
        [(true, at, brace)] => {
            match sse.fn_span("preconsume_request") {
                Some((open, close)) if open < *at && *at < close => {}
                _ => violations.push(
                    "the gateway's one ConsumeApprovalRequest literal is not inside preconsume_request"
                        .into(),
                ),
            }
            // `omit_call: true` is a field OF that literal, not a mention somewhere else in the
            // gateway: a field dropped from the literal leaves a full consume that returns the
            // approved call, whatever `omit_call: true` survives elsewhere.
            let states_it = matching_brace(&sse.skel, *brace).is_some_and(|close| {
                Regex::new(r"\bomit_call\s*:\s*true\s*[,}]")
                    .expect("pattern")
                    .is_match(&sse.text[*brace..=close])
            });
            if !states_it {
                violations.push(
                    "the gateway's one ConsumeApprovalRequest literal does not state `omit_call: true` itself: a consume without it asks session-core for the whole approved call"
                        .into(),
                );
            }
        }
        more => violations.push(format!(
            "the gateway builds {} ConsumeApprovalRequest literals in production code, expected exactly one, in sse.rs preconsume_request",
            more.len()
        )),
    }
    if omit_mentions == 0 {
        violations.push(
            "the gateway's pre-consume never states `omit_call: true`: it would ask session-core for the whole approved call"
                .into(),
        );
    }

    // The consume RPC is called in one place, inside the bound, and its answer is read for
    // `.consumed` alone.
    let consumer = block_span(
        &sse,
        r"\bimpl\s+ApprovalConsumer\s+for\s+GrpcApprovalConsumer\s*\{",
    );
    match (consume_calls.as_slice(), consumer) {
        ([(true, at)], Some((open, close))) if open < *at && *at < close => {}
        (calls, _) => violations.push(format!(
            "the gateway calls `.consume_approval(` {} times in production code; the one call is in `impl ApprovalConsumer for GrpcApprovalConsumer`",
            calls.len()
        )),
    }
    match consumer {
        None => violations.push("sse.rs has no `impl ApprovalConsumer for GrpcApprovalConsumer`".into()),
        Some((open, close)) => {
            if !sse.skel[open..=close].contains("consume_within_bound(") {
                violations.push(
                    "the gateway's real consumer does not apply the bound (`consume_within_bound(`): a session-core that never answers would hold the click open"
                        .into(),
                );
            }
        }
    }
    match sse.fn_body_skel("consume_within_bound") {
        None => violations.push("sse.rs has no `fn consume_within_bound`".into()),
        Some(body) => {
            if matches_of(&body, r"\.\s*consumed\b") != 1 {
                violations.push(
                    "consume_within_bound must read `.consumed` of the answer exactly once: the won flag is the one thing the gateway uses"
                        .into(),
                );
            }
        }
    }

    // What the pre-consume flow may read, and where it may spawn.
    let reads = Regex::new(
        r"\.\s*(tool_input_json|tool_name|authority|step_id|same_claim|already_consumed)\b",
    )
    .expect("pattern");
    let spawn = r"\btokio\s*::\s*spawn\s*\(";
    let mut flow: Vec<(String, Option<String>, usize)> = [
        ("consume_within_bound", 0),
        ("consume_then_launch", 0),
        ("spawn_consume_then_launch", 1),
        ("preconsume_refusal", 0),
        ("preconsume_request", 0),
        ("continue_run", 0),
    ]
    .into_iter()
    .map(|(name, spawns)| (format!("fn {name}"), sse.fn_body_skel(name), spawns))
    .collect();
    flow.push((
        "impl ApprovalConsumer for GrpcApprovalConsumer".to_owned(),
        consumer.map(|(open, close)| sse.skel[open..=close].to_owned()),
        0,
    ));
    for (what, body, spawns) in &flow {
        let Some(body) = body else {
            violations.push(format!("sse.rs has no {what}: re-point this guard"));
            continue;
        };
        for found in reads.captures_iter(body) {
            violations.push(format!(
                "{what} reads `.{}` in the pre-consume flow: the gateway reads only the won flag of a consume",
                &found[1]
            ));
        }
        let found = matches_of(body, spawn);
        if found != *spawns {
            violations.push(format!(
                "{what} has {found} tokio::spawn( calls, expected {spawns}: the pre-consume flow has exactly one detached task, in spawn_consume_then_launch"
            ));
        }
    }

    // The supertraits the spawned task needs.
    match block_span(&sse, r"\btrait\s+ApprovalConsumer\s*:[^{]*\{") {
        None => violations.push("sse.rs has no `trait ApprovalConsumer` with supertraits".into()),
        Some((open, _)) => {
            let header_start = sse.skel[..open].rfind("trait").unwrap_or(0);
            let supertraits: BTreeSet<String> = sse.skel[header_start..open]
                .split_once(':')
                .map(|(_, rest)| {
                    rest.split('+')
                        .map(|name| name.trim().to_owned())
                        .collect()
                })
                .unwrap_or_default();
            for required in ["Send", "Sync"] {
                if !supertraits.contains(required) {
                    violations.push(format!(
                        "trait ApprovalConsumer no longer declares `{required}` as a supertrait (it declares {supertraits:?}): the spawned consume-and-launch task holds `&C` across an await and is `Send` only if `C: Sync`"
                    ));
                }
            }
        }
    }

    // continue_run: one read of the switch, one launch, the order of the flow.
    match (sse.fn_body_skel("continue_run"), sse.fn_body("continue_run")) {
        (Some(body), Some(text)) => {
            let count = |pattern: &str| matches_of(&body, pattern);
            let reads_switch = count(r"\bloop_resume_enabled\s*\(");
            if reads_switch != 1 {
                violations.push(format!(
                    "continue_run reads loop_resume_enabled() {reads_switch} times, expected exactly once: the stage, the pre-consume and the spend state must come from one read"
                ));
            }
            if !Regex::new(r"\blet\s+resume_enabled\s*=\s*[\w:]*loop_resume_enabled\s*\(\s*\)\s*;")
                .expect("pattern")
                .is_match(&body)
            {
                violations.push(
                    "continue_run does not bind the switch once as `let resume_enabled = ..loop_resume_enabled();`"
                        .into(),
                );
            }
            if text.contains("EXECUTION_LOOP_RESUME_ENABLED") || count(r"\benv\s*::\s*var\b") > 0 {
                violations.push(
                    "continue_run names the resume switch's env var: the gateway reads it through mp_contracts::loop_fold::loop_resume_enabled alone"
                        .into(),
                );
            }
            let launches = count(r"\bagentic_run_stream\s*\(");
            if launches != 1 {
                violations.push(format!(
                    "continue_run calls agentic_run_stream( {launches} times, expected exactly one (the launch closure)"
                ));
            }
            if count(r"\bspawn_consume_then_launch\s*\(") != 1 {
                violations.push(
                    "continue_run must call spawn_consume_then_launch( exactly once".into(),
                );
            }
            if count(r"\bconsume_then_launch\s*\(") > 0 {
                violations.push(
                    "continue_run calls consume_then_launch( directly: the consume and the launch must run in the detached task of spawn_consume_then_launch"
                        .into(),
                );
            }
            let at = |pattern: &str| {
                Regex::new(pattern)
                    .expect("pattern")
                    .find(&body)
                    .map(|found| found.start())
            };
            let off_path = at(r"\bif\s*!\s*resume_enabled\s*\{\s*return\s+Ok\s*\(\s*launch\s*\(\s*\)\s*\)\s*;?\s*\}");
            if off_path.is_none() {
                violations.push(
                    "continue_run no longer returns Ok(launch()) before any consume when the resume switch is off"
                        .into(),
                );
            }
            if let (Some(preflight), Some(switch), Some(stream), Some(off), Some(spawned)) = (
                at(r"\bcontinue_preflight\s*\("),
                at(r"\bloop_resume_enabled\s*\("),
                at(r"\bagentic_run_stream\s*\("),
                off_path,
                at(r"\bspawn_consume_then_launch\s*\("),
            ) {
                if !(preflight < switch && switch < stream && stream < off && off < spawned) {
                    violations.push(
                        "continue_run's order is no longer continue_preflight( < loop_resume_enabled() < the launch closure (agentic_run_stream() < the off-switch return < spawn_consume_then_launch("
                            .into(),
                    );
                }
            }
            if !Regex::new(r"\bagentic_run_stream\s*\([^;]*\bapproval_spend_for\s*\(\s*resume_enabled\s*\)\s*,?\s*\)")
                .expect("pattern")
                .is_match(&body)
            {
                violations.push(
                    "continue_run's launch does not state its spend as approval_spend_for(resume_enabled)"
                        .into(),
                );
            }
        }
        _ => violations.push("sse.rs has no `fn continue_run`: re-point this guard".into()),
    }

    // No attestation: the one AgentResume literal names the fields it always named.
    match sse.fn_body("continue_resume") {
        None => violations.push("sse.rs has no `fn continue_resume`".into()),
        Some(body) => {
            let allowed: BTreeSet<&str> =
                ["mode", "approval_id", "claim_token", "brief", "segment_no"].into();
            for field in Regex::new(r"(?:^|,|\{)\s*([a-z_]+)\s*:([^:]|$)")
                .expect("pattern")
                .captures_iter(&body)
            {
                if !allowed.contains(&field[1]) {
                    violations.push(format!(
                        "continue_resume names the field `{}` of AgentResume: the gateway attests nothing to the executor, it states the approval, the claim and the stage it asks for",
                        &field[1]
                    ));
                }
            }
        }
    }
    violations
}

/// The one place a dispatch arm of `agentic_run_stream` is: the `Ok(Ok(Err(status)))` pattern,
/// the text of its guard (what stands between the pattern and `=>`) and of its braced body.
struct DispatchArm {
    start: usize,
    end: usize,
    guard: String,
    body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArmRole {
    /// The click is spent by the gateway: never terminalized.
    Spent,
    /// The committed confirmed-rejection arm.
    Confirmed,
    /// A continuation the gateway did not pre-consume, refused with a closed token.
    Typed,
    /// The committed never-connected arm.
    Unreachable,
    /// The committed ambiguous-outcome arm.
    Final,
    Other,
}

/// The dispatch arms of `agentic_run_stream`, in source order, over comment-free source.
fn dispatch_arms(code: &Code) -> Vec<DispatchArm> {
    let Some((open, close)) = code.fn_span("agentic_run_stream") else {
        return Vec::new();
    };
    let pattern = Regex::new(r"\bOk\(Ok\(Err\(status\)\)\)").expect("pattern");
    let bytes = code.skel.as_bytes();
    let mut arms = Vec::new();
    for found in pattern.find_iter(&code.skel[open..close]) {
        let start = open + found.start();
        let after = open + found.end();
        // The guard runs to the first `=>` outside any bracket.
        let mut depth = 0_i32;
        let mut arrow = None;
        for index in after..close.saturating_sub(1) {
            match bytes[index] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b'=' if depth == 0 && bytes[index + 1] == b'>' => {
                    arrow = Some(index);
                    break;
                }
                _ => {}
            }
        }
        let Some(arrow) = arrow else {
            continue;
        };
        let Some(brace) = code.skel[arrow..close].find('{').map(|offset| arrow + offset) else {
            continue;
        };
        let Some(end) = matching_brace(&code.skel, brace) else {
            continue;
        };
        arms.push(DispatchArm {
            start,
            end: end + 1,
            guard: code.text[after..arrow].to_owned(),
            body: code.text[brace..=end].to_owned(),
        });
    }
    arms
}

fn arm_role(guard: &str) -> ArmRole {
    let guard = guard.trim();
    if Regex::new(r"^if\s+spend\.is_spent\(\)$")
        .expect("pattern")
        .is_match(guard)
    {
        ArmRole::Spent
    } else if guard.contains("is_confirmed_agent_dispatch_rejection(") {
        ArmRole::Confirmed
    } else if guard.contains("classify_continue_dispatch_failure(") {
        ArmRole::Typed
    } else if guard.contains("dispatch_never_reached_execution_core(") {
        ArmRole::Unreachable
    } else if guard.is_empty() {
        ArmRole::Final
    } else {
        ArmRole::Other
    }
}

const TERMINALIZE: &str = r"\bterminalize_agent_dispatch_rejection_authenticated\b";

/// What a frame of a continuation says about the spend is the GATEWAY's own knowledge
/// (`ApprovalSpend`), never the executor's request (S7c, D1: no attestation).
///
/// * the first dispatch arm is guarded by `spend.is_spent()` alone (not by `resume.is_some()`:
///   an unspent continuation must keep the committed arms), comes before the committed
///   confirmed-rejection arm, and neither terminalizes nor names `AgentDispatchFailure`: the
///   click is gone, the run's renewed recovery window ends it, and a refusal can be
///   `continuation_already_consumed` while another segment of the run is live;
/// * the request-construction failure checks `spend.is_spent()` before its terminalization;
/// * `ApprovalSpend::SpentByGateway` is named only where the spend is built or expressed
///   (`approval_spend_for`, `continue_failure_event`, `dispatch_unknown_event`, the enum and
///   `is_spent`): nothing in the stream claims it;
/// * the two callers of `agentic_run_stream` state their spend as `ApprovalSpend::NotSpent`
///   (an ordinary turn) and `approval_spend_for(resume_enabled)` (a continuation);
/// * the three ambiguous-outcome sites go through `dispatch_unknown_event(spend, ..)` and the
///   `connected` frame through `connected_payload(.., spend)`.
fn spend_gate_violations(gateway_sse: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let sse = Code::new(gateway_sse).without_tests();
    let Some(body) = sse.fn_body_skel("agentic_run_stream") else {
        return vec!["sse.rs has no agentic_run_stream".into()];
    };
    let arms = dispatch_arms(&sse);
    if arms.is_empty() {
        return vec!["agentic_run_stream has no `Ok(Ok(Err(status)))` dispatch arm: re-point this guard".into()];
    }
    let roles: Vec<ArmRole> = arms.iter().map(|arm| arm_role(&arm.guard)).collect();
    for arm in &arms {
        if Regex::new(r"\bresume\b").expect("pattern").is_match(&arm.guard) {
            violations.push(format!(
                "a dispatch arm is guarded by `{}`: the spend is the gateway's own knowledge (ApprovalSpend), never the executor's request",
                arm.guard.trim()
            ));
        }
    }
    match roles.iter().position(|role| *role == ArmRole::Spent) {
        None => violations.push(format!(
            "no dispatch arm is guarded by `spend.is_spent()` alone: a click the gateway spent would take the committed arms and be terminalized (guards: {:?})",
            arms.iter().map(|arm| arm.guard.trim().to_owned()).collect::<Vec<_>>()
        )),
        Some(at) => {
            let arm = &arms[at];
            match roles.iter().position(|role| *role == ArmRole::Confirmed) {
                Some(confirmed) if at > confirmed => violations.push(
                    "the spent arm comes after the is_confirmed_agent_dispatch_rejection arm: a spent click refused with InvalidArgument, Unauthenticated or PermissionDenied would be terminalized"
                        .into(),
                ),
                Some(_) => {}
                None => violations.push(
                    "the committed is_confirmed_agent_dispatch_rejection arm is gone".into(),
                ),
            }
            if Regex::new(TERMINALIZE).expect("pattern").is_match(&arm.body) {
                violations.push(
                    "the spent arm terminalizes the run: the gateway never terminalizes a continuation whose click it spent"
                        .into(),
                );
            }
            if arm.body.contains("AgentDispatchFailure") {
                violations.push("the spent arm names AgentDispatchFailure: it must not terminalize".into());
            }
            if !arm.body.contains("continue_failure_event(") {
                violations.push(
                    "the spent arm does not answer with continue_failure_event(..): every failure after a spend is a typed frame saying approval_spent"
                        .into(),
                );
            }
        }
    }

    // The request-construction failure checks the spend before it terminalizes.
    let terminalize = Regex::new(TERMINALIZE).expect("pattern");
    match (
        body.find("spend.is_spent()"),
        terminalize.find(&body).map(|found| found.start()),
    ) {
        (Some(checked), Some(terminalized)) if checked < terminalized => {}
        _ => violations.push(
            "agentic_run_stream terminalizes a request-construction failure without asking spend.is_spent() first"
                .into(),
        ),
    }

    // Nothing but the builders of the spend names SpentByGateway.
    let mut cuts = Vec::new();
    for name in ["approval_spend_for", "continue_failure_event", "dispatch_unknown_event", "is_spent"] {
        match sse.fn_span(name) {
            Some(span) => cuts.push(span),
            None => violations.push(format!("sse.rs has no `fn {name}`: re-point this guard")),
        }
    }
    match block_span(&sse, r"\benum\s+ApprovalSpend\s*\{") {
        Some(span) => cuts.push(span),
        None => violations.push("sse.rs has no `enum ApprovalSpend`".into()),
    }
    cuts.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    let mut remaining = sse.text.clone();
    for (open, close) in cuts {
        remaining.replace_range(open..=close, "");
    }
    if remaining.contains("SpentByGateway") {
        violations.push(
            "SpentByGateway is named outside approval_spend_for, continue_failure_event, dispatch_unknown_event and ApprovalSpend itself: something other than the pre-consume claims a spent click"
                .into(),
        );
    }

    // The two callers state their spend.
    let call = Regex::new(r"\bagentic_run_stream\s*\(").expect("pattern");
    let mut stated = Vec::new();
    for found in call.find_iter(&sse.skel) {
        if sse.skel[..found.start()].trim_end().ends_with("fn") {
            continue;
        }
        let arguments = call_arguments(&sse, found.end() - 1);
        stated.push(
            arguments
                .last()
                .map(|last| last.split_whitespace().collect::<String>())
                .unwrap_or_default(),
        );
    }
    stated.sort();
    if stated != ["ApprovalSpend::NotSpent", "approval_spend_for(resume_enabled)"] {
        violations.push(format!(
            "the callers of agentic_run_stream state their spend as {stated:?}, expected an ordinary turn's `ApprovalSpend::NotSpent` and a continuation's `approval_spend_for(resume_enabled)`"
        ));
    }

    // The ambiguous outcomes and the first frame.
    let unknown = matches_of(&body, r"\bdispatch_unknown_event\s*\(\s*spend\s*,");
    if unknown != 3 {
        violations.push(format!(
            "agentic_run_stream asks dispatch_unknown_event(spend, ..) {unknown} times, expected the three ambiguous-outcome sites"
        ));
    }
    if matches_of(&body, r"\bagent_dispatch_unavailable_event\s*\(") > 0 {
        violations.push(
            "agentic_run_stream builds agent_dispatch_unavailable_event( itself: after a spend a retry is a 409, so every ambiguous outcome goes through dispatch_unknown_event(spend, ..)"
                .into(),
        );
    }
    if matches_of(&body, r"\bconnected_payload\s*\([^)]*,\s*spend\s*\)") != 1 {
        violations.push(
            "the connected frame is not built by connected_payload(.., spend): a client must learn from the first frame that the click is spent"
                .into(),
        );
    }
    violations
}

/// A continuation the gateway did NOT pre-consume that the executor refuses with
/// `FailedPrecondition`, `Aborted` or `NotFound` (or a zero-retention trailer) is a typed,
/// non-retryable refusal frame: no `approval_spent`, no `RUN_DISPATCH_DEGRADED`, no
/// terminalization. The five arms keep their order: spent, committed confirmed-rejection,
/// typed refusal, committed unreachable, committed final. The typed arm must come AFTER the
/// confirmed one (an `InvalidArgument`, `Unauthenticated` or `PermissionDenied` refusal keeps
/// terminalizing, as it always did) and BEFORE the unreachable and final arms (which would
/// otherwise swallow it as a degraded dispatch).
fn refusal_arm_violations(gateway_sse: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let sse = Code::new(gateway_sse).without_tests();
    let arms = dispatch_arms(&sse);
    let roles: Vec<ArmRole> = arms.iter().map(|arm| arm_role(&arm.guard)).collect();
    let expected = [
        ArmRole::Spent,
        ArmRole::Confirmed,
        ArmRole::Typed,
        ArmRole::Unreachable,
        ArmRole::Final,
    ];
    for role in expected {
        let found = roles.iter().filter(|found| **found == role).count();
        if found != 1 {
            violations.push(format!(
                "the dispatch match of agentic_run_stream has {found} {role:?} arms, expected exactly one (arm roles in order: {roles:?})"
            ));
        }
    }
    let at = |role: ArmRole| roles.iter().position(|found| *found == role);
    for (earlier, later, why) in [
        (ArmRole::Confirmed, ArmRole::Typed, "an InvalidArgument, Unauthenticated or PermissionDenied refusal would stop terminalizing"),
        (ArmRole::Typed, ArmRole::Unreachable, "a refused continuation would be taken for a degraded dispatch"),
        (ArmRole::Unreachable, ArmRole::Final, "the ambiguous arm would swallow a never-connected dispatch"),
    ] {
        if let (Some(first), Some(second)) = (at(earlier), at(later)) {
            if second < first {
                violations.push(format!(
                    "the {later:?} dispatch arm comes before the {earlier:?} arm: {why}"
                ));
            }
        }
    }
    if let Some(typed) = at(ArmRole::Typed) {
        let arm = &arms[typed];
        if !Regex::new(r"\bcontinuing\b").expect("pattern").is_match(&arm.guard)
            || !Regex::new(r"!\s*spend\.is_spent\(\)").expect("pattern").is_match(&arm.guard)
        {
            violations.push(format!(
                "the typed-refusal arm is not guarded by `continuing` and `!spend.is_spent()` (guard: `{}`): it would take an ordinary run's refusal, or a spent click's",
                arm.guard.trim()
            ));
        }
        for (needle, why) in [
            ("terminalize_agent_dispatch_rejection_authenticated", "it must not terminalize: a refused or replayed segment can be refused while another segment of the run is live"),
            ("AgentDispatchFailure", "it must not terminalize"),
            ("RUN_DISPATCH_DEGRADED", "a refused or replayed segment is never a degraded dispatch"),
            ("approval_spent", "the gateway does not know whether the executor spent the click first"),
            ("SpentByGateway", "the gateway did not spend the click"),
        ] {
            if arm.body.contains(needle) {
                violations.push(format!("the typed-refusal arm names `{needle}`: {why}"));
            }
        }
        if !arm.body.contains("continue_refusal_event(") {
            violations.push("the typed-refusal arm does not answer with continue_refusal_event(..)".into());
        }
    }
    for (role, needle, what) in [
        (ArmRole::Confirmed, "AgentDispatchFailure::Rejected", "the committed confirmed-rejection arm no longer terminalizes as Rejected"),
        (ArmRole::Unreachable, "AgentDispatchFailure::Unreachable", "the committed unreachable arm no longer terminalizes as Unreachable"),
        (ArmRole::Final, "dispatch_unknown_event(", "the final arm no longer calls dispatch_unknown_event"),
    ] {
        if let Some(found) = at(role) {
            if !arms[found].body.contains(needle) {
                violations.push(what.to_owned());
            }
        }
    }
    violations
}

/// The run subject (`mp.v1.run.{run_id}.event`) is told how the attempt of a SPENT continuation
/// ended without starting a segment, and nothing more (WS03, the spent-frames unit).
///
/// `RUN_STARTED` is published before the dispatch result, so an executor refusal would leave a
/// started run with no closing event until session-core recovery ends it, about 15 minutes after
/// the spend. The gateway must close the ATTEMPT without ending the RUN: a refusal can be
/// `continuation_already_consumed` while another segment of the run is live, which is why the
/// run-level `RUN_DISPATCH_DEGRADED` is wrong for it and why a terminal event is out of the
/// question. So:
///
/// * `spent_dispatch_run_event` maps `Refused(_)` to `RUN_CONTINUE_REFUSED` and `Unreachable` and
///   `Unknown` to `RUN_DISPATCH_DEGRADED`, and nothing else;
/// * `RUN_CONTINUE_REFUSED` is written in one place (that map) and the map is asked in one place
///   (the spent dispatch arm), so it is published for a spent continuation alone, never for an
///   ordinary turn or for an unspent continuation's refusal, and a request that cannot be built
///   (which publishes nothing, `RUN_STARTED` included) cannot publish it either;
/// * the spent arm publishes exactly one run-subject event, before its frame, ignoring the
///   publisher's answer (a failed publish changes nothing the client sees), and names no
///   terminal type (`RUN_FAILED`, `RUN_COMPLETED`, `RUN_CANCELLED`), no `RUN_DISPATCH_DEGRADED`
///   and no `RUN_CONTINUE_REFUSED` of its own: the type comes from the map.
///
/// Every consumer of the subject (capability-core's run watch, task completion and session
/// review, insight-core's agent metrics, the Convex agent-run projection) reads the type string
/// and ignores one it does not know, so the new type breaks none of them and ends nothing.
fn spent_closing_violations(gateway_sse: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let sse = Code::new(gateway_sse).without_tests();
    let arms = dispatch_arms(&sse);
    let spent: Vec<&DispatchArm> = arms
        .iter()
        .filter(|arm| arm_role(&arm.guard) == ArmRole::Spent)
        .collect();
    match spent.as_slice() {
        [arm] => {
            let asked = matches_of(&arm.body, r"\bspent_dispatch_run_event\s*\(");
            if asked != 1 {
                violations.push(format!(
                    "the spent arm asks spent_dispatch_run_event(..) {asked} times, expected once: the closing event of a spent attempt is the pure map's alone"
                ));
            }
            let published = matches_of(&arm.body, r"\.\s*publish\s*\(");
            if published != 1 {
                violations.push(format!(
                    "the spent arm publishes {published} run-subject events, expected exactly one per attempt"
                ));
            }
            for word in [
                "RUN_FAILED",
                "RUN_COMPLETED",
                "RUN_CANCELLED",
                "RUN_DISPATCH_DEGRADED",
                "RUN_CONTINUE_REFUSED",
            ] {
                if arm.body.contains(word) {
                    violations.push(format!(
                        "the spent arm names `{word}` itself: the gateway never ends the run it spent the click of, and the type of the closing event comes from spent_dispatch_run_event"
                    ));
                }
            }
            match (arm.body.find(".publish("), arm.body.find("continue_failure_event(")) {
                (Some(published), Some(frame)) if published < frame => {}
                _ => violations.push(
                    "the spent arm does not publish before it sends its frame (or has no frame): the closing event stands after the click is known spent and before the client is told"
                        .into(),
                ),
            }
            if !Regex::new(r"let\s+_\s*=\s*state\s*\.\s*publisher\s*\.\s*publish\s*\(")
                .expect("pattern")
                .is_match(&arm.body)
            {
                violations.push(
                    "the spent arm does not ignore the publisher's answer (`let _ = state.publisher.publish(..)`): a failed publish must not change what the client sees"
                        .into(),
                );
            }
        }
        other => violations.push(format!(
            "agentic_run_stream has {} spent dispatch arms, expected exactly one",
            other.len()
        )),
    }
    for arm in arms.iter().filter(|arm| arm_role(&arm.guard) != ArmRole::Spent) {
        if arm.body.contains("RUN_CONTINUE_REFUSED")
            || Regex::new(r"\bspent_dispatch_run_event\s*\(")
                .expect("pattern")
                .is_match(&arm.body)
        {
            violations.push(format!(
                "a dispatch arm other than the spent one (guard `{}`) publishes the closing event of a spent attempt: a continuation nobody pre-consumed spent nothing",
                arm.guard.trim()
            ));
        }
    }
    match sse.fn_body("spent_dispatch_run_event") {
        None => violations.push("sse.rs has no `fn spent_dispatch_run_event`: re-point this guard".into()),
        Some(body) => {
            let refused = Regex::new(r#"Refused\(_\)\s*=>\s*"RUN_CONTINUE_REFUSED""#).expect("pattern");
            let other = Regex::new(
                r#"Unreachable\s*\|\s*ContinueDispatchFailure::Unknown\s*=>\s*\{?\s*"RUN_DISPATCH_DEGRADED""#,
            )
            .expect("pattern");
            if !refused.is_match(&body) {
                violations.push(
                    "spent_dispatch_run_event does not map `Refused(_)` to RUN_CONTINUE_REFUSED: an executor refusal would leave a started run with no closing event"
                        .into(),
                );
            }
            if !other.is_match(&body) {
                violations.push(
                    "spent_dispatch_run_event does not map `Unreachable` and `Unknown` to RUN_DISPATCH_DEGRADED: those two keep their run-level mark exactly"
                        .into(),
                );
            }
            let literals = matches_of(&body, r#""[A-Z_]+""#);
            if literals != 2 {
                violations.push(format!(
                    "spent_dispatch_run_event names {literals} event types, expected the two"
                ));
            }
        }
    }
    let written = matches_of(&sse.text, r#""RUN_CONTINUE_REFUSED""#);
    if written != 1 {
        violations.push(format!(
            "sse.rs writes \"RUN_CONTINUE_REFUSED\" {written} times, expected once (in spent_dispatch_run_event)"
        ));
    }
    let asked = matches_of(&sse.text, r"\bspent_dispatch_run_event\s*\(");
    if asked != 2 {
        violations.push(format!(
            "sse.rs mentions spent_dispatch_run_event( {asked} times, expected its definition and the one call of the spent arm"
        ));
    }
    violations
}

/// Every `error` frame `agentic_run_stream` can send after the executor's ANSWER is built by a
/// builder that takes the spend, and every code a spent continuation can send there is in a
/// closed list both ends pin (WS03, the spent-frames unit).
///
/// Three run-end frames (`agent_run_failed`, `agent_run_state_unknown`,
/// `agent_run_output_unavailable`) go through `run_end_error_event(.., spend)`; the typed
/// ChatGPT-subscription failure goes through `subscription_run_end_event(.., spend)`, which keeps
/// its code and text and changes only `retryable`, `run_id` and `approval_spent`. The stream
/// hand-builds neither a `ChatEvent::Error` nor a `subscription_error` after the answer (it would
/// say `retryable: true` of a click that is gone). The lists the SPA mirrors are the codes the
/// stream sends (`CONTINUE_SPENT_RUN_END_FRAMES`) and the codes `subscription_failure.rs`
/// defines (`CONTINUE_SPENT_SUBSCRIPTION_FRAMES`), so a code added on one side alone fails here
/// before it fails in the BFF test that pins the SPA to the gateway.
fn after_answer_frame_violations(gateway_sse: &str, subscription_failure: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let sse = Code::new(gateway_sse).without_tests();
    let run_end = const_words(&sse, "CONTINUE_SPENT_RUN_END_FRAMES");
    let subscription = const_words(&sse, "CONTINUE_SPENT_SUBSCRIPTION_FRAMES");
    if run_end.is_empty() || subscription.is_empty() {
        return vec!["sse.rs has no CONTINUE_SPENT_RUN_END_FRAMES or CONTINUE_SPENT_SUBSCRIPTION_FRAMES: re-point this guard".into()];
    }
    for (name, list) in [
        ("CONTINUE_SPENT_RUN_END_FRAMES", &run_end),
        ("CONTINUE_SPENT_SUBSCRIPTION_FRAMES", &subscription),
    ] {
        let unique: BTreeSet<&String> = list.iter().collect();
        if unique.len() != list.len() {
            violations.push(format!("{name} lists a code twice"));
        }
    }
    let Some(stream) = sse.fn_body("agentic_run_stream") else {
        violations.push("sse.rs has no agentic_run_stream".into());
        return violations;
    };
    let Some(answered) = stream.find("Ok(Ok(Ok(response))) => response") else {
        violations.push("agentic_run_stream has no `Ok(Ok(Ok(response))) => response` arm: re-point this guard".into());
        return violations;
    };
    let after = &stream[answered..];

    // The run-end codes the stream sends are the closed list.
    let sent: BTreeSet<String> = Regex::new(
        r#"run_end_error_event\s*\(\s*&request_id\s*,\s*&run\.run_id\s*,\s*"([a-z_]+)""#,
    )
    .expect("pattern")
    .captures_iter(after)
    .map(|captured| captured[1].to_owned())
    .collect();
    let listed: BTreeSet<String> = run_end.iter().cloned().collect();
    if sent != listed {
        violations.push(format!(
            "the run-end codes agentic_run_stream sends after the answer ({sent:?}) differ from CONTINUE_SPENT_RUN_END_FRAMES ({listed:?}): a code added on one side alone is a frame that stays retryable after a spend, or a dead entry"
        ));
    }
    let calls = matches_of(after, r"\brun_end_error_event\s*\(");
    if calls != run_end.len() {
        violations.push(format!(
            "agentic_run_stream asks run_end_error_event(..) {calls} times after the answer, expected once per code ({})",
            run_end.len()
        ));
    }
    let spent_flag = Regex::new(r"run_end_error_event\s*\([^)]*\bspend\s*,?\s*\)").expect("pattern");
    if matches_of(after, r"\brun_end_error_event\s*\(") != spent_flag.find_iter(after).count() {
        violations.push(
            "a run_end_error_event(..) call after the answer does not pass `spend` last: the frame would not know whether the click is gone"
                .into(),
        );
    }

    // Nothing after the answer builds an error frame by hand.
    for needle in ["ChatEvent::Error", "subscription_error("] {
        if after.contains(needle) {
            violations.push(format!(
                "agentic_run_stream names `{needle}` after the executor's answer: a hand-built error frame says `retryable` of a click the gateway spent"
            ));
        }
    }
    let site = matches_of(
        after,
        r"\bsubscription_run_end_event\s*\(\s*&request_id\s*,\s*&run\.run_id\s*,\s*&failure\s*,\s*message\s*,\s*spend\s*,?\s*\)",
    );
    if site != 1 {
        violations.push(format!(
            "agentic_run_stream builds the typed subscription failure frame through subscription_run_end_event(&request_id, &run.run_id, &failure, message, spend) {site} times, expected once"
        ));
    }

    // The builders apply the spend.
    match sse.fn_body("run_end_error_event") {
        Some(body) if body.contains("retryable: !spend.is_spent()") => {}
        _ => violations.push(
            "run_end_error_event does not say `retryable: !spend.is_spent()`: a run-end frame of a spent continuation would stay retryable"
                .into(),
        ),
    }
    match sse.fn_body("subscription_run_end_event") {
        Some(body)
            if body.contains("spend.is_spent()")
                && Regex::new(r#"payload\["retryable"\]\s*=\s*json!\(false\)"#)
                    .expect("pattern")
                    .is_match(&body)
                && body.contains("payload[\"approval_spent\"]")
                && body.contains("payload[\"run_id\"]") => {}
        _ => violations.push(
            "subscription_run_end_event does not withdraw the retry, name the run and say approval_spent for a spent continuation"
                .into(),
        ),
    }

    // The subscription list is the codes the failure module defines.
    let production = &subscription_failure[..subscription_failure
        .find("#[cfg(test)]")
        .unwrap_or(subscription_failure.len())];
    let defined: BTreeSet<String> =
        Regex::new(r#"(?m)^\s*pub const [A-Z_]+: &str = "([a-z_]+)";"#)
            .expect("pattern")
            .captures_iter(production)
            .map(|captured| captured[1].to_owned())
            .collect();
    let listed: BTreeSet<String> = subscription.iter().cloned().collect();
    if defined.is_empty() {
        violations.push("subscription_failure.rs defines no code: re-point this guard".into());
    } else if defined != listed {
        violations.push(format!(
            "CONTINUE_SPENT_SUBSCRIPTION_FRAMES ({listed:?}) differs from the codes subscription_failure.rs defines ({defined:?}): a typed failure added there alone stays a retryable frame after a spend"
        ));
    }
    for code in &subscription {
        if run_end.contains(code) {
            violations.push(format!("`{code}` is in both closed lists: a frame is read by one guard"));
        }
    }
    violations
}

/// The words of the continue vocabulary agree with what the executor can send (WS03 S7c).
///
/// The gateway names a refusal of a continuation by the message the executor sends it with
/// (`CONTINUE_EXECUTOR_MESSAGE_TOKENS`) and answers any other message with `continue_refused`,
/// never echoing it. So a new executor refusal that nobody adds to the list is silently
/// anonymous, and a token on the list that nothing sends is dead. Both directions are read:
///
/// * every message of a `failed_precondition` / `aborted` / `invalid_argument` /
///   `permission_denied` / `unauthenticated` status built on the executor's resume path
///   (`enforce_resume`, `consume_for_resume`, `released_call`, `build_segment`,
///   `begin_continuation`, `restore_continuation`, `run_rounds`, `select_stage`,
///   `in_place_scope`, and `Refusal` in resume.rs) is in the list; `Refusal`'s own words are in
///   the frame list, and all but the zero-retention one (whose status is the catalogue's) in
///   the message list. `internal(..)` statuses are not refusals: they classify as unknown;
/// * every token of the list is a quoted word in the executor's or session-core's production
///   code (`produced_elsewhere`: their comment-free production text, joined);
/// * the frame list is exactly the message list, the two derived codes
///   (`approval_not_found`, `durable_mutation_forbidden`) and the three the gateway makes.
fn continue_vocabulary_violations(
    gateway_sse: &str,
    executor_grpc: &str,
    executor_loop: &str,
    executor_resume: &str,
    produced_elsewhere: &str,
) -> Vec<String> {
    let mut violations = Vec::new();
    let sse = Code::new(gateway_sse).without_tests();
    let tokens = const_words(&sse, "CONTINUE_EXECUTOR_MESSAGE_TOKENS");
    let frames = const_words(&sse, "CONTINUE_FRAME_REFUSALS");
    if tokens.is_empty() || frames.is_empty() {
        return vec!["sse.rs has no CONTINUE_EXECUTOR_MESSAGE_TOKENS or CONTINUE_FRAME_REFUSALS: re-point this guard".into()];
    }
    for (name, list) in [("CONTINUE_EXECUTOR_MESSAGE_TOKENS", &tokens), ("CONTINUE_FRAME_REFUSALS", &frames)] {
        let unique: BTreeSet<&String> = list.iter().collect();
        if unique.len() != list.len() {
            violations.push(format!("{name} lists a token twice"));
        }
    }
    let token_set: BTreeSet<&str> = tokens.iter().map(String::as_str).collect();
    let frame_set: BTreeSet<&str> = frames.iter().map(String::as_str).collect();
    let mut expected_frames = token_set.clone();
    expected_frames.extend([
        "approval_not_found",
        "durable_mutation_forbidden",
        "continue_refused",
        "continue_unreachable",
        "continue_outcome_unknown",
    ]);
    if frame_set != expected_frames {
        violations.push(format!(
            "CONTINUE_FRAME_REFUSALS differs from the message tokens plus the five derived and gateway-made codes: only in the frame list {:?}, only in the expected set {:?}",
            frame_set.difference(&expected_frames).collect::<Vec<_>>(),
            expected_frames.difference(&frame_set).collect::<Vec<_>>()
        ));
    }

    // The statuses of the resume path.
    let agent = Code::new(executor_loop).without_tests();
    let grpc = Code::new(executor_grpc).without_tests();
    let resume = Code::new(executor_resume).without_tests();
    let carrier = Regex::new(
        r#"\b(?:failed_precondition|aborted|invalid_argument|permission_denied|unauthenticated)\s*\(\s*"([^"]*)""#,
    )
    .expect("pattern");
    let mut bodies: Vec<(String, String)> = Vec::new();
    for name in [
        "consume_for_resume",
        "released_call",
        "build_segment",
        "begin_continuation",
        "restore_continuation",
        "run_rounds",
    ] {
        match agent.fn_body(name) {
            Some(body) => bodies.push((format!("agent.rs fn {name}"), body)),
            None => violations.push(format!("agent.rs has no `fn {name}`: re-point this guard")),
        }
    }
    for name in ["select_stage", "in_place_scope"] {
        match resume.fn_body(name) {
            Some(body) => bodies.push((format!("resume.rs fn {name}"), body)),
            None => violations.push(format!("resume.rs has no `fn {name}`: re-point this guard")),
        }
    }
    match grpc.fn_body("enforce_resume") {
        Some(body) => bodies.push(("grpc.rs fn enforce_resume".to_owned(), body)),
        None => violations.push("grpc.rs has no `fn enforce_resume`".into()),
    }
    for (what, body) in &bodies {
        for captured in carrier.captures_iter(body) {
            let word = &captured[1];
            if !token_set.contains(word) {
                violations.push(format!(
                    "{what} refuses a continuation with `{word}`, which CONTINUE_EXECUTOR_MESSAGE_TOKENS does not list: the gateway would answer it as the anonymous `continue_refused`"
                ));
            }
        }
    }
    match block_span(&resume, r"\bimpl\s+Refusal\s*\{") {
        None => violations.push("resume.rs has no `impl Refusal`".into()),
        Some((open, close)) => {
            let words = quoted(&resume.text[open..=close]);
            if words.is_empty() {
                violations.push("resume.rs `impl Refusal` names no token: re-point this guard".into());
            }
            for word in words.iter().filter(|word| !word.is_empty()) {
                if !frame_set.contains(word.as_str()) {
                    violations.push(format!(
                        "resume.rs Refusal names `{word}`, which CONTINUE_FRAME_REFUSALS does not list"
                    ));
                }
                if word != "durable_mutation_forbidden" && !token_set.contains(word.as_str()) {
                    violations.push(format!(
                        "resume.rs Refusal sends `{word}` as a status message, which CONTINUE_EXECUTOR_MESSAGE_TOKENS does not list"
                    ));
                }
            }
        }
    }

    // Every token is sent by something.
    for token in &tokens {
        if !produced_elsewhere.contains(&format!("\"{token}\"")) {
            violations.push(format!(
                "CONTINUE_EXECUTOR_MESSAGE_TOKENS lists `{token}`, which no executor or session-core production code sends: a dead token"
            ));
        }
    }
    violations
}

/// The D1 pin: the gateway pre-consumes, so the executor's own consume of the same approval is
/// a same-claim replay, and the executor accepts a replay ONLY when the effect ledger fences a
/// second dispatch of the approved call. With the ledger off nothing does and the claim token is
/// client-chosen, so two concurrent continues with one token would both run the call. This is
/// why the gateway resume switch, the executor resume switch, the ledger, the continue switches
/// and the checkpoint switches are one coupled flip.
///
/// * every read of `.same_claim` in the executor's production code is followed by
///   `&& replay_is_fenced`, in the ONE shape the executor has: the accept is the plain read,
///   inside the condition of the refusal `if !( .. ) { .. continuation_already_consumed .. }`
///   (the replay is accepted only where the ledger fences it); the NEGATED read
///   (`&& !replay_is_fenced`) is allowed only as the condition of the operator's warn line
///   (`continue_replay_refused_ledger_off`), inside that refusal branch. A negated read
///   anywhere else, such as in the accept expression itself, is an inverted accept: a replay
///   accepted only when the ledger is OFF, exactly where nothing fences a second dispatch;
/// * `replay_is_fenced` is bound once, to `effect::ledger_enabled()`, and never assigned again.
fn resume_pair_violations(executor_sources: &[(String, String)]) -> Vec<String> {
    let mut violations = Vec::new();
    let mut reads = 0;
    let mut plain_reads = 0;
    let mut bindings = 0;
    let mut assignments = 0;
    let followed =
        Regex::new(r"^\.\s*same_claim\s*&&\s*(!?)\s*replay_is_fenced\b").expect("pattern");
    let binding = Regex::new(
        r"\blet\s+replay_is_fenced\s*=\s*(?:\w+\s*::\s*)*ledger_enabled\s*\(\s*\)\s*;",
    )
    .expect("pattern");
    for (path, source) in executor_sources {
        let code = Code::new(source).without_tests();
        let mut refusal_bodies = Vec::new();
        let mut negated = Vec::new();
        for found in Regex::new(r"\.\s*same_claim\b")
            .expect("pattern")
            .find_iter(&code.text)
        {
            reads += 1;
            let Some(shape) = followed.captures(&code.text[found.start()..]) else {
                violations.push(format!(
                    "{path} reads `.same_claim` without `&& replay_is_fenced` after it: a same-claim replay is accepted only where the effect ledger fences a second dispatch"
                ));
                continue;
            };
            let enclosing = enclosing_if(&code, found.start());
            if shape[1].is_empty() {
                plain_reads += 1;
                match enclosing {
                    Some((condition, body, span))
                        if condition.starts_with('!')
                            && body.contains("continuation_already_consumed") =>
                    {
                        refusal_bodies.push(span);
                    }
                    _ => violations.push(format!(
                        "{path} reads `.same_claim && replay_is_fenced` outside a refusal condition `if !( .. ) {{ .. continuation_already_consumed .. }}`: the replay is accepted only when the ledger fences it, and the executor refuses everything else"
                    )),
                }
            } else {
                negated.push(enclosing);
            }
        }
        // The negated read: the warn line's own `if`, inside a refusal branch found above.
        for enclosing in negated {
            let inside_refusal = |span: &(usize, usize)| {
                refusal_bodies
                    .iter()
                    .any(|(open, close)| *open < span.0 && span.1 < *close)
            };
            match enclosing {
                Some((condition, body, span))
                    if !condition.starts_with('!')
                        && body.contains("continue_replay_refused_ledger_off")
                        && !body.contains("Ok(")
                        && inside_refusal(&span) => {}
                _ => violations.push(format!(
                    "{path} reads `.same_claim && !replay_is_fenced` outside the refusal branch's warn line: an inverted accept, a replay accepted only when the ledger is OFF (nothing then fences a second dispatch of the approved call)"
                )),
            }
        }
        bindings += binding.find_iter(&code.text).count();
        assignments += matches_of(&code.text, r"\breplay_is_fenced\s*=([^=]|$)");
    }
    if reads == 0 {
        violations.push(
            "the executor no longer reads `.same_claim`: the same-claim replay rule moved, re-point this guard"
                .into(),
        );
    } else if plain_reads == 0 {
        violations.push(
            "no read of `.same_claim && replay_is_fenced` remains: the accept of a same-claim replay is gone or inverted, re-point this guard"
                .into(),
        );
    }
    if bindings != 1 || assignments != 1 {
        violations.push(format!(
            "replay_is_fenced must be bound exactly once, to effect::ledger_enabled(), and never assigned again: {bindings} such bindings, {assignments} assignments"
        ));
    }
    violations
}

/// The gateway reads the in-place resume switch through `mp_contracts::loop_fold::loop_resume_enabled`
/// ONCE, in `continue_run`, and through nothing else; and because it reads it, compose sets it
/// for model-gateway (a `switch_table()` row for the service), as it does for the executor.
fn gateway_resume_reader_violations(
    gateway_sources: &[(String, String)],
    table: &[(&'static str, String, &'static str)],
) -> Vec<String> {
    let mut violations = Vec::new();
    let env = mp_contracts::loop_fold::LOOP_RESUME_ENV;
    let mut readers = Vec::new();
    for (path, source) in gateway_sources {
        let code = Code::new(source).without_tests();
        for word in [env, "LOOP_RESUME_ENV"] {
            if code.text.contains(word) {
                violations.push(format!(
                    "{path} names {word} itself: the gateway reads the resume switch through mp_contracts::loop_fold::loop_resume_enabled alone"
                ));
            }
        }
        for found in Regex::new(r"\bloop_resume_enabled\s*\(")
            .expect("pattern")
            .find_iter(&code.text)
        {
            let in_continue_run = code
                .fn_span("continue_run")
                .is_some_and(|(open, close)| open < found.start() && found.start() < close);
            readers.push((path.clone(), in_continue_run));
        }
    }
    match readers.as_slice() {
        [(path, true)] if path.replace('\\', "/").ends_with("/sse.rs") => {}
        other => violations.push(format!(
            "the gateway must call loop_resume_enabled() exactly once, inside sse.rs continue_run; it calls it {} times ({other:?})",
            other.len()
        )),
    }
    if !readers.is_empty() {
        for service in ["model-gateway", "execution-core"] {
            if !table.iter().any(|(row, key, _)| *row == service && key == env) {
                violations.push(format!(
                    "switch_table() has no {service} row for {env}, but {service} reads it: compose must set it there so the coupled flip is one flip"
                ));
            }
        }
    }
    violations
}

/// Whether `text` names the attestation field in any casing a language gives it:
/// `consumed_by_gateway` (Rust, proto, a JSON tag), `ConsumedByGateway` (a Go field),
/// `consumedByGateway` (TypeScript). The underscores and the case are folded away.
fn names_attestation(text: &str) -> bool {
    text.chars()
        .filter(|c| *c != '_')
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .contains("consumedbygateway")
}

/// `source` with `//` line comments and `/* */` block comments removed: the comment syntax of
/// Go and TypeScript. A string that holds `//` loses the rest of its line, which can only
/// hide a mention in the same line from this scan and never invents one.
fn strip_c_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek().copied()) {
            ('/', Some('/')) => {
                for skipped in chars.by_ref() {
                    if skipped == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut previous = ' ';
                for skipped in chars.by_ref() {
                    if previous == '*' && skipped == '/' {
                        break;
                    }
                    if skipped == '\n' {
                        out.push('\n');
                    }
                    previous = skipped;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// The trees outside the Model Plane's Rust and proto sources that could carry an attestation
/// field: the Go bindings generated from the protos (and the hand-written Go beside them), the
/// BFF (`apps/gateway/src`, Rust) and the SPA (`src`, TypeScript). Relative to this crate; the
/// extensions each is read by.
const ATTESTATION_TREES: [(&str, &[&str], usize); 3] = [
    ("../../../go", &["go"], 100),
    ("../../../../Frontend Plane/verevonv3/apps/gateway/src", &["rs"], 20),
    ("../../../../Frontend Plane/verevonv3/src", &["ts", "tsx"], 100),
];

/// Every source of [`ATTESTATION_TREES`], as (path, text): generated Go included, tests
/// (`*_test.go`, `*.test.ts`, `*.spec.ts`) and dependency directories left out, this test
/// file and the docs being outside the trees. A tree with fewer files than its floor fails
/// loudly: a scan that found nothing proves nothing.
fn attestation_sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !["node_modules", "target", "vendor", "dist", ".git"].contains(&name.as_str()) {
                    walk(&path, extensions, out);
                }
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| extensions.contains(&ext))
                && !name.ends_with("_test.go")
                && !name.contains(".test.")
                && !name.contains(".spec.")
            {
                out.push(path);
            }
        }
    }
    let mut found = Vec::new();
    for (tree, extensions, floor) in ATTESTATION_TREES {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(tree);
        assert!(
            root.is_dir(),
            "{} is not a directory: re-point ATTESTATION_TREES",
            root.display()
        );
        let mut paths = Vec::new();
        walk(&root, extensions, &mut paths);
        assert!(
            paths.len() >= floor,
            "{} holds {} source files, expected at least {floor}: the attestation scan is not looking at the tree",
            root.display(),
            paths.len()
        );
        paths.sort();
        for path in paths {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
                .replace("\r\n", "\n");
            found.push((path.display().to_string(), text));
        }
    }
    found
}

/// No attestation (D1): the executor decides what the gateway did from its own consume, so no
/// field says "the gateway consumed it". The name is absent, in any casing, from every Rust
/// production source, every non-comment proto line, and (`other_sources`) the generated Go
/// bindings, the BFF and the SPA, comments and (for Rust) test modules aside. A field that
/// reappeared only on the wire's far side, in a Go struct or a TypeScript type, would be an
/// attestation the executor could never have meant to read.
fn no_attestation_violations(
    rust_sources: &[(String, String)],
    proto_sources: &[(String, String)],
    other_sources: &[(String, String)],
) -> Vec<String> {
    let mut violations = Vec::new();
    for (path, source) in rust_sources {
        if names_attestation(&Code::new(source).without_tests().text) {
            violations.push(format!(
                "{path} names consumed_by_gateway: there is no attestation field, the executor's consume is a same-claim replay (D1)"
            ));
        }
    }
    for (path, source) in proto_sources {
        let code: String = source
            .lines()
            .map(|line| line.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        if names_attestation(&code) {
            violations.push(format!(
                "{path} declares consumed_by_gateway: there is no attestation field (D1)"
            ));
        }
    }
    for (path, source) in other_sources {
        let code = if path.ends_with(".rs") {
            Code::new(source).without_tests().text
        } else {
            strip_c_comments(source)
        };
        if names_attestation(&code) {
            violations.push(format!(
                "{path} names consumed_by_gateway (in some casing): there is no attestation field in the Go bindings, the BFF or the SPA (D1)"
            ));
        }
    }
    violations
}

/// A delegated child never writes a fold, and a release holder is a resumed segment (WS03 S6/S7b).
///
/// `WriterGate::decide` refuses `depth > 0` (`OffReason::Delegated`), and `run_rounds` is the one
/// place the loop builds its gate, so what keeps a child out is `writer_gate` being handed
/// `ctx.depth`. A literal `0` there is invisible to every behavioural test that does not run a
/// delegated child with the writer on, and would let a child overwrite the root run's fold.
/// So the call is read in exactly this shape:
/// `writer_gate(req, &ctx.route, ctx.effect_ledger, ctx.depth, ctx.release.is_some(), in_place)`.
/// `ctx.release.is_some()` is the release flag (`holds_release`), `in_place` the resume's.
/// A child's context is built from `depth: parent.depth + 1`, and the gate's `decide` still
/// reads `self.depth > 0`.
fn writer_gate_argument_violations(executor_loop: &str, executor_fold: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let agent = Code::new(executor_loop).without_tests();
    let fold = Code::new(executor_fold).without_tests();
    let Some((open, close)) = agent.fn_span("run_rounds") else {
        return vec!["agent.rs has no `fn run_rounds`: re-point this guard".into()];
    };
    let calls: Vec<usize> = Regex::new(r"\bwriter_gate\s*\(")
        .expect("pattern")
        .find_iter(&agent.skel[open..close])
        .map(|found| open + found.end() - 1)
        .collect();
    match calls.as_slice() {
        [paren] => {
            let arguments: Vec<String> = call_arguments(&agent, *paren)
                .iter()
                .map(|argument| argument.split_whitespace().collect::<String>())
                .collect();
            let expected = [
                ("depth", 3, "ctx.depth", "a delegated child must never write folds"),
                (
                    "release flag",
                    4,
                    "ctx.release.is_some()",
                    "a release holder is a resumed segment",
                ),
                ("in-place flag", 5, "in_place", "an in-place resume is not a stage-1 segment"),
            ];
            if arguments.len() != 6 {
                violations.push(format!(
                    "run_rounds calls writer_gate with {} arguments, expected (req, &ctx.route, ctx.effect_ledger, ctx.depth, ctx.release.is_some(), in_place)",
                    arguments.len()
                ));
            } else {
                for (name, index, want, why) in expected {
                    if arguments[index] != want {
                        violations.push(format!(
                            "run_rounds passes `{}` as the writer gate's {name}, expected `{want}`: {why}",
                            arguments[index]
                        ));
                    }
                }
            }
        }
        other => violations.push(format!(
            "run_rounds calls writer_gate( {} times, expected the one place the loop builds its gate",
            other.len()
        )),
    }
    // The loop's gate is not the only one: the in-place resume builds a second, in
    // `restore_continuation`, to decide whether the writer it restores into is on. Its literal
    // shape is `writer_gate(req, route, runtime_loop::effect::ledger_enabled(), 0, true, true)`
    // and each literal is right for a reason, not by accident: depth `0` because only the ROOT
    // run is resumed (a resume is a RunAgent-level request, and a delegated child's context is
    // built from `depth: parent.depth + 1` and never resumes); release `true` and in-place
    // `true` because this IS the resumed segment and it is in place, so the gate's `resumed`
    // flag (`holds_release && !in_place`) is false: an in-place resume is not a stage-1
    // segment. A different literal, or a third call, changes who may write a fold, and no test
    // runs a delegated child or a resume with the writer on to notice. (The last two literals
    // are also read by `in_place_writer_violations`; the depth and the ledger are read only
    // here.)
    let every_call: Vec<usize> = Regex::new(r"\bwriter_gate\s*\(")
        .expect("pattern")
        .find_iter(&agent.skel)
        .filter(|found| !agent.skel[..found.start()].trim_end().ends_with("fn"))
        .map(|found| found.end() - 1)
        .collect();
    if every_call.len() != 2 {
        violations.push(format!(
            "agent.rs calls writer_gate( {} times in production code, expected exactly two: run_rounds (the loop's gate) and restore_continuation (the in-place resume's gate)",
            every_call.len()
        ));
    }
    match every_call.iter().find(|paren| {
        agent
            .fn_span("restore_continuation")
            .is_some_and(|(open, close)| open < **paren && **paren < close)
    }) {
        None => violations.push(
            "restore_continuation no longer calls writer_gate(: the in-place resume's gate moved, re-point this guard"
                .into(),
        ),
        Some(paren) => {
            let arguments: Vec<String> = call_arguments(&agent, *paren)
                .iter()
                .map(|argument| argument.split_whitespace().collect::<String>())
                .collect();
            let expected = [
                ("request", "req"),
                ("route", "route"),
                ("effect ledger", "runtime_loop::effect::ledger_enabled()"),
                ("depth", "0"),
                ("release flag", "true"),
                ("in-place flag", "true"),
            ];
            if arguments.len() == expected.len() {
                for (index, (name, want)) in expected.iter().enumerate() {
                    if arguments[index] != *want {
                        violations.push(format!(
                            "restore_continuation passes `{}` as the in-place resume gate's {name}, expected `{want}`: only the root run is resumed, and an in-place resume is the resumed segment but not a stage-1 one",
                            arguments[index]
                        ));
                    }
                }
            } else {
                violations.push(format!(
                    "restore_continuation calls writer_gate with {} arguments, expected (req, route, runtime_loop::effect::ledger_enabled(), 0, true, true)",
                    arguments.len()
                ));
            }
        }
    }
    match agent.fn_body("writer_gate") {
        None => violations.push("agent.rs has no `fn writer_gate`".into()),
        Some(body) => {
            if !Regex::new(r"\bdepth\s*(?:,|:\s*depth\b)")
                .expect("pattern")
                .is_match(&body)
            {
                violations.push("writer_gate does not hand its `depth` argument to the gate".into());
            }
        }
    }
    if !Regex::new(r"\bdepth\s*:\s*parent\s*\.\s*depth\s*\+\s*1\b")
        .expect("pattern")
        .is_match(&agent.text)
    {
        violations.push(
            "a delegated child's context is no longer built with `depth: parent.depth + 1`"
                .into(),
        );
    }
    match fold.fn_body("decide") {
        Some(body) if Regex::new(r"\bself\s*\.\s*depth\s*>\s*0\b").expect("pattern").is_match(&body) => {}
        _ => violations.push("WriterGate::decide no longer refuses `self.depth > 0`".into()),
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
            gate: "approval consume: CONSUME_APPROVAL_SQL joins runs.retention_class = 'persistent' (session-core); the gateway's spend-only consume (omit_call) goes through the same statement",
            tests: &[
                (
                    SESSION_APPROVALS,
                    "approval_consume_refuses_a_waived_run_and_a_zdr_run",
                ),
                // The spend-only consume (WS03 S7c-A2): no call content leaves session-core, and
                // the winning transaction renews the run's recovery deadline. The DB-backed ones
                // are #[ignore] (the disposable-Postgres harness runs them, `cargo test` does not).
                (
                    SESSION_APPROVAL_RPC,
                    "omit_call_withholds_the_call_but_keeps_the_flags",
                ),
                (
                    SESSION_APPROVAL_RPC,
                    "without_omit_call_the_response_is_unchanged",
                ),
                (
                    SESSION_APPROVALS,
                    "a_renewal_statement_is_scoped_to_the_run_org_and_open_states",
                ),
                (
                    SESSION_APPROVALS,
                    "a_won_spend_only_consume_renews_the_recovery_deadline",
                ),
                (
                    SESSION_APPROVALS,
                    "a_lost_or_replayed_spend_only_consume_does_not_renew",
                ),
                (
                    SESSION_TERMINALIZATION,
                    "a_spent_click_is_not_recovered_before_the_executor_heartbeats",
                ),
            ],
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
                // The gateway's own pre-consume (WS03 S7c): the order of consume and launch, the
                // single detached task, the closed refusal of a lost consume, the typed body a
                // zero-retention refusal of the consume keeps, and the frame arms.
                (
                    GATEWAY_SSE,
                    "continue_run_consumes_through_the_spawned_helper_after_the_preflight",
                ),
                (
                    GATEWAY_SSE,
                    "a_consume_that_did_not_win_is_a_409_before_any_stream_opens",
                ),
                (
                    GATEWAY_SSE,
                    "a_dropped_receiver_cannot_spend_an_approval_without_dispatching",
                ),
                (
                    GATEWAY_SSE,
                    "a_zero_retention_refusal_of_the_consume_keeps_its_typed_body",
                ),
                (
                    GATEWAY_SSE,
                    "the_dispatch_arms_of_a_continuation_are_spent_then_confirmed_then_typed_then_the_committed_ones",
                ),
                (
                    GATEWAY_PRECONSUME_TESTS,
                    "the_gateway_consumes_the_approval_before_it_dispatches_the_run",
                ),
                (
                    GATEWAY_PRECONSUME_TESTS,
                    "a_lost_consume_is_a_409_and_dispatches_nothing",
                ),
                (
                    GATEWAY_PRECONSUME_TESTS,
                    "a_client_that_disconnects_mid_consume_still_gets_exactly_one_dispatch",
                ),
                (
                    GATEWAY_PRECONSUME_TESTS,
                    "frame_codes_pair_with_the_spend_state_per_switch_state",
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
    // A file is read and cleaned once, however many of its tests are named.
    let mut cleaned: std::collections::BTreeMap<&str, String> = std::collections::BTreeMap::new();
    for proof in proofs {
        if proof.tests.is_empty() && proof.store != Some(DurableStore::Park) {
            violations.push(format!("{} names no test that drives it", proof.gate));
        }
        for (path, name) in proof.tests {
            let source = cleaned
                .entry(*path)
                .or_insert_with(|| Code::new(&file(path)).text);
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
    let loop_fold = read("../../crates/mp-contracts/src/loop_fold.rs");
    assert!(Code::new(&loop_fold)
        .text
        .contains("pub fn loop_resume_enabled()"));
    assert_green(&resume_parser_strictness_violations(&loop_fold));
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

/// The in-place resume has one seeded writer, and fences, then pre-reserves the release, then
/// dispatches, in that order, inside `run_rounds`.
#[test]
fn the_in_place_resume_is_seeded_fenced_and_reserved_in_order() {
    let mut every = Vec::new();
    for tree in SOURCE_TREES {
        every.extend(rust_files(tree));
    }
    assert_green(&in_place_writer_violations(
        &every,
        &read(EXECUTOR_LOOP),
        &read(EXECUTOR_FOLD),
        &read(EXECUTOR_RESUME),
    ));
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

/// The protobuf sources, as (path, text).
fn proto_files() -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(PROTO_DIR);
    assert!(
        root.is_dir(),
        "{} is not a directory: re-point PROTO_DIR",
        root.display()
    );
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("the proto tree is readable").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "proto") {
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
                    .replace("\r\n", "\n");
                found.push((path.display().to_string(), text));
            }
        }
    }
    found.sort();
    found
}

/// The comment-free production text of the executor and of session-core, joined: what the
/// continue vocabulary's tokens are looked up in.
fn executor_and_session_production_text() -> String {
    let mut all = rust_files("src");
    all.extend(rust_files("../session-core/src"));
    all.iter()
        .map(|(_, source)| Code::new(source).without_tests().text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The gateway's pre-consume (WS03 S7c-B): one consume literal that spends only, one RPC call
/// inside the bound, one detached task, `Send + Sync`, one read of the switch.
#[test]
fn the_gateway_pre_consumes_once_spends_only_and_consumes_and_launches_in_one_detached_task() {
    assert_green(&gateway_consume_violations(&gateway_sources()));
}

/// What a frame of a continuation says about the spend is the gateway's own knowledge; the
/// executor attests nothing.
#[test]
fn the_frames_of_a_continuation_are_gated_on_the_gateways_own_spend_knowledge() {
    let sse = read(GATEWAY_SSE);
    assert_green(&spend_gate_violations(&sse));
    assert_green(&refusal_arm_violations(&sse));
}

/// A spent continuation's attempt is closed on the run subject by one attempt-level event that
/// ends nothing, and every error frame after the executor's answer is final once the click is
/// spent.
#[test]
fn a_spent_attempt_is_closed_on_the_run_subject_and_every_frame_after_the_answer_is_final() {
    let sse = read(GATEWAY_SSE);
    assert_green(&spent_closing_violations(&sse));
    assert_green(&after_answer_frame_violations(&sse, &read(GATEWAY_SUBSCRIPTION)));
}

/// Every refusal the executor can send is a word of the gateway's closed vocabulary, and every
/// word is sent by something.
#[test]
fn every_refusal_the_executor_can_send_is_in_the_gateways_closed_vocabulary() {
    assert_green(&continue_vocabulary_violations(
        &read(GATEWAY_SSE),
        &read(EXECUTOR_GRPC),
        &read(EXECUTOR_LOOP),
        &read(EXECUTOR_RESUME),
        &executor_and_session_production_text(),
    ));
}

/// The coupled flip (D1): the executor's replay rule, the one gateway reader of the switch, and
/// no attestation field anywhere.
#[test]
fn the_gateway_and_the_executor_flip_together_the_replay_rule_the_one_reader_no_attestation() {
    assert_green(&resume_pair_violations(&rust_files("src")));
    assert_green(&gateway_resume_reader_violations(
        &gateway_sources(),
        &switch_table(),
    ));
    let mut every = Vec::new();
    for tree in SOURCE_TREES {
        every.extend(rust_files(tree));
    }
    assert_green(&no_attestation_violations(
        &every,
        &proto_files(),
        &attestation_sources(),
    ));
}

/// A delegated child never writes a fold: `run_rounds` hands the writer gate `ctx.depth`.
#[test]
fn a_delegated_child_never_writes_a_fold_and_the_gate_is_handed_the_depth_and_the_release() {
    assert_green(&writer_gate_argument_violations(
        &read(EXECUTOR_LOOP),
        &read(EXECUTOR_FOLD),
    ));
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

/// `text` with the first match of `pattern` replaced by `to`; fails the test if nothing
/// matches, for the same reason [`mutate`] does. For a target whose spacing a formatter owns.
fn mutate_re(text: &str, pattern: &str, to: &str) -> String {
    let regex = Regex::new(pattern).expect("a mutation pattern is a valid regex");
    let found = regex
        .find(text)
        .unwrap_or_else(|| panic!("the mutation pattern `{pattern}` matches nothing: re-point this test"));
    format!("{}{}{}", &text[..found.start()], to, &text[found.end()..])
}

/// `sources` with the file whose path ends in `suffix` replaced by `text`.
fn replace_source(
    sources: &[(String, String)],
    suffix: &str,
    text: &str,
) -> Vec<(String, String)> {
    let mut changed = sources.to_vec();
    let at = changed
        .iter()
        .position(|(path, _)| path.replace('\\', "/").ends_with(suffix))
        .unwrap_or_else(|| panic!("no source ends in `{suffix}`: re-point this test"));
    changed[at].1 = text.to_owned();
    changed
}

/// The comment-free text of `source` with dispatch arm `from` of `agentic_run_stream` moved to
/// stand immediately before arm `before` (`from` > `before`, both indices into the arms in
/// source order). What a careless reordering of the match would do.
fn move_arm(source: &str, from: usize, before: usize) -> String {
    assert!(from > before, "move_arm moves an arm earlier");
    let code = Code::new(source);
    let arms = dispatch_arms(&code);
    assert!(from < arms.len(), "the match has no arm {from}: re-point this test");
    let moved = &code.text[arms[from].start..arms[from].end];
    format!(
        "{}{}\n{}{}",
        &code.text[..arms[before].start],
        moved,
        &code.text[arms[before].start..arms[from].start],
        &code.text[arms[from].end..]
    )
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
            &mutate_in(&sse, "fn resume_mode_for", "\"segment\"", "\"resume\""),
            &grpc,
            &agent,
        ),
        "executor accepts only",
    );
    must_flag(
        builder_and_validator_violations(
            &mutate_in(&sse, "fn resume_mode_for", "\"loop\"", "\"segment\""),
            &grpc,
            &agent,
        ),
        "no longer names the `loop` stage",
    );
    // The builder goes back to naming a stage itself, or stops using the one it is given.
    must_flag(
        builder_and_validator_violations(
            &mutate(&sse, "mode: mode.to_owned()", "mode: \"segment\".to_owned()"),
            &grpc,
            &agent,
        ),
        "does not build its mode from its argument",
    );
    must_flag(
        builder_and_validator_violations(
            &mutate(&sse, "mode: mode.to_owned()", "mode: mode.to_owned(), brief: \"loop\".to_owned()"),
            &grpc,
            &agent,
        ),
        "names a stage itself",
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
            &mutate(&agent, "waived: route.is_waived(),", "waived: false,"),
            &fold,
        ),
        "without `waived: route.is_waived()`",
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
    // ANY value of `omit_call` but the literal `false` is flagged, computed ones included: a
    // flag, a negation, the field shorthand, an assignment, an expression that merely starts
    // with `false`, `true` with any spacing.
    for rogue in [
        "fn rogue() { let _ = X { omit_call:  true }; }",
        "fn rogue() { let _ = X { omit_call:true }; }",
        "fn rogue(flag: bool) { let _ = X { omit_call: flag }; }",
        "fn rogue(x: bool) { let _ = X { omit_call: !x }; }",
        "fn rogue(omit_call: bool) { let _ = X { omit_call, ..Default::default() }; }",
        "fn rogue(x: &mut X, flag: bool) { x.omit_call = flag; }",
        "fn rogue(flag: bool) { let _ = X { omit_call: false || flag }; }",
        "fn rogue() { let _ = X { omit_call: ( true ) }; }",
    ] {
        let mut omitting = executor_sources.clone();
        omitting.push(("src/rogue.rs".to_owned(), rogue.to_owned()));
        must_flag(
            consume_request_violations(&omitting),
            "something other than the literal `false`",
        );
    }
    // The literal `false`, however it is spaced or ended, is the one thing that is fine.
    for fine in [
        "fn ok() { let _ = X { omit_call: false, y: 1 }; }",
        "fn ok() { let _ = X { omit_call : false }; }",
        "fn ok() { let _ = X { y: 1, omit_call:false }; }",
    ] {
        let mut allowed = executor_sources.clone();
        allowed.push(("src/fine.rs".to_owned(), fine.to_owned()));
        assert_green(&consume_request_violations(&allowed));
    }
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
    // The reads that are allowed are exactly the three the file makes, by count and not by the
    // name of a receiver: the client's resume under the name `fold` or `response` is one more.
    for (rogue, flagged) in [
        (
            "fn rogue(fold: &pb::AgentResume) -> u32 { fold.segment_no }",
            "reads `fold.segment_no` 3 times",
        ),
        (
            "fn rogue(response: &pb::AgentResume) -> u32 { response.segment_no }",
            "`response.segment_no` 2 times",
        ),
        (
            "fn rogue(claimed: &ClaimedFold) -> u32 { claimed.fold.segment_no }",
            "reads `fold.segment_no` 3 times",
        ),
        // Destructuring and every other way to get at the field without `.segment_no`.
        (
            "fn rogue(resume: &pb::AgentResume) -> u32 { let pb::AgentResume { segment_no, .. } = resume; *segment_no }",
            "destructures or builds an AgentResume",
        ),
        (
            "fn rogue(resume: &pb::AgentResume) -> u32 { let pb::AgentResume { segment_no, .. } = resume; *segment_no }",
            "bare `segment_no`",
        ),
        (
            "fn rogue(resume: pb::AgentResume) -> u32 { let n = resume.segment_no; n }",
            "reads `resume.segment_no`",
        ),
        (
            "fn rogue(r: &pb::AgentResume) -> u32 { r.clone().segment_no }",
            "bare `segment_no`",
        ),
        // Any other field of the client's resume than the two the claim needs.
        (
            "fn rogue(resume: &pb::AgentResume) -> String { resume.step_id.clone() }",
            "reads `resume.step_id`",
        ),
        (
            "fn rogue(resume: &pb::AgentResume) -> String { resume.approval_id.clone() }",
            "reads `resume.approval_id`",
        ),
    ] {
        must_flag(
            resume_module_segment_violations(&format!("{resume_rs}\n{rogue}")),
            flagged,
        );
    }
    // A comment, a test module and the two fields the claim needs are not reads.
    assert_green(&resume_module_segment_violations(&format!(
        "{resume_rs}\n// resume.segment_no and AgentResume {{ segment_no }}\n#[cfg(test)]\nmod t {{ fn f(resume: &pb::AgentResume) -> u32 {{ resume.segment_no }} }}\nfn fine(resume: &pb::AgentResume) -> (String, String) {{ (resume.claim_token.clone(), resume.action_fingerprint.clone()) }}"
    )));

    // The strict switch parser: the one function must be a single `switch_from` call. Any
    // lenient parser in its place, or beside it, is flagged (the continue switch also accepts
    // `1`, `on` and `yes`).
    let loop_fold = read("../../crates/mp-contracts/src/loop_fold.rs");
    assert_green(&resume_parser_strictness_violations(&loop_fold));
    must_flag(
        resume_parser_strictness_violations(&mutate_in(
            &loop_fold,
            "pub fn loop_resume_enabled()",
            "switch_from(std::env::var(LOOP_RESUME_ENV)",
            "continue_after_approval_from(std::env::var(LOOP_RESUME_ENV)",
        )),
        "strict `switch_from` alone",
    );
    must_flag(
        resume_parser_strictness_violations(&mutate_in(
            &loop_fold,
            "pub fn loop_resume_enabled()",
            "switch_from(std::env::var(LOOP_RESUME_ENV)",
            "let _ = switch_from(None); continue_after_approval_from(std::env::var(LOOP_RESUME_ENV)",
        )),
        "strict `switch_from` alone",
    );
    must_flag(
        resume_parser_strictness_violations("pub fn something_else() -> bool { true }"),
        "no `fn loop_resume_enabled`",
    );

    // The in-place resume's writer and its order. The real source is green; each regression a
    // careless edit would make is flagged.
    let mut every = Vec::new();
    for tree in SOURCE_TREES {
        every.extend(rust_files(tree));
    }
    let in_place = |sources: &[(String, String)], agent: &str, fold: &str, resume: &str| {
        in_place_writer_violations(sources, agent, fold, resume)
    };
    assert_green(&in_place(&every, &agent, &fold, &resume_rs));
    // A second seeded writer, or none.
    must_flag(
        in_place(
            &every,
            &format!(
                "{agent}\nfn sneaky() {{ let _ = runtime_loop::fold::FoldWriter::seeded(c, b, r, g, 0, 1); }}"
            ),
            &fold,
            &resume_rs,
        ),
        "calls `FoldWriter::seeded(` 2 times",
    );
    must_flag(
        in_place(
            &every,
            &mutate(
                &agent,
                "runtime_loop::fold::FoldWriter::seeded(",
                "runtime_loop::fold::FoldWriter::replayed(",
            ),
            &fold,
            &resume_rs,
        ),
        "calls `FoldWriter::seeded(` 0 times",
    );
    // The gate stops saying an in-place resume is not a stage-1 segment, or run_rounds stops
    // handing it `in_place`, or the pre-claim decision stops using the same gate.
    must_flag(
        in_place(
            &every,
            &mutate(&agent, "resumed: holds_release && !in_place,", "resumed: holds_release,"),
            &fold,
            &resume_rs,
        ),
        "does not read `!in_place`",
    );
    must_flag(
        in_place(
            &every,
            &mutate_in(
                &agent,
                "let gate = writer_gate(",
                "        in_place,\n    );",
                "        false,\n    );",
            ),
            &fold,
            &resume_rs,
        ),
        "`in_place` as the last argument",
    );
    must_flag(
        in_place(
            &every,
            &mutate(&agent, "ledger_enabled(), 0, true, true);", "ledger_enabled(), 0, true, false);"),
            &fold,
            &resume_rs,
        ),
        "pre-claim writer decision",
    );
    // The two deciders stop forcing the stage-1 flag off.
    must_flag(
        in_place(
            &every,
            &agent,
            &mutate(&fold, "resumed: false,\n            ..gate", "..gate"),
            &resume_rs,
        ),
        "no longer forces `resumed: false`",
    );
    must_flag(
        in_place(
            &every,
            &agent,
            &fold,
            &mutate_in(
                &resume_rs,
                "fn in_place_writer_off(",
                "resumed: false",
                "resumed: gate.resumed",
            ),
        ),
        "in_place_writer_off no longer forces",
    );
    // A second fence or release pre-reserve, outside run_rounds.
    must_flag(
        in_place(
            &every,
            &format!("{agent}\nfn sneaky() {{ writer.save_fence(x); }}"),
            &fold,
            &resume_rs,
        ),
        "calls `save_fence(` 2 times",
    );
    must_flag(
        in_place(
            &every,
            &format!("{agent}\nfn sneaky() {{ reserve_release(a, b); }}"),
            &fold,
            &resume_rs,
        ),
        "calls `reserve_release(` 2 times",
    );
    // The order. The release reserved before the fence: another claimant that lost the seq
    // would already hold the release.
    must_flag(
        in_place(
            &every,
            &mutate(
                &mutate(
                    &agent,
                    "folds.save_fence(open_round_fold(authority))",
                    "folds.save_fence_old(open_round_fold(authority))",
                ),
                "let released_step_replayed = std::sync::atomic::AtomicBool::new(false);",
                "let _ = folds.save_fence(open_round_fold(authority));\n        let released_step_replayed = std::sync::atomic::AtomicBool::new(false);",
            ),
            &fold,
            &resume_rs,
        ),
        "comes before `save_fence(`",
    );
    // A dispatch before the release is reserved.
    must_flag(
        in_place(
            &every,
            &mutate(
                &mutate(
                    &agent,
                    "match reserve_release(ctx, release).await {",
                    "match reserve_release_old(ctx, release).await {",
                ),
                "for dispatch in dispatches.iter().flatten() {",
                "let _ = reserve_release(ctx, release).await;\n            for dispatch in dispatches.iter().flatten() {",
            ),
            &fold,
            &resume_rs,
        ),
        "`join_all(` comes before `reserve_release(`",
    );
    // The switch's env var read by a second parser.
    let mut rogue = every.clone();
    rogue.push((
        "src/rogue.rs".to_owned(),
        format!(
            "fn on() -> bool {{ std::env::var(\"{}\").is_ok() }}",
            mp_contracts::loop_fold::LOOP_RESUME_ENV
        ),
    ));
    must_flag(
        in_place(&rogue, &agent, &fold, &resume_rs),
        "names EXECUTION_LOOP_RESUME_ENABLED itself",
    );
    // A comment or a test module that names the calls is not a call.
    assert_green(&in_place(
        &every,
        &format!(
            "{agent}\n// FoldWriter::seeded( save_fence( reserve_release(\n#[cfg(test)]\nmod t {{ fn f() {{ FoldWriter::seeded(a); w.save_fence(x); reserve_release(a, b); }} }}"
        ),
        &fold,
        &resume_rs,
    ));

    // ---- WS03 S7c-B2: the gateway's pre-consume, its frames and the coupled flip -----------

    let gateway = gateway_sources();
    assert_green(&gateway_consume_violations(&gateway));
    let consume =
        |text: &str| gateway_consume_violations(&replace_source(&gateway, "/sse.rs", text));
    // A second ConsumeApprovalRequest, in sse.rs or anywhere else in the gateway.
    must_flag(
        consume(&format!(
            "{sse}\nfn rogue() -> mp_contracts::model_plane::v1::ConsumeApprovalRequest {{ mp_contracts::model_plane::v1::ConsumeApprovalRequest {{ omit_call: true, ..Default::default() }} }}"
        )),
        "2 ConsumeApprovalRequest literals",
    );
    must_flag(
        gateway_consume_violations(&replace_source(
            &gateway,
            "/http_routes.rs",
            &format!(
                "{routes}\nfn rogue() {{ let _ = ConsumeApprovalRequest {{ omit_call: true, ..Default::default() }}; }}"
            ),
        )),
        "2 ConsumeApprovalRequest literals",
    );
    // The consume stops being spend-only: false, a computed value, or no value at all.
    for to in [
        "omit_call: false",
        "omit_call: !flag",
        "omit_call: flag",
        "omit_call: Default::default()",
    ] {
        must_flag(
            consume(&mutate_in(&sse, "fn preconsume_request(", "omit_call: true", to)),
            "something other than the literal `true`",
        );
    }
    // `omit_call: true` survives somewhere else in the gateway, but the field is gone from the
    // one literal: a full consume that returns the approved call. The mention count alone
    // (`omit_mentions`) stays satisfied by the survivor.
    must_flag(
        consume(&format!(
            "{}\nfn rogue() {{ let _ = X {{ omit_call: true }}; }}",
            mutate_in(&sse, "fn preconsume_request(", "        omit_call: true,\n", "")
        )),
        "does not state `omit_call: true` itself",
    );
    // The gateway starts reading more than the won flag, or calls the RPC from a second place,
    // or calls it without the bound.
    must_flag(
        consume(&mutate_in(
            &sse,
            "async fn consume_within_bound",
            "response.into_inner().consumed",
            "response.into_inner().same_claim",
        )),
        "reads `.same_claim`",
    );
    must_flag(
        consume(&format!(
            "{sse}\nfn rogue(mut c: C, r: R) {{ let _ = c.consume_approval(r); }}"
        )),
        "calls `.consume_approval(` 2 times",
    );
    must_flag(
        consume(&mutate_in(
            &sse,
            "impl ApprovalConsumer for GrpcApprovalConsumer {",
            "consume_within_bound(client.consume_approval(request)).await",
            "client.consume_approval(request).await.map(|answer| answer.into_inner().consumed)",
        )),
        "does not apply the bound",
    );
    // The detach: no tokio::spawn in the spawning helper, a second one elsewhere in the flow.
    must_flag(
        consume(&mutate_in(
            &sse,
            "fn spawn_consume_then_launch<",
            "tokio::spawn(",
            "run_inline(",
        )),
        "fn spawn_consume_then_launch has 0 tokio::spawn( calls",
    );
    must_flag(
        consume(&mutate_in(
            &sse,
            "async fn consume_then_launch<",
            "preconsume_refusal(consumer.consume(request).await)?;",
            "tokio::spawn(async {});\n    preconsume_refusal(consumer.consume(request).await)?;",
        )),
        "fn consume_then_launch has 1 tokio::spawn( calls",
    );
    must_flag(
        consume(&mutate_re(
            &sse,
            r"if\s*!\s*resume_enabled\s*\{",
            "tokio::spawn(async {});\n    if !resume_enabled {",
        )),
        "fn continue_run has 1 tokio::spawn( calls",
    );
    // `Sync` dropped from the trait. The textual mutation proves the guard names the supertraits;
    // the real consequence is a COMPILE error (not reproducible in memory): with `trait
    // ApprovalConsumer: Send` alone, `spawn_consume_then_launch` stops building, because the
    // spawned future holds `&C` across the consume's await and is `Send` only if `C: Sync`.
    must_flag(
        consume(&mutate(
            &sse,
            "trait ApprovalConsumer: Send + Sync {",
            "trait ApprovalConsumer: Send {",
        )),
        "no longer declares `Sync`",
    );
    must_flag(
        consume(&mutate(
            &sse,
            "trait ApprovalConsumer: Send + Sync {",
            "trait ApprovalConsumer: Sync {",
        )),
        "no longer declares `Send`",
    );
    // continue_run consumes inline, or reads the switch twice, or by name, or not at all.
    must_flag(
        consume(&mutate(
            &sse,
            "spawn_consume_then_launch(consumer, preconsume, launch).await",
            "consume_then_launch(&consumer, preconsume, launch).await",
        )),
        "calls consume_then_launch( directly",
    );
    must_flag(
        consume(&mutate_re(
            &sse,
            r"if\s*!\s*resume_enabled\s*\{",
            "if !mp_contracts::loop_fold::loop_resume_enabled() {",
        )),
        "reads loop_resume_enabled() 2 times",
    );
    must_flag(
        consume(&mutate_re(
            &sse,
            r"let resume_enabled = mp_contracts::loop_fold::loop_resume_enabled\(\);",
            "let resume_enabled = std::env::var(\"EXECUTION_LOOP_RESUME_ENABLED\").is_ok();",
        )),
        "names the resume switch's env var",
    );
    must_flag(
        consume(&mutate_re(
            &sse,
            r"if\s*!\s*resume_enabled\s*\{\s*return\s+Ok\(launch\(\)\);\s*\}",
            "",
        )),
        "no longer returns Ok(launch()) before any consume",
    );
    must_flag(
        consume(&mutate_re(
            &sse,
            r"if\s*!\s*resume_enabled\s*\{",
            "let _ = agentic_run_stream(a);\n    if !resume_enabled {",
        )),
        "calls agentic_run_stream( 2 times",
    );
    must_flag(
        consume(&mutate_in(
            &sse,
            "let launch = move || {",
            "approval_spend_for(resume_enabled)",
            "ApprovalSpend::SpentByGateway",
        )),
        "does not state its spend as approval_spend_for(resume_enabled)",
    );
    // The switch is read before the preflight: the order the flow depends on is gone.
    must_flag(
        consume(&mutate_in(
            &mutate_re(
                &sse,
                r"    let resume_enabled = mp_contracts::loop_fold::loop_resume_enabled\(\);\n",
                "",
            ),
            "pub(crate) async fn continue_run(",
            "    continue_preflight(\n",
            "    let resume_enabled = mp_contracts::loop_fold::loop_resume_enabled();\n    continue_preflight(\n",
        )),
        "continue_run's order is no longer",
    );
    // An attestation: the one AgentResume literal grows a field about the gateway's consume.
    must_flag(
        consume(&mutate_in(
            &sse,
            "fn continue_resume(",
            "segment_no: 0,",
            "segment_no: 0,\n        consumed_by_gateway: true,",
        )),
        "names the field `consumed_by_gateway`",
    );

    // The frames are gated on the gateway's own spend knowledge.
    assert_green(&spend_gate_violations(&sse));
    assert_green(&refusal_arm_violations(&sse));
    // The spent arm keyed on the resume, or on nothing it should be keyed on.
    must_flag(
        spend_gate_violations(&mutate_re(
            &sse,
            r"Ok\(Ok\(Err\(status\)\)\) if spend\.is_spent\(\) =>",
            "Ok(Ok(Err(status))) if resume.is_some() =>",
        )),
        "guarded by `if resume.is_some()`",
    );
    must_flag(
        spend_gate_violations(&mutate_re(
            &sse,
            r"Ok\(Ok\(Err\(status\)\)\) if spend\.is_spent\(\) =>",
            "Ok(Ok(Err(status))) if continuing =>",
        )),
        "no dispatch arm is guarded by `spend.is_spent()` alone",
    );
    // The spent arm after the committed confirmed-rejection arm (so a spent click refused with
    // InvalidArgument would be terminalized).
    must_flag(
        spend_gate_violations(&move_arm(&sse, 1, 0)),
        "the spent arm comes after the is_confirmed_agent_dispatch_rejection arm",
    );
    // The spent arm terminalizes.
    must_flag(
        spend_gate_violations(&mutate_in(
            &sse,
            "RunAgent of a pre-consumed continuation did not start",
            "let closing = build_stream_envelope(",
            "let _ = crate::session_flow::terminalize_agent_dispatch_rejection_authenticated(&state, &run, crate::session_flow::AgentDispatchFailure::Rejected, &model_bearer).await;\n                let closing = build_stream_envelope(",
        )),
        "the spent arm terminalizes the run",
    );
    // The request-construction failure terminalizes a spent click.
    must_flag(
        spend_gate_violations(&mutate_in(
            &sse,
            "RunAgent request construction rejected before dispatch",
            "if spend.is_spent() {",
            "if false {",
        )),
        "without asking spend.is_spent() first",
    );
    // Something other than the pre-consume claims a spent click: the first frame, an ordinary
    // turn's call.
    must_flag(
        spend_gate_violations(&mutate(
            &sse,
            "connected_payload(&run_id, &thread_id, &request_id, spend)",
            "connected_payload(&run_id, &thread_id, &request_id, ApprovalSpend::SpentByGateway)",
        )),
        "SpentByGateway is named outside",
    );
    must_flag(
        spend_gate_violations(&mutate_in(
            &sse,
            "// An ordinary turn is never a pre-consumed continuation.",
            "ApprovalSpend::NotSpent",
            "ApprovalSpend::SpentByGateway",
        )),
        "SpentByGateway is named outside",
    );
    // An ambiguous outcome builds the retryable frame itself, or the first frame stops saying
    // the click is spent.
    must_flag(
        spend_gate_violations(&mutate_in(
            &sse,
            "RunAgent transport outcome is ambiguous",
            "dispatch_unknown_event(spend, &request_id, &run.run_id)",
            "agent_dispatch_unavailable_event(&request_id, &run.run_id)",
        )),
        "builds agent_dispatch_unavailable_event( itself",
    );
    must_flag(
        spend_gate_violations(&mutate(
            &sse,
            "connected_payload(&run_id, &thread_id, &request_id, spend)",
            "connected_payload(&run_id, &thread_id, &request_id, ApprovalSpend::NotSpent)",
        )),
        "is not built by connected_payload(.., spend)",
    );

    // (c2) The typed-refusal arm for a continuation nobody pre-consumed.
    must_flag(
        refusal_arm_violations(&move_arm(&sse, 2, 1)),
        "the Typed dispatch arm comes before the Confirmed arm",
    );
    let typed_log = "\"Execution Core refused a continuation\");";
    must_flag(
        refusal_arm_violations(&mutate(
            &sse,
            typed_log,
            &format!(
                "{typed_log}\n                let _ = crate::session_flow::terminalize_agent_dispatch_rejection_authenticated(&state, &run, crate::session_flow::AgentDispatchFailure::Rejected, &model_bearer).await;"
            ),
        )),
        "the typed-refusal arm names `terminalize_agent_dispatch_rejection_authenticated`",
    );
    must_flag(
        refusal_arm_violations(&mutate(
            &sse,
            typed_log,
            &format!(
                "{typed_log}\n                let _ = build_stream_envelope(&request_id, \"RUN_DISPATCH_DEGRADED\", &org_id, &user_id, &model);"
            ),
        )),
        "the typed-refusal arm names `RUN_DISPATCH_DEGRADED`",
    );
    must_flag(
        refusal_arm_violations(&mutate(
            &sse,
            typed_log,
            &format!("{typed_log}\n                let _ = \"approval_spent\";"),
        )),
        "the typed-refusal arm names `approval_spent`",
    );
    must_flag(
        refusal_arm_violations(&mutate_re(
            &sse,
            r"if\s+continuing\s*&&\s*!spend\.is_spent\(\)\s*&&\s*matches!\(",
            "if matches!(",
        )),
        "not guarded by `continuing` and `!spend.is_spent()`",
    );
    must_flag(
        refusal_arm_violations(&mutate_re(
            &sse,
            r"&&\s*!spend\.is_spent\(\)\s*&&\s*matches!\(",
            "&& matches!(",
        )),
        "not guarded by `continuing` and `!spend.is_spent()`",
    );
    must_flag(
        refusal_arm_violations(&mutate_in(
            &sse,
            "RunAgent transport outcome is ambiguous",
            "dispatch_unknown_event(spend, &request_id, &run.run_id)",
            "agent_dispatch_unavailable_event(&request_id, &run.run_id)",
        )),
        "the final arm no longer calls dispatch_unknown_event",
    );
    // An arm removed: the typed arm's guard turned into something that is no role.
    must_flag(
        refusal_arm_violations(&mutate_re(
            &sse,
            r"classify_continue_dispatch_failure\(&status\),\s*ContinueDispatchFailure::Refused\(_\)\s*\)\s*=>",
            "true) =>",
        )),
        "0 Typed arms",
    );

    // The vocabulary agrees with what the executor can send.
    let produced = executor_and_session_production_text();
    let vocabulary = |sse: &str, grpc: &str, agent: &str, resume: &str| {
        continue_vocabulary_violations(sse, grpc, agent, resume, &produced)
    };
    assert_green(&vocabulary(&sse, &grpc, &agent, &resume_rs));
    // A new executor refusal nobody listed, on the consume path and in the validator and in
    // the refusal enum.
    must_flag(
        vocabulary(
            &sse,
            &grpc,
            &mutate(
                &agent,
                "tonic::Status::failed_precondition(\"approval_action_mismatch\")",
                "tonic::Status::failed_precondition(\"approval_brand_new_refusal\")",
            ),
            &resume_rs,
        ),
        "refuses a continuation with `approval_brand_new_refusal`",
    );
    must_flag(
        vocabulary(
            &sse,
            &mutate(
                &grpc,
                "Status::invalid_argument(\"resume_incomplete\")",
                "Status::invalid_argument(\"resume_new_shape\")",
            ),
            &agent,
            &resume_rs,
        ),
        "refuses a continuation with `resume_new_shape`",
    );
    must_flag(
        vocabulary(
            &sse,
            &grpc,
            &agent,
            &mutate_in(
                &resume_rs,
                "pub(crate) const fn as_str(self)",
                "Self::Forbidden => \"resume_forbidden\",",
                "Self::Forbidden => \"resume_forbidden_x\",",
            ),
        ),
        "Refusal sends `resume_forbidden_x`",
    );
    // A token dropped from the gateway's list, a dead token, a frame the list no longer has.
    must_flag(
        vocabulary(
            &mutate_in(
                &sse,
                "CONTINUE_EXECUTOR_MESSAGE_TOKENS: &[&str]",
                "    \"resume_forbidden\",\n",
                "",
            ),
            &grpc,
            &agent,
            &resume_rs,
        ),
        "Refusal sends `resume_forbidden`",
    );
    must_flag(
        vocabulary(
            &mutate_in(
                &sse,
                "CONTINUE_EXECUTOR_MESSAGE_TOKENS: &[&str]",
                "\"continue_disabled\",",
                "\"continue_disabled\",\n    \"never_sent_token\",",
            ),
            &grpc,
            &agent,
            &resume_rs,
        ),
        "lists `never_sent_token`",
    );
    must_flag(
        vocabulary(
            &mutate_in(
                &sse,
                "CONTINUE_FRAME_REFUSALS: &[&str]",
                "    \"continue_outcome_unknown\",\n",
                "",
            ),
            &grpc,
            &agent,
            &resume_rs,
        ),
        "CONTINUE_FRAME_REFUSALS differs",
    );

    // The D1 pin: the executor accepts a same-claim replay only through the ledger fence.
    let executor = rust_files("src");
    assert_green(&resume_pair_violations(&executor));
    let executor_with =
        |text: &str| replace_source(&executor, "runtime_loop/agent.rs", text);
    must_flag(
        resume_pair_violations(&executor_with(&mutate_re(
            &agent,
            r"&&\s*replay_is_fenced\s*\)\)",
            "))",
        ))),
        "without `&& replay_is_fenced`",
    );
    must_flag(
        resume_pair_violations(&executor_with(&mutate_re(
            &agent,
            r"let replay_is_fenced = super::effect::ledger_enabled\(\);",
            "let replay_is_fenced = true;",
        ))),
        "bound exactly once, to effect::ledger_enabled()",
    );
    must_flag(
        resume_pair_violations(&executor_with(&format!(
            "{agent}\nfn rogue() {{ let mut replay_is_fenced = false; replay_is_fenced = true; }}"
        ))),
        "bound exactly once",
    );
    must_flag(
        resume_pair_violations(&[(
            "src/other.rs".to_owned(),
            "fn f(c: &Consumed) -> bool { c.same_claim }".to_owned(),
        )]),
        "without `&& replay_is_fenced`",
    );
    must_flag(
        resume_pair_violations(&[]),
        "no longer reads `.same_claim`",
    );
    // The negated form is the refusal branch's warn line and nothing else. An INVERTED accept
    // (the replay accepted only when the ledger is OFF) used to pass, because the pattern had an
    // optional negation: the same text with the negation moved into the accept expression.
    must_flag(
        resume_pair_violations(&executor_with(&mutate_re(
            &agent,
            r"consumed\.same_claim\s*&&\s*replay_is_fenced\)\)",
            "consumed.same_claim && !replay_is_fenced))",
        ))),
        "an inverted accept",
    );
    // The warn line loses its negation: the plain read is then outside a refusal condition.
    must_flag(
        resume_pair_violations(&executor_with(&mutate_re(
            &agent,
            r"consumed\.same_claim\s*&&\s*!replay_is_fenced\s*\{",
            "consumed.same_claim && replay_is_fenced {",
        ))),
        "outside a refusal condition",
    );
    // A negated read somewhere that is not the warn line, and an accept written as a positive
    // `if`, are both flagged.
    must_flag(
        resume_pair_violations(&[(
            "src/other.rs".to_owned(),
            "fn f(c: &Consumed, replay_is_fenced: bool) -> bool { c.same_claim && !replay_is_fenced }"
                .to_owned(),
        )]),
        "an inverted accept",
    );
    must_flag(
        resume_pair_violations(&executor_with(&format!(
            "{agent}\nfn rogue(c: &Consumed, replay_is_fenced: bool) -> bool {{ if c.already_consumed && c.same_claim && replay_is_fenced {{ return true; }} false }}"
        ))),
        "outside a refusal condition",
    );
    // A comment or a test that names the pattern is not a read.
    assert_green(&resume_pair_violations(&executor_with(&format!(
        "{agent}\n// c.same_claim && !replay_is_fenced\n#[cfg(test)]\nmod t {{ fn f(c: &C, r: bool) -> bool {{ c.same_claim && !r }} }}"
    ))));

    // The gateway's one reader of the switch, and the compose row that goes with it.
    let table = switch_table();
    assert_green(&gateway_resume_reader_violations(&gateway, &table));
    let resume_env = mp_contracts::loop_fold::LOOP_RESUME_ENV;
    let reader = |text: &str, rows: &[(&'static str, String, &'static str)]| {
        gateway_resume_reader_violations(&replace_source(&gateway, "/sse.rs", text), rows)
    };
    must_flag(
        reader(
            &format!("{sse}\nfn rogue() -> bool {{ std::env::var(\"{resume_env}\").is_ok() }}"),
            &table,
        ),
        "names EXECUTION_LOOP_RESUME_ENABLED itself",
    );
    must_flag(
        reader(
            &format!(
                "{sse}\nfn rogue() -> bool {{ std::env::var(mp_contracts::loop_fold::LOOP_RESUME_ENV).is_ok() }}"
            ),
            &table,
        ),
        "names LOOP_RESUME_ENV itself",
    );
    must_flag(
        reader(
            &mutate_re(
                &sse,
                r"let resume_enabled = mp_contracts::loop_fold::loop_resume_enabled\(\);",
                "let resume_enabled = false;",
            ),
            &table,
        ),
        "it calls it 0 times",
    );
    must_flag(
        reader(
            &format!("{sse}\nfn second() -> bool {{ mp_contracts::loop_fold::loop_resume_enabled() }}"),
            &table,
        ),
        "it calls it 2 times",
    );
    for service in ["model-gateway", "execution-core"] {
        let without_row: Vec<(&'static str, String, &'static str)> = table
            .iter()
            .filter(|(row, key, _)| !(*row == service && key.as_str() == resume_env))
            .cloned()
            .collect();
        must_flag(
            reader(&sse, &without_row),
            &format!("no {service} row for {resume_env}"),
        );
    }

    // No attestation field in Rust production code, in a proto, in the generated Go bindings,
    // in the BFF or in the SPA, comments and tests aside.
    let rust = rust_files("src");
    let protos = proto_files();
    let others = attestation_sources();
    assert_green(&no_attestation_violations(&rust, &protos, &others));
    let mut rogue_rust = rust.clone();
    rogue_rust.push((
        "src/rogue.rs".to_owned(),
        "fn f(r: &AgentResume) -> bool { r.consumed_by_gateway }".to_owned(),
    ));
    must_flag(
        no_attestation_violations(&rogue_rust, &protos, &others),
        "src/rogue.rs names consumed_by_gateway",
    );
    let mut rogue_proto = protos.clone();
    rogue_proto.push((
        "x.proto".to_owned(),
        "message AgentResume {\n  bool consumed_by_gateway = 9;\n}".to_owned(),
    ));
    must_flag(
        no_attestation_violations(&rust, &rogue_proto, &others),
        "x.proto declares consumed_by_gateway",
    );
    // The same name in another casing, in Rust or in a proto, is still the name.
    let mut camel_rust = rust.clone();
    camel_rust.push((
        "src/camel.rs".to_owned(),
        "enum Flag { ConsumedByGateway }".to_owned(),
    ));
    must_flag(
        no_attestation_violations(&camel_rust, &protos, &others),
        "src/camel.rs names consumed_by_gateway",
    );
    // A Go binding, the BFF and the SPA: a field that reappears only on the far side of the wire.
    for (path, text) in [
        // A generated Go struct field, as protoc-gen-go writes it.
        (
            "go/gen/model_plane/v1/rogue.pb.go",
            "type AgentResume struct {\n\tConsumedByGateway bool `protobuf:\"varint,9,opt,name=consumed_by_gateway\"`\n}",
        ),
        // A hand-written Go getter.
        (
            "go/services/x/rogue.go",
            "func f(r *AgentResume) bool { return r.GetConsumedByGateway() }",
        ),
        // The BFF.
        (
            "apps/gateway/src/rogue.rs",
            "fn f(r: &Resume) -> bool { r.consumed_by_gateway }",
        ),
        // The SPA: a type, and a request body.
        (
            "src/shared/rogue.ts",
            "export type Resume = { consumedByGateway: boolean }",
        ),
        (
            "src/shared/rogue.tsx",
            "const body = { consumed_by_gateway: true }",
        ),
    ] {
        let mut rogue = others.clone();
        rogue.push((path.to_owned(), text.to_owned()));
        must_flag(
            no_attestation_violations(&rust, &protos, &rogue),
            &format!("{path} names consumed_by_gateway"),
        );
    }
    let mut quiet_rust = rust.clone();
    quiet_rust.push((
        "src/quiet.rs".to_owned(),
        "// consumed_by_gateway\n#[cfg(test)]\nmod t { fn f() { let _ = \"consumed_by_gateway\"; } }"
            .to_owned(),
    ));
    let mut quiet_proto = protos.clone();
    quiet_proto.push((
        "quiet.proto".to_owned(),
        "// no consumed_by_gateway field exists\nmessage X {} // consumed_by_gateway".to_owned(),
    ));
    assert_green(&no_attestation_violations(&quiet_rust, &quiet_proto, &others));
    // A comment in Go or TypeScript, and a Rust test module in the BFF, explain the absence and
    // are not the field.
    let mut quiet_others = others.clone();
    quiet_others.extend([
        (
            "go/gen/quiet.go".to_owned(),
            "// ConsumedByGateway is not a field\n/* consumed_by_gateway\n   either */\nfunc f() {}"
                .to_owned(),
        ),
        (
            "src/shared/quiet.ts".to_owned(),
            "// consumedByGateway does not exist\n/* consumedByGateway */\nexport const x = 1"
                .to_owned(),
        ),
        (
            "apps/gateway/src/quiet.rs".to_owned(),
            "// consumed_by_gateway\n#[cfg(test)]\nmod t { fn f() { let _ = \"consumed_by_gateway\"; } }"
                .to_owned(),
        ),
    ]);
    assert_green(&no_attestation_violations(&quiet_rust, &quiet_proto, &quiet_others));
    // The comment stripper itself: both comment forms go, code stays, the line count is kept.
    assert_eq!(
        strip_c_comments("a // x\nb /* y\nz */ c\nd"),
        "a \nb \n c\nd"
    );
    // The scan is looking at the trees it names.
    let normalized = |path: &String| path.replace('\\', "/");
    assert!(others.iter().any(|(path, _)| path.ends_with(".pb.go")));
    assert!(others
        .iter()
        .any(|(path, _)| normalized(path).ends_with("shared/actions/chat-run-continue-contract.ts")));
    assert!(others
        .iter()
        .any(|(path, _)| normalized(path).ends_with("apps/gateway/src/domains/orchestration.rs")));

    // The writer gate is handed the depth and the release in exactly the shape the guard reads:
    // a literal 0 depth lets a delegated child overwrite the root run's fold.
    assert_green(&writer_gate_argument_violations(&agent, &fold));
    must_flag(
        writer_gate_argument_violations(
            &mutate_in(&agent, "let gate = writer_gate(", "ctx.depth,", "0,"),
            &fold,
        ),
        "passes `0` as the writer gate's depth",
    );
    must_flag(
        writer_gate_argument_violations(
            &mutate_in(&agent, "let gate = writer_gate(", "ctx.release.is_some(),", "false,"),
            &fold,
        ),
        "as the writer gate's release flag",
    );
    must_flag(
        writer_gate_argument_violations(
            &mutate_in(&agent, "let gate = writer_gate(", "in_place,", "false,"),
            &fold,
        ),
        "as the writer gate's in-place flag",
    );
    must_flag(
        writer_gate_argument_violations(
            &mutate_in(&agent, "let gate = writer_gate(", "        ctx.depth,\n", ""),
            &fold,
        ),
        "with 5 arguments",
    );
    must_flag(
        writer_gate_argument_violations(
            &mutate_in(&agent, "fn writer_gate(", "        depth,\n", "        depth: 0,\n"),
            &fold,
        ),
        "does not hand its `depth` argument to the gate",
    );
    must_flag(
        writer_gate_argument_violations(
            &mutate(&agent, "depth: parent.depth + 1", "depth: 0"),
            &fold,
        ),
        "no longer built with `depth: parent.depth + 1`",
    );
    must_flag(
        writer_gate_argument_violations(
            &agent,
            &mutate(&fold, "} else if self.depth > 0 {", "} else if false {"),
        ),
        "no longer refuses `self.depth > 0`",
    );
    // The in-place resume's own gate, in `restore_continuation`: the literal shape
    // `(req, route, runtime_loop::effect::ledger_enabled(), 0, true, true)`, and no third call.
    for (from, to, expect) in [
        (
            "ledger_enabled(), 0, true, true)",
            "ledger_enabled(), 1, true, true)",
            "passes `1` as the in-place resume gate's depth",
        ),
        (
            "runtime_loop::effect::ledger_enabled(), 0, true, true)",
            "true, 0, true, true)",
            "as the in-place resume gate's effect ledger",
        ),
        (
            "ledger_enabled(), 0, true, true)",
            "ledger_enabled(), 0, false, true)",
            "as the in-place resume gate's release flag",
        ),
        (
            "ledger_enabled(), 0, true, true)",
            "ledger_enabled(), 0, true, false)",
            "as the in-place resume gate's in-place flag",
        ),
        (
            "ledger_enabled(), 0, true, true)",
            "ledger_enabled(), 0, true)",
            "calls writer_gate with 5 arguments",
        ),
    ] {
        must_flag(
            writer_gate_argument_violations(
                &mutate_in(&agent, "async fn restore_continuation(", from, to),
                &fold,
            ),
            expect,
        );
    }
    must_flag(
        writer_gate_argument_violations(
            &format!(
                "{agent}\nfn rogue(req: &R, route: &M) {{ let _ = writer_gate(req, route, true, 0, true, true); }}"
            ),
            &fold,
        ),
        "calls writer_gate( 3 times in production code",
    );
    must_flag(
        writer_gate_argument_violations(
            &mutate_in(
                &agent,
                "async fn restore_continuation(",
                "let gate = writer_gate(req, route, runtime_loop::effect::ledger_enabled(), 0, true, true);",
                "let gate = other_gate(req);",
            ),
            &fold,
        ),
        "restore_continuation no longer calls writer_gate(",
    );
    // A comment or a test that calls it is not a production call.
    assert_green(&writer_gate_argument_violations(
        &format!(
            "{agent}\n// writer_gate(req, route, true, 0, true, true)\n#[cfg(test)]\nmod t {{ fn f() {{ let _ = writer_gate(a, b, c, 9, d, e); }} }}"
        ),
        &fold,
    ));

    // The new store_proofs rows are as checkable as the old ones: a renamed or commented-out
    // test of the gateway's pre-consume, of session-core's spend-only consume or of its
    // recovery renewal is flagged.
    let proofs = store_proofs();
    let broken = |target: &'static str, from: &str, to: &str| {
        let text = mutate(&read(target), from, to);
        move |path: &str| {
            if path == target {
                text.clone()
            } else {
                read(path)
            }
        }
    };
    for (target, name, is_async) in [
        (GATEWAY_SSE, "a_dropped_receiver_cannot_spend_an_approval_without_dispatching", true),
        (GATEWAY_SSE, "continue_run_consumes_through_the_spawned_helper_after_the_preflight", false),
        (GATEWAY_PRECONSUME_TESTS, "a_lost_consume_is_a_409_and_dispatches_nothing", true),
        (GATEWAY_PRECONSUME_TESTS, "frame_codes_pair_with_the_spend_state_per_switch_state", true),
        (SESSION_APPROVAL_RPC, "omit_call_withholds_the_call_but_keeps_the_flags", false),
        (SESSION_APPROVALS, "a_won_spend_only_consume_renews_the_recovery_deadline", true),
        (SESSION_TERMINALIZATION, "a_spent_click_is_not_recovered_before_the_executor_heartbeats", true),
    ] {
        let keyword = if is_async { "async fn" } else { "fn" };
        let from = format!("{keyword} {name}(");
        must_flag(
            store_proof_violations(
                &proofs,
                &broken(target, &from, &format!("{keyword} {name}_x(")),
                &[],
            ),
            &format!("has no test `{name}`"),
        );
    }

    // The attempt of a spent continuation is closed on the run subject, and nothing after the
    // executor's answer stays retryable once the click is spent.
    let subscription_src = read(GATEWAY_SUBSCRIPTION);
    assert_green(&spent_closing_violations(&sse));
    assert_green(&after_answer_frame_violations(&sse, &subscription_src));
    // The spent arm stops asking the pure map, or names a type of its own.
    must_flag(
        spent_closing_violations(&mutate_re(
            &sse,
            r"spent_dispatch_run_event\(&failure\)",
            "\"RUN_DISPATCH_DEGRADED\"",
        )),
        "asks spent_dispatch_run_event(..) 0 times",
    );
    must_flag(
        spent_closing_violations(&mutate_in(
            &sse,
            "let closing = build_stream_envelope(",
            "let closing = build_stream_envelope(",
            "let _ = \"RUN_COMPLETED\";\n                let closing = build_stream_envelope(",
        )),
        "names `RUN_COMPLETED` itself",
    );
    // It publishes twice (a second event per attempt), or lets a failed publish through.
    must_flag(
        spent_closing_violations(&mutate_in(
            &sse,
            "RunAgent of a pre-consumed continuation did not start",
            "let _ = tx",
            "let _ = state.publisher.publish(&subjects::run_event_subject(&run.run_id), &closing).await;\n                let _ = tx",
        )),
        "publishes 2 run-subject events",
    );
    must_flag(
        spent_closing_violations(&mutate_re(
            &sse,
            r"let _ = state\s*\.publisher\s*\.publish\(&subjects::run_event_subject\(&run\.run_id\), &closing\)",
            "state.publisher.publish(&subjects::run_event_subject(&run.run_id), &closing)",
        )),
        "does not ignore the publisher's answer",
    );
    // The map sends a refusal to the wrong type, or the ambiguous cases to the new one.
    must_flag(
        spent_closing_violations(&mutate_in(
            &sse,
            "fn spent_dispatch_run_event(",
            "\"RUN_CONTINUE_REFUSED\"",
            "\"RUN_FAILED\"",
        )),
        "does not map `Refused(_)` to RUN_CONTINUE_REFUSED",
    );
    must_flag(
        spent_closing_violations(&mutate_in(
            &sse,
            "fn spent_dispatch_run_event(",
            "\"RUN_DISPATCH_DEGRADED\"",
            "\"RUN_CONTINUE_REFUSED\"",
        )),
        "does not map `Unreachable` and `Unknown` to RUN_DISPATCH_DEGRADED",
    );
    // An unspent continuation's refusal publishes the closing event too.
    must_flag(
        spent_closing_violations(&mutate(
            &sse,
            typed_log,
            &format!(
                "{typed_log}\n                let _ = spent_dispatch_run_event(&ContinueDispatchFailure::Unknown);"
            ),
        )),
        "a dispatch arm other than the spent one",
    );

    // A frame after the answer built by hand, or a list widened on one side alone.
    must_flag(
        after_answer_frame_violations(
            &mutate_re(
                &sse,
                r"subscription_run_end_event\(\s*&request_id,\s*&run\.run_id,\s*&failure,\s*message,\s*spend,\s*\)",
                "crate::sse_events::ChatEvent::subscription_error(&failure, message).to_sse(&request_id)",
            ),
            &subscription_src,
        ),
        "names `subscription_error(` after the executor's answer",
    );
    must_flag(
        after_answer_frame_violations(
            &mutate_re(
                &sse,
                r#"run_end_error_event\(\s*&request_id,\s*&run\.run_id,\s*"agent_run_output_unavailable",\s*"The completed agent run did not provide an answer\.",\s*spend,\s*\)"#,
                "crate::sse_events::ChatEvent::Error { code: \"agent_run_output_unavailable\".to_owned(), message: String::new(), retryable: true }.to_sse(&request_id)",
            ),
            &subscription_src,
        ),
        "names `ChatEvent::Error` after the executor's answer",
    );
    must_flag(
        after_answer_frame_violations(
            &mutate_in(
                &sse,
                "const CONTINUE_SPENT_RUN_END_FRAMES",
                "\"agent_run_output_unavailable\",",
                "\"agent_run_output_unavailable\",\n    \"agent_run_new\",",
            ),
            &subscription_src,
        ),
        "differ from CONTINUE_SPENT_RUN_END_FRAMES",
    );
    must_flag(
        after_answer_frame_violations(
            &mutate_in(
                &sse,
                "const CONTINUE_SPENT_SUBSCRIPTION_FRAMES",
                "\"prompt_too_long\",",
                "",
            ),
            &subscription_src,
        ),
        "differs from the codes subscription_failure.rs defines",
    );
    must_flag(
        after_answer_frame_violations(
            &sse,
            &mutate(
                &subscription_src,
                "pub const PROMPT_TOO_LONG: &str = \"prompt_too_long\";",
                "pub const PROMPT_TOO_LONG: &str = \"prompt_too_long\";\npub const SUBSCRIPTION_NEW: &str = \"subscription_new\";",
            ),
        ),
        "differs from the codes subscription_failure.rs defines",
    );
    // The builders stop applying the spend.
    must_flag(
        after_answer_frame_violations(
            &mutate_in(
                &sse,
                "fn run_end_error_event(",
                "retryable: !spend.is_spent(),",
                "retryable: true,",
            ),
            &subscription_src,
        ),
        "run_end_error_event does not say `retryable: !spend.is_spent()`",
    );
    must_flag(
        after_answer_frame_violations(
            &mutate_in(
                &sse,
                "fn subscription_run_end_event(",
                "payload[\"retryable\"] = json!(false);",
                "",
            ),
            &subscription_src,
        ),
        "subscription_run_end_event does not withdraw the retry",
    );
}
