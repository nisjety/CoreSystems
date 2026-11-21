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
//! Counts, ids and closed tokens only, except the completion judge's reasons (EP-7 S3): model text
//! that rides the one verdict write to session-core and is never logged.
//!
//! The completion judge (EP-7 S3) answers after a settled, completed turn. Its verdict is the
//! gateway's one write to the goal store: [`record_verdict`] sends `RecordGoalVerdict` under a token
//! that [`GoalVerdictTokens`] mints from the dedicated goal-judge service credential.

use std::fmt;
use std::time::Duration;

use mp_contracts::model_plane::v1::{
    thread_goal_service_client::ThreadGoalServiceClient, BindGoalTurnRunRequest,
    ClaimGoalTurnRequest, ClaimGoalTurnResponse, GetGoalTurnForJudgeRequest,
    GetGoalTurnForJudgeResponse, InferRequest, JudgeToolCall, ListRunToolCallsForJudgeRequest,
    RecordGoalVerdictRequest, RunUsage, SettleGoalTurnRequest, ThreadGoal, TopUpGoalTurnRequest,
};
use tonic::transport::Channel;

use crate::auth::{VerifiedInferenceBearer, VerifiedSessionBearer};
use crate::goal_judge::{
    goal_judge_prompt, goal_judge_request, judge_metered_tokens, parse_goal_verdict, EvidenceLine,
    JudgeVerdict,
};
use crate::http_routes::authenticated_inference_request;
use crate::pricing::CostBasis;
use crate::sse::AgenticRunOutcome;
use crate::sse_events::run_ceiling::RunCeiling;
use crate::sse_events::ChatEvent;
use crate::state::AppState;

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

/// The completion judge's closed verdict tokens, as a `goal_progress` frame names them (EP-7 S3).
const VERDICTS: [&str; 4] = ["met", "not_met", "blocked", "unsure"];

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
        // The run paused for the owner's answer (WS14 S5); `thread_goal_turns` admits the token (migration 0092).
        AgenticRunOutcome::AwaitingInput => "awaiting_input",
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
/// build does not know (the frame names closed tokens only). `last_verdict` is the judge's verdict on
/// the goal's latest judged turn, or empty when none is known.
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
    let last_verdict = VERDICTS
        .iter()
        .copied()
        .find(|known| *known == goal.last_verdict)
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
        last_verdict,
    })
}

/// A settled goal turn: the goal as session-core answered the settle, objective and criteria
/// included for the judge, and the turn it settled.
pub(crate) struct SettledGoalTurn {
    pub(crate) goal: ThreadGoal,
    pub(crate) turn_no: u32,
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

    /// Settle the turn from its run's answer and return the settled goal with its turn number; `None`
    /// when the settle failed (logged). The caller answers the goal's `goal_progress` frame and judges
    /// the turn from it.
    pub(crate) async fn settle(mut self, settlement: &TurnSettlement) -> Option<SettledGoalTurn> {
        self.settled = true;
        let request = self.request(settlement);
        let goal = send_settle(self.client.clone(), &self.bearer, request).await?;
        Some(SettledGoalTurn {
            goal,
            turn_no: self.turn_no,
        })
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

/// The switch for the segment top-up (F-f09-11), read at each call as the other goal switches are.
pub(crate) const SEGMENT_TOPUP_ENV: &str = "MODEL_GATEWAY_GOAL_SEGMENT_TOPUP";

/// Whether a goal run's /continue segments debit their usage to the goal: only the literal `true`, so
/// with the switch off the continuation is exactly what it was. It also needs the goal switch, since
/// session-core answers `thread_goals_disabled` otherwise.
#[must_use]
pub(crate) fn segment_topup_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(SEGMENT_TOPUP_ENV).ok().as_deref())
}

/// How long a top-up may delay the end of a continuation's stream; a slower one is a warning and the
/// segment goes undebited (the goal is under-counted, never over-counted).
const TOP_UP_TIMEOUT: Duration = Duration::from_secs(5);

/// Whether a continuation segment asks for a top-up: the switch is on, the dispatch is a continuation
/// of a paused run, and the run is neither zero-retention nor waived (no goal exists for one). An
/// ordinary turn never tops up; its first segment is settled by the goal turn ledger.
#[must_use]
pub(crate) fn should_top_up(switch_on: bool, continuing: bool, zdr: bool, waived: bool) -> bool {
    switch_on && continuing && !zdr && !waived
}

