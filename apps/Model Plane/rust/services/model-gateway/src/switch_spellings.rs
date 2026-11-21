//! Startup report for the default-off switches that read only their exact value, and the drift test
//! that keeps the report in step with the readers.
//!
//! Each switch in [`EXACT_SWITCHES`] is read exactly by its reader: the literal `true` (or `on`, for
//! the review proposal rule), or `1` for the gates in [`LITERAL_ONE_EXCEPTIONS`]. Before those
//! readers were made exact, several of them also took `1`, `on`, `yes`, `enabled`, or `true` in
//! another case or with surrounding whitespace. A live environment that still sets one of those
//! spellings now reads as off, silently.
//!
//! This module changes no reader and no switch value. A switch set to such a spelling stays off,
//! as it does under its exact reader. [`warn_former_spellings`] logs one warning per such switch
//! at startup, so the mismatch shows in the logs instead of only as a missing feature. It also warns
//! about a set value that a security-strengthening mode does not read ([`MODE_SWITCHES`]).
//!
//! The table names each switch through its reader's own constant, so a renamed constant fails to
//! compile. The drift test checks each reader: every place in this crate that reads a listed switch
//! holds the exact read of its literal, so a literal change or a lenient reader fails a test even
//! when another reader of the same switch, in the same file, is still exact.
//!
//! [`DOCUMENTED_EXCEPTIONS`] lists the switches this service reads by another rule (default-on kill
//! switches, switches that are `true` in any case, wire booleans) and says why. The tests check this
//! file against the production code: each listed switch must be read there by its exact reader.

use tracing::warn;

/// Each switch this service reads exactly, with the one value that turns it on. Every switch read
/// through `mp_contracts::loop_fold::switch_from` (default off) is here, so a padded or case-changed
/// value is reported at startup, except the ones that `coordinator` names (see `reported_switches`)
/// and the ones that `mp_contracts` reads for this service ([`SHARED_READER_SWITCHES`]). Each name is
/// the reader's own constant: keep it in step with the reader, and the drift test checks the reader.
pub const EXACT_SWITCHES: &[(&str, &str)] = &[
    (crate::auth::DEV_BYPASS_ENV, "true"),
    (crate::auth::INSECURE_DEV_DEFAULTS_ENV, "1"),
    (crate::auth::SERVICE_FLAG_AI_PII, "true"),
    (crate::auth::SERVICE_FLAG_AI_LAYOUT, "true"),
    (crate::finetune_poller::FEATURE_ENV, "true"),
    (crate::latency_levers::DATED_POST_SECTION_ONLY_ENV, "true"),
    (crate::latency_levers::FINER_REVIEW_BATCHES_ENV, "true"),
    (crate::prompt_context::STABLE_PROMPT_ORDER_ENV, "true"),
    (crate::prompt_context::DEDUP_CONTEXT_HISTORY_ENV, "true"),
    (crate::web_extraction::FLAG_ENV, "true"),
    (crate::room_metadata::LEGACY_PASSTHROUGH_ENV, "true"),
    (crate::browser_run::ATTACH_ENV, "true"),
    (crate::browser_run::ATTACH_POLICY_ENFORCED_ENV, "true"),
    (crate::browser_run::POLICY_ASK_ENV, "true"),
    (crate::browser_run::BUDGET_CLAIM_ENV, "true"),
    (crate::browser_run::PLANNER_QUOTAS_ENV, "true"),
    (crate::browser_grants::KEY_GRANTS_ENV, "true"),
    // Browser, planner and attachment switches.
    (crate::browser_page_context::ENABLED_ENV, "true"),
    (crate::browser_run::RECIPE_ENV, "true"),
    (crate::budget::CAP_MIN_ENV, "true"),
    (crate::budget::BUDGET_WINDOW_ENV, "true"),
    (crate::budget::BUDGET_WINDOW_PER_CAP_ENV, "true"),
    (crate::budget::DO_RUN_CAP_ENV, "true"),
    (crate::latency_levers::SPEED_TIER_ENV, "true"),
    (crate::normalize::BYO_GATEWAY_ENV, "true"),
    (crate::attachment_policy::SANDBOX_FILES_VAR, "true"),
    (crate::vision::parts::ENABLED_VAR, "true"),
    (crate::attachment_policy::DO_ATTACHMENTS_VAR, "true"),
    (crate::tools::SANDBOX_THREAD_WORKSPACE_ENV, "true"),
    // Agent, thread and run switches.
    (crate::agent_api_routes::ENABLED_ENV, "true"),
    (crate::ask_user_ask_mode::SWITCH_ENV, "true"),
    (crate::sse::AGENT_ASK_USER_ENV, "true"),
    (crate::sse::AGENT_ASK_USER_RESUME_ENV, "true"),
    (crate::sse::EXECUTION_COORDINATOR_MODE_ENV, "true"),
    (crate::sse::PLAN_EXECUTE_ON_APPROVE_ENV, "true"),
    (crate::assistant_navigation::ENABLED_ENV, "true"),
    (crate::thread_export_routes::ENABLED_ENV, "true"),
    (crate::thread_goal_routes::ENABLED_ENV, "true"),
    (crate::thread_reference_tools::ENABLED_ENV, "true"),
    (crate::thread_share_routes::ENABLED_ENV, "true"),
    (crate::http_routes::NAMED_AGENT_THREADS_ENV, "true"),
    (crate::http_routes::MESSAGE_VERSIONS_ENV, "true"),
    (crate::http_routes::REQUEST_INSPECTOR_ENV, "true"),
    (crate::http_routes::RUN_ORIGIN_FIELD_ENV, "true"),
    (crate::http_routes::APPROVAL_WRITE_PREVIEW_ENV, "true"),
    (crate::http_routes::APPROVAL_EDIT_ENV, "true"),
    (crate::artifact_edit_routes::EDITING_ENV, "true"),
    (crate::artifact_revisions::MODE_ENV, "on"),
    (crate::http_routes::WIDE_RESEARCH_ENTRY_ENV, "true"),
    (crate::sse_events::run_ceiling::WARN_ENV, "true"),
    (crate::sse_events::run_ceiling::ENFORCE_ENV, "true"),
    (crate::run_control::BUS_ENV, "true"),
    (crate::run_timing_persist::ENABLED_ENV, "true"),
    (crate::room_run::WORK_SUMMARY_ENV, "true"),
    (crate::room_run::LIVE_VIEW_ENV, "true"),
    (crate::room_run::LIVE_TEXT_ENV, "true"),
    (crate::room_run::MANAGER_STOP_ENV, "true"),
    (crate::chat_turn_registry::SHARED_TURN_RECORD_ENV, "true"),
    (crate::idempotency_registry::SHARED_IDEMPOTENCY_ENV, "true"),
    (crate::queued_input::SEND_RECEIPTS_ENV, "true"),
    (crate::side_chat_routes::ENABLED_ENV, "true"),
    (crate::research_runs::ENABLED_ENV, "true"),
    (crate::reasoning_translate_routes::ENABLED_ENV, "true"),
    (crate::recall_conversations::ENABLED_ENV, "true"),
    (crate::event_trigger_routes::ENABLED_ENV, "true"),
    (crate::goal_auto::ENABLED_ENV, "true"),
    (crate::goal_judge::GOAL_JUDGE_ENABLED_VAR, "true"),
    (crate::goal_unattended::ENABLED_ENV, "true"),
    (crate::thread_goal::SEGMENT_TOPUP_ENV, "true"),
    (crate::council_routes::ENABLED_ENV, "true"),
    (crate::selection_guard::ENABLED_ENV, "true"),
    (crate::role_routing::ROLE_ROUTING_ENV, "true"),
    (crate::policy_effective::POLICY_EFFECTIVE_ENV, "true"),
    (crate::search_provenance::SEARCH_PROVENANCE_ENV, "true"),
    (crate::context_refs::ENABLED_ENV, "true"),
    (crate::catalogue_prices::CATALOGUE_PRICES_ENV, "true"),
    (crate::page_reader::WEB_READER_ENV, "true"),
    (crate::web_bundle::WEB_BUNDLE_ENV, "true"),
    (crate::retrieval_tools::ENABLED_ENV, "true"),
    (crate::retrieval::GRAPH_SPACE_SCOPE_ENV, "true"),
    (crate::image_tool::ENABLED_ENV, "true"),
    (crate::deck_tool::DECK_TOOL_ENV, "true"),
    (crate::deck_tool::DECK_FROM_ARTIFACT_ENV, "true"),
    (crate::deck_templates::DECK_TEMPLATES_ENV, "true"),
    (crate::format_guidance::WIDGET_GUIDANCE_ENV, "true"),
    (crate::locale_context::USER_LOCALE_CONTEXT_VAR, "true"),
    (crate::run_context::TONE_PRESETS_VAR, "true"),
    (crate::run_context::USER_PROFILE_PREFIX_VAR, "true"),
    (crate::sse_events::CITATION_PASSAGES_ENV, "true"),
    (crate::thinking::LADDER_ENV, "true"),
    (crate::tool_loop::WEB_SEARCH_BATCH_ENV, "true"),
    (crate::tool_loop::VERIFY_ENFORCE_ENV, "true"),
    (crate::tool_loop::SKILL_BUNDLE_MOUNT_ENV, "true"),
    (crate::tool_result_handles::EXTENDED_ENV, "true"),
    (crate::zdr_posture::TYPED_REFUSALS_ENV, "true"),
    (crate::sse::ZDR_STREAM_COST_VAR, "true"),
    (crate::sse::ZDR_LEDGER_ROWS_VAR, "true"),
    (crate::sse::AGENT_USAGE_ENVELOPE_ENV, "true"),
    (crate::sse::TURN_USAGE_COMPLETE_ENV, "true"),
    (crate::sse::USAGE_PROGRESS_ENV, "true"),
    (crate::sse::ROUTING_DECISION_ENV, "true"),
    (crate::sse::SPACE_APPEND_RETRIEVAL_VAR, "true"),
    (crate::sse::SPACE_AGENTIC_RETRIEVAL_VAR, "true"),
    (crate::sse::OWNER_LIST_SPACE_REFUSE_VAR, "true"),
    (crate::sse::SUBAGENT_ROLE_CATALOGUE_VAR, "true"),
    (crate::sse::AGENT_TOOL_ALLOWLIST_VAR, "true"),
    (crate::sse::QUEUED_UNDELIVERED_IDS_VAR, "true"),
    (crate::quarry::attribution::ENABLED_ENV, "true"),
    (crate::review_proposal_rule::REVIEW_PROPOSAL_RULE_ENV, "on"),
    // Connectors, MCP, plugins, skills and deep research.
    (crate::mcp_probe::ENABLED_VAR, "true"),
    (crate::mcp_elicitation::ENABLED_ENV, "true"),
    (crate::mcp_apps::ENABLED_VAR, "true"),
    (crate::mcp_apps::NETWORK_ENABLED_VAR, "true"),
    (crate::mcp_app_calls::ENABLED_VAR, "true"),
    (crate::mcp_credential_ref::ENABLED_VAR, "true"),
    (
        crate::runtime_registries::capability_observation::OBSERVATION_ENV,
        "true",
    ),
    (crate::grpc::PLUGIN_LOADER_ENV, "true"),
    (crate::runtime_registries::HOOKS_DURABLE_ENV, "true"),
    (
        crate::skills::builtin_skills::BUILTIN_OFFICE_SKILLS_ENV,
        "true",
    ),
    (crate::tool_loop::connector_reads::ENABLED_ENV, "true"),
    (crate::tool_loop::connector_suggestion::ENABLED_ENV, "true"),
    (crate::semantic_screening::INLINE_ENV, "true"),
    (crate::skills::SKILL_PINS_ENV, "true"),
    (crate::skills::SKILL_SPACE_SCOPE_ENV, "true"),
    (crate::skills::SKILL_EXT_POLICY_ENV, "true"),
    (crate::deep_research::CONNECTOR_SOURCES_ENV, "true"),
    (crate::deep_research::KNOWLEDGE_SOURCES_ENV, "true"),
    (crate::deep_research::DEEP_RESEARCH_PLAN_TURN_ENV, "true"),
    (crate::deep_research::SECOND_PASS_ENV, "true"),
    (crate::profile::AGENT_MODES_ENV, "true"),
    // The cross-plane gates that read `1` (see LITERAL_ONE_EXCEPTIONS).
    (
        crate::doc_indexed_consumer::ALLOW_UNVERIFIED_LEGACY_EVENTS_ENV,
        "1",
    ),
    (crate::doc_indexed_consumer::ISOLATED_E2E_ENV, "1"),
    (crate::nats_connection::ALLOW_TOKEN_FALLBACK_ENV, "1"),
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
        "ORG_AGENT_POLICY_MODE",
        "a mode, shadow or enforce: any other value enforces, fail closed (org_agent_policy PolicyMode)",
    ),
    (
        "HOOKS_DO_RUNS",
        "a mode, shadow or enforce: any other value enforces, fail closed (runtime_registries HooksDoRunsMode, sse)",
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
    (
        "NATS_ALLOW_TOKEN_FALLBACK",
        "the NATS token fallback gate: the gateway and session-core read this name with exactly 1",
    ),
];

