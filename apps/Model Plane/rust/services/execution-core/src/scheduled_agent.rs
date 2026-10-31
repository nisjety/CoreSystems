//! The scheduled Do run (WS20 S6): a bounded agent loop for a schedule whose owner stored a
//! read-only consent contract (DEC-16, amended not reversed). It offers the contract's read
//! tools and nothing else, records every other gated effect as withheld instead of pausing for
//! an approval that no one is present to give, and reports counts. Off by default: the lane
//! runs only when `EXECUTION_SCHEDULED_AGENT_ENABLED` is the literal `true`, and a step without
//! a contract never takes it.
//!
//! This module holds the pure parts: admitting the contract the step carries, narrowing the
//! offer, shaping the request, recording withheld effects and rendering the transcript. The
//! driver call itself is made by the gRPC lane, which owns the bearers and channels.

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1 as pb;
use mp_contracts::scheduled_contract::{delegable, non_delegable_tool, ScheduledContract};
use sha2::{Digest, Sha256};

use crate::state::{StateStore, WithheldEffect};

pub(crate) const SWITCH_ENV: &str = "EXECUTION_SCHEDULED_AGENT_ENABLED";
/// The model the scheduled lane runs on. It is never user-selectable (section 2.6 row 19).
pub(crate) const MODEL: &str = "verevon-balance";
/// The round budget a scheduled run may spend, whatever its contract asks for.
pub(crate) const MAX_ROUNDS: u32 = 8;
/// The wall clock of one scheduled run. The activity's start-to-close timeout sits above it.
pub(crate) const WALL_CLOCK_SECS: u64 = 240;

/// A gated effect the run would have needed an approval for, and which it withheld.
pub(crate) const REASON_NOT_COVERED: &str = "scheduled_not_covered";
/// A `web_fetch` whose URL carries a query string, refused in the scheduled lane.
pub(crate) const REASON_EGRESS_QUERY: &str = "scheduled_egress_query";
/// The step's contract is past its expiry at the moment it is checked.
pub(crate) const REFUSAL_EXPIRED: &str = "scheduled_contract_expired";
/// The step's contract does not match its digest, or the step's own expiry field.
pub(crate) const REFUSAL_INVALID: &str = "scheduled_contract_invalid";
/// The run is zero-retention or waived: an unattended loop keeps no durable state for it.
pub(crate) const REFUSAL_RETENTION: &str = "scheduled_zdr_refused";

/// What the scheduled lane changes in the loop it drives. `None` for every other run, whose
/// loop is unchanged. Its presence also keeps the loop from offering held results and from
/// appending the plain answer, because the scheduled lane writes its transcript itself.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LoopScope {
    /// The contract's per-round output cap, in place of the loop's fixed cap.
    pub(crate) max_tokens_per_round: i32,
}

/// Whether the scheduled lane is on. Only the literal `true` enables it.
pub(crate) fn enabled() -> bool {
    enabled_from(std::env::var(SWITCH_ENV).ok().as_deref())
}

fn enabled_from(value: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(value)
}

/// Verify the stored contract a step carries before any work: its digest over the exact text,
/// its own shape and version, that its expiry is the one the step names, and that the expiry
/// has not passed. Refusals are closed tokens and never echo the contract.
pub(crate) fn admit(
    contract_json: &str,
    contract_digest: &str,
    contract_expires_at: &str,
    now: DateTime<Utc>,
) -> Result<ScheduledContract, &'static str> {
    if mp_contracts::scheduled_contract::digest(contract_json) != contract_digest {
        return Err(REFUSAL_INVALID);
    }
    let contract = ScheduledContract::parse(contract_json).map_err(|refusal| refusal.as_str())?;
    if contract.expires_at != contract_expires_at {
        return Err(REFUSAL_INVALID);
    }
    let expires =
        DateTime::parse_from_rfc3339(&contract.expires_at).map_err(|_| REFUSAL_INVALID)?;
    if now >= expires.with_timezone(&Utc) {
        return Err(REFUSAL_EXPIRED);
    }
    Ok(contract)
}

/// A zero-retention or waived run is refused before any work: the scheduled lane has no
/// durable state to keep its content in, so it refuses rather than persist (DEC-32).
pub(crate) fn refuses_retention(zdr: bool, waived: bool) -> Option<&'static str> {
    (zdr || waived).then_some(REFUSAL_RETENTION)
}

