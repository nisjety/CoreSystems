//! The approval gate contract (build plan WS13 S8).
//!
//! An approval is a pause for a human decision. Four of its properties are facts about where code
//! sits, which no single run can observe, so they are read from source the way
//! `autonomy_ladder_contract.rs` reads its call shapes:
//!
//! 1. Every approval this crate creates states its cause, and the creators are an inventory: a new
//!    creator fails until it is named in `CREATORS` with the function that holds it.
//! 2. Every site that answers `awaiting_approval` is inventoried and states its cause. The
//!    constructor `StepOutcome::awaiting_approval` takes the cause as an argument, so a call without
//!    one does not compile and is not scanned again here.
//! 3. The cause vocabulary is closed and exhaustive against `api/contracts/approval-causes.json`:
//!    one exhaustive match names every variant with its token, the golden lists the same tokens in
//!    the same order, and every code is raised by production code in this crate, or is reserved and
//!    raised by none.
//! 4. The five approval golden cases of the zero-data-retention clause are ordering guards. The
//!    refusal (or the omission) comes before the durable write it protects, in the file that owns
//!    it. Each guard names the behavioural test that proves the same case where one exists. The
//!    repository sync check (`scripts/tests/zdr-contract-sync.test.sh`, `APPROVAL_ZDR_CASES`) needs a
//!    guard function of each case name here, so a renamed or deleted guard fails that check.
//!
//! The scan reads production code only: test items are blanked before any site is looked for, so a
//! test helper that builds an approval without a cause is not a finding.

use std::path::Path;

use mp_contracts::approval_cause::CauseCode;
use serde_json::Value;

const GOLDEN: &str = include_str!("../../../../api/contracts/approval-causes.json");

/// The codes no production site raises yet. Each is in the golden; the test insists that it stays
/// unraised until the slice that owns it wires it, so the reservation cannot outlive its reason.
// `CapabilityPolicyAsk` left this list when main's Space gate (WS10 S3, `space_gate.rs`) began raising it.
const RESERVED: &[&str] = &[];

/// Main's creators that state no cause, and why: (file under `src`, enclosing function). The
/// quarantine hold (WS10 S8) is an `UntrustedContent` review of held tool output, not a tool call
/// waiting for consent; the standing-rule use (WS10 S3) records a call a Control rule already
/// passed, granted at creation, so no person is asked. Whether either should carry a cause code
/// is a depth item, not something the merge of main decides.
const KIND_CARD_CREATORS: &[(&str, &str)] = &[
    ("quarantine.rs", "enforce"),
    ("space_gate.rs", "try_standing_rule"),
];

/// Every production site that creates an approval: (file under `src`, enclosing function).
const CREATORS: &[(&str, &str)] = &[
    ("browser_events.rs", "require_approval"),
    ("grpc.rs", "execute_step"),
    ("runtime_loop/agent.rs", "pause_for_approval"),
    ("runtime_loop/mod.rs", "resolve_write_approval"),
];

/// Every production site that answers `awaiting_approval`: (file under `src`, enclosing function,
/// the text that function must hold for its cause to be stated).
const PAUSES: &[(&str, &str, &str)] = &[
    (
        "runtime_loop/agent.rs",
        "run_rounds",
        "cause: Some(CauseCode::RiskyEffectUnderAsk)",
    ),
    (
        "runtime_loop/agent.rs",
        "pause_for_approval",
        "cause: Some(approval_cause_for_pause(",
    ),
    (
        "runtime_loop/mod.rs",
        "awaiting_approval",
        "cause: Some(cause)",
    ),
];

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("the golden is JSON")
}

