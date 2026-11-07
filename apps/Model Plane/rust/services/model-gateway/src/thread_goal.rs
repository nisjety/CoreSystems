//! The goal turn ledger, the gateway's half (Wave 3 EP-7 S2).
//!
//! A Continue turn that names its goal (`InvokeRequest.goal_id`) is admitted by session-core
//! before its run is prepared (`ClaimGoalTurn`), names its run once the run exists
//! (`BindGoalTurnRun`), and is settled from the run's own usage when the run answers
//! (`SettleGoalTurn`); the settled goal rides back to the goal bar as one `goal_progress` frame.
//! Session Core owns the record and the stop rules. This module holds the pure rules the gateway
//! applies (the D10 cap clamp, what counts as a metered token, the outcome tokens, the refusal
//! codes) and [`GoalTurnLedger`], which settles a claimed turn on every exit of the stream, so a
//! turn that never reaches its run's answer frees its goal at once rather than after the lease.
//!
//! Owner decisions taken by their recommendation (EP-7 section 3): D2, only metered tokens count
//! (a subscription leg counts none, so the turn cap is what bounds a subscription goal) and the
//! judge's tokens count; D10, the run cap a goal turn may use is `min(remaining, org per-run
//! ceiling)` per dimension, never above the org's, whatever budget the goal was given. No switch of
//! its own: it runs only for a turn MODEL_GATEWAY_THREAD_GOALS_ENABLED already let name a goal.
//! Counts, ids and closed tokens only: nothing here carries content.

use mp_contracts::model_plane::v1::{
    thread_goal_service_client::ThreadGoalServiceClient, BindGoalTurnRunRequest,
    ClaimGoalTurnRequest, ClaimGoalTurnResponse, RunUsage, SettleGoalTurnRequest, ThreadGoal,
};
use tonic::transport::Channel;

use crate::auth::VerifiedSessionBearer;
use crate::pricing::CostBasis;
use crate::sse::AgenticRunOutcome;
use crate::sse_events::run_ceiling::RunCeiling;
use crate::sse_events::ChatEvent;

/// The statuses a goal can be in (the table's CHECK). A frame names only these.
const GOAL_STATUSES: [&str; 7] = [
    "active",
    "paused",
    "met",
    "budget_reached",
    "turns_reached",
    "blocked",
    "cleared",
];

/// The reasons session-core writes beside a status; anything else is sent as none.
const STATUS_REASONS: [&str; 5] = [
    "token_budget",
    "cost_budget",
    "max_turns",
    "replaced",
    "user_marked_met",
];

/// What a claim says the goal still allows; `None` where the goal sets no budget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GoalAllowance {
    pub(crate) tokens: Option<u64>,
    pub(crate) cost_usd_micros: Option<u64>,
}

impl GoalAllowance {
    fn of_claim(claim: &ClaimGoalTurnResponse) -> Self {
        Self {
            tokens: claim.token_budget_set.then_some(claim.tokens_remaining),
            cost_usd_micros: claim
                .cost_budget_set
                .then_some(claim.cost_remaining_usd_micros),
        }
    }
}

/// D10: the caps a goal turn's run may use, dimension by dimension the tighter of what the goal
/// has left and the org's per-run ceiling. A goal with a large budget therefore never raises a run
/// above the org's cap, and a dimension neither sets stays uncapped. `org_ceiling` is the org's
/// ceiling alone, never one a request cap was folded into (register D2-698/699: a request cap can
/// loosen the org ceiling, so it is not an input here).
#[must_use]
pub(crate) fn goal_turn_cap(remaining: GoalAllowance, org_ceiling: RunCeiling) -> RunCeiling {
    let org_usd = org_ceiling.usd.filter(|usd| usd.is_finite() && *usd > 0.0);
    let org_tokens = org_ceiling.tokens.filter(|tokens| *tokens > 0);
    let goal_usd = remaining.cost_usd_micros.map(micros_to_usd);
    RunCeiling {
        usd: match (goal_usd, org_usd) {
            (Some(goal), Some(org)) => Some(goal.min(org)),
            (goal, None) => goal,
            (None, org) => org,
        },
        tokens: match (remaining.tokens, org_tokens) {
            (Some(goal), Some(org)) => Some(goal.min(org)),
            (goal, None) => goal,
            (None, org) => org,
        },
    }
}

