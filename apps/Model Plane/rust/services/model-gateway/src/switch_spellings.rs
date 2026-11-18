//! Startup report for the default-off switches that read only their exact value.
//!
//! Each switch in [`EXACT_SWITCHES`] is read exactly by its reader: the literal `true` (or `on`, for
//! the review proposal rule), or `1` for the three gates in [`LITERAL_ONE_EXCEPTIONS`]. Before those
//! readers were made exact, several of
//! them also took `1`, `on`, `yes`, `enabled`, or `true` in another case or with surrounding
//! whitespace. A live environment that still sets one of those spellings now reads as off, silently.
//!
//! This module changes no reader and no switch value. A switch set to such a spelling stays off,
//! as it does under its exact reader. [`warn_former_spellings`] logs one warning per such switch
//! at startup, so the mismatch shows in the logs instead of only as a missing feature.
//!
//! [`DOCUMENTED_EXCEPTIONS`] lists the switches this service reads by another rule (default-on kill
//! switches, switches that are `true` in any case, wire booleans) and says why. The tests check this
//! file against the production code: each listed switch must be read there by its exact reader, so a
//! reader whose name or literal changes fails a test instead of drifting silently.

use tracing::warn;

use crate::{auth, latency_levers, prompt_context, room_metadata};

/// Each switch this service reads exactly, with the one value that turns it on. Every switch read
/// through `mp_contracts::loop_fold::switch_from` (default off) is here, so a padded or case-changed
/// value is reported at startup, except the two that `coordinator` names (see `reported_switches`).
/// Keep each name in step with its reader: the tests fail when one drifts.
pub const EXACT_SWITCHES: &[(&str, &str)] = &[
    ("MODEL_GATEWAY_AUTH_DEV_BYPASS", "true"),
    ("ALLOW_INSECURE_DEV_DEFAULTS", "1"),
    (auth::SERVICE_FLAG_AI_PII, "true"),
    (auth::SERVICE_FLAG_AI_LAYOUT, "true"),
    ("FINETUNE_ENABLED", "true"),
    (latency_levers::DATED_POST_SECTION_ONLY_ENV, "true"),
    (latency_levers::FINER_REVIEW_BATCHES_ENV, "true"),
    (prompt_context::STABLE_PROMPT_ORDER_ENV, "true"),
    (prompt_context::DEDUP_CONTEXT_HISTORY_ENV, "true"),
    ("MODEL_GATEWAY_EXTRACT_TOOL", "true"),
    (room_metadata::LEGACY_PASSTHROUGH_ENV, "true"),
    ("MODEL_GATEWAY_BROWSER_ATTACH", "true"),
    ("MODEL_GATEWAY_BROWSER_ATTACH_POLICY_ENFORCED", "true"),
    ("MODEL_GATEWAY_BROWSER_POLICY_ASK", "true"),
    ("MODEL_GATEWAY_BROWSER_BUDGET_CLAIM", "true"),
    ("MODEL_GATEWAY_BROWSER_PLANNER_QUOTAS", "true"),
    ("MODEL_GATEWAY_BROWSER_KEY_GRANTS", "true"),
    // Browser, planner and attachment switches.
    ("MODEL_GATEWAY_BROWSER_PAGE_CONTEXT_ENABLED", "true"),
    ("MODEL_GATEWAY_BROWSER_RECIPE", "true"),
    ("MODEL_GATEWAY_CAP_MIN", "true"),
    ("MODEL_GATEWAY_BUDGET_WINDOW", "true"),
    ("MODEL_GATEWAY_SPEED_TIER", "true"),
    ("MODEL_GATEWAY_BYO_GATEWAY", "true"),
    ("CHAT_SANDBOX_ATTACHMENTS_ENABLED", "true"),
    ("CHAT_VISION_AS_PARTS_ENABLED", "true"),
    ("CHAT_DO_ATTACHMENTS_ENABLED", "true"),
    ("SANDBOX_THREAD_WORKSPACE_ENABLED", "true"),
    // Agent, thread and run switches.
    ("MODEL_GATEWAY_AGENT_API", "true"),
    ("MODEL_GATEWAY_ASK_USER_ASK_MODE_ENABLED", "true"),
    ("AGENT_ASK_USER_ENABLED", "true"),
    ("AGENT_ASK_USER_RESUME_ENABLED", "true"),
    ("EXECUTION_COORDINATOR_MODE_ENABLED", "true"),
    ("MODEL_GATEWAY_PLAN_EXECUTE_ON_APPROVE", "true"),
    ("MODEL_GATEWAY_ASSISTANT_NAVIGATION", "true"),
    ("MODEL_GATEWAY_THREAD_EXPORT", "true"),
    ("MODEL_GATEWAY_THREAD_GOALS_ENABLED", "true"),
    ("MODEL_GATEWAY_THREAD_REFERENCES_ENABLED", "true"),
    ("MODEL_GATEWAY_THREAD_SHARES", "true"),
    ("MODEL_GATEWAY_NAMED_AGENT_THREADS", "true"),
    ("MODEL_GATEWAY_MESSAGE_VERSIONS_ENABLED", "true"),
    ("MODEL_GATEWAY_REQUEST_INSPECTOR", "true"),
    ("MODEL_GATEWAY_RUN_ORIGIN_FIELD", "true"),
    ("APPROVAL_WRITE_PREVIEW_ENABLED", "true"),
    ("APPROVAL_EDIT_ENABLED", "true"),
    ("WIDE_RESEARCH_ENTRY_ENABLED", "true"),
    ("MODEL_GATEWAY_RUN_CEILING_WARN", "true"),
    ("MODEL_GATEWAY_RUN_CEILING_ENFORCE", "true"),
    ("MODEL_GATEWAY_RUN_CONTROL_BUS", "true"),
    ("MODEL_GATEWAY_RUN_TIMING_PERSIST", "true"),
    ("MODEL_GATEWAY_SHARED_TURN_RECORD", "true"),
    ("MODEL_GATEWAY_SHARED_IDEMPOTENCY", "true"),
    ("MODEL_GATEWAY_SEND_RECEIPTS", "true"),
    ("MODEL_GATEWAY_SIDE_CHAT_ENABLED", "true"),
    ("MODEL_GATEWAY_RESEARCH_RUNS", "true"),
    ("MODEL_GATEWAY_REASONING_TRANSLATE_ENABLED", "true"),
    ("MODEL_GATEWAY_RECALL_CONVERSATIONS_ENABLED", "true"),
    ("MODEL_GATEWAY_EVENT_TRIGGERS", "true"),
    ("MODEL_GATEWAY_GOAL_AUTO_CONTINUE", "true"),
    ("MODEL_GATEWAY_GOAL_JUDGE_ENABLED", "true"),
    ("MODEL_GATEWAY_GOAL_UNATTENDED_ENABLED", "true"),
    ("MODEL_GATEWAY_MODEL_COUNCIL_ENABLED", "true"),
    ("MODEL_GATEWAY_SELECTION_EDIT_ENABLED", "true"),
    ("MODEL_GATEWAY_ROLE_ROUTING", "true"),
    ("MODEL_GATEWAY_POLICY_EFFECTIVE", "true"),
    ("MODEL_GATEWAY_SEARCH_PROVENANCE", "true"),
    ("MODEL_GATEWAY_CONTEXT_REFS_ENABLED", "true"),
    ("MODEL_GATEWAY_CATALOGUE_PRICES", "true"),
    ("MODEL_GATEWAY_WEB_READER", "true"),
    ("MODEL_GATEWAY_WEB_BUNDLE_ARTIFACTS", "true"),
    ("MODEL_GATEWAY_DATA_SOURCE_CATALOGUE", "true"),
    ("MODEL_GATEWAY_GRAPH_SPACE_SCOPE", "true"),
    ("MODEL_GATEWAY_IMAGE_TOOL", "true"),
    ("MODEL_GATEWAY_DECK_TOOL", "true"),
    ("MODEL_GATEWAY_DECK_FROM_ARTIFACT", "true"),
    ("SKILL_DECK_TEMPLATES_ENABLED", "true"),
    ("MODEL_GATEWAY_WIDGET_GUIDANCE", "true"),
    ("MODEL_GATEWAY_USER_LOCALE_CONTEXT", "true"),
    ("MODEL_GATEWAY_TONE_PRESETS", "true"),
    ("MODEL_GATEWAY_USER_PROFILE_PREFIX", "true"),
    ("MODEL_GATEWAY_CITATION_PASSAGES", "true"),
    ("MODEL_GATEWAY_EFFORT_LADDER", "true"),
    ("MODEL_GATEWAY_WEB_SEARCH_BATCH", "true"),
    ("CODE_INTERPRETER_VERIFY_ENFORCE", "true"),
    ("SKILL_BUNDLE_MOUNT_ENABLED", "true"),
    ("RESULT_HANDLES_EXTENDED", "true"),
    ("ZDR_TYPED_REFUSALS_ENABLED", "true"),
    ("MODEL_GATEWAY_ZDR_STREAM_COST", "true"),
    ("MODEL_GATEWAY_ZDR_LEDGER_ROWS", "true"),
    ("MODEL_GATEWAY_AGENT_USAGE_ENVELOPE", "true"),
    ("MODEL_GATEWAY_TURN_USAGE_COMPLETE", "true"),
    ("MODEL_GATEWAY_USAGE_PROGRESS", "true"),
    ("MODEL_GATEWAY_ROUTING_DECISION_ENABLED", "true"),
    ("MODEL_GATEWAY_SPACE_APPEND_RETRIEVAL", "true"),
    ("AGENTIC_SPACE_RETRIEVAL_ENABLED", "true"),
    ("OWNER_LIST_SPACE_REFUSE", "true"),
    ("MODEL_GATEWAY_SUBAGENT_ROLE_CATALOGUE", "true"),
    ("MODEL_GATEWAY_AGENT_TOOL_ALLOWLIST", "true"),
    ("MODEL_GATEWAY_QUARRY_ATTRIBUTION", "true"),
    ("MODEL_GATEWAY_REVIEW_PROPOSAL_RULE", "on"),
    // Connectors, MCP, plugins, skills and deep research.
    ("MODEL_GATEWAY_MCP_PROBE", "true"),
    ("MCP_ELICITATION_ENABLED", "true"),
    ("MCP_APPS_ENABLED", "true"),
    ("MCP_APPS_NETWORK_ENABLED", "true"),
    ("MCP_APP_TOOL_CALLS", "true"),
    ("MCP_SECRET_REF_AUTH_ENABLED", "true"),
    ("MCP_TOOL_OBSERVATION_ENABLED", "true"),
    ("PLUGIN_LOADER_ENABLED", "true"),
    ("HOOKS_DURABLE", "true"),
    ("BUILTIN_OFFICE_SKILLS_ENABLED", "true"),
    ("CONNECTOR_ASK_READS_ENABLED", "true"),
    ("CONNECTOR_SUGGESTIONS_ENABLED", "true"),
    ("CONTENT_SAFETY_SEMANTIC_INLINE", "true"),
    ("MODEL_GATEWAY_SKILL_PINS", "true"),
    ("SKILL_SPACE_SCOPE", "true"),
    ("SKILL_EXT_POLICY_ENFORCED", "true"),
    ("DEEP_RESEARCH_CONNECTOR_SOURCES", "true"),
    ("DEEP_RESEARCH_KNOWLEDGE_SOURCES", "true"),
    ("DEEP_RESEARCH_PLAN_TURN", "true"),
    ("DEEP_RESEARCH_SECOND_PASS", "true"),
    ("AGENT_MODES", "true"),
    // The cross-plane gates that read `1` (see LITERAL_ONE_EXCEPTIONS).
    ("ALLOW_UNVERIFIED_LEGACY_EVENTS", "1"),
    ("ISOLATED_E2E", "1"),
];