/// The loop scope a verified contract gives its run.
pub(crate) fn loop_scope(contract: &ScheduledContract) -> LoopScope {
    LoopScope {
        // The contract parser bounds this to 1..=4096, so it always fits the wire type.
        max_tokens_per_round: contract.max_tokens_per_round as i32,
    }
}

/// The wall clock of one scheduled run: the lane's ceiling, or the time the contract has left
/// when that is shorter, so a run never outlives the consent it runs under. A contract whose
/// expiry does not parse gets no time at all.
pub(crate) fn wall_clock(contract: &ScheduledContract, now: DateTime<Utc>) -> std::time::Duration {
    let Ok(expires) = DateTime::parse_from_rfc3339(&contract.expires_at) else {
        return std::time::Duration::ZERO;
    };
    let left = u64::try_from((expires.with_timezone(&Utc) - now).num_seconds()).unwrap_or(0);
    std::time::Duration::from_secs(left.min(WALL_CLOCK_SECS))
}

/// The tools the scheduled run is offered: the built-in set narrowed to the contract's tools,
/// each still delegable. A browser, process, MCP or schedule tool is never offered, whatever
/// the contract names, and a contract tool the built-in set does not know is not offered.
pub(crate) fn offered_defs(
    contract: &ScheduledContract,
    built_in: Vec<pb::ToolDefinition>,
) -> Vec<pb::ToolDefinition> {
    built_in
        .into_iter()
        .filter(|def| {
            !non_delegable_tool(&def.name)
                && contract
                    .tools
                    .iter()
                    .any(|tool| tool.tool == def.name && delegable(&tool.class))
        })
        .collect()
}

/// The run request the scheduled lane drives. It is always plan mode at the read-only rung,
/// carries no caller tools and no subagents, is never zero-retention, and is bounded by the
/// contract's rounds and the lane's ceiling. The subject is the owner the schedule acts for.
pub(crate) fn run_request(
    step: &pb::ExecuteScheduledStepRequest,
    goal: &str,
    contract: &ScheduledContract,
) -> pb::RunAgentRequest {
    pb::RunAgentRequest {
        run_id: step.run_id.clone(),
        thread_id: step.thread_id.clone(),
        goal: goal.to_owned(),
        org_id: step.org_id.clone(),
        user_id: step.subject_id.clone(),
        model: MODEL.to_owned(),
        mode: "ask".to_owned(),
        max_rounds: contract.max_rounds.min(MAX_ROUNDS),
        zdr: false,
        tools: Vec::new(),
        plan_mode: true,
        autonomy_rung: pb::AutonomyRung::ReadOnly as i32,
        space_id: step.space_id.clone(),
        max_subagents: 0,
        ..Default::default()
    }
}

/// Record a gated effect the run withholds: the tool, a closed reason and the digest of the
/// arguments, never the arguments. Returns whether the run is unattended, so an attended run
/// keeps its own refusal path unchanged.
pub(crate) fn withhold(
    state: &StateStore,
    run_id: &str,
    tool: &str,
    arguments_json: &str,
    reason: &'static str,
) -> bool {
    state.withhold(
        run_id,
        WithheldEffect {
            tool: tool.to_owned(),
            reason_code: reason,
            args_sha256: format!("sha256:{:x}", Sha256::digest(arguments_json.as_bytes())),
        },
    )
}

/// A URL with a query string in an unattended run is refused: the query is the one way the
/// run's own content leaves it, since `web_fetch` is only a GET (WS20 S6, K3-23). The refusal
/// is always on in the scheduled lane, which is stricter than refusing only after a flagged
/// result, because the lane has no flag to read at this point.
pub(crate) fn egress_refusal(tool: &str, arguments_json: &str) -> Option<&'static str> {
    if tool != "web_fetch" {
        return None;
    }
    let url = serde_json::from_str::<serde_json::Value>(arguments_json)
        .ok()?
        .get("url")?
        .as_str()?
        .to_owned();
    url.contains('?').then_some(REASON_EGRESS_QUERY)
}

/// What the model reads for a withheld call. It names the reason and never the arguments.
pub(crate) fn withheld_refusal(tool: &str) -> String {
    format!(
        "tool '{tool}' needs the owner's approval, and no one is present to give it during a \
         scheduled run. This effect was withheld and the owner will see that it was. Describe \
         what you would have done instead of doing it."
    )
}