/// Every variant with its name and wire token. The match is exhaustive on purpose: a variant added
/// to `CauseCode` stops this file from compiling until it is listed here, and the golden must then
/// list its token too.
fn variant_and_token(code: CauseCode) -> (&'static str, &'static str) {
    match code {
        CauseCode::OwnerEffectReserved => ("OwnerEffectReserved", "owner_effect_reserved"),
        CauseCode::ConsentToDisclose => ("ConsentToDisclose", "consent_to_disclose"),
        CauseCode::RiskyEffectUnderAsk => ("RiskyEffectUnderAsk", "risky_effect_under_ask"),
        CauseCode::ProviderWrite => ("ProviderWrite", "provider_write"),
        CauseCode::CapabilityHighRisk => ("CapabilityHighRisk", "capability_high_risk"),
        CauseCode::CapabilityPolicyAsk => ("CapabilityPolicyAsk", "capability_policy_ask"),
        CauseCode::HookAsk => ("HookAsk", "hook_ask"),
        CauseCode::BrowserActionRisk => ("BrowserActionRisk", "browser_action_risk"),
        CauseCode::AutoPostureProviderWrite => {
            ("AutoPostureProviderWrite", "auto_posture_provider_write")
        }
        CauseCode::BrowserHumanAssist => ("BrowserHumanAssist", "browser_human_assist"),
        CauseCode::UnclassifiedGate => ("UnclassifiedGate", "unclassified_gate"),
    }
}

#[test]
fn cause_code_is_exhaustive_against_the_golden() {
    let golden = golden();
    let golden_tokens: Vec<&str> = golden["codes"]
        .as_array()
        .expect("codes is a list")
        .iter()
        .map(|row| row["code"].as_str().expect("a code is text"))
        .collect();
    let listed: Vec<&str> = CauseCode::ALL
        .into_iter()
        .map(|code| variant_and_token(code).1)
        .collect();
    assert_eq!(
        listed, golden_tokens,
        "CauseCode::ALL and approval-causes.json name different codes: a variant was added to one \
         of them and not the other"
    );
    for code in CauseCode::ALL {
        assert_eq!(
            code.as_str(),
            variant_and_token(code).1,
            "{code:?} names a token the exhaustive match does not"
        );
    }
    assert_eq!(
        golden["unknown_degrades_to"],
        CauseCode::UnclassifiedGate.as_str()
    );
    assert_eq!(
        CauseCode::from_wire("a_code_from_a_newer_creator"),
        CauseCode::UnclassifiedGate,
        "a code this reader does not know degrades, it is never guessed"
    );
}

#[test]
fn every_cause_code_is_raised_by_production_code_or_is_reserved() {
    let sources = production_sources();
    for code in CauseCode::ALL {
        let (variant, token) = variant_and_token(code);
        let raised = sources
            .iter()
            .any(|source| mentions(&source.text, &format!("CauseCode::{variant}")));
        if RESERVED.contains(&variant) {
            assert!(
                !raised,
                "{variant} ({token}) is reserved, but production code raises it: take it out of RESERVED"
            );
        } else {
            assert!(
                raised,
                "{variant} ({token}) is in the golden, but no production site in this crate raises it"
            );
        }
    }
}

#[test]
fn every_approval_creator_is_inventoried_and_states_a_cause() {
    let mut found = Vec::new();
    for source in production_sources() {
        for (line, block) in literal_blocks(&source.text, "CreateApprovalRequest {") {
            let function = enclosing_fn(&source.text, line);
            if KIND_CARD_CREATORS
                .iter()
                .any(|(file, name)| source.path.ends_with(file) && function == *name)
            {
                continue;
            }
            assert!(
                block.contains("cause: Some("),
                "{}:{line} ({function}) creates an approval without stating its cause: build the \
                 cause with approval_cause (for_call, for_code or a site helper)",
                source.path
            );
            found.push((source.path.clone(), function));
        }
    }
    let mut expected: Vec<(String, String)> = CREATORS
        .iter()
        .map(|(file, function)| ((*file).to_owned(), (*function).to_owned()))
        .collect();
    found.sort();
    expected.sort();
    assert_eq!(
        found, expected,
        "the approval creators changed: name each new creator in CREATORS with its function"
    );
}