/// The switches this service reads by a rule other than the exact literal in [`EXACT_SWITCHES`]:
/// the default-on kill switches, the switches that are `true` in any case, and the wire booleans,
/// which are query parameters and not environment switches. Each stays as it is. None of them is
/// listed in [`EXACT_SWITCHES`].
pub const DOCUMENTED_EXCEPTIONS: &[(&str, &str)] = &[
    (
        "MODEL_GATEWAY_SOLE_NOTE_PREFETCH",
        "default-on kill switch: on unless 0, off, false, no or disabled (latency_levers)",
    ),
    (
        "MODEL_GATEWAY_RECHECK_PROPOSES_EDITS",
        "default-on kill switch: the same off words (latency_levers)",
    ),
    (
        "MODEL_GATEWAY_BALANCED_REVIEW_BATCHES",
        "default-on kill switch: the same off words (latency_levers)",
    ),
    (
        "MODEL_GATEWAY_REVISION_REVIEW_REUSE",
        "default-on kill switch: the same off words (latency_levers)",
    ),
    (
        "MODEL_GATEWAY_REVISION_FRAMING_GUARD",
        "default-on kill switch: the same off words (latency_levers)",
    ),
    (
        "MODEL_GATEWAY_INTURN_SCOPED_RECHECK",
        "default-on kill switch: the same off words (latency_levers)",
    ),
    (
        "MODEL_GATEWAY_TINY_CANDIDATE_SKIPS_PROPOSE_EDITS",
        "default-on kill switch: the same off words (latency_levers)",
    ),
    (
        "MODEL_GATEWAY_TARGETED_SOURCE_REPAIR",
        "default-on kill switch: the same off words (latency_levers)",
    ),
    (
        "MODEL_GATEWAY_LIST_ITEM_SEGMENTS",
        "default-on kill switch: the same off words (latency_levers)",
    ),
    (
        "MODEL_GATEWAY_VERIFICATION",
        "default-on kill switch: 0, off, false, no or disabled, any case, trimmed (verification)",
    ),
    (
        "MODEL_GATEWAY_SELF_CONSISTENCY",
        "default-on kill switch: the same off words (verification)",
    ),
    (
        "CHAT_ATTACHMENT_POLICY_ENABLED",
        "default-on kill switch: only 0 and false, any case, trimmed, turn it off (attachment_policy)",
    ),
    (
        "MODEL_GATEWAY_REVIEW_RUN_ENABLED",
        "true in any case, trimmed: switch_from_any_case, as the flag registry says (review_run)",
    ),
    (
        "MODEL_GATEWAY_TEACH_MODE_ENABLED",
        "true in any case, trimmed: switch_from_any_case, as the flag registry says (teach_routes)",
    ),
    (
        "VEREVON_RELEVANCE_CROSS_ENCODER_SCORES",
        "exact true, and any other spelling but an off word is refused at load, not read as off (relevance_calibration)",
    ),
    (
        "include_first",
        "wire boolean on the thread-messages query: 1 or true, not an environment switch",
    ),
    (
        "include_pinned",
        "wire boolean on the thread-list query: 1 or true, not an environment switch",
    ),
];