/// The answer the owner reads, with any withheld effects listed after it. Tool names and
/// reasons only: the arguments stay out of the transcript.
pub(crate) fn transcript_text(answer: &str, withheld: &[WithheldEffect]) -> String {
    if withheld.is_empty() {
        return answer.to_owned();
    }
    let mut text = answer.trim_end().to_owned();
    text.push_str("\n\nWithheld for your review:");
    for effect in withheld {
        text.push_str(&format!("\n- {} ({})", effect.tool, effect.reason_code));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use mp_contracts::scheduled_contract::{digest, ContractRefusal};

    const CONTRACT: &str = r#"{"expires_at":"2026-11-23T10:00:00Z","max_rounds":6,"max_tokens_per_round":2048,"tools":[{"approved_at":"2026-10-24T10:00:00Z","capability":"cap.tool.web_search","class":"read","tool":"web_search"}],"v":1}"#;

    fn before_expiry() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 25, 0, 0, 0).unwrap()
    }

    fn after_expiry() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 24, 0, 0, 0).unwrap()
    }

    fn def(name: &str) -> pb::ToolDefinition {
        pb::ToolDefinition {
            name: name.to_owned(),
            description: String::new(),
            parameters_json: "{}".to_owned(),
        }
    }

    fn step() -> pb::ExecuteScheduledStepRequest {
        pb::ExecuteScheduledStepRequest {
            run_id: "run_01".to_owned(),
            thread_id: "thr_01".to_owned(),
            org_id: "org_01".to_owned(),
            subject_id: "usr_owner".to_owned(),
            space_id: "spc_01".to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn switch_is_on_only_for_the_literal_true() {
        assert!(enabled_from(Some("true")));
        assert!(!enabled_from(Some("1")));
        assert!(!enabled_from(Some("")));
        assert!(!enabled_from(None));
    }

    #[test]
    fn admit_accepts_a_matching_unexpired_contract() {
        let contract = admit(
            CONTRACT,
            &digest(CONTRACT),
            "2026-11-23T10:00:00Z",
            before_expiry(),
        )
        .expect("a matching, unexpired contract is admitted");
        assert_eq!(contract.max_rounds, 6);
    }

    #[test]
    fn admit_refuses_a_forged_or_mismatched_contract() {
        let forged = digest(&CONTRACT.replacen("web_search", "shell_x", 1));
        assert_eq!(
            admit(CONTRACT, &forged, "2026-11-23T10:00:00Z", before_expiry()),
            Err(REFUSAL_INVALID)
        );
        assert_eq!(
            admit(
                CONTRACT,
                &digest(CONTRACT),
                "2026-11-30T10:00:00Z",
                before_expiry()
            ),
            Err(REFUSAL_INVALID)
        );
    }

    #[test]
    fn admit_refuses_an_expired_contract_and_a_malformed_one() {
        assert_eq!(
            admit(
                CONTRACT,
                &digest(CONTRACT),
                "2026-11-23T10:00:00Z",
                after_expiry()
            ),
            Err(REFUSAL_EXPIRED)
        );
        let malformed = r#"{"v":1,"extra":true}"#;
        assert_eq!(
            admit(
                malformed,
                &digest(malformed),
                "2026-11-23T10:00:00Z",
                before_expiry()
            ),
            Err(ContractRefusal::Malformed.as_str())
        );
    }

    #[test]
    fn offer_keeps_only_the_contracts_read_tools() {
        let contract = ScheduledContract::parse(CONTRACT).unwrap();
        let offered = offered_defs(
            &contract,
            vec![
                def("web_search"),
                def("knowledge_search"),
                def("send_email"),
            ],
        );
        let names: Vec<&str> = offered.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["web_search"]);
    }

    #[test]
    fn offer_never_includes_a_non_delegable_tool() {
        let contract = ScheduledContract::parse(CONTRACT).unwrap();
        let offered = offered_defs(&contract, vec![def("shell"), def("mcp__github__search")]);
        assert!(offered.is_empty());
    }

    #[test]
    fn run_request_is_plan_mode_read_only_bounded_and_not_zdr() {
        let contract = ScheduledContract::parse(CONTRACT).unwrap();
        let request = run_request(&step(), "summarise the week", &contract);
        assert!(request.plan_mode);
        assert_eq!(request.autonomy_rung, pb::AutonomyRung::ReadOnly as i32);
        assert!(!request.zdr);
        assert!(request.tools.is_empty());
        assert_eq!(request.max_subagents, 0);
        assert_eq!(request.model, MODEL);
        assert_eq!(request.user_id, "usr_owner");
        assert_eq!(request.max_rounds, 6);
    }

    #[test]
    fn run_request_caps_rounds_at_the_lane_ceiling() {
        let mut contract = ScheduledContract::parse(CONTRACT).unwrap();
        contract.max_rounds = MAX_ROUNDS;
        let request = run_request(&step(), "goal", &contract);
        assert_eq!(request.max_rounds, MAX_ROUNDS);
    }

    #[test]
    fn withhold_keeps_the_digest_and_never_the_arguments() {
        let state = StateStore::new();
        state.begin_unattended("run_01");
        assert!(withhold(
            &state,
            "run_01",
            "send_email",
            r#"{"to":"a@example.com"}"#,
            REASON_NOT_COVERED,
        ));
        let taken = state.take_withheld("run_01");
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].tool, "send_email");
        assert_eq!(taken[0].reason_code, REASON_NOT_COVERED);
        assert!(taken[0].args_sha256.starts_with("sha256:"));
        assert!(!taken[0].args_sha256.contains("example.com"));
    }

    #[test]
    fn withhold_is_a_no_op_for_an_attended_run() {
        let state = StateStore::new();
        assert!(!withhold(
            &state,
            "run_02",
            "send_email",
            "{}",
            REASON_NOT_COVERED
        ));
        assert!(state.take_withheld("run_02").is_empty());
    }

    #[test]
    fn egress_refuses_a_query_string_and_passes_a_plain_fetch() {
        assert_eq!(
            egress_refusal("web_fetch", r#"{"url":"https://example.com/a?q=secret"}"#),
            Some(REASON_EGRESS_QUERY)
        );
        assert_eq!(
            egress_refusal("web_fetch", r#"{"url":"https://example.com/a"}"#),
            None
        );
        assert_eq!(egress_refusal("web_search", r#"{"query":"a?b"}"#), None);
        assert_eq!(egress_refusal("web_fetch", "not json"), None);
    }

    #[test]
    fn transcript_lists_withheld_tools_without_arguments() {
        let effects = vec![WithheldEffect {
            tool: "send_email".to_owned(),
            reason_code: REASON_NOT_COVERED,
            args_sha256: "sha256:00".to_owned(),
        }];
        let text = transcript_text("Here is the week.\n", &effects);
        assert!(text.starts_with("Here is the week."));
        assert!(text.contains("- send_email (scheduled_not_covered)"));
        assert!(!text.contains("sha256"));
        assert_eq!(transcript_text("only the answer", &[]), "only the answer");
    }

    #[test]
    fn scheduled_agent_refuses_zdr_and_waived() {
        assert_eq!(refuses_retention(true, false), Some(REFUSAL_RETENTION));
        assert_eq!(refuses_retention(false, true), Some(REFUSAL_RETENTION));
        assert_eq!(refuses_retention(true, true), Some(REFUSAL_RETENTION));
        assert_eq!(refuses_retention(false, false), None);
    }

    #[test]
    fn wall_clock_is_the_lane_ceiling_or_the_contract_time_left() {
        let contract = ScheduledContract::parse(CONTRACT).unwrap();
        let long_before = Utc.with_ymd_and_hms(2026, 10, 25, 0, 0, 0).unwrap();
        assert_eq!(
            wall_clock(&contract, long_before),
            std::time::Duration::from_secs(WALL_CLOCK_SECS)
        );
        let ninety_seconds_before = Utc.with_ymd_and_hms(2026, 11, 23, 9, 58, 30).unwrap();
        assert_eq!(
            wall_clock(&contract, ninety_seconds_before),
            std::time::Duration::from_secs(90)
        );
        assert_eq!(
            wall_clock(&contract, after_expiry()),
            std::time::Duration::ZERO
        );
    }

    #[test]
    fn loop_scope_takes_the_contracts_round_cap() {
        let contract = ScheduledContract::parse(CONTRACT).unwrap();
        assert_eq!(loop_scope(&contract).max_tokens_per_round, 2048);
    }
}