/// What one continuation segment spent, as a top-up states it: the metered tokens and the priced cost.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SegmentUsage {
    tokens: MeteredTokens,
    cost_usd_micros: Option<u64>,
    cost_basis: &'static str,
}

impl SegmentUsage {
    /// The segment's usage from the executor's answer. `None` when the answer carried no usage: the
    /// round tally of a progress stream may span the whole run, so it is never used here (a segment
    /// with no usage goes undebited rather than risk a double count).
    #[must_use]
    pub(crate) fn of_segment(
        usage: Option<&RunUsage>,
        cost_usd: Option<f64>,
        route: CostBasis,
        provider_hint: &str,
        subscription_connection_id: &str,
    ) -> Option<Self> {
        let usage = usage?;
        Some(Self {
            tokens: metered_tokens(usage, provider_hint, subscription_connection_id),
            cost_usd_micros: cost_usd.and_then(usd_to_micros),
            cost_basis: route.as_str(),
        })
    }

    /// Whether the segment spent anything a goal counts; a subscription segment that spent no metered
    /// token and no priced cost has nothing to add, so no call is made.
    #[must_use]
    pub(crate) fn has_spend(&self) -> bool {
        self.tokens != MeteredTokens::default()
            || self.cost_usd_micros.is_some_and(|micros| micros > 0)
    }
}

/// The goal turn top-up client (F-f09-11): `TopUpGoalTurn` with the owner's session bearer, so
/// session-core's owner, org and ZDR checks apply as to a settle.
pub(crate) struct GoalTopUp {
    client: ThreadGoalServiceClient<Channel>,
    bearer: VerifiedSessionBearer,
    org_id: String,
}

impl GoalTopUp {
    pub(crate) fn new(
        client: ThreadGoalServiceClient<Channel>,
        bearer: &VerifiedSessionBearer,
        org_id: &str,
    ) -> Self {
        Self {
            client,
            bearer: bearer.clone(),
            org_id: org_id.to_owned(),
        }
    }

    fn request(
        &self,
        run_id: &str,
        segment_key: &str,
        usage: &SegmentUsage,
    ) -> TopUpGoalTurnRequest {
        TopUpGoalTurnRequest {
            org_id: self.org_id.clone(),
            run_id: run_id.to_owned(),
            segment_key: segment_key.to_owned(),
            metered_input_tokens: usage.tokens.input,
            metered_output_tokens: usage.tokens.output,
            cost_usd_micros: usage
                .cost_usd_micros
                .map_or(-1, |micros| i64::try_from(micros).unwrap_or(i64::MAX)),
            cost_basis: usage.cost_basis.to_owned(),
        }
    }

    /// Debit one continuation segment's usage to the goal its run belongs to; the goal as session-core
    /// answered, or `None`. A run in no goal (`NotFound`) is the common case and is ignored quietly; any
    /// other failure is a warning carrying ids and the status code only. Idempotent per `segment_key`.
    pub(crate) async fn top_up(
        &self,
        run_id: &str,
        segment_key: &str,
        usage: &SegmentUsage,
    ) -> Option<ThreadGoal> {
        if !usage.has_spend() {
            return None;
        }
        let request = crate::sse::authenticated_session_request(
            self.request(run_id, segment_key, usage),
            &self.bearer,
        )
        .ok()?;
        let mut client = self.client.clone();
        match tokio::time::timeout(TOP_UP_TIMEOUT, client.top_up_goal_turn(request)).await {
            Ok(Ok(response)) => {
                let response = response.into_inner();
                response.goal.filter(|_| response.found)
            }
            Ok(Err(status)) if status.code() == tonic::Code::NotFound => None,
            Ok(Err(status)) => {
                tracing::warn!(code = ?status.code(), %run_id, %segment_key, "goal segment top-up failed; the segment goes undebited");
                None
            }
            Err(_) => {
                tracing::warn!(%run_id, %segment_key, "goal segment top-up timed out; the segment goes undebited");
                None
            }
        }
    }
}