#[test]
fn every_awaiting_approval_site_is_inventoried_and_states_its_cause() {
    let mut found = Vec::new();
    for source in production_sources() {
        for line in lines_containing(&source.text, "\"awaiting_approval\".to_owned()") {
            let function = enclosing_fn(&source.text, line);
            let Some((_, _, must_state)) = PAUSES
                .iter()
                .find(|(file, name, _)| *file == source.path && *name == function)
            else {
                panic!(
                    "{}:{line} ({function}) answers awaiting_approval and is not inventoried in PAUSES",
                    source.path
                );
            };
            let body = body_of(&source.text, &format!("fn {function}("));
            assert!(
                body.contains(must_state),
                "{}:{line} ({function}) answers awaiting_approval without stating its cause: \
                 expected `{must_state}`",
                source.path
            );
            found.push((source.path.clone(), function));
        }
    }
    let mut expected: Vec<(String, String)> = PAUSES
        .iter()
        .map(|(file, function, _)| ((*file).to_owned(), (*function).to_owned()))
        .collect();
    found.sort();
    expected.sort();
    assert_eq!(
        found, expected,
        "the awaiting_approval sites changed: name each new site in PAUSES with the text that states its cause"
    );
}

/// `approval_cause_not_persisted_under_zdr`: a zero-retention caller's pause stores no cause. The
/// stored metadata drops the cause for a zero-retention caller, and `create_approval` builds that
/// metadata from the caller's own posture. Behavioural proof: session-core's
/// `a_zero_retention_caller_stores_no_cause`. A waiver's credential has `zdr` false, so this covers
/// a zero-retention caller only; a waived run still stores its cause (open, WS13 S8 depth finding).
#[test]
fn approval_cause_not_persisted_under_zdr() {
    let grpc = production("../session-core/src/orchestration_grpc.rs");
    let metadata = body_of(&grpc, "fn new_approval_metadata(");
    assert!(
        metadata.contains(".filter(|_| !zdr)"),
        "new_approval_metadata must drop the cause for a zero-retention caller"
    );
    let create = body_of(&grpc, "fn create_approval(");
    assert!(
        create.contains("new_approval_metadata(&req, caller.zdr())"),
        "create_approval must build the stored metadata from the caller's posture"
    );
}

/// `approval_audit_not_emitted_under_zdr`: a zero-retention caller cannot decide, so no decision row
/// and no `approval_decided` outbox row is written for it. The refusal comes before the decision
/// store call, which writes both. The same limit as the cause guard: a waiver's credential is not
/// refused here, so a waived decision still writes its audit row (open, WS13 S8 depth finding).
#[test]
fn approval_audit_not_emitted_under_zdr() {
    let grpc = production("../session-core/src/orchestration_grpc.rs");
    assert_before(
        body_of(&grpc, "fn decide_approval("),
        "refuse_zdr_continuation(&caller)?;",
        "store::decide_approval_recorded(",
        "a zero-retention decision is refused before the decision row and its audit row are written",
    );
}

/// `approval_denial_memory_not_consulted_under_zdr`: a zero-retention or waived run never reads its
/// declined calls, so the list call is not made for it. Behavioural twin: agent.rs's
/// `a_denied_call_is_not_re_asked_in_the_same_run`, which runs a persistent run.
#[test]
fn approval_denial_memory_not_consulted_under_zdr() {
    let agent = production("src/runtime_loop/agent.rs");
    assert_before(
        body_of(&agent, "fn declined_earlier_in_run("),
        "ctx.req.zdr || ctx.route.is_waived()",
        ".list_approvals(",
        "a zero-retention or waived run returns before it reads the declined calls",
    );
}