/// The switches that `mp_contracts` reads for this service through a helper, each with the value that
/// turns it on: the durable-loop, lifecycle, skill and custom-REST switches, and the space route
/// switches that the gateway's routes call. The helper's own reader is the exact one, so a padded or
/// mis-cased value is off in that crate too, and the report covers them here, where the operator looks.
/// None of them is read by name in this crate, so the per-reader scan does not cover them; the helpers
/// have their own tests.
pub const SHARED_READER_SWITCHES: &[(&str, &str)] = &[
    (mp_contracts::lifecycle_notice::ENABLED_ENV, "true"),
    (mp_contracts::loop_fold::LOOP_RESUME_ENV, "true"),
    (mp_contracts::loop_fold::LOOP_PARKING_ENV, "true"),
    (mp_contracts::loop_fold::CONTINUE_AFTER_APPROVAL_ENV, "true"),
    (mp_contracts::loop_guard::RESULT_AWARE_ENV, "true"),
    (mp_contracts::skill_bundle::SKILL_FILE_ENV, "true"),
    (mp_contracts::custom_rest::ENABLED_ENV, "true"),
    (mp_contracts::shell_rule::RULE_PROPOSALS_ENV, "true"),
    (mp_contracts::space_timeline::API_ENV, "true"),
    (mp_contracts::space_message_lifecycle::API_ENV, "true"),
    (mp_contracts::space_mentions::WAKE_API_ENV, "true"),
    (mp_contracts::space_mentions::WAKE_ROOM_CONTEXT_ENV, "true"),
    (mp_contracts::space_human_posts::API_ENV, "true"),
    (mp_contracts::space_history::API_ENV, "true"),
    (mp_contracts::space_attention::SUMMARY_ENV, "true"),
];

/// One security-strengthening mode switch, as the startup report sees it: the name its reader reads, the rule that says
/// a set value is one that reader does not take, and the words it does take, for the startup line. The rule is the
/// reader's own function, so the report and the reader cannot drift apart.
#[derive(Clone, Copy)]
pub struct ModeSwitch {
    /// The switch's environment name: the reader's own constant.
    pub name: &'static str,
    /// Whether a set value is one the reader does not take. An unset or blank value is never unreadable.
    pub unreadable: fn(&str) -> bool,
    /// The words the reader takes, as the startup line names them.
    pub readable: &'static str,
}