/// The switches that read the exact value `1` where the rest of the table reads `true`. The Data
/// Plane and Ingestion services read these same names with `1`, so the gateway keeps that spelling
/// rather than change a cross-plane contract. Each one is in [`EXACT_SWITCHES`] with the value `1`.
pub const LITERAL_ONE_EXCEPTIONS: &[(&str, &str)] = &[
    (
        "ALLOW_INSECURE_DEV_DEFAULTS",
        "the second dev gate: the Data Plane readers of this name take exactly 1",
    ),
    (
        "ALLOW_UNVERIFIED_LEGACY_EVENTS",
        "the doc-ready gate: the Data Plane readers of this name take exactly 1",
    ),
    (
        "ISOLATED_E2E",
        "the isolated e2e marker: the Data Plane and Ingestion readers of this name take exactly 1",
    ),
];

/// Every switch the report covers: [`EXACT_SWITCHES`], then the two that `coordinator` names (it is
/// the one file that names them, so they are read from there).
fn reported_switches() -> impl Iterator<Item = (&'static str, &'static str)> {
    EXACT_SWITCHES
        .iter()
        .chain(crate::coordinator::STARTUP_REPORT_SWITCHES.iter())
        .copied()
}

/// The reported switches whose value, as `lookup` returns it, is a spelling the exact reader does
/// not take. Each is returned with its exact value, in table order.
#[must_use]
pub fn former_spellings(
    lookup: impl Fn(&str) -> Option<String>,
) -> Vec<(&'static str, &'static str)> {
    reported_switches()
        .filter(|&(name, literal)| {
            mp_contracts::loop_fold::former_spelling(literal, lookup(name).as_deref())
        })
        .collect()
}