/// `approval_scope_grant_under_zdr_refused`: a zero-retention or waived run never gets a standing
/// grant. The gateway refuses a zero-retention credential before it asks for the decision,
/// session-core refuses a zero-retention human caller before it writes the decision, and the store
/// creates a grant only for a persistent run.
#[test]
fn approval_scope_grant_under_zdr_refused() {
    let gateway = production("../model-gateway/src/http_routes.rs");
    assert_before(
        body_of(&gateway, "async fn decide_approval("),
        "crate::zdr_posture::continuation_gate(&claims)?;",
        ".decide_approval(authenticated_session_request(",
        "the gateway refuses a zero-retention decision before it reaches session-core",
    );
    let grpc = production("../session-core/src/orchestration_grpc.rs");
    assert_before(
        body_of(&grpc, "fn decide_approval("),
        "refuse_zdr_continuation(&caller)?;",
        "store::decide_approval_recorded(",
        "session-core refuses a zero-retention decision before the grant is written",
    );
    let store = production("../session-core/src/orchestration_store.rs");
    let grant = body_of(&store, "fn create_scope_grant(");
    assert!(
        grant.contains("if retention_class != \"persistent\" {"),
        "create_scope_grant must refuse a run that keeps no state"
    );
    assert!(
        grant.contains("ScopeGrantRefusal::NotAvailable"),
        "the refusal for a run that keeps no state is the typed NotAvailable"
    );
}

/// `approval_grant_use_under_zdr_refused`: a standing grant releases no call of a zero-retention or
/// waived run. The handler refuses a zero-retention human caller before the release, and the release
/// statement matches only a persistent run. Behavioural twin: session-core's DB test
/// `a_thread_grant_never_releases_a_run_that_keeps_no_state` (ignored without `DATABASE_URL`).
#[test]
fn approval_grant_use_under_zdr_refused() {
    let grpc = production("../session-core/src/orchestration_grpc.rs");
    assert_before(
        body_of(&grpc, "fn use_approval_scope_grant("),
        "refuse_zdr_continuation(&caller)?;",
        "store::use_approval_scope_grant(",
        "a zero-retention caller is refused before any grant is released",
    );
    let store = production("../session-core/src/orchestration_store.rs");
    let release = const_text(&store, "const USE_APPROVAL_SCOPE_GRANT_SQL: &str");
    assert!(
        release.contains("AND r.retention_class = 'persistent'"),
        "the release statement must match only a persistent run"
    );
}

#[test]
fn the_scanner_blanks_test_items_and_nothing_else() {
    let text = "fn live() {\n    1\n}\n#[cfg(test)]\nmod tests {\n    fn t() { let _ = 1; }\n}\nfn after() {}\n";
    let blanked = blank_test_items(text);
    assert_eq!(blanked.lines().count(), text.lines().count());
    assert!(blanked.contains("fn live()") && blanked.contains("fn after()"));
    assert!(!blanked.contains("fn t()") && !blanked.contains("mod tests"));
}

/// One production source file of this crate: its path under `src`, and its text with every test
/// item blanked out. Line numbers are unchanged.
struct Source {
    path: String,
    text: String,
}

fn production_sources() -> Vec<Source> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    collect(&root, &root, &mut found);
    assert!(
        found.len() > 20,
        "the scan found only {} files under {}",
        found.len(),
        root.display()
    );
    found
}

fn collect(root: &Path, directory: &Path, found: &mut Vec<Source>) {
    for entry in std::fs::read_dir(directory).expect("the crate's src directory exists") {
        let path = entry.expect("a readable directory entry").path();
        if path.is_dir() {
            collect(root, &path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string()
                .replace('\\', "/");
            found.push(Source {
                path: relative,
                text: blank_test_items(&read_text(&path)),
            });
        }
    }
}

/// A file a guard reads, relative to this crate, with its test items blanked: a guard only ever
/// looks at production code.
fn production(relative: &str) -> String {
    blank_test_items(&read_text(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(relative),
    ))
}

fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| {
            panic!(
                "cannot read {} ({error}). If the file moved, re-point this test; do not delete it: \
                 it is what keeps the approval gate contract honest",
                path.display()
            )
        })
        .replace("\r\n", "\n")
}