/// The security-strengthening mode switches this service reads, each with its reader's rule for a value it does not
/// take. `shadow` and `enforce` are read trimmed and case-folded by the loop guard and the step-up gate, so a mis-cased
/// word keeps its mode. Any other set value falls to the weaker reading (observe, or off), or to the fallback of the
/// reader, without an error, so [`warn_former_spellings`] reports each such value at startup. No reader changes. The
/// rules are unit-tested where their readers are defined.
pub const MODE_SWITCHES: &[ModeSwitch] = &[
    ModeSwitch {
        name: crate::http_routes::APPROVAL_STEP_UP_ENV,
        unreadable: mp_contracts::loop_guard::unreadable_step_up_word,
        readable: "off, shadow or enforce (any case, surrounding spaces allowed), or unset",
    },
    ModeSwitch {
        name: mp_contracts::loop_guard::ENV,
        unreadable: mp_contracts::loop_guard::unreadable_mode_word,
        readable: "shadow or enforce (any case, surrounding spaces allowed), or unset",
    },
    ModeSwitch {
        name: crate::sse::POSTURE_FLOOR_ENV,
        unreadable: crate::sse::unreadable_posture_floor,
        readable: "ask or auto (any case, surrounding spaces allowed), or unset",
    },
    ModeSwitch {
        name: crate::moderation::MODE_DEFAULT_ENV,
        unreadable: crate::moderation::unreadable_mode_default,
        readable: "exactly frame, tighten or enforce (surrounding spaces allowed), or unset",
    },
    ModeSwitch {
        name: crate::http_routes::AI_CHAT_METERING_ENV,
        unreadable: crate::http_routes::unreadable_ai_chat_metering,
        readable: "exactly service or all, or off, or unset",
    },
    ModeSwitch {
        name: mp_contracts::site_policy::WEB_DOMAIN_POLICY_MODE_ENV,
        unreadable: mp_contracts::site_policy::unreadable_web_domain_mode,
        readable: "exactly off, shadow or enforce (surrounding spaces allowed), or unset",
    },
];

/// Every switch the report covers: [`EXACT_SWITCHES`], the two that `coordinator` names (it is the one
/// file that names them, so they are read from there), and [`SHARED_READER_SWITCHES`].
fn reported_switches() -> impl Iterator<Item = (&'static str, &'static str)> {
    EXACT_SWITCHES
        .iter()
        .chain(crate::coordinator::STARTUP_REPORT_SWITCHES.iter())
        .chain(SHARED_READER_SWITCHES.iter())
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

/// The [`MODE_SWITCHES`] whose value, as `lookup` returns it, is set to a value its reader does not take (see
/// [`ModeSwitch::unreadable`]), in table order.
#[must_use]
pub fn unreadable_modes(lookup: impl Fn(&str) -> Option<String>) -> Vec<&'static ModeSwitch> {
    MODE_SWITCHES
        .iter()
        .filter(|switch| lookup(switch.name).is_some_and(|value| (switch.unreadable)(&value)))
        .collect()
}