#[allow(clippy::cast_precision_loss)] // a budget in micro-USD is far below 2^52
fn micros_to_usd(micros: u64) -> f64 {
    micros as f64 / 1_000_000.0
}

/// A spend in US dollars as the ledger's micro-USD; `None` for a figure that is not a spend.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // checked finite and >= 0
fn usd_to_micros(usd: f64) -> Option<u64> {
    (usd.is_finite() && usd >= 0.0).then(|| (usd * 1_000_000.0).round() as u64)
}

/// The tokens of a run that count against a goal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MeteredTokens {
    pub(crate) input: u64,
    pub(crate) output: u64,
}

/// D2: the metered tokens of a run's usage. A leg a subscription served counts none, the rule of
/// the run ceiling and cost-core; an executor that sent totals and no legs is one leg.
#[must_use]
pub(crate) fn metered_tokens(
    usage: &RunUsage,
    provider_hint: &str,
    subscription_connection_id: &str,
) -> MeteredTokens {
    let route = CostBasis::of_route(provider_hint, subscription_connection_id);
    let metered = |provider: &str| route.served_by(provider) == CostBasis::Metered;
    if usage.legs.is_empty() {
        return if metered(&usage.provider_used) {
            MeteredTokens {
                input: usage.input_tokens,
                output: usage.output_tokens,
            }
        } else {
            MeteredTokens::default()
        };
    }
    usage
        .legs
        .iter()
        .filter(|leg| metered(&leg.provider_used))
        .fold(MeteredTokens::default(), |sum, leg| MeteredTokens {
            input: sum.input.saturating_add(leg.input_tokens),
            output: sum.output.saturating_add(leg.output_tokens),
        })
}

/// The outcome token a settle names for how the run ended.
#[must_use]
pub(crate) fn settle_outcome(outcome: AgenticRunOutcome) -> &'static str {
    match outcome {
        AgenticRunOutcome::Completed => "completed",
        AgenticRunOutcome::Failed => "failed",
        AgenticRunOutcome::AwaitingApproval => "awaiting_approval",
        AgenticRunOutcome::Parked => "parked",
        AgenticRunOutcome::Unknown => "unknown",
    }
}

/// What one turn is settled with.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TurnSettlement {
    outcome: &'static str,
    tokens: MeteredTokens,
    cost_usd_micros: Option<u64>,
    cost_basis: &'static str,
    usage_source: &'static str,
}

impl TurnSettlement {
    /// A turn settled with no usage: it never reached its run's answer.
    fn without_usage(outcome: &'static str) -> Self {
        Self {
            outcome,
            tokens: MeteredTokens::default(),
            cost_usd_micros: None,
            cost_basis: "",
            usage_source: "none",
        }
    }

    /// A turn settled from its run's answer: the run's own usage when it carries one; else the
    /// metered round tally the stream counted (`progress`, register D2-005: a failed run may end
    /// without its usage); else none. Either way the turn counts toward the cap, so a run that
    /// keeps failing without usage cannot loop forever. `cost_usd` is the run's priced spend.
    pub(crate) fn of_run(
        outcome: &'static str,
        usage: Option<&RunUsage>,
        progress: Option<MeteredTokens>,
        cost_usd: Option<f64>,
        route: CostBasis,
        provider_hint: &str,
        subscription_connection_id: &str,
    ) -> Self {
        let (tokens, usage_source) = match (usage, progress) {
            (Some(usage), _) => (
                metered_tokens(usage, provider_hint, subscription_connection_id),
                "run_response",
            ),
            (None, Some(progress)) => (progress, "progress"),
            (None, None) => (MeteredTokens::default(), "none"),
        };
        Self {
            outcome,
            tokens,
            cost_usd_micros: cost_usd.and_then(usd_to_micros),
            cost_basis: route.as_str(),
            usage_source,
        }
    }
}

/// A claim's refusal, as the turn's typed stream error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClaimRefusal {
    pub(crate) code: &'static str,
    pub(crate) message: &'static str,
    pub(crate) retryable: bool,
}

const fn refusal(code: &'static str, message: &'static str, retryable: bool) -> ClaimRefusal {
    ClaimRefusal {
        code,
        message,
        retryable,
    }
}