/// The source with every `#[cfg(test)]` item blanked: the attribute and the item it marks, through
/// the matching close of its body or through its `;`. Newlines are kept, so line numbers stay true.
fn blank_test_items(source: &str) -> String {
    const MARK: &str = "#[cfg(test)]";
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(index) = rest.find(MARK) {
        out.push_str(&rest[..index]);
        let after = &rest[index + MARK.len()..];
        let end = match (after.find('{'), after.find(';')) {
            (Some(open), Some(semi)) if semi < open => semi + 1,
            (Some(open), _) => matching_close(after, open),
            (None, Some(semi)) => semi + 1,
            (None, None) => after.len(),
        };
        let blanked = &rest[index..index + MARK.len() + end];
        out.extend(blanked.chars().map(|c| if c == '\n' { '\n' } else { ' ' }));
        rest = &rest[index + MARK.len() + end..];
    }
    out.push_str(rest);
    out
}

/// The byte offset just past the brace that closes the one opened at `open`.
fn matching_close(text: &str, open: usize) -> usize {
    let mut depth = 0_usize;
    for (position, character) in text[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return open + position + 1;
                }
            }
            _ => {}
        }
    }
    text.len()
}

/// The text of the one function whose signature contains `signature` (for example `fn decide(`),
/// from the signature to its closing brace. A signature that names no function, or more than one,
/// is a failure: the guard would otherwise read the wrong body.
fn body_of<'a>(text: &'a str, signature: &str) -> &'a str {
    let found: Vec<usize> = text
        .match_indices(signature)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "`{signature}` must name exactly one function in its file, found {}",
        found.len()
    );
    let start = found[0];
    let open = start + text[start..].find('{').expect("a function has a body");
    &text[start..matching_close(text, open)]
}

/// The declaration and value of a `const` string, up to the end of its literal.
fn const_text<'a>(text: &'a str, declaration: &str) -> &'a str {
    let start = text
        .find(declaration)
        .unwrap_or_else(|| panic!("`{declaration}` is gone; re-point this test"));
    let end = text[start..]
        .find("\";")
        .map_or(text.len(), |offset| start + offset + 2);
    &text[start..end]
}

/// Each `opener` (a literal that ends in `{`, such as `CreateApprovalRequest {`) with the line it is
/// on and the text inside its braces.
fn literal_blocks<'a>(text: &'a str, opener: &str) -> Vec<(usize, &'a str)> {
    text.match_indices(opener)
        .map(|(index, _)| {
            let open = index + opener.len() - 1;
            let close = matching_close(text, open);
            let line = text[..index].matches('\n').count() + 1;
            (line, &text[open + 1..close - 1])
        })
        .collect()
}

/// The 1-based lines that contain `needle` outside a comment.
fn lines_containing(text: &str, needle: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.contains(needle) && !line.trim_start().starts_with("//"))
        .map(|(index, _)| index + 1)
        .collect()
}

/// The name of the nearest function whose signature is on or above `line` (1-based).
fn enclosing_fn(text: &str, line: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    (0..line)
        .rev()
        .find_map(|index| fn_name(lines[index]))
        .unwrap_or_else(|| panic!("line {line} is in no function"))
}

fn fn_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
        return None;
    }
    let mut words = trimmed.split_whitespace();
    while let Some(word) = words.next() {
        if word == "fn" {
            let name: String = words
                .next()?
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            return (!name.is_empty()).then_some(name);
        }
    }
    None
}

/// Whether `path` (for example `CauseCode::ProviderWrite`) occurs as a whole name in `text`.
fn mentions(text: &str, path: &str) -> bool {
    text.match_indices(path).any(|(index, _)| {
        !text[index + path.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
    })
}

fn assert_before(body: &str, first: &str, then: &str, why: &str) {
    let first_at = body
        .find(first)
        .unwrap_or_else(|| panic!("`{first}` is gone: {why}"));
    let then_at = body
        .find(then)
        .unwrap_or_else(|| panic!("`{then}` is gone: {why}"));
    assert!(
        first_at < then_at,
        "`{first}` must come before `{then}`: {why}"
    );
}
