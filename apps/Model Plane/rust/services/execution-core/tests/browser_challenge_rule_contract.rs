//! Guards the one rule text two independently deployed services give a browser
//! planner about a bot wall (Quarry WS3.J, Model Plane half).
//!
//! execution-core's loop planner (`src/llm_planner.rs`, `BROWSER_CHALLENGE_RULE`)
//! and model-gateway's browser suggestion prompt (`src/http_routes.rs`, its own
//! `BROWSER_CHALLENGE_RULE`) must tell a model the same thing: only a strong
//! signal is a bot wall, never solve a captcha or change identity, at most one
//! recovery step, then report the task blocked and hand it to the user. If the
//! two drift, plain chat and a deployed agent behave differently at the same
//! wall, and neither errors.
//!
//! Like `cross_service_loop_contract.rs`, this reads the sources of record
//! rather than importing the constants: the services do not depend on each
//! other, and both constants are private to their modules. If either file moves
//! this fails; re-point the path, do not delete the assertion.

use std::path::PathBuf;

const PLANNER: &str = "src/llm_planner.rs";
const GATEWAY: &str = "../model-gateway/src/http_routes.rs";

/// Phrases the rule must keep, on both sides. Equality alone would stay green if
/// both texts lost the same sentence.
const ANCHORS: &[&str] = &[
    "Only strength=strong says the site refuses automation",
    "Never solve a captcha",
    "never change identity",
    "ONE recovery step",
    "up to 5 seconds",
    "report the task as blocked",
    "take over the browser or to use another source",
    "A login page, a consent banner, a paywall, a rate limit and a bare 403 are not bot blocks",
];

fn read(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}).\n\
             This file holds one half of the browser challenge rule. If it moved, \
             re-point the path in this test; do not delete the assertions.",
            path.display()
        )
    })
}

/// The text of `const BROWSER_CHALLENGE_RULE: &str = r"...";`, line endings
/// normalised to LF so a CRLF working copy reads the same as an LF one. A
/// match inside a comment line is skipped.
fn rule_text(source: &str) -> Option<String> {
    let needle = "const BROWSER_CHALLENGE_RULE: &str = r\"";
    let mut from = 0;
    while let Some(relative) = source[from..].find(needle) {
        let at = from + relative;
        let line_start = source[..at].rfind('\n').map_or(0, |newline| newline + 1);
        if !source[line_start..at].trim_start().starts_with("//") {
            let body = &source[at + needle.len()..];
            let end = body.find("\";")?;
            return Some(body[..end].replace("\r\n", "\n"));
        }
        from = at + needle.len();
    }
    None
}

fn rule_violations(planner: &str, gateway: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let planner_rule = rule_text(planner);
    let gateway_rule = rule_text(gateway);
    match (&planner_rule, &gateway_rule) {
        (Some(planner_rule), Some(gateway_rule)) => {
            if planner_rule != gateway_rule {
                violations.push(
                    "execution-core's BROWSER_CHALLENGE_RULE and model-gateway's differ: the \
                     same bot wall gets different instructions in a deployed agent and in plain chat"
                        .to_owned(),
                );
            }
            for anchor in ANCHORS {
                if !planner_rule.contains(anchor) {
                    violations.push(format!(
                        "the rule no longer says `{anchor}` (execution-core side)"
                    ));
                }
                if !gateway_rule.contains(anchor) {
                    violations.push(format!(
                        "the rule no longer says `{anchor}` (model-gateway side)"
                    ));
                }
            }
        }
        (planner_rule, gateway_rule) => {
            if planner_rule.is_none() {
                violations.push("execution-core no longer defines BROWSER_CHALLENGE_RULE".to_owned());
            }
            if gateway_rule.is_none() {
                violations.push("model-gateway no longer defines BROWSER_CHALLENGE_RULE".to_owned());
            }
        }
    }
    // A constant nobody sends is the same as no rule.
    if !planner.contains("{BROWSER_CHALLENGE_RULE}") {
        violations.push(
            "the loop planner's system prompt no longer includes BROWSER_CHALLENGE_RULE".to_owned(),
        );
    }
    if !gateway.contains("{BROWSER_SUGGEST_SYSTEM_PROMPT} {BROWSER_CHALLENGE_RULE}") {
        violations.push(
            "model-gateway's browser suggestion prompt no longer includes BROWSER_CHALLENGE_RULE"
                .to_owned(),
        );
    }
    violations
}