/// Session Core's refusal of a claim as the turn's error. Nothing was prepared yet, so a refused
/// turn creates no run.
#[must_use]
pub(crate) fn claim_refusal(status: &tonic::Status) -> ClaimRefusal {
    let message = status.message();
    match status.code() {
        tonic::Code::FailedPrecondition => match message {
            "thread_goal_not_active" => refusal(
                "thread_goal_not_active",
                "This goal is not active. Resume it or set a new goal to continue.",
                false,
            ),
            "thread_goal_turn_in_flight" => refusal(
                "thread_goal_turn_in_flight",
                "A turn toward this goal is still running. Wait for it to finish.",
                true,
            ),
            "thread_goal_budget_reached" => refusal(
                "thread_goal_budget_reached",
                "This goal has spent its budget, so no further turn starts.",
                false,
            ),
            "thread_goal_turns_reached" => refusal(
                "thread_goal_turns_reached",
                "This goal has used all of its turns.",
                false,
            ),
            "thread_goal_space_unsupported" => refusal(
                "thread_goal_space_unsupported",
                "Goals are not available in a shared Space thread yet.",
                false,
            ),
            "thread_goal_waived_thread" => refusal(
                "thread_goal_waived_thread",
                "This thread holds a run kept outside retention, so it cannot hold a goal.",
                false,
            ),
            // EP-7 S4: a goal turn waits while a decision is open in the thread.
            "thread_goal_pending_decision" => refusal(
                "thread_goal_pending_decision",
                "A decision waits on you in this conversation.",
                false,
            ),
            _ => unavailable(),
        },
        // EP-7 S4: the pump's hourly limit on one goal.
        tonic::Code::ResourceExhausted if message == "thread_goal_rate_limited" => refusal(
            "thread_goal_rate_limited",
            "This goal continued too often in the last hour.",
            true,
        ),
        tonic::Code::Aborted => refusal(
            "thread_goal_revision_conflict",
            "The goal changed since it was read. Reload the thread and try again.",
            false,
        ),
        // Someone else's goal reads like one that is not the thread's.
        tonic::Code::NotFound | tonic::Code::PermissionDenied => refusal(
            "thread_goal_mismatch",
            "The goal this turn names is not the thread's active goal. Reload the thread and try again.",
            false,
        ),
        _ => unavailable(),
    }
}

fn unavailable() -> ClaimRefusal {
    refusal(
        "thread_goal_unavailable",
        "The thread's goal could not be read. Try again.",
        true,
    )
}

/// The `goal_progress` frame of a settled goal, or `None` when session-core answered a status this
/// build does not know (the frame names closed tokens only).
#[must_use]
pub(crate) fn progress_event(goal: &ThreadGoal, turn_no: u32) -> Option<ChatEvent> {
    let status = GOAL_STATUSES
        .iter()
        .copied()
        .find(|known| *known == goal.status)?;
    let status_reason = STATUS_REASONS
        .iter()
        .copied()
        .find(|known| *known == goal.status_reason)
        .unwrap_or("");
    Some(ChatEvent::GoalProgress {
        goal_id: goal.id.clone(),
        turn_no,
        tokens_used: goal.tokens_used,
        token_budget: (goal.token_budget > 0).then_some(goal.token_budget),
        cost_used_usd_micros: goal.cost_used_usd_micros,
        cost_budget_usd_micros: (goal.cost_budget_usd_micros > 0)
            .then_some(goal.cost_budget_usd_micros),
        turns_used: goal.turns_used,
        max_turns: goal.max_turns,
        status,
        status_reason,
    })
}

/// One claimed goal turn. Settled explicitly from the run's answer; any other exit of the stream
/// (a refused start, a Stop, a follow deadline, a dispatch error) drops it, and the drop settles
/// the turn without usage: `cancelled` before a run was bound (no turn counted), the outcome set
/// by [`Self::end_as`] or `unknown` after. The settle on drop is best effort; a lost one is
/// settled `lease_expired` by the goal's next claim.
pub(crate) struct GoalTurnLedger {
    client: ThreadGoalServiceClient<Channel>,
    bearer: VerifiedSessionBearer,
    org_id: String,
    goal_id: String,
    turn_no: u32,
    run_id: String,
    allowance: GoalAllowance,
    /// The outcome a settle on drop names.
    end: &'static str,
    settled: bool,
}