/// The scope the verdict write checks. `RecordGoalVerdict` answers only a caller that holds it and
/// is the `service:model-gateway-goal-judge` principal (see [`record_verdict`]).
const VERDICT_SCOPE: &str = "session:goal-verdict:write";
/// The reason Auth Core records beside a minted verdict token.
const VERDICT_REASON: &str = "model-gateway goal verdict";
/// The Auth Core audience every Session Core token names.
const SESSION_CORE_AUDIENCE: &str = "session-core";
/// The longest lifetime a minted verdict token may claim; a longer one is refused as invalid.
const MAX_VERDICT_TOKEN_TTL_SECONDS: u64 = 3600;
/// The longest judge model name session-core keeps; a longer one is cut before the write.
const JUDGE_MODEL_MAX_CHARS: usize = 128;
/// The longest a verdict-token request to Auth Core may take.
const VERDICT_TOKEN_TIMEOUT: Duration = Duration::from_secs(10);
/// The scope the judge's read of an unattended fire holds (EP-7 S6): session-core's
/// `GetGoalTurnForJudge`, which answers only the judge's own principal.
const JUDGE_READ_SCOPE: &str = "session:goal-judge:read";
/// The reason Auth Core records beside a minted judge read token.
const JUDGE_READ_REASON: &str = "model-gateway goal judge read";
/// The longest a completion judge call may take. The judge answers one short structured verdict once
/// the run has ended, so it may take longer than the source judge; a timeout leaves the goal as it
/// stands (EP-7 S3).
const GOAL_JUDGE_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a verdict token could not be minted. It names no credential and no content.
#[derive(Debug, thiserror::Error)]
pub(crate) enum GoalVerdictTokenError {
    #[error("the goal verdict credential is not configured: {0}")]
    Configuration(&'static str),
    #[error("a goal verdict token needs an organization")]
    MissingOrganization,
    #[error("Auth Core goal verdict token request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Auth Core refused the goal verdict credential ({0})")]
    Refused(reqwest::StatusCode),
    #[error("Auth Core returned an invalid goal verdict token")]
    InvalidResponse,
    /// The minted token's `zdr` claim was true or unreadable: the goal-judge principal is registered
    /// zero-retention, and session-core would refuse it. The message names the registry row, never the
    /// token.
    #[error("the goal judge token is zero-retention scoped; service:model-gateway-goal-judge must be persistent for session-core")]
    ZdrScoped,
}

/// The gateway's minting client for the verdict write: its own service credential, exchanged at Auth
/// Core for a short-lived `aud=session-core` token that holds [`VERDICT_SCOPE`] for one org. The
/// long-lived credential is never logged or shown in `Debug`.
///
/// The credential is the dedicated, persistent goal-judge principal (`service:model-gateway-goal-judge`,
/// OD-58): Auth Core grants the scopes only when the registry does, and session-core accepts a read or
/// write only from that principal with a persistent token. A token whose `zdr` claim is true (or
/// unreadable) is refused at the mint ([`GoalVerdictTokenError::ZdrScoped`]). Unset, the judge does not
/// run and the goal stays as it is. `mail_task_bridge_auth.rs` explains why a durable write needs its
/// own registry row rather than the gateway's zero-retention one.
#[derive(Clone)]
pub(crate) struct GoalVerdictTokens {
    auth_core_url: String,
    service_id: String,
    credential: String,
    http: reqwest::Client,
}

impl fmt::Debug for GoalVerdictTokens {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GoalVerdictTokens")
            .field("auth_core_url", &self.auth_core_url)
            .field("service_id", &self.service_id)
            .field("credential", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MintedVerdictToken {
    token: String,
    expires_in_seconds: u64,
    #[serde(default)]
    audience: String,
}

impl GoalVerdictTokens {
    /// Read from the environment: the dedicated goal-judge credential,
    /// `MODEL_GATEWAY_GOAL_JUDGE_SERVICE_ID` (default `model-gateway-goal-judge`) and
    /// `MODEL_GATEWAY_GOAL_JUDGE_SERVICE_API_KEY` (required). It never falls back to
    /// `MODEL_GATEWAY_SERVICE_API_KEY`, the zero-retention principal.
    pub(crate) fn from_env() -> Result<Self, GoalVerdictTokenError> {
        Self::from_lookup(&|name| std::env::var(name).ok())
    }

    /// [`Self::from_env`] over any variable lookup, so a test needs no environment mutation.
    pub(crate) fn from_lookup(
        lookup: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, GoalVerdictTokenError> {
        let auth_core_url = required_value(lookup, "AUTH_CORE_URL")?;
        let service_id = lookup("MODEL_GATEWAY_GOAL_JUDGE_SERVICE_ID")
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "model-gateway-goal-judge".to_owned());
        let credential = required_value(lookup, "MODEL_GATEWAY_GOAL_JUDGE_SERVICE_API_KEY")?;
        Self::from_service_credential(&auth_core_url, service_id, credential)
    }

    pub(crate) fn from_service_credential(
        auth_core_url: &str,
        service_id: String,
        credential: String,
    ) -> Result<Self, GoalVerdictTokenError> {
        let http = reqwest::Client::builder()
            .timeout(VERDICT_TOKEN_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id,
            credential,
            http,
        })
    }

    /// A fresh token for `org_id`, holding [`VERDICT_SCOPE`]. One is minted per settled, completed goal
    /// turn, so no cache is kept.
    pub(crate) async fn verdict_token(
        &self,
        org_id: &str,
    ) -> Result<String, GoalVerdictTokenError> {
        self.mint(org_id, VERDICT_SCOPE, VERDICT_REASON).await
    }

    /// A fresh token for `org_id`, holding [`JUDGE_READ_SCOPE`], for the judge's read of an unattended
    /// fire's goal (EP-7 S6). Minted per judged fire, so no cache is kept.
    pub(crate) async fn read_token(&self, org_id: &str) -> Result<String, GoalVerdictTokenError> {
        self.mint(org_id, JUDGE_READ_SCOPE, JUDGE_READ_REASON).await
    }

    /// Mint a short-lived `aud=session-core` token that holds `scope` for `org_id`, with `reason`
    /// recorded beside it at Auth Core.
    async fn mint(
        &self,
        org_id: &str,
        scope: &str,
        reason: &str,
    ) -> Result<String, GoalVerdictTokenError> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            return Err(GoalVerdictTokenError::MissingOrganization);
        }
        let mut credential =
            reqwest::header::HeaderValue::from_str(&self.credential).map_err(|_| {
                GoalVerdictTokenError::Configuration("MODEL_GATEWAY_GOAL_JUDGE_SERVICE_API_KEY")
            })?;
        credential.set_sensitive(true);
        let response = self
            .http
            .post(format!(
                "{}/api/{SESSION_CORE_AUDIENCE}/internal-token",
                self.auth_core_url
            ))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", credential)
            .json(&serde_json::json!({
                "orgId": org_id,
                "scopes": [scope],
                "reason": reason,
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(GoalVerdictTokenError::Refused(response.status()));
        }
        let minted = response
            .json::<MintedVerdictToken>()
            .await
            .map_err(|_| GoalVerdictTokenError::InvalidResponse)?;
        if minted.token.trim().is_empty()
            || !(1..=MAX_VERDICT_TOKEN_TTL_SECONDS).contains(&minted.expires_in_seconds)
            || minted.audience != SESSION_CORE_AUDIENCE
        {
            return Err(GoalVerdictTokenError::InvalidResponse);
        }
        // A token whose own `zdr` claim is true (or cannot be read) would make session-core refuse the
        // judge's read and write as a retention-exempt caller, so it is refused here, before any use.
        if crate::mail_task_bridge_auth::decoded_zdr_claim(&minted.token).unwrap_or(true) {
            return Err(GoalVerdictTokenError::ZdrScoped);
        }
        Ok(minted.token)
    }
}

/// `name` from `lookup`, or [`GoalVerdictTokenError::Configuration`] when it is unset or blank.
fn required_value(
    lookup: &dyn Fn(&str) -> Option<String>,
    name: &'static str,
) -> Result<String, GoalVerdictTokenError> {
    lookup(name)
        .filter(|value| !value.trim().is_empty())
        .ok_or(GoalVerdictTokenError::Configuration(name))
}

/// The verdict write for one judged turn. The judge's reasons and evidence ids are kept as the parser
/// left them (already held to the evidence rule); the judge model's name is cut to session-core's cap,
/// so a long name from a provider cannot refuse the write.
#[must_use]
pub(crate) fn verdict_request(
    goal_id: &str,
    org_id: &str,
    turn_no: u32,
    judged: JudgeVerdict,
    judge_model: &str,
    judge_tokens: u64,
) -> RecordGoalVerdictRequest {
    RecordGoalVerdictRequest {
        goal_id: goal_id.to_owned(),
        org_id: org_id.to_owned(),
        turn_no,
        verdict: judged.verdict.as_str().to_owned(),
        reasons: judged.reasons,
        evidence_ids: judged.evidence_ids,
        judge_model: judge_model.chars().take(JUDGE_MODEL_MAX_CHARS).collect(),
        judge_tokens,
    }
}

/// `RecordGoalVerdict` under the gateway's verdict token. `None` when session-core refused the write or
/// could not be reached; the failure is logged by its code, never by content, and the goal stays as it
/// was.
pub(crate) async fn record_verdict(
    mut client: ThreadGoalServiceClient<Channel>,
    token: &str,
    request: RecordGoalVerdictRequest,
) -> Option<ThreadGoal> {
    let turn_no = request.turn_no;
    let authorization =
        format!("Bearer {token}").parse::<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>();
    let Ok(authorization) = authorization else {
        tracing::warn!(
            turn_no,
            "goal verdict token is not forwardable; the goal stays as it was"
        );
        return None;
    };
    let mut request = tonic::Request::new(request);
    request
        .metadata_mut()
        .insert("authorization", authorization);
    match client.record_goal_verdict(request).await {
        Ok(response) => {
            let response = response.into_inner();
            response.goal.filter(|_| response.found)
        }
        Err(status) => {
            tracing::warn!(code = ?status.code(), turn_no, "goal verdict write failed; the goal stays as it was");
            None
        }
    }
}

/// `GetGoalTurnForJudge` under the judge's read token (EP-7 S6): the goal and the unattended turn one
/// scheduled fire ledgered. The status is session-core's, and the caller maps it. Nothing is logged
/// here, so no ids or content leave the call.
pub(crate) async fn read_goal_turn_for_judge(
    mut client: ThreadGoalServiceClient<Channel>,
    token: &str,
    request: GetGoalTurnForJudgeRequest,
) -> Result<GetGoalTurnForJudgeResponse, tonic::Status> {
    let authorization = format!("Bearer {token}")
        .parse::<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>()
        .map_err(|_| tonic::Status::internal("goal judge read token is not forwardable"))?;
    let mut request = tonic::Request::new(request);
    request
        .metadata_mut()
        .insert("authorization", authorization);
    client
        .get_goal_turn_for_judge(request)
        .await
        .map(tonic::Response::into_inner)
}

/// `ListRunToolCallsForJudge` under the judge's read token: the names and statuses of the tool calls
/// one run reserved. The status is session-core's, and the caller decides what a failure costs.
/// Nothing is logged here, so no ids leave the call.
pub(crate) async fn read_run_tool_calls_for_judge(
    mut client: ThreadGoalServiceClient<Channel>,
    token: &str,
    org_id: &str,
    run_id: &str,
) -> Result<Vec<JudgeToolCall>, tonic::Status> {
    let authorization = format!("Bearer {token}")
        .parse::<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>()
        .map_err(|_| tonic::Status::internal("goal judge read token is not forwardable"))?;
    let mut request = tonic::Request::new(ListRunToolCallsForJudgeRequest {
        org_id: org_id.to_owned(),
        run_id: run_id.to_owned(),
    });
    request
        .metadata_mut()
        .insert("authorization", authorization);
    client
        .list_run_tool_calls_for_judge(request)
        .await
        .map(|response| response.into_inner().calls)
}

/// The attended judge must not run: the thread check did not answer (OD-58). It carries no cause, so
/// nothing about the thread, the run or the answer can travel with it; the cause is logged by code at
/// the point it arises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JudgeSkipped;

/// What an answered or failed `ListRunToolCallsForJudge` means for the attended judge. A successful read
/// (an empty list when the run made no tool calls) offers its evidence lines. Any non-OK status, a
/// catalogue refusal (a thread holding a zero-retention or waived run) or any other failure, skips the
/// judge, because session-core's thread check did not answer. Logs the status code only.
pub(crate) fn evidence_or_skip(
    read: Result<Vec<JudgeToolCall>, tonic::Status>,
) -> Result<Vec<EvidenceLine>, JudgeSkipped> {
    match read {
        Ok(calls) => Ok(crate::goal_judge::tool_call_evidence(&calls)),
        Err(status) => {
            tracing::warn!(code = ?status.code(), "goal judge tool-call read did not answer; the turn is not judged");
            Err(JudgeSkipped)
        }
    }
}

/// The tool-call evidence the judge may cite for `run_id` (`tool:<name>:<status>`), read with the judge
/// read token. This read is also the thread check of the attended judge (OD-58): session-core refuses
/// it for a thread that holds a zero-retention or waived run, so the judge model sees no answer of such
/// a thread. It fails closed: `Err(JudgeSkipped)` when the turn is zero-retention, the run id is empty,
/// the judge credential is not configured, the read token is not minted, or the read answers any
/// non-OK status; `Ok` (an empty list when the run made no tool calls) only after a successful read.
/// Each skip cause is logged by its code, with no ids and no content.
pub(crate) async fn run_tool_call_evidence(
    state: &AppState,
    org_id: &str,
    run_id: &str,
    zdr: bool,
) -> Result<Vec<EvidenceLine>, JudgeSkipped> {
    if zdr {
        tracing::warn!("goal judge skipped: the turn is zero-retention");
        return Err(JudgeSkipped);
    }
    if run_id.trim().is_empty() {
        tracing::warn!("goal judge skipped: the turn has no run to check");
        return Err(JudgeSkipped);
    }
    let Some(provider) = state.goal_verdict_tokens.as_ref() else {
        tracing::warn!("goal judge skipped: the goal judge credential is not configured");
        return Err(JudgeSkipped);
    };
    let token = match provider.read_token(org_id).await {
        Ok(token) => token,
        Err(error) => {
            tracing::warn!(%error, "goal judge skipped: the read token was not minted");
            return Err(JudgeSkipped);
        }
    };
    evidence_or_skip(
        read_run_tool_calls_for_judge(state.thread_goal_client.clone(), &token, org_id, run_id)
            .await,
    )
}

/// The tool-call evidence of `run_id` under an already minted judge read token (the unattended judge
/// route mints one for its goal read). Best-effort, unlike [`run_tool_call_evidence`]: that route reads
/// the turn through `GetGoalTurnForJudge` first, which session-core refuses for a thread holding a
/// zero-retention or waived run before any answer is read.
pub(crate) async fn tool_call_evidence_for_run(
    state: &AppState,
    token: &str,
    org_id: &str,
    run_id: &str,
) -> Vec<EvidenceLine> {
    match read_run_tool_calls_for_judge(state.thread_goal_client.clone(), token, org_id, run_id)
        .await
    {
        Ok(calls) => crate::goal_judge::tool_call_evidence(&calls),
        Err(status) => {
            tracing::warn!(code = ?status.code(), "goal judge tool-call read failed; the judge has no tool-call evidence");
            Vec::new()
        }
    }
}

/// One settled goal turn to judge (EP-7 S3, S6): the route the judge request copies, the model the turn
/// served, the request id and org, the turn's retention, the goal, the turn's number, the answer the
/// judge reads and the evidence it may cite.
pub(crate) struct GoalJudgeCall<'a> {
    pub(crate) route: &'a InferRequest,
    pub(crate) served_model: &'a str,
    pub(crate) request_id: &'a str,
    pub(crate) org_id: &'a str,
    pub(crate) zdr: bool,
    pub(crate) goal: &'a ThreadGoal,
    pub(crate) turn_no: u32,
    pub(crate) answer: &'a str,
    pub(crate) evidence: &'a [EvidenceLine],
}

/// The completion judge of one settled goal turn, shared by the attended settle (`sse.rs`) and the
/// unattended judge route (EP-7 S3, S6). The verdict credential is minted first, so a judge call whose
/// verdict could not be recorded never spends tokens. The judge's reply is held to the evidence rule and
/// recorded. Best-effort: every failure is logged by its code, with no content, and leaves the goal as it
/// stands. Returns the goal session-core answered the verdict write with.
pub(crate) async fn judge_goal_turn(
    state: &AppState,
    inference_bearer: &VerifiedInferenceBearer,
    call: GoalJudgeCall<'_>,
) -> Option<ThreadGoal> {
    let GoalJudgeCall {
        route,
        served_model,
        request_id,
        org_id,
        zdr,
        goal,
        turn_no,
        answer,
        evidence,
    } = call;
    let Some(provider) = state.goal_verdict_tokens.as_ref() else {
        tracing::warn!(%request_id, turn_no, "goal verdict credential is not configured; the judge did not run");
        return None;
    };
    let token = match provider.verdict_token(org_id).await {
        Ok(token) => token,
        Err(error) => {
            tracing::warn!(%error, %request_id, turn_no, "goal verdict token was not minted; the judge did not run");
            return None;
        }
    };
    let prompt = goal_judge_prompt(&goal.objective, &goal.success_criteria, answer, evidence);
    let request = goal_judge_request(route, served_model, request_id, org_id, prompt, zdr);
    let mut client = state.inference_client.clone();
    let reply = tokio::time::timeout(
        GOAL_JUDGE_TIMEOUT,
        client.infer(authenticated_inference_request(request, inference_bearer)),
    )
    .await;
    let response = match reply {
        Ok(Ok(response)) => response.into_inner(),
        Ok(Err(status)) => {
            tracing::warn!(code = ?status.code(), %request_id, turn_no, "goal judge call failed; the goal stays active");
            return None;
        }
        Err(_) => {
            tracing::warn!(%request_id, turn_no, "goal judge call timed out; the goal stays active");
            return None;
        }
    };
    // The judge's tokens count toward the goal only when metered (EP-7 D2).
    let judge_tokens = judge_metered_tokens(&response, route);
    let supplied: Vec<String> = evidence.iter().map(|item| item.id.clone()).collect();
    let judged = parse_goal_verdict(&response.content, &supplied);
    let verdict = judged.verdict;
    let record = verdict_request(
        &goal.id,
        org_id,
        turn_no,
        judged,
        &response.model_used,
        judge_tokens,
    );
    let recorded = record_verdict(state.thread_goal_client.clone(), &token, record).await?;
    tracing::info!(%request_id, turn_no, verdict = verdict.as_str(), "goal verdict recorded");
    Some(recorded)
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
    fn a_goal_turns_run_token_cap_is_the_smaller_of_the_request_the_org_and_the_goal() {
        // F-f04-08: the Do run states min(request, org, goal allowance). The goal half is `goal_turn_cap`'s
        // token figure, already clamped to the org's ceiling, so it can only lower what the request states.
        let org = RunCeiling {
            usd: None,
            tokens: Some(500),
        };
        let with_goal = |tokens: u64| {
            goal_turn_cap(
                GoalAllowance {
                    tokens: Some(tokens),
                    cost_usd_micros: None,
                },
                org,
            )
            .tokens
        };
        // The goal's remaining allowance wins where it is the smallest of the three.
        assert_eq!(
            crate::budget::run_token_cap(true, Some(800), Some(500), with_goal(120)),
            Some(120)
        );
        // The org's ceiling wins where the goal has more than it, and the request still lowers both.
        assert_eq!(
            crate::budget::run_token_cap(true, Some(800), Some(500), with_goal(9_000)),
            Some(500)
        );
        assert_eq!(
            crate::budget::run_token_cap(true, Some(300), Some(500), with_goal(9_000)),
            Some(300)
        );
        // A goal with nothing left never reads as the wire's 0, which is no cap: it is held at one token.
        assert_eq!(with_goal(0), Some(0));
        assert_eq!(
            crate::budget::run_token_cap(true, Some(800), Some(500), with_goal(0)),
            Some(1)
        );
        // With the switch off the Do run states nothing, whatever the goal has left.
        assert_eq!(
            crate::budget::run_token_cap(false, Some(800), Some(500), with_goal(120)),
            None
        );
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
        assert_eq!(
            settle_outcome(AgenticRunOutcome::AwaitingInput),
            "awaiting_input"
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

    fn metered_usage(input: u64, output: u64) -> RunUsage {
        RunUsage {
            provider_used: "openai".to_owned(),
            input_tokens: input,
            output_tokens: output,
            ..Default::default()
        }
    }

    #[test]
    fn a_segment_tops_up_only_for_a_non_zdr_continuation_with_the_switch_on() {
        assert!(should_top_up(true, true, false, false));
        assert!(!should_top_up(false, true, false, false), "switch off");
        assert!(
            !should_top_up(true, false, false, false),
            "an ordinary turn is settled by the ledger"
        );
        assert!(!should_top_up(true, true, true, false), "zero retention");
        assert!(!should_top_up(true, true, false, true), "waived");
    }

    #[test]
    fn the_top_up_switch_reads_only_the_literal_true() {
        assert_eq!(SEGMENT_TOPUP_ENV, "MODEL_GATEWAY_GOAL_SEGMENT_TOPUP");
        for off in [
            None,
            Some(""),
            Some("1"),
            Some("TRUE"),
            Some("on"),
            Some(" true "),
        ] {
            assert!(!mp_contracts::loop_fold::switch_from(off), "{off:?}");
        }
        assert!(mp_contracts::loop_fold::switch_from(Some("true")));
    }

    #[test]
    fn a_segments_usage_is_its_metered_tokens_and_priced_cost() {
        let usage = metered_usage(400, 100);
        let segment = SegmentUsage::of_segment(
            Some(&usage),
            Some(0.000_25),
            CostBasis::Metered,
            "openai",
            "",
        )
        .expect("a segment with usage");
        assert_eq!(
            segment.tokens,
            MeteredTokens {
                input: 400,
                output: 100
            }
        );
        assert_eq!(segment.cost_usd_micros, Some(250));
        assert!(segment.has_spend());
        // An answer without usage is never debited from a tally.
        assert!(
            SegmentUsage::of_segment(None, Some(1.0), CostBasis::Metered, "openai", "").is_none()
        );
        // Unpriced stays unpriced; tokens still count.
        let unpriced =
            SegmentUsage::of_segment(Some(&usage), None, CostBasis::Metered, "openai", "").unwrap();
        assert_eq!(unpriced.cost_usd_micros, None);
        assert!(unpriced.has_spend());
    }

    #[test]
    fn a_segment_that_spent_nothing_metered_makes_no_call() {
        let usage = RunUsage {
            legs: vec![leg(SUBSCRIPTION, 900, 100)],
            ..Default::default()
        };
        let segment = SegmentUsage::of_segment(
            Some(&usage),
            Some(0.0),
            CostBasis::Subscription,
            "",
            "conn-1",
        )
        .unwrap();
        assert_eq!(segment.tokens, MeteredTokens::default());
        assert!(!segment.has_spend());
    }

    #[tokio::test]
    async fn the_top_up_request_carries_ids_and_counts_only() {
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
        let bearer =
            crate::auth::VerifiedSessionBearer::for_test("fixture-token-aaaaaaaaaaaaaaaaaaaa");
        let topup = GoalTopUp::new(ThreadGoalServiceClient::new(channel), &bearer, "org-1");
        let usage = SegmentUsage {
            tokens: MeteredTokens {
                input: 7,
                output: 3,
            },
            cost_usd_micros: None,
            cost_basis: "metered",
        };
        let request = topup.request("run-1", "01JREQUEST", &usage);
        assert_eq!(request.org_id, "org-1");
        assert_eq!(request.run_id, "run-1");
        assert_eq!(request.segment_key, "01JREQUEST");
        assert_eq!(request.metered_input_tokens, 7);
        assert_eq!(request.metered_output_tokens, 3);
        assert_eq!(request.cost_usd_micros, -1, "unpriced is -1 on the wire");
        assert_eq!(request.cost_basis, "metered");
        // A segment with nothing to add never reaches the wire (the dead endpoint is never dialled).
        let nothing = SegmentUsage {
            tokens: MeteredTokens::default(),
            cost_usd_micros: None,
            cost_basis: "subscription",
        };
        assert!(topup
            .top_up("run-1", "01JREQUEST", &nothing)
            .await
            .is_none());
    }
}