#[test]
fn the_loop_planner_and_the_gateway_give_a_model_the_same_bot_wall_rule() {
    let violations = rule_violations(&read(PLANNER), &read(GATEWAY));
    assert!(violations.is_empty(), "{violations:#?}");
}

/// `text` with the first `from` replaced by `to`; the target must exist.
fn mutate(text: &str, from: &str, to: &str) -> String {
    assert!(
        text.contains(from),
        "the mutation target `{from}` is gone: re-point this test"
    );
    text.replacen(from, to, 1)
}

/// The same rule in CRLF, as the working copy has it, is not a drift.
#[test]
fn line_endings_are_not_drift() {
    let planner = read(PLANNER);
    let gateway = read(GATEWAY);
    let crlf = |source: &str| source.replace("\r\n", "\n").replace('\n', "\r\n");
    assert!(rule_violations(&crlf(&planner), &gateway).is_empty());
    assert!(rule_violations(&planner, &crlf(&gateway)).is_empty());
}

/// Each pin goes red when either side drifts.
#[test]
fn the_rule_pins_go_red_when_either_side_drifts() {
    let planner = read(PLANNER);
    let gateway = read(GATEWAY);
    let flagged = |violations: Vec<String>, containing: &str| {
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains(containing)),
            "expected a violation containing {containing:?}, got {violations:?}"
        );
    };

    // One side loosens the recovery budget, then the other.
    flagged(
        rule_violations(&planner, &mutate(&gateway, "ONE recovery step", "TWO recovery steps")),
        "differ",
    );
    flagged(
        rule_violations(&mutate(&planner, "up to 5 seconds", "up to 30 seconds"), &gateway),
        "differ",
    );
    // Both lose the same sentence: equality holds, the anchor catches it.
    flagged(
        rule_violations(
            &mutate(&planner, "Never solve a captcha, ", ""),
            &mutate(&gateway, "Never solve a captcha, ", ""),
        ),
        "Never solve a captcha",
    );
    // A constant that is no longer defined, or no longer sent.
    flagged(
        rule_violations(
            &mutate(&planner, "const BROWSER_CHALLENGE_RULE", "const BROWSER_BOT_WALL_RULE"),
            &gateway,
        ),
        "execution-core no longer defines",
    );
    flagged(
        rule_violations(
            &planner,
            &mutate(&gateway, "const BROWSER_CHALLENGE_RULE", "const BROWSER_BOT_WALL_RULE"),
        ),
        "model-gateway no longer defines",
    );
    flagged(
        rule_violations(&mutate(&planner, "\\n{BROWSER_CHALLENGE_RULE}\\n", "\\n"), &gateway),
        "system prompt no longer includes",
    );
    flagged(
        rule_violations(
            &planner,
            &mutate(
                &gateway,
                "{BROWSER_SUGGEST_SYSTEM_PROMPT} {BROWSER_CHALLENGE_RULE}",
                "{BROWSER_SUGGEST_SYSTEM_PROMPT}",
            ),
        ),
        "browser suggestion prompt no longer includes",
    );
}

/// A rule that only appears in a comment is not a definition.
#[test]
fn a_commented_out_definition_does_not_count() {
    let planner = read(PLANNER);
    let gateway = read(GATEWAY);
    let commented = mutate(
        &gateway,
        "const BROWSER_CHALLENGE_RULE: &str = r\"",
        "// const BROWSER_CHALLENGE_RULE: &str = r\"",
    );
    assert!(rule_violations(&planner, &commented)
        .iter()
        .any(|violation| violation.contains("model-gateway no longer defines")));
}