impl GoalTurnLedger {
    /// Claim one turn toward `goal_id` with the turn's own session bearer, so session-core's owner,
    /// ZDR and Space checks apply to the claim as to the goal routes.
    pub(crate) async fn claim(
        client: ThreadGoalServiceClient<Channel>,
        bearer: &VerifiedSessionBearer,
        org_id: &str,
        goal_id: &str,
    ) -> Result<Self, ClaimRefusal> {
        let request = crate::sse::authenticated_session_request(
            ClaimGoalTurnRequest {
                goal_id: goal_id.to_owned(),
                org_id: org_id.to_owned(),
                // A Continue click names the goal, not the revision it was read at.
                expected_revision: 0,
            },
            bearer,
        )
        .map_err(|_| unavailable())?;
        let claimed = client
            .clone()
            .claim_goal_turn(request)
            .await
            .map_err(|status| claim_refusal(&status))?
            .into_inner();
        Ok(Self {
            client,
            bearer: bearer.clone(),
            org_id: org_id.to_owned(),
            goal_id: goal_id.to_owned(),
            turn_no: claimed.turn_no,
            run_id: String::new(),
            allowance: GoalAllowance::of_claim(&claimed),
            end: "cancelled",
            settled: false,
        })
    }

    /// What the goal had left when this turn was claimed.
    pub(crate) fn allowance(&self) -> GoalAllowance {
        self.allowance
    }

    /// Name the run this turn started. A failed bind is logged and nothing more: the settle names
    /// the run too, so the turn is still settled against it. A pump-sent turn (`goal_auto`, EP-7 S4)
    /// carries the mark so session-core stamps the run for audit.
    pub(crate) async fn bind(&mut self, run_id: &str, goal_auto: bool) {
        self.run_id = run_id.to_owned();
        // From here an exit without the run's answer is a run that started and ended unknown.
        self.end = "unknown";
        let request = crate::sse::authenticated_session_request(
            BindGoalTurnRunRequest {
                goal_id: self.goal_id.clone(),
                org_id: self.org_id.clone(),
                turn_no: self.turn_no,
                run_id: run_id.to_owned(),
            },
            &self.bearer,
        );
        let bound = match request {
            Ok(mut request) => {
                crate::goal_auto::mark(&mut request, goal_auto);
                self.client
                    .clone()
                    .bind_goal_turn_run(request)
                    .await
                    .map(|_| ())
            }
            Err(status) => Err(status),
        };
        if let Err(status) = bound {
            tracing::warn!(code = ?status.code(), turn_no = self.turn_no, "goal turn bind failed; the settle names the run");
        }
    }

    /// The outcome a later drop settles with (a Stop: `cancelled`).
    pub(crate) fn end_as(&mut self, outcome: &'static str) {
        self.end = outcome;
    }

    /// Settle the turn from its run's answer and return the `goal_progress` frame of the settled
    /// goal; `None` when the settle failed (logged) or answered nothing to show.
    pub(crate) async fn settle(mut self, settlement: &TurnSettlement) -> Option<ChatEvent> {
        self.settled = true;
        let request = self.request(settlement);
        let goal = send_settle(self.client.clone(), &self.bearer, request).await?;
        progress_event(&goal, self.turn_no)
    }

    fn request(&self, settlement: &TurnSettlement) -> SettleGoalTurnRequest {
        SettleGoalTurnRequest {
            goal_id: self.goal_id.clone(),
            org_id: self.org_id.clone(),
            turn_no: self.turn_no,
            run_id: self.run_id.clone(),
            outcome: settlement.outcome.to_owned(),
            metered_input_tokens: settlement.tokens.input,
            metered_output_tokens: settlement.tokens.output,
            cost_usd_micros: settlement
                .cost_usd_micros
                .map_or(-1, |micros| i64::try_from(micros).unwrap_or(i64::MAX)),
            cost_basis: settlement.cost_basis.to_owned(),
            // The completion judge (EP-7 S3) settles its own tokens; this turn's run spent none.
            judge_tokens: 0,
            usage_source: settlement.usage_source.to_owned(),
        }
    }
}

impl Drop for GoalTurnLedger {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let request = self.request(&TurnSettlement::without_usage(self.end));
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(turn_no = self.turn_no, "goal turn left unsettled; its lease will expire");
            return;
        };
        let client = self.client.clone();
        let bearer = self.bearer.clone();
        runtime.spawn(async move {
            send_settle(client, &bearer, request).await;
        });
    }
}