/// Logs one warning for each switch that is set to a former spelling, and one for each mode switch
/// set to a value it does not read. Called once, at startup, before either listener opens. It changes
/// no switch: each one keeps the reading it had.
pub fn warn_former_spellings() {
    for (name, exact_value) in former_spellings(|name| std::env::var(name).ok()) {
        warn!(
            switch = name,
            exact_value,
            "switch is set to a value its exact reader does not take, so it reads as off; \
             set it to exactly this value to turn it on"
        );
    }
    for switch in unreadable_modes(|name| std::env::var(name).ok()) {
        warn!(
            switch = switch.name,
            readable = switch.readable,
            "mode switch is set to a value its reader does not take, so the fallback reading applies; \
             set it to one of the readable words, or unset it"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, HashMap};

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
        let mut seen = BTreeSet::new();
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
            "MODEL_GATEWAY_ARTIFACT_EDITING",
            "MODEL_GATEWAY_ARTIFACT_REVISIONS",
            "NATS_ALLOW_TOKEN_FALLBACK",
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
    fn the_token_fallback_gate_reports_true_and_not_its_exact_one() {
        assert!(names(&[("NATS_ALLOW_TOKEN_FALLBACK", "1")]).is_empty());
        assert_eq!(
            names(&[("NATS_ALLOW_TOKEN_FALLBACK", "true")]),
            vec!["NATS_ALLOW_TOKEN_FALLBACK"]
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

    #[test]
    fn the_shared_reader_switches_are_reported_for_a_padded_value_and_not_for_the_exact_one() {
        assert_eq!(SHARED_READER_SWITCHES.len(), 15);
        for &(name, literal) in SHARED_READER_SWITCHES {
            assert_eq!(literal, "true", "{name}");
            assert_eq!(names(&[(name, " true ")]), vec![name], "{name}");
            assert_eq!(names(&[(name, "true")]), Vec::<&str>::new(), "{name}");
        }
    }

    /// Per mode switch, the words its reader takes and values it does not, keyed by the reader's own constant.
    const MODE_SAMPLES: &[(&str, &[&str], &[&str])] = &[
        (
            mp_contracts::loop_guard::ENV,
            &["", "   ", "off", "shadow", "Shadow", " enforce ", "ENFORCE"],
            &["enforced", "true", "1", "on", "OFF", "Off", "enforce-all"],
        ),
        (
            crate::http_routes::APPROVAL_STEP_UP_ENV,
            &[
                "",
                "   ",
                "off",
                "OFF",
                "Off",
                " off ",
                "shadow",
                "Shadow",
                " enforce ",
                "ENFORCE",
            ],
            &["enforced", "true", "1", "on", "enforce-all"],
        ),
        (
            crate::sse::POSTURE_FLOOR_ENV,
            &["", "   ", "ask", " ASK ", "auto", " Auto "],
            &["asks", "ask-always", "always", "Enforce", "1", "on"],
        ),
        (
            crate::moderation::MODE_DEFAULT_ENV,
            &["", "   ", "frame", " tighten ", "enforce", "\tenforce\n"],
            &["Enforce", "ENFORCE", "Frame", "shadow", "1", "frames"],
        ),
        (
            crate::http_routes::AI_CHAT_METERING_ENV,
            &["", "   ", "off", "service", "all"],
            &["Service", "SERVICE", "ALL", " all ", "1", "on"],
        ),
        (
            mp_contracts::site_policy::WEB_DOMAIN_POLICY_MODE_ENV,
            &["", "   ", "off", " shadow ", "enforce"],
            &["Enforce", "ENFORCE", "Shadow", "on", "1", "true"],
        ),
    ];

    /// The mode switches the report names for `name` set to `value`.
    fn flagged(name: &str, value: &str) -> Vec<&'static str> {
        unreadable_modes(|set| (set == name).then(|| value.to_owned()))
            .into_iter()
            .map(|switch| switch.name)
            .collect()
    }

    #[test]
    fn the_mode_switches_flag_only_a_value_their_reader_does_not_take() {
        assert_eq!(MODE_SWITCHES.len(), MODE_SAMPLES.len());
        for (name, readable, unreadable) in MODE_SAMPLES {
            for word in *readable {
                assert!(flagged(name, word).is_empty(), "{name} reads {word:?}");
            }
            for word in *unreadable {
                assert_eq!(
                    flagged(name, word),
                    vec![*name],
                    "{name} does not read {word:?}"
                );
            }
            assert!(unreadable_modes(|_| None).is_empty(), "{name} unset");
        }
        // The loop guard keeps the word rule of `mp_contracts`, where `OFF` reads as shadow. The step-up
        // gate has its own rule, checked with the other mode switches above.
        for word in ["enforced", "true", "1", "on", "OFF", "Off", "enforce-all"] {
            assert!(
                mp_contracts::loop_guard::unreadable_mode_word(word),
                "{word:?}"
            );
        }
    }

    #[test]
    fn the_posture_floor_flags_asks_and_ask_always_and_reads_ask_auto_and_blank() {
        let name = crate::sse::POSTURE_FLOOR_ENV;
        assert_eq!(name, "HARNESS_POSTURE_FLOOR");
        // The floor folds case and reads only `ask` as Ask. `asks` and `ask-always` fall to the Auto floor, so the report names them.
        for value in ["asks", "ask-always", "ask always", "always", "Enforce"] {
            assert_eq!(flagged(name, value), vec![name], "{value:?}");
        }
        for value in ["ask", " ASK ", "auto", " Auto ", "", "   "] {
            assert!(flagged(name, value).is_empty(), "{value:?}");
        }
    }

    #[test]
    fn the_startup_mode_words_are_flagged_where_their_reader_does_not_take_them() {
        // Content safety and the web domain switch read exactly their words, after trimming: a mis-cased word is not one.
        for name in [
            crate::moderation::MODE_DEFAULT_ENV,
            mp_contracts::site_policy::WEB_DOMAIN_POLICY_MODE_ENV,
        ] {
            for value in ["Enforce", "ENFORCE"] {
                assert_eq!(flagged(name, value), vec![name], "{name}={value:?}");
            }
        }
        // The metering switch reads the exact word `service` or `all`, so a mis-cased `Service` is flagged.
        for value in ["Service", "SERVICE"] {
            assert_eq!(
                flagged(crate::http_routes::AI_CHAT_METERING_ENV, value),
                vec![crate::http_routes::AI_CHAT_METERING_ENV],
                "{value:?}"
            );
        }
        // The words each reader takes, a blank value and an unset switch are never flagged.
        for (name, words) in [
            (
                crate::moderation::MODE_DEFAULT_ENV,
                &["frame", "tighten", "enforce", " enforce ", ""][..],
            ),
            (
                mp_contracts::site_policy::WEB_DOMAIN_POLICY_MODE_ENV,
                &["off", "shadow", "enforce", ""][..],
            ),
            (
                crate::http_routes::AI_CHAT_METERING_ENV,
                &["service", "all", "off", ""][..],
            ),
        ] {
            for value in words {
                assert!(flagged(name, value).is_empty(), "{name}={value:?}");
            }
            assert!(unreadable_modes(|_| None).is_empty(), "{name} unset");
        }
    }

    #[test]
    fn the_new_mode_rows_are_listed_under_the_readers_own_names_and_not_as_exceptions() {
        for (name, literal) in [
            (crate::sse::POSTURE_FLOOR_ENV, "HARNESS_POSTURE_FLOOR"),
            (
                crate::moderation::MODE_DEFAULT_ENV,
                "CONTENT_SAFETY_MODE_DEFAULT",
            ),
            (
                crate::http_routes::AI_CHAT_METERING_ENV,
                "MODEL_GATEWAY_AI_CHAT_METERING",
            ),
            (
                mp_contracts::site_policy::WEB_DOMAIN_POLICY_MODE_ENV,
                "WEB_DOMAIN_POLICY_MODE",
            ),
        ] {
            assert_eq!(name, literal);
            assert!(
                MODE_SWITCHES.iter().any(|switch| switch.name == name),
                "{name} is not listed"
            );
            assert!(
                DOCUMENTED_EXCEPTIONS
                    .iter()
                    .all(|(listed, _)| *listed != name),
                "{name} is also a documented exception"
            );
        }
    }

    /// The crate's production source, one entry per file. Its lines are the ones outside
    /// `#[cfg(test)]` items and outside comments. This file is left out, since it names every switch.
    struct Source {
        /// The file name, such as `sse.rs`.
        file: String,
        /// The module names that declare this file (see `module_names_of`): a `module::NAME` path through
        /// one of them is a read of this file's constants.
        modules: Vec<String>,
        /// Whether each production line belongs to a `use` statement, which may span lines (`in_use_lines`).
        in_use: Vec<bool>,
        /// The production lines, and the 1-based line of each one in the file, for the report.
        lines: Vec<String>,
        numbers: Vec<usize>,
    }

    fn production_sources() -> Vec<Source> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(dir).expect("the crate's source directory is readable") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    walk(&path, out);
                    continue;
                }
                let file = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned();
                let is_rust = path.extension().and_then(|ext| ext.to_str()) == Some("rs");
                if !is_rust || file == "switch_spellings.rs" {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("a source file is readable");
                out.push((file, text.replace("\r\n", "\n")));
            }
        }
        let mut raw = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut raw,
        );
        // A file that a `#[cfg(test)]` module loads is test code, not production.
        let test_only = test_only_files(&raw);
        let mut out: Vec<Source> = raw
            .into_iter()
            .filter(|(file, _)| !test_only.contains(file))
            .map(|(file, text)| {
                let (numbers, lines): (Vec<usize>, Vec<String>) =
                    production_lines(&text).into_iter().unzip();
                Source {
                    file,
                    modules: Vec::new(),
                    in_use: in_use_lines(&lines),
                    lines,
                    numbers,
                }
            })
            .collect();
        assert!(
            out.len() > 30,
            "the scan saw the crate's sources ({} files)",
            out.len()
        );
        let modules: Vec<Vec<String>> = (0..out.len())
            .map(|owner| module_names_of(&out, owner))
            .collect();
        for (source, names) in out.iter_mut().zip(modules) {
            source.modules = names;
        }
        out
    }

    /// The module name, if `line` is a `mod NAME;` declaration that loads a file.
    fn declared_module(line: &str) -> Option<&str> {
        let rest = line.trim_start();
        let rest = rest
            .strip_prefix("pub(crate) ")
            .or_else(|| rest.strip_prefix("pub "))
            .unwrap_or(rest);
        let name = rest.strip_prefix("mod ")?.strip_suffix(';')?;
        (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')).then_some(name)
    }

    /// The file a `#[path = "..."]` attribute names, without its directories, if `line` is one.
    fn path_attribute(line: &str) -> Option<String> {
        let rest = line.trim_start().strip_prefix("#[path")?;
        let start = rest.find('"')? + 1;
        let end = start + rest[start..].find('"')?;
        rest[start..end]
            .rsplit(|c| c == '/' || c == '\\')
            .next()
            .map(str::to_owned)
    }

    /// The file the `mod NAME;` at `lines[l]` loads: the file a `#[path]` attribute above it names, or
    /// else `NAME.rs`. Also whether a `#[cfg(test)]` attribute above it gates the declaration.
    fn declared_file<S: AsRef<str>>(lines: &[S], l: usize, name: &str) -> (String, bool) {
        let mut file = format!("{name}.rs");
        let mut gated = false;
        let mut above = l;
        while above > 0 && lines[above - 1].as_ref().trim_start().starts_with("#[") {
            above -= 1;
            gated |= lines[above].as_ref().trim() == "#[cfg(test)]";
            if let Some(path) = path_attribute(lines[above].as_ref()) {
                file = path;
            }
        }
        (file, gated)
    }

    /// The files that a `#[cfg(test)]` module declaration loads. Those are test code, not production.
    fn test_only_files(raw: &[(String, String)]) -> Vec<String> {
        let mut out = Vec::new();
        for (_, text) in raw {
            let lines: Vec<&str> = text.lines().collect();
            for (l, line) in lines.iter().enumerate() {
                let Some(name) = declared_module(line) else {
                    continue;
                };
                let (file, gated) = declared_file(&lines, l, name);
                if gated {
                    out.push(file);
                }
            }
        }
        out
    }

    /// The module names that declare `sources[owner]`: each `mod NAME;` whose file is that one. So
    /// `pub mod parts;` under `#[path = "vision_parts.rs"]` gives `parts`, and not the file stem.
    fn module_names_of(sources: &[Source], owner: usize) -> Vec<String> {
        let file = sources[owner].file.as_str();
        let mut names = Vec::new();
        for source in sources {
            for (l, line) in source.lines.iter().enumerate() {
                let Some(name) = declared_module(line) else {
                    continue;
                };
                if declared_file(&source.lines, l, name).0 == file {
                    names.push(name.to_owned());
                }
            }
        }
        names
    }

    /// `text` without its `#[cfg(test)]` items and its comment lines, each with its 1-based line number
    /// in `text`. A `#[cfg(test)]` on a one-line item drops that line; on any other item it drops the
    /// lines through the closing brace at the attribute's own indentation.
    fn production_lines(text: &str) -> Vec<(usize, String)> {
        let lines: Vec<&str> = text.lines().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim_start();
            if trimmed.starts_with("#[cfg(test)]") {
                let indent = line.len() - trimmed.len();
                // The item is the first line after any further attributes, such as `#[path = "..."]`.
                let item = (i + 1..lines.len())
                    .find(|&next| !lines[next].trim_start().starts_with("#["))
                    .unwrap_or(lines.len());
                let next = lines.get(item).map_or("", |next| next.trim());
                let one_line = (next.ends_with(';') && !next.contains('{'))
                    || (next.contains('{') && next.ends_with('}'));
                if one_line {
                    i = item + 1;
                    continue;
                }
                let mut end = item;
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
                out.push((i + 1, line.to_owned()));
            }
            i += 1;
        }
        out
    }

    /// The name of the function whose header is `line`, when it is one.
    fn fn_name_of(line: &str) -> Option<&str> {
        let trimmed = line.trim_start();
        let at = trimmed.find("fn ")?;
        let qualifiers_only = trimmed[..at].split_whitespace().all(|word| {
            matches!(
                word,
                "pub" | "pub(crate)" | "pub(super)" | "async" | "const" | "unsafe"
            )
        });
        if !qualifiers_only {
            return None;
        }
        let name = trimmed[at + 3..]
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .next()?;
        (!name.is_empty()).then_some(name)
    }

    /// The `[start, end)` line range of every function in `lines`, with its name. A body ends at the
    /// closing brace at the header's own indentation, which rustfmt keeps.
    fn functions_in(lines: &[String]) -> Vec<(usize, usize, String)> {
        let mut out = Vec::new();
        for (start, line) in lines.iter().enumerate() {
            let Some(name) = fn_name_of(line) else {
                continue;
            };
            let header = line.trim_end();
            let indent = line.len() - line.trim_start().len();
            let one_line = header.ends_with(';')
                || (header.contains('{')
                    && header.matches('{').count() == header.matches('}').count());
            let end = if one_line {
                start + 1
            } else {
                lines[start + 1..]
                    .iter()
                    .position(|later| {
                        let body = later.trim_start();
                        later.len() - body.len() == indent && body.starts_with('}')
                    })
                    .map_or(lines.len(), |offset| start + offset + 2)
            };
            out.push((start, end, name.to_owned()));
        }
        out
    }

    /// Where each identifier and each quoted literal of the crate occurs, as `(source, line)` pairs.
    /// A quoted literal is keyed with its quotes, so the key `"NAME"` finds the string `NAME`.
    fn postings(sources: &[Source]) -> HashMap<String, Vec<(usize, usize)>> {
        let mut map: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
        for (s, source) in sources.iter().enumerate() {
            for (l, line) in source.lines.iter().enumerate() {
                let mut seen = BTreeSet::new();
                let mut ident = String::new();
                let mut quoted: Option<String> = None;
                for c in line.chars() {
                    if let Some(mut text) = quoted.take() {
                        if c == '"' {
                            seen.insert(format!("\"{text}\""));
                        } else {
                            text.push(c);
                            quoted = Some(text);
                        }
                        continue;
                    }
                    if c == '"' {
                        quoted = Some(String::new());
                    } else if c.is_alphanumeric() || c == '_' {
                        ident.push(c);
                    } else if !ident.is_empty() {
                        seen.insert(std::mem::take(&mut ident));
                    }
                }
                if !ident.is_empty() {
                    seen.insert(ident);
                }
                for key in seen {
                    map.entry(key).or_default().push((s, l));
                }
            }
        }
        map
    }

    /// `(source, identifier)` for each `const IDENT: &str = "VALUE";` in the crate, keyed by VALUE.
    fn const_definitions(sources: &[Source]) -> HashMap<String, Vec<(usize, String)>> {
        let mut out: HashMap<String, Vec<(usize, String)>> = HashMap::new();
        for (s, source) in sources.iter().enumerate() {
            for line in &source.lines {
                let Some(at) = line.find("const ") else {
                    continue;
                };
                let rest = &line[at + "const ".len()..];
                let Some(colon) = rest.find(':') else {
                    continue;
                };
                let ident = rest[..colon].trim();
                let shaped = !ident.is_empty()
                    && ident
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
                let Some(eq) = rest.find("= \"") else {
                    continue;
                };
                let value_start = eq + "= \"".len();
                let Some(close) = rest[value_start..].find('"') else {
                    continue;
                };
                let after = rest[value_start + close + 1..].trim_start();
                if !shaped || !after.starts_with(';') {
                    continue;
                }
                let value = &rest[value_start..value_start + close];
                out.entry(value.to_owned())
                    .or_default()
                    .push((s, ident.to_owned()));
            }
        }
        out
    }

    /// The production lines that name a listed switch without reading it: a route-table flag, a
    /// message, a startup table. The read check skips each one. Every name is read where it is used,
    /// by the reader the reason gives, and `every_naming_site_still_occurs_in_production_code` fails
    /// when a line here no longer exists, so an entry cannot outlive its line.
    const NAMING_SITES: &[(&str, &str, &str)] = &[
        (
            "auth.rs",
            "flag: Some(SERVICE_FLAG_AI_LAYOUT)",
            "a route-table flag, read by service_route_switched_off through service_flag_on (switch_from)",
        ),
        (
            "auth.rs",
            "flag: Some(SERVICE_FLAG_AI_PII)",
            "a route-table flag, read by service_route_switched_off through service_flag_on (switch_from)",
        ),
        (
            "coordinator.rs",
            // Split, so this file never names the constant (coordinator.rs's one-parser guards).
            concat!("(PLAN_EXECUTE", "_ENV, \"true\")"),
            "the startup report table: PlanModeStore::new reads it through plan_execute_from (switch_from)",
        ),
        (
            "coordinator.rs",
            concat!("(DURABLE_PLAN_GRANT", "_ENV, \"true\")"),
            "the startup report table: PlanModeStore::new reads it through durable_plan_grant_from (switch_from)",
        ),
        (
            "artifact_edit_routes.rs",
            "crate::artifact_revisions::MODE_ENV",
            "named in the refusal message only: artifact_revisions::ArtifactRevisionsMode::from_env reads it",
        ),
        (
            "review_proposal_rule.rs",
            "env = REVIEW_PROPOSAL_RULE_ENV",
            "a field of the warning that names the switch; the read is the `from_env` line above it",
        ),
    ];

    /// Whether the line at `l` of `source` only names a switch, and reads nothing (see `NAMING_SITES`).
    fn names_without_reading(source: &Source, l: usize) -> bool {
        NAMING_SITES
            .iter()
            .any(|(file, needle, _)| source.file == *file && source.lines[l].contains(needle))
    }

    /// Whether each production line of `lines` belongs to a `use` statement, which may span lines.
    fn in_use_lines(lines: &[String]) -> Vec<bool> {
        let mut flags = Vec::with_capacity(lines.len());
        let mut inside = false;
        for line in lines {
            let trimmed = line.trim_start();
            if !inside
                && ["use ", "pub use ", "pub(crate) use "]
                    .iter()
                    .any(|start| trimmed.starts_with(start))
            {
                inside = true;
            }
            flags.push(inside);
            if inside && line.trim_end().ends_with(';') {
                inside = false;
            }
        }
        flags
    }

    /// Whether `text` holds `segment` as a whole path segment followed by `::`: `parts::` but not
    /// `vision_parts::`.
    fn names_segment(text: &str, segment: &str) -> bool {
        let path = format!("{segment}::");
        text.match_indices(&path).any(|(at, _)| {
            !text[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
    }

    /// The names `source` binds to the constant `ident` of one of `modules` through a `use` statement:
    /// `ident` itself, or the alias of `ident as ALIAS`. A statement may span lines.
    fn names_bound_to(source: &Source, modules: &[String], ident: &str) -> Vec<String> {
        let mut names = Vec::new();
        let mut statement = String::new();
        for (l, line) in source.lines.iter().enumerate() {
            if !source.in_use[l] {
                continue;
            }
            statement.push_str(line);
            statement.push(' ');
            if !line.trim_end().ends_with(';') {
                continue;
            }
            let text = std::mem::take(&mut statement);
            if !(modules.iter().any(|module| names_segment(&text, module))
                && mentions(&text, ident))
            {
                continue;
            }
            let alias = text
                .split_once(format!("{ident} as ").as_str())
                .map(|(_, rest)| {
                    rest.chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect::<String>()
                })
                .filter(|name| !name.is_empty());
            names.push(alias.unwrap_or_else(|| ident.to_owned()));
        }
        names
    }

    /// The reads of the switch `name` in production code, as `(source, line)` pairs: each line that
    /// holds its quoted name, and each line that holds an identifier bound to it. An identifier is
    /// read in its own file, or as a path through a module that declares its file (`module::IDENT`),
    /// or, in a file that imports it, under the name the import binds (`IDENT`, or the alias of
    /// `IDENT as ALIAS`). A definition and a `use` statement are not reads.
    fn reader_sites(
        name: &str,
        sources: &[Source],
        postings: &HashMap<String, Vec<(usize, usize)>>,
        defs: &HashMap<String, Vec<(usize, String)>>,
    ) -> Vec<(usize, usize)> {
        let quoted = format!("\"{name}\"");
        // Each key with its owner and, when set, the one file that binds it with a `use` statement. A key
        // of the owner is read where its module path names it; a bound key is read only in its file.
        let mut keys: Vec<(String, Option<usize>, Option<usize>)> =
            vec![(quoted.clone(), None, None)];
        for (owner, ident) in defs.get(name).map(Vec::as_slice).unwrap_or_default() {
            keys.push((ident.clone(), Some(*owner), None));
            for (file, source) in sources.iter().enumerate() {
                if file == *owner {
                    continue;
                }
                for bound in names_bound_to(source, &sources[*owner].modules, ident) {
                    keys.push((bound, Some(*owner), Some(file)));
                }
            }
        }
        let mut sites = BTreeSet::new();
        for (key, owner, file) in keys {
            for &(s, l) in postings.get(&key).map(Vec::as_slice).unwrap_or_default() {
                let line = sources[s].lines[l].as_str();
                let is_definition =
                    line.contains("const ") && line.contains(&format!("= {quoted};"));
                if sources[s].in_use[l] || is_definition {
                    continue;
                }
                let read = match (owner, file) {
                    (None, _) => true,
                    (Some(_), Some(file)) => s == file,
                    (Some(owner), None) => {
                        s == owner
                            || sources[owner]
                                .modules
                                .iter()
                                .any(|module| mentions(line, &format!("{module}::{key}")))
                    }
                };
                if read {
                    sites.insert((s, l));
                }
            }
        }
        sites.into_iter().collect()
    }

    /// The statement lines per side of a read: a statement ends at a line that ends with `;`, `,`, `{`
    /// or `}`, so a wrapped call, a struct field and a `let` each keep their own lines. Bounded, so a
    /// read is never judged by a long run of neighbours.
    const MAX_STATEMENT_LINES: usize = 12;

    /// The indices of the lines of the statement that holds line `l`: the lines above it back to the
    /// one that ends a statement or a block, and the lines below it down to the first that does.
    fn statement_lines(lines: &[String], l: usize) -> Vec<usize> {
        let ends = |line: &str| {
            let text = line.trim_end();
            [";", ",", "{", "}"].iter().any(|end| text.ends_with(end))
        };
        let mut first = l;
        while first > 0 && l - first < MAX_STATEMENT_LINES && !ends(&lines[first - 1]) {
            first -= 1;
        }
        let mut last = l;
        while last + 1 < lines.len() && last - l < MAX_STATEMENT_LINES && !ends(&lines[last]) {
            last += 1;
        }
        (first..=last).collect()
    }

    /// The name a `let` statement binds, if `line` starts one: `raw` for `let raw = ...` or `let mut raw`.
    fn bound_name(line: &str) -> Option<String> {
        let rest = line.trim_start().strip_prefix("let ")?;
        let rest = rest.strip_prefix("mut ").unwrap_or(rest);
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        (!name.is_empty()).then_some(name)
    }

    /// The line that opens the call a line sits in, when that call wraps the line: the nearest line above
    /// that leaves a parenthesis open, within the bound. An argument of a call (`std::env::var(X)...,` under
    /// `if !enabled(`) is judged with the line that opens the call.
    fn opener_line(lines: &[String], l: usize) -> Option<usize> {
        let mut owed: i64 = 0;
        for j in (l.saturating_sub(MAX_STATEMENT_LINES)..l).rev() {
            let text = &lines[j];
            owed += text.matches(')').count() as i64 - text.matches('(').count() as i64;
            if owed < 0 {
                return Some(j);
            }
        }
        None
    }

    /// The text of the given lines, in order.
    fn text_of(lines: &[String], indices: &BTreeSet<usize>) -> String {
        indices
            .iter()
            .map(|&index| lines[index].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The two texts one read of a switch is checked by. The first is its own statement with the line that
    /// opens the call it sits in. The second adds the statement that first uses the variable the read binds
    /// (`let raw = std::env::var(X).ok();`), through the end of the block. The second is used only when the
    /// first does not vouch for the read, so a later use of the variable cannot widen a read that is exact.
    fn read_texts(lines: &[String], l: usize) -> (String, String) {
        let own = statement_lines(lines, l);
        let mut first_step: BTreeSet<usize> = own.iter().copied().collect();
        if let Some(opener) = opener_line(lines, l) {
            // Only the opening line: its statement would reach the sibling arguments after it.
            first_step.insert(opener);
        }
        let mut second_step = first_step.clone();
        if let (Some(&first), Some(&last)) = (own.first(), own.last()) {
            if let Some(name) = bound_name(&lines[first]) {
                for j in last + 1..lines.len().min(last + 1 + MAX_STATEMENT_LINES) {
                    let text = lines[j].trim();
                    if text == "}" || text.starts_with("fn ") {
                        break;
                    }
                    if mentions(&lines[j], &name) {
                        second_step.extend(statement_lines(lines, j));
                        break;
                    }
                }
            }
        }
        (text_of(lines, &first_step), text_of(lines, &second_step))
    }

    /// The texts one read of a switch is checked by (see `read_texts`), and the name of the function that
    /// holds it (for the report).
    fn read_text(
        sources: &[Source],
        functions: &[Vec<(usize, usize, String)>],
        (s, l): (usize, usize),
    ) -> (String, String, String) {
        let (first, second) = read_texts(&sources[s].lines, l);
        let function = functions[s]
            .iter()
            .filter(|(start, end, _)| *start <= l && l < *end)
            .max_by_key(|(start, _, _)| *start)
            .map_or_else(
                || String::from("(no function)"),
                |(_, _, name)| name.clone(),
            );
        (function, first, second)
    }

    /// Reads that fold, trim or otherwise widen the value: a switch reader may use none of them.
    const LENIENT_READS: &[&str] = &[
        ".trim()",
        "str::trim",
        "to_ascii_lowercase",
        "to_lowercase",
        "to_ascii_uppercase",
        "to_uppercase",
        "eq_ignore_ascii_case",
        "switch_from_any_case",
        "is_explicit_off",
    ];

    /// Whether `text` folds or trims the value, or accepts another spelling of the switch.
    fn widens(text: &str, literal: &str) -> bool {
        LENIENT_READS.iter().any(|read| text.contains(read))
            || ["1", "on", "yes", "enabled", "true", "TRUE", "True", "ON"]
                .into_iter()
                .filter(|word| *word != literal)
                .any(|word| text.contains(&format!("\"{word}\"")))
    }

    /// Whether `text` holds an exact read of `literal`: a comparison with it, a match arm on it, or,
    /// for `true`, the shared exact reader: `switch_from`, or its alias `switch_value_is_on`.
    fn has_exact_read(text: &str, literal: &str) -> bool {
        let mut markers = vec![
            format!("Some(\"{literal}\")"),
            format!("Ok(\"{literal}\")"),
            format!("== \"{literal}\""),
            format!("\"{literal}\" =>"),
        ];
        if literal == "true" {
            markers.push("switch_from(".to_owned());
            markers.push("switch_value_is_on".to_owned());
        }
        markers.iter().any(|marker| text.contains(marker.as_str()))
    }

    /// The names a text calls or names as a path: identifiers that are followed by `(`, and the last
    /// segment of a `::` path, as in `and_then(Self::recognise)`, which passes a function as a value.
    fn callees(text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        let mut at = 0;
        while at < chars.len() {
            if !(chars[at].is_alphanumeric() || chars[at] == '_') {
                at += 1;
                continue;
            }
            let start = at;
            while at < chars.len() && (chars[at].is_alphanumeric() || chars[at] == '_') {
                at += 1;
            }
            let after_path = start >= 2 && chars[start - 1] == ':' && chars[start - 2] == ':';
            if after_path || chars.get(at) == Some(&'(') {
                out.push(chars[start..at].iter().collect());
            }
        }
        out
    }

    /// The function bodies of the crate, by name: each with the index of the file that holds it.
    type Bodies = HashMap<String, Vec<(usize, String)>>;

    /// The bodies a name resolves to from `file`: the functions of that name in the same file or, when
    /// the file has none, every function of that name in the crate. A read is exact only when every one
    /// of them is, so a same-named lenient helper elsewhere cannot be passed off by an exact one.
    fn resolve<'a>(bodies: &'a Bodies, file: usize, name: &str) -> Vec<(usize, &'a str)> {
        let Some(all) = bodies.get(name) else {
            return Vec::new();
        };
        let same: Vec<(usize, &str)> = all
            .iter()
            .filter(|(owner, _)| *owner == file)
            .map(|(owner, body)| (*owner, body.as_str()))
            .collect();
        if same.is_empty() {
            all.iter()
                .map(|(owner, body)| (*owner, body.as_str()))
                .collect()
        } else {
            same
        }
    }

    /// Whether some callee of `text` reads `literal` exactly: it resolves to bodies that all do, followed
    /// `depth` levels deep.
    fn callee_reads_exactly(
        bodies: &Bodies,
        file: usize,
        text: &str,
        literal: &str,
        depth: u8,
    ) -> bool {
        callees(text).iter().any(|callee| {
            let targets = resolve(bodies, file, callee);
            !targets.is_empty()
                && targets
                    .iter()
                    .all(|&(owner, body)| helper_reads_exactly(bodies, owner, body, literal, depth))
        })
    }

    /// A helper that a read calls, followed `depth` levels deep: it reads the value exactly, or it calls
    /// one that does.
    fn helper_reads_exactly(
        bodies: &Bodies,
        file: usize,
        body: &str,
        literal: &str,
        depth: u8,
    ) -> bool {
        if widens(body, literal) {
            return false;
        }
        if has_exact_read(body, literal) {
            return true;
        }
        depth > 0 && callee_reads_exactly(bodies, file, body, literal, depth - 1)
    }

    /// Whether one read of a switch is exact. Its statement may not widen the value, and it must hold the
    /// exact read, unless a helper that the statement calls does, followed one more level down. The
    /// enclosing function is not a fallback: another read in it would vouch for this one. A read that
    /// folds, trims or takes another spelling fails.
    fn read_is_exact(bodies: &Bodies, file: usize, statement: &str, literal: &str) -> bool {
        if widens(statement, literal) {
            return false;
        }
        has_exact_read(statement, literal)
            || callee_reads_exactly(bodies, file, statement, literal, 1)
    }

    /// Every listed switch is read in production code, and each of its reads holds the exact read of
    /// its literal. The check runs per read, not per file: a file with several readers of several
    /// switches is no longer exempt for the one reader that drifts.
    #[test]
    fn every_reader_of_a_listed_switch_reads_its_exact_literal() {
        let sources = production_sources();
        let functions: Vec<Vec<(usize, usize, String)>> = sources
            .iter()
            .map(|source| functions_in(&source.lines))
            .collect();
        let mut bodies: Bodies = HashMap::new();
        for (s, source) in sources.iter().enumerate() {
            for (start, end, name) in &functions[s] {
                bodies
                    .entry(name.clone())
                    .or_default()
                    .push((s, source.lines[*start..*end].join("\n")));
            }
        }
        let postings = postings(&sources);
        let defs = const_definitions(&sources);
        let mut problems = Vec::new();
        let listed = EXACT_SWITCHES
            .iter()
            .chain(crate::coordinator::STARTUP_REPORT_SWITCHES.iter());
        for &(name, literal) in listed {
            let sites = reader_sites(name, &sources, &postings, &defs);
            if sites.is_empty() {
                problems.push(format!("{name} is listed, but no production code reads it"));
            }
            for (s, l) in sites {
                if names_without_reading(&sources[s], l) {
                    continue;
                }
                let (function, first, second) = read_text(&sources, &functions, (s, l));
                if !read_is_exact(&bodies, s, &first, literal)
                    && !read_is_exact(&bodies, s, &second, literal)
                {
                    problems.push(format!(
                        "{name}: {} production line {} (in {function}) does not read exactly {literal:?}",
                        sources[s].file,
                        sources[s].numbers[l]
                    ));
                }
            }
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }

    /// Each naming site still occurs in production code, so a `NAMING_SITES` entry cannot outlive its
    /// line, and each one gives its reason.
    #[test]
    fn every_naming_site_still_occurs_in_production_code() {
        let sources = production_sources();
        for (file, needle, reason) in NAMING_SITES {
            assert!(!reason.is_empty(), "{file}: {needle} has no reason");
            assert!(
                sources.iter().any(|source| {
                    source.file == *file && source.lines.iter().any(|line| line.contains(needle))
                }),
                "{needle} is no longer in production code of {file}"
            );
        }
    }

    /// A read bound to a variable is judged by the first statement that uses the variable, and only when
    /// its own statement does not vouch for it. A later use of the variable cannot widen it.
    #[test]
    fn a_bound_read_is_judged_with_the_first_statement_that_uses_the_variable() {
        let lines: Vec<String> = [
            "    let raw = std::env::var(A).ok();",
            "    let flag = switch_from(raw.as_deref());",
            "    let shown = raw.as_deref().map(str::trim);",
            "    let other = std::env::var(B).is_ok();",
            "    let lenient = std::env::var(C).ok();",
            "    let shown_c = lenient.is_some();",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        let none: Bodies = HashMap::new();
        let (own, bound) = read_texts(&lines, 0);
        assert!(!read_is_exact(&none, 0, &own, "true"));
        assert!(read_is_exact(&none, 0, &bound, "true"));
        let (own, bound) = read_texts(&lines, 3);
        assert!(!read_is_exact(&none, 0, &own, "true"));
        assert!(!read_is_exact(&none, 0, &bound, "true"));
        let (own, bound) = read_texts(&lines, 4);
        assert!(!read_is_exact(&none, 0, &own, "true"));
        assert!(!read_is_exact(&none, 0, &bound, "true"));
    }

    /// An argument of a call that wraps it is judged with the line that opens the call, when the argument's
    /// own statement ends at the line above it.
    #[test]
    fn an_argument_of_a_wrapping_call_is_judged_with_the_call() {
        let lines: Vec<String> = [
            "    Self::from_values(",
            "        std::env::var(A).ok().as_deref(),",
            "        std::env::var(B).ok().as_deref(),",
            "    )",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        let mut bodies: Bodies = HashMap::new();
        bodies.insert(
            "from_values".to_owned(),
            vec![(
                0,
                "fn from_values(a: Option<&str>, b: Option<&str>) -> bool { let on = switch_value_is_on; on(a) && on(b) }"
                    .to_owned(),
            )],
        );
        assert_eq!(opener_line(&lines, 2), Some(0));
        let (first, _) = read_texts(&lines, 2);
        assert!(read_is_exact(&bodies, 0, &first, "true"));
        assert!(!read_is_exact(
            &bodies,
            0,
            "std::env::var(B).ok().as_deref(),",
            "true"
        ));
    }

    /// A sibling argument's exact read does not vouch for a lenient argument of the same call.
    #[test]
    fn a_sibling_argument_does_not_vouch_for_a_lenient_one() {
        let lines: Vec<String> = [
            "    Self::combine(",
            "        switch_from(std::env::var(A).ok().as_deref()),",
            "        std::env::var(B).is_some(),",
            "    )",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        let none: Bodies = HashMap::new();
        let (first, second) = read_texts(&lines, 2);
        assert!(!read_is_exact(&none, 0, &first, "true"));
        assert!(!read_is_exact(&none, 0, &second, "true"));
    }

    /// The scan keys a path on the module name that declares a file, not on the file's stem: a
    /// `parts::ENABLED_VAR` read from another file is seen, and `vision_parts::ENABLED_VAR` is not. A
    /// file that imports the constant, under its name or an alias, reads it through that name; a file
    /// that does not import it does not read a bare name.
    #[test]
    fn a_read_through_a_module_path_or_an_import_is_seen_and_an_unimported_name_is_not() {
        let source = |file: &str, lines: &[&str]| {
            let lines: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
            Source {
                file: file.to_owned(),
                modules: Vec::new(),
                in_use: in_use_lines(&lines),
                numbers: (1..=lines.len()).collect(),
                lines,
            }
        };
        let mut sources = vec![
            source(
                "vision_parts.rs",
                &["pub const ENABLED_VAR: &str = \"CHAT_VISION_AS_PARTS_ENABLED\";"],
            ),
            source(
                "vision.rs",
                &["#[path = \"vision_parts.rs\"]", "pub mod parts;"],
            ),
            source(
                "reader.rs",
                &[
                    "fn on() -> bool { switch_from(std::env::var(parts::ENABLED_VAR).ok().as_deref()) }",
                    "fn stem() -> bool { switch_from(std::env::var(vision_parts::ENABLED_VAR).ok().as_deref()) }",
                ],
            ),
            source(
                "alias.rs",
                &[
                    "use crate::vision::{parts::ENABLED_VAR as VISION_AS_PARTS_VAR, Other};",
                    "fn aliased() -> bool { switch_from(get(VISION_AS_PARTS_VAR).as_deref()) }",
                    "fn bare_unimported() -> bool { switch_from(std::env::var(ENABLED_VAR).ok().as_deref()) }",
                ],
            ),
        ];
        let modules: Vec<Vec<String>> = (0..sources.len())
            .map(|owner| module_names_of(&sources, owner))
            .collect();
        for (source, names) in sources.iter_mut().zip(modules) {
            source.modules = names;
        }
        assert_eq!(sources[0].modules, vec!["parts".to_owned()]);
        let postings = postings(&sources);
        let defs = const_definitions(&sources);
        assert_eq!(
            reader_sites("CHAT_VISION_AS_PARTS_ENABLED", &sources, &postings, &defs),
            vec![(2, 0), (3, 1)]
        );
    }

    /// The scan itself: an exact read passes, and a read that folds, trims, takes another spelling or
    /// accepts any value fails, with or without a helper in between.
    #[test]
    fn the_reader_check_tells_an_exact_read_from_a_lenient_or_wider_one() {
        let none: Bodies = HashMap::new();
        assert!(read_is_exact(
            &none,
            0,
            "switch_from(std::env::var(X).ok().as_deref())",
            "true"
        ));
        assert!(read_is_exact(
            &none,
            0,
            "std::env::var(X).as_deref() == Ok(\"1\")",
            "1"
        ));
        assert!(!read_is_exact(
            &none,
            0,
            "std::env::var(X).ok().as_deref() == Some(\"1\")",
            "true"
        ));
        assert!(!read_is_exact(
            &none,
            0,
            "std::env::var(X).ok().map(|v| v.trim().to_owned()).as_deref() == Some(\"true\")",
            "true"
        ));
        assert!(!read_is_exact(
            &none,
            0,
            "switch_from_any_case(std::env::var(X).ok().as_deref())",
            "true"
        ));
        assert!(!read_is_exact(&none, 0, "std::env::var(X).is_ok()", "true"));
        let mut helpers: Bodies = HashMap::new();
        helpers.insert(
            "parse_enabled".to_owned(),
            vec![(
                0,
                "fn parse_enabled(raw: Option<&str>) -> bool { switch_from(raw) }".to_owned(),
            )],
        );
        assert!(read_is_exact(
            &helpers,
            0,
            "parse_enabled(std::env::var(X).ok().as_deref())",
            "true"
        ));
        helpers.insert(
            "parse_enabled".to_owned(),
            vec![(
                0,
                "fn parse_enabled(raw: Option<&str>) -> bool { raw.is_some() }".to_owned(),
            )],
        );
        assert!(!read_is_exact(
            &helpers,
            0,
            "parse_enabled(std::env::var(X).ok().as_deref())",
            "true"
        ));
        // A lenient statement fails, whatever else holds the exact read.
        assert!(!read_is_exact(&none, 0, "let v = raw.trim();", "true"));
    }

    /// A same-named helper is resolved in the file that calls it. A read is exact only when every helper
    /// it can reach is, so a lenient `parse_enabled` in one file is not passed by an exact one in another.
    #[test]
    fn a_helper_is_resolved_in_its_own_file_and_every_candidate_must_be_exact() {
        let exact = "fn parse_enabled(raw: Option<&str>) -> bool { switch_from(raw) }";
        let lenient = "fn parse_enabled(raw: Option<&str>) -> bool { raw.is_some() }";
        let mut bodies: Bodies = HashMap::new();
        bodies.insert(
            "parse_enabled".to_owned(),
            vec![(0, exact.to_owned()), (1, lenient.to_owned())],
        );
        let call = "parse_enabled(std::env::var(X).ok().as_deref())";
        assert!(
            read_is_exact(&bodies, 0, call, "true"),
            "file 0 has its own exact helper"
        );
        assert!(
            !read_is_exact(&bodies, 1, call, "true"),
            "file 1 has the lenient helper"
        );
        assert!(
            !read_is_exact(&bodies, 2, call, "true"),
            "a file with none of its own needs every helper of that name to be exact"
        );
    }

    /// A function passed as a value (`and_then(Self::recognise)`) is followed to its body, as a call is.
    #[test]
    fn a_path_passed_as_a_value_is_followed_to_its_body() {
        let mut bodies: Bodies = HashMap::new();
        bodies.insert(
            "recognise".to_owned(),
            vec![(
                0,
                "fn recognise(value: &str) -> Option<Self> { match value { \"on\" => Some(Self::On), _ => None } }"
                    .to_owned(),
            )],
        );
        assert!(read_is_exact(
            &bodies,
            0,
            "value.and_then(Self::recognise)",
            "on"
        ));
    }

    /// A statement is the lines from the one after a line that ends a statement or block, through the
    /// first line that ends one. A wrapped call is one statement, and a struct field is its own.
    #[test]
    fn statement_lines_keep_a_wrapped_call_whole_and_a_field_on_its_own() {
        let lines: Vec<String> = [
            "Self {",
            "    sandbox: switch_from(get(A).as_deref()),",
            "    vision: switch_from(",
            "        get(B)",
            "            .as_deref(),",
            "    ),",
            "    other: get(C).is_some(),",
            "}",
            "let on = switch_value_is_on;",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        assert_eq!(statement_lines(&lines, 1), vec![1]);
        assert_eq!(statement_lines(&lines, 3), vec![2, 3, 4]);
        assert_eq!(statement_lines(&lines, 6), vec![6]);
        assert_eq!(statement_lines(&lines, 8), vec![8]);
    }

    /// A lenient read is judged by its own statement. An exact read on the line before it, or elsewhere
    /// in its function, does not vouch for it: the scan must fail the lenient read.
    #[test]
    fn an_exact_read_next_to_a_lenient_one_does_not_vouch_for_it() {
        let lines: Vec<String> = [
            "    let a = switch_from(std::env::var(A).ok().as_deref());",
            "    let b = std::env::var(B).is_ok();",
            "    a && b",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        let none: Bodies = HashMap::new();
        let statement = |l: usize| {
            statement_lines(&lines, l)
                .into_iter()
                .map(|index| lines[index].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert!(read_is_exact(&none, 0, &statement(0), "true"));
        assert!(
            !read_is_exact(&none, 0, &statement(1), "true"),
            "the exact read of A on the line before vouched for B"
        );
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
                    .any(|source| source.lines.iter().any(|line| mentions(line, name))),
                "{name} is not read by production code"
            );
        }
    }

    /// Whether `line` names `word` as a whole identifier, or as a quoted literal.
    fn mentions(line: &str, word: &str) -> bool {
        let is_ident = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        line.match_indices(word).any(|(at, _)| {
            !is_ident(line[..at].chars().next_back())
                && !is_ident(line[at + word.len()..].chars().next())
        })
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
        let mut seen = BTreeSet::new();
        for (name, _) in reported_switches() {
            assert!(seen.insert(name), "{name} is listed twice");
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