/// Logs one warning for each switch that is set to a former spelling. Called once, at startup,
/// before either listener opens. It changes no switch: each one stays off.
pub fn warn_former_spellings() {
    for (name, exact_value) in former_spellings(|name| std::env::var(name).ok()) {
        warn!(
            switch = name,
            exact_value,
            "switch is set to a value its exact reader does not take, so it reads as off; \
             set it to exactly this value to turn it on"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names reported for the given `(name, value)` environment.
    fn names(env: &[(&'static str, &'static str)]) -> Vec<&'static str> {
        former_spellings(|name| {
            env.iter()
                .find(|(set, _)| *set == name)
                .map(|(_, value)| (*value).to_owned())
        })
        .into_iter()
        .map(|(name, _)| name)
        .collect()
    }

    #[test]
    fn the_verified_switches_are_all_listed_once() {
        let mut seen = std::collections::BTreeSet::new();
        for (name, _) in EXACT_SWITCHES {
            assert!(seen.insert(*name), "{name} is listed twice");
        }
        for required in [
            "MODEL_GATEWAY_AUTH_DEV_BYPASS",
            "FINETUNE_ENABLED",
            "MODEL_GATEWAY_DATED_POST_SECTION_ONLY",
            "MODEL_GATEWAY_FINER_REVIEW_BATCHES",
            "MODEL_GATEWAY_BROWSER_ATTACH",
            "MODEL_GATEWAY_BROWSER_POLICY_ASK",
            "MODEL_GATEWAY_BROWSER_BUDGET_CLAIM",
            "MODEL_GATEWAY_BROWSER_PLANNER_QUOTAS",
        ] {
            assert!(seen.contains(required), "{required} is not listed");
        }
    }

    #[test]
    fn a_former_on_spelling_is_reported_for_every_listed_switch() {
        // `1`, `on`, `yes`, `enabled`, `TRUE` and padded `true` are the spellings the old readers
        // took as on. Each one is reported against each switch, except its own exact value.
        for (name, exact_value) in reported_switches() {
            for former in ["1", "on", "yes", "enabled", "true", "TRUE", " true "] {
                if former == exact_value {
                    continue;
                }
                assert_eq!(names(&[(name, former)]), vec![name], "{name}={former:?}");
            }
        }
    }

    #[test]
    fn the_exact_value_and_the_off_values_are_not_reported() {
        for (name, exact_value) in reported_switches() {
            assert!(
                names(&[(name, exact_value)]).is_empty(),
                "{name}={exact_value:?}"
            );
            for off in ["", "0", "false", "off", "no"] {
                assert!(names(&[(name, off)]).is_empty(), "{name}={off:?}");
            }
        }
        assert!(
            names(&[]).is_empty(),
            "an unset environment reports nothing"
        );
    }

    #[test]
    fn the_insecure_gate_reports_true_and_not_its_own_exact_value() {
        assert!(names(&[("ALLOW_INSECURE_DEV_DEFAULTS", "1")]).is_empty());
        assert_eq!(
            names(&[("ALLOW_INSECURE_DEV_DEFAULTS", "true")]),
            vec!["ALLOW_INSECURE_DEV_DEFAULTS"]
        );
    }

    #[test]
    fn a_report_carries_each_switch_with_its_own_exact_value() {
        let env = [
            ("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1"),
            ("FINETUNE_ENABLED", "on"),
            ("MODEL_GATEWAY_BROWSER_ATTACH", "true"),
        ];
        let reported = former_spellings(|name| {
            env.iter()
                .find(|(set, _)| *set == name)
                .map(|(_, value)| (*value).to_owned())
        });
        assert_eq!(
            reported,
            vec![
                ("MODEL_GATEWAY_AUTH_DEV_BYPASS", "true"),
                ("FINETUNE_ENABLED", "true"),
            ]
        );
    }

    /// The crate's production source, one entry per file: `(file name, lines)`, where the lines are
    /// the ones outside `#[cfg(test)]` items and outside comments. This file is left out, since it
    /// names every switch.
    fn production_sources() -> Vec<(String, Vec<String>)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, Vec<String>)>) {
            for entry in std::fs::read_dir(dir).expect("the crate's source directory is readable") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    walk(&path, out);
                    continue;
                }
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned();
                let is_rust = path.extension().and_then(|ext| ext.to_str()) == Some("rs");
                if !is_rust || name == "switch_spellings.rs" {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("a source file is readable");
                out.push((name, production_lines(&text.replace("\r\n", "\n"))));
            }
        }
        let mut out = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut out,
        );
        assert!(
            out.len() > 100,
            "the scan saw the crate's sources ({} files)",
            out.len()
        );
        out
    }

    /// `text` without its `#[cfg(test)]` items and its comment lines. A `#[cfg(test)]` on a one-line
    /// item drops that line; on any other item it drops the lines through the closing brace at the
    /// attribute's own indentation.
    fn production_lines(text: &str) -> Vec<String> {
        let lines: Vec<&str> = text.lines().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim_start();
            if trimmed.starts_with("#[cfg(test)]") {
                let indent = line.len() - trimmed.len();
                let next = lines.get(i + 1).map_or("", |next| next.trim());
                let one_line = (next.ends_with(';') && !next.contains('{'))
                    || (next.contains('{') && next.ends_with('}'));
                if one_line {
                    i += 2;
                    continue;
                }
                let mut end = i + 1;
                while end < lines.len() {
                    let body = lines[end];
                    let body_trimmed = body.trim_start();
                    if body.len() - body_trimmed.len() == indent && body_trimmed.starts_with('}') {
                        break;
                    }
                    end += 1;
                }
                i = end + 1;
                continue;
            }
            if !trimmed.starts_with("//") {
                out.push(line.to_owned());
            }
            i += 1;
        }
        out
    }

    /// The identifiers that `const NAME: &str = "value";` binds to `value` in `lines`.
    fn bound_idents(lines: &[String], value: &str) -> Vec<String> {
        let binding = format!("= \"{value}\";");
        lines
            .iter()
            .filter(|line| line.contains("const ") && line.contains(binding.as_str()))
            .filter_map(|line| {
                let declared = line.split("const ").nth(1)?;
                Some(declared.split(':').next()?.trim().to_owned())
            })
            .collect()
    }

    /// Whether `line` names `word` as a whole identifier.
    fn mentions(line: &str, word: &str) -> bool {
        let is_ident = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        line.match_indices(word).any(|(at, _)| {
            !is_ident(line[..at].chars().next_back())
                && !is_ident(line[at + word.len()..].chars().next())
        })
    }

    /// Whether `code` holds the exact reader of `literal`: the shared `switch_from` reader, which takes
    /// only `true`, or the quoted literal in an exact comparison or match arm.
    fn exact_reader_of(code: &str, literal: &str) -> bool {
        let quoted = format!("\"{literal}\"");
        let compares = code.contains(&format!("Some({quoted})"))
            || code.contains(&format!("== {quoted}"))
            || code.contains(&format!("{quoted} =>"));
        compares || (literal == "true" && code.contains("switch_from("))
    }

    /// Every listed switch is read by production code, and the code that holds its name or its
    /// constant holds the exact reader of its literal. A reader whose name changes, or whose literal
    /// changes away from the table, fails here.
    #[test]
    fn every_listed_switch_is_read_by_production_code_through_its_exact_reader() {
        let sources = production_sources();
        for (name, literal) in reported_switches() {
            assert!(
                literal == "true" || literal == "1" || literal == "on",
                "{name} takes {literal:?}, which no exact reader here is written for"
            );
            let quoted = format!("\"{name}\"");
            let definition = format!("= {quoted};");
            // The identifiers bound to the name anywhere in the crate. A file may read the name
            // through one of them (a constant imported from its defining module) or by the name.
            let idents: Vec<String> = sources
                .iter()
                .flat_map(|(_, lines)| bound_idents(lines, name))
                .collect();
            let mut reads = 0;
            for (file, lines) in &sources {
                let mut names_it = false;
                for line in lines.iter().map(String::as_str) {
                    if line.contains("const ") && line.contains(definition.as_str()) {
                        continue;
                    }
                    let by_name = line.contains(quoted.as_str());
                    names_it |= by_name;
                    if by_name || idents.iter().any(|ident| mentions(line, ident)) {
                        reads += 1;
                    }
                }
                // The file that defines the constant, or that holds the name, holds the exact reader.
                let defines = !bound_idents(lines, name).is_empty();
                if defines || names_it {
                    assert!(
                        exact_reader_of(&lines.join("\n"), literal),
                        "{file} holds {name}, but no exact reader of {literal:?} is in it"
                    );
                }
            }
            assert!(
                reads > 0,
                "{name} is listed, but no production code reads it"
            );
        }
    }

    /// The switches that read `1` are exactly the documented gates, and every other entry reads `true`
    /// or `on`.
    #[test]
    fn the_literal_one_switches_are_exactly_the_documented_gates() {
        let mut ones: Vec<&str> = EXACT_SWITCHES
            .iter()
            .filter(|(_, literal)| *literal == "1")
            .map(|(name, _)| *name)
            .collect();
        let mut documented: Vec<&str> = LITERAL_ONE_EXCEPTIONS
            .iter()
            .map(|(name, _)| *name)
            .collect();
        ones.sort_unstable();
        documented.sort_unstable();
        assert_eq!(ones, documented);
        let mut seen = std::collections::BTreeSet::new();
        for (name, _) in reported_switches() {
            assert!(seen.insert(name), "{name} is listed twice");
        }
    }

    /// Each documented exception is named by production code and is not also an exact switch.
    #[test]
    fn every_documented_exception_is_read_by_production_code_and_is_not_also_listed() {
        let sources = production_sources();
        for (name, reason) in DOCUMENTED_EXCEPTIONS {
            assert!(!reason.is_empty(), "{name} has no reason");
            assert!(
                reported_switches().all(|(listed, _)| listed != *name),
                "{name} is listed as exact and as an exception"
            );
            assert!(
                sources
                    .iter()
                    .any(|(_, lines)| lines.iter().any(|line| mentions(line, name))),
                "{name} is not read by production code"
            );
        }
    }

    /// The exact literal of every `true` entry is the one `switch_from` takes, and nothing else is.
    #[test]
    fn the_true_entries_are_the_literal_the_shared_reader_takes() {
        for (name, literal) in reported_switches() {
            if literal == "true" {
                assert!(
                    mp_contracts::loop_fold::switch_from(Some(literal)),
                    "{name} is listed as true, and switch_from does not take it"
                );
            }
        }
        assert!(!mp_contracts::loop_fold::switch_from(Some("1")));
    }
}