/// One `SettleGoalTurn` call; a failure is logged by its code and answers `None`.
async fn send_settle(
    mut client: ThreadGoalServiceClient<Channel>,
    bearer: &VerifiedSessionBearer,
    request: SettleGoalTurnRequest,
) -> Option<ThreadGoal> {
    let turn_no = request.turn_no;
    let request = crate::sse::authenticated_session_request(request, bearer).ok()?;
    match client.settle_goal_turn(request).await {
        Ok(response) => {
            let response = response.into_inner();
            response.goal.filter(|_| response.found)
        }
        Err(status) => {
            tracing::warn!(code = ?status.code(), turn_no, "goal turn settle failed; its lease will expire");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_contracts::model_plane::v1::RunUsageLeg;

    const SUBSCRIPTION: &str = mp_contracts::agentic_subscription::PROVIDER_ID;

    fn leg(provider: &str, input: u64, output: u64) -> RunUsageLeg {
        RunUsageLeg {
            provider_used: provider.to_owned(),
            input_tokens: input,
            output_tokens: output,
            inference_rounds: 1,
            ..Default::default()
        }
    }

    #[test]
    fn goal_cap_never_exceeds_org_ceiling() {
        // Register D2-699's example: a goal with 50.0 USD left under an org cap of 5.0 runs at 5.0.
        let org = RunCeiling {
            usd: Some(5.0),
            tokens: Some(100_000),
        };
        let large = GoalAllowance {
            tokens: Some(5_000_000),
            cost_usd_micros: Some(50_000_000),
        };
        let cap = goal_turn_cap(large, org);
        assert_eq!(cap.usd, Some(5.0));
        assert_eq!(cap.tokens, Some(100_000));
        // Whatever the goal has left, no dimension rises above the org's.
        for tokens in [0, 1, 99_999, 100_000, 100_001, u64::MAX] {
            let cap = goal_turn_cap(
                GoalAllowance {
                    tokens: Some(tokens),
                    cost_usd_micros: Some(tokens),
                },
                org,
            );
            assert!(cap.tokens.unwrap() <= 100_000, "{tokens}");
            assert!(cap.usd.unwrap() <= 5.0, "{tokens}");
        }
        // A goal with no budget leaves the org's ceiling exactly as it was.
        assert_eq!(goal_turn_cap(GoalAllowance::default(), org), org);
    }

    #[test]
    fn goal_cap_takes_the_tighter_goal_and_fills_an_uncapped_org() {
        let org = RunCeiling {
            usd: Some(5.0),
            tokens: None,
        };
        let small = GoalAllowance {
            tokens: Some(30_000),
            cost_usd_micros: Some(1_500_000),
        };
        let cap = goal_turn_cap(small, org);
        assert_eq!(cap.usd, Some(1.5));
        assert_eq!(cap.tokens, Some(30_000), "an uncapped org dimension takes the goal's");
        // An org cap that is not positive is no cap, as `RunCeiling::resolve` reads one.
        let unset = RunCeiling {
            usd: Some(0.0),
            tokens: Some(0),
        };
        assert_eq!(goal_turn_cap(GoalAllowance::default(), unset), RunCeiling::default());
    }

    #[test]
    fn subscription_leg_counts_zero_metered_tokens() {
        let usage = RunUsage {
            legs: vec![leg(SUBSCRIPTION, 9_000, 1_000), leg("openai", 300, 200)],
            ..Default::default()
        };
        assert_eq!(
            metered_tokens(&usage, "", ""),
            MeteredTokens {
                input: 300,
                output: 200
            },
            "only the metered leg counts"
        );
        let pinned = RunUsage {
            provider_used: String::new(),
            input_tokens: 8_000,
            output_tokens: 2_000,
            ..Default::default()
        };
        assert_eq!(
            metered_tokens(&pinned, SUBSCRIPTION, "conn-1"),
            MeteredTokens::default(),
            "a subscription route with no provenance spends no metered token"
        );
        assert_eq!(
            metered_tokens(&pinned, "", ""),
            MeteredTokens {
                input: 8_000,
                output: 2_000
            }
        );
    }

    #[test]
    fn settle_outcome_maps_every_status() {
        assert_eq!(settle_outcome(AgenticRunOutcome::Completed), "completed");
        assert_eq!(settle_outcome(AgenticRunOutcome::Failed), "failed");
        assert_eq!(
            settle_outcome(AgenticRunOutcome::AwaitingApproval),
            "awaiting_approval"
        );
        assert_eq!(settle_outcome(AgenticRunOutcome::Parked), "parked");
        assert_eq!(settle_outcome(AgenticRunOutcome::Unknown), "unknown");
    }

    #[test]
    fn failed_run_without_usage_still_counts_a_turn() {
        // No usage and no tally: settled with none, under the run's own outcome, which
        // session-core counts as a turn because the run started.
        let bare = TurnSettlement::of_run("failed", None, None, None, CostBasis::Metered, "", "");
        assert_eq!(bare.outcome, "failed");
        assert_eq!(bare.usage_source, "none");
        assert_eq!(bare.tokens, MeteredTokens::default());
        assert_eq!(bare.cost_usd_micros, None);
        // The round tally stands in for the missing usage (register D2-005).
        let tallied = MeteredTokens {
            input: 700,
            output: 50,
        };
        let progress =
            TurnSettlement::of_run("failed", None, Some(tallied), None, CostBasis::Metered, "", "");
        assert_eq!(progress.usage_source, "progress");
        assert_eq!(progress.tokens, tallied);
        // A run's own usage wins over the tally.
        let usage = RunUsage {
            legs: vec![leg("openai", 10, 5)],
            ..Default::default()
        };
        let answered = TurnSettlement::of_run(
            "completed",
            Some(&usage),
            Some(tallied),
            Some(0.25),
            CostBasis::Metered,
            "",
            "",
        );
        assert_eq!(answered.usage_source, "run_response");
        assert_eq!(answered.tokens, MeteredTokens { input: 10, output: 5 });
        assert_eq!(answered.cost_usd_micros, Some(250_000));
        assert_eq!(answered.cost_basis, "metered");
        // A turn that never reached its answer carries no usage at all.
        assert_eq!(TurnSettlement::without_usage("cancelled").usage_source, "none");
    }

    #[test]
    fn claim_refusals_are_typed_and_create_no_run() {
        let refused = |code, message: &str| claim_refusal(&tonic::Status::new(code, message)).code;
        use tonic::Code;
        assert_eq!(
            refused(Code::FailedPrecondition, "thread_goal_budget_reached"),
            "thread_goal_budget_reached"
        );
        assert_eq!(
            refused(Code::FailedPrecondition, "thread_goal_turns_reached"),
            "thread_goal_turns_reached"
        );
        assert_eq!(
            refused(Code::FailedPrecondition, "thread_goal_turn_in_flight"),
            "thread_goal_turn_in_flight"
        );
        assert_eq!(
            refused(Code::FailedPrecondition, "thread_goal_not_active"),
            "thread_goal_not_active"
        );
        assert_eq!(refused(Code::NotFound, "x"), "thread_goal_mismatch");
        assert_eq!(refused(Code::Aborted, "x"), "thread_goal_revision_conflict");
        assert_eq!(refused(Code::Unavailable, "thread_goals_disabled"), "thread_goal_unavailable");
        assert_eq!(
            refused(Code::FailedPrecondition, "some free text"),
            "thread_goal_unavailable",
            "an unknown refusal never echoes its message"
        );
    }

    #[test]
    fn progress_frame_carries_counts_and_closed_tokens_only() {
        let goal = ThreadGoal {
            id: "goal_1".to_owned(),
            objective: "secret objective".to_owned(),
            status: "budget_reached".to_owned(),
            status_reason: "token_budget".to_owned(),
            token_budget: 30_000,
            tokens_used: 31_000,
            max_turns: 10,
            turns_used: 3,
            ..Default::default()
        };
        let frame = progress_event(&goal, 3).expect("a known status");
        let payload = frame.payload("req-1");
        assert_eq!(payload["goal_id"], "goal_1");
        assert_eq!(payload["turn_no"], 3);
        assert_eq!(payload["tokens_used"], 31_000);
        assert_eq!(payload["token_budget"], 30_000);
        assert!(payload["cost_budget_usd_micros"].is_null());
        assert_eq!(payload["status"], "budget_reached");
        assert_eq!(payload["status_reason"], "token_budget");
        assert!(
            !payload.to_string().contains("secret objective"),
            "the frame never carries the goal's text"
        );
        let odd = ThreadGoal {
            status: "who_knows".to_owned(),
            ..goal.clone()
        };
        assert!(progress_event(&odd, 3).is_none());
        let free_reason = ThreadGoal {
            status_reason: "free text".to_owned(),
            ..goal
        };
        assert_eq!(
            progress_event(&free_reason, 3).unwrap().payload("r")["status_reason"],
            ""
        );
    }
}
