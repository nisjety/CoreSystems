//! The saved recipe as a planner: a deterministic [`ActionPlanner`] that replays one stored
//! procedure one step at a time.
//!
//! Each step is proposed from the latest snapshot and checked with Quarry's replay check
//! (`replay-check`, prefix mode) before the loop dispatches it. No model is consulted, so the
//! planner chooses nothing beyond the stored steps. The loop still runs its own gates on every
//! action (risk, approval, budget, takeover), because each proposal is an ordinary
//! [`BrowserAction`].
//!
//! The hand-back is the only reaction to a mismatch. A check that is not `exact`, a target that
//! does not resolve to exactly one element, a step the loop did not carry out, a step whose outcome
//! did not verify, or a widened verb this run does not allow ends the recipe with a fixed reason.
//! The planner never re-proposes a step, never asks a model, and never substitutes another action.
//!
//! The loop rewrites an action's `action_id` and `grant_id` before it dispatches it, so an action
//! is matched to its proposal by content, not by id. Bound slot text lives here for one run only.
//! It goes to Quarry inside the check and the step, and it is never persisted or logged.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::Value;

use super::admission::is_widened;
use super::recipe::{proposal_for_step, BoundRecipe, RecipeOutcome};
use super::{ActionType, BrowserAction};
use crate::llm_planner::{
    ActionPlanner, DoneOutcome, DonePayload, PlannerContext, PlannerError, PlannerReply,
};
use crate::quarry_agent::{
    AgentClientError, QuarryAgentClient, WireBrowserSnapshot, WireObservation, WireSnapshotTarget,
};

/// Asks Quarry whether proposed actions match a stored procedure. Nothing runs and nothing is
/// persisted. The answer is Quarry's `ReplayDecision`: `"exact"`, or an object that names a repair
/// or a refusal.
#[tonic::async_trait]
pub trait ReplayChecker: Send + Sync {
    /// Checks `actions` against `procedure`. With `prefix`, the actions must be a prefix of it.
    ///
    /// # Errors
    ///
    /// Whatever the check call fails with. The planner reads every error as a mismatch.
    async fn check(
        &self,
        procedure: &Value,
        actions: &[Value],
        prefix: bool,
    ) -> Result<Value, AgentClientError>;
}

/// The [`ReplayChecker`] over Quarry's `replay-check` endpoint, for one org and one request.
#[derive(Debug, Clone)]
pub struct QuarryReplayChecker {
    client: QuarryAgentClient,
    org_id: String,
    request_id: Option<String>,
}

impl QuarryReplayChecker {
    /// A checker that sends its checks for `org_id`, under `request_id` when one is given.
    #[must_use]
    pub fn new(
        client: QuarryAgentClient,
        org_id: impl Into<String>,
        request_id: Option<String>,
    ) -> Self {
        Self {
            client,
            org_id: org_id.into(),
            request_id,
        }
    }
}

#[tonic::async_trait]
impl ReplayChecker for QuarryReplayChecker {
    async fn check(
        &self,
        procedure: &Value,
        actions: &[Value],
        prefix: bool,
    ) -> Result<Value, AgentClientError> {
        self.client
            .replay_check(
                &self.org_id,
                procedure,
                actions,
                prefix,
                self.request_id.as_deref(),
            )
            .await
    }
}

/// A saved recipe bound to one run's slot values, replayed one step at a time.
///
/// `procedure` is the stored procedure as Quarry holds it, the form the replay check compares
/// against. `recipe` is that same procedure, read and bound to the person's values. Both belong to
/// this run alone.
pub struct RecipePlanner {
    procedure: Value,
    recipe: BoundRecipe,
    checker: Arc<dyn ReplayChecker>,
    state: Mutex<State>,
}

impl RecipePlanner {
    /// A planner at the first step of `recipe`.
    #[must_use]
    pub fn new(procedure: Value, recipe: BoundRecipe, checker: Arc<dyn ReplayChecker>) -> Self {
        Self {
            procedure,
            recipe,
            checker,
            state: Mutex::new(State::default()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The planner's decision for one call. It holds the lock and never awaits.
    fn decide(&self, ctx: &PlannerContext<'_>) -> Decision {
        let mut state = self.lock();
        if let Some(reason) = state.halted.as_deref() {
            return Decision::Reply(PlannerReply::done_with(DonePayload::new(
                reason,
                DoneOutcome::Incomplete,
            )));
        }
        let Some(step) = self.recipe.recipe().steps.get(state.cursor) else {
            return Decision::Reply(PlannerReply::done_with(DonePayload::new(
                "recipe completed",
                DoneOutcome::Success,
            )));
        };
        // The last proposal never came back as an observation. The loop refused it before Quarry,
        // or it did not run. The recipe hands back rather than proposing it again.
        if state.pending.is_some() {
            return Decision::Reply(halt(&mut state, needs_repair(step.step)));
        }
        if step.needs_snapshot() && state.snapshot.is_none() {
            if state.snapshot_requested {
                return Decision::Reply(halt(&mut state, needs_repair(step.step)));
            }
            state.snapshot_requested = true;
            let action = observe_action(&ctx.config.grant_id, step.step);
            state.pending = Some(Pending::Snapshot {
                action: action.clone(),
            });
            return Decision::Reply(PlannerReply::action(action));
        }
        let proposal = {
            let (snapshot_id, generation, targets): (&str, u32, &[WireSnapshotTarget]) =
                match state.snapshot.as_ref() {
                    Some(snapshot) => (
                        snapshot.snapshot_id.as_str(),
                        snapshot.generation,
                        snapshot.targets.as_slice(),
                    ),
                    None => ("", 0, &[]),
                };
            proposal_for_step(
                step,
                self.recipe.text_for(step),
                snapshot_id,
                generation,
                targets,
                &ctx.config.grant_id,
            )
        };
        let Ok(mut proposal) = proposal else {
            return Decision::Reply(halt(&mut state, needs_repair(step.step)));
        };
        // Quarry names the top-level frame on its top-level targets. `observation_from_wire` reads
        // that as no frame, so the proposal does the same, or the loop would refuse the target.
        if let (Some(target), Some(root)) = (
            proposal.action.target.as_mut(),
            state.snapshot.as_ref().and_then(root_frame_id),
        ) {
            if target.frame_id.as_deref() == Some(root) {
                target.frame_id = None;
            }
        }
        // With widened actions off the loop would read this verb as an observe. That would be a
        // substitution, so the recipe hands back instead.
        if is_widened(&proposal.action.action_type) && !ctx.config.widened_actions {
            return Decision::Reply(halt(&mut state, needs_repair(step.step)));
        }
        let mut actions = state.carried.clone();
        actions.push(proposal.check_json.clone());
        Decision::Check(Check {
            action: proposal.action,
            step: step.step,
            check: proposal.check_json,
            expect: proposal.expect,
            verified: step.expected_outcome == RecipeOutcome::Verified,
            actions,
        })
    }
}

impl std::fmt::Debug for RecipePlanner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecipePlanner")
            .field("recipe", &self.recipe)
            .finish_non_exhaustive()
    }
}

#[tonic::async_trait]
impl ActionPlanner for RecipePlanner {
    /// The next recipe step, or the end of the recipe. A step is proposed only after Quarry's
    /// replay check answers `exact` for it.
    async fn plan_next(&self, ctx: &PlannerContext<'_>) -> Result<PlannerReply, PlannerError> {
        let check = match self.decide(ctx) {
            Decision::Reply(reply) => return Ok(reply),
            Decision::Check(check) => check,
        };
        // Quarry answers without running anything. The lock is not held across the call.
        let verdict = self
            .checker
            .check(&self.procedure, &check.actions, true)
            .await;
        let exact = matches!(&verdict, Ok(decision) if decision.as_str() == Some("exact"));
        let mut state = self.lock();
        if !exact {
            return Ok(halt(&mut state, needs_repair(check.step)));
        }
        state.pending = Some(Pending::Step {
            action: check.action.clone(),
            step: check.step,
            check: check.check,
            expect: check.expect,
            verified: check.verified,
        });
        Ok(PlannerReply::action(check.action))
    }

    /// The postcondition of the step this action carries, when it is the proposal waiting to run.
    fn step_expect(&self, action: &BrowserAction) -> Option<Value> {
        let state = self.lock();
        match state.pending.as_ref()? {
            Pending::Step {
                action: proposed,
                expect,
                ..
            } if same_action(proposed, action) => expect.clone(),
            _ => None,
        }
    }

    /// Keeps the latest snapshot. When the observation answers the pending proposal, it either
    /// advances the recipe or halts it, on Quarry's outcome for the step.
    fn note_wire_observation(&self, action: &BrowserAction, wire: &WireObservation) {
        let mut state = self.lock();
        let answers = matches!(
            &state.pending,
            Some(pending) if same_action(pending.action(), action)
        );
        // The latest snapshot is kept whether or not this observation answers the proposal.
        state.snapshot = wire.snapshot.clone();
        if !answers {
            return;
        }
        let Some(pending) = state.pending.take() else {
            return;
        };
        let Pending::Step {
            step,
            check,
            expect,
            verified,
            ..
        } = pending
        else {
            // An observe answered. It moved the snapshot and nothing else.
            return;
        };
        let must_verify = expect.is_some() || verified;
        let status = wire
            .action_outcome
            .as_ref()
            .map(|outcome| outcome.status.as_str());
        let refused = match status {
            Some("verified") => false,
            Some("failed") => true,
            // Unknown, another status, or no outcome: a step that must verify does not pass.
            _ => must_verify,
        };
        if refused {
            state.halted = Some(did_not_verify(step));
        } else {
            state.carried.push(check);
            state.cursor += 1;
            state.snapshot_requested = false;
        }
    }
}

/// What the run has carried out so far, and what the planner is waiting to see come back.
#[derive(Default)]
struct State {
    /// The index of the next recipe step to carry out.
    cursor: usize,
    /// The snapshot of the latest observation, when that observation carried one.
    snapshot: Option<WireBrowserSnapshot>,
    /// Whether the current step has already asked for a snapshot. A second observation with none
    /// halts the recipe.
    snapshot_requested: bool,
    /// The checks of the steps carried out, in order. Each later check sends them as its prefix.
    carried: Vec<Value>,
    /// The last thing proposed, until its observation comes back.
    pending: Option<Pending>,
    /// Why the recipe stopped, in fixed text. Set once, and every later call repeats it.
    halted: Option<String>,
}

/// A proposal the loop has not answered yet.
enum Pending {
    /// An observe that asked for a snapshot. It advances nothing.
    Snapshot { action: BrowserAction },
    /// A recipe step, with what is needed to judge the observation that answers it.
    Step {
        action: BrowserAction,
        step: u32,
        check: Value,
        expect: Option<Value>,
        /// Whether the step was verified when the recipe was compiled.
        verified: bool,
    },
}

impl Pending {
    fn action(&self) -> &BrowserAction {
        match self {
            Self::Snapshot { action } | Self::Step { action, .. } => action,
        }
    }
}

/// What one call decides: a reply that needs no check, or a step that needs Quarry's check first.
enum Decision {
    Reply(PlannerReply),
    Check(Check),
}

/// A proposed step that still needs Quarry's replay check.
struct Check {
    action: BrowserAction,
    step: u32,
    check: Value,
    expect: Option<Value>,
    verified: bool,
    /// The carried checks followed by this one: the actions the replay check compares.
    actions: Vec<Value>,
}

/// Ends the recipe with a fixed reason, and returns the planner's done for it.
fn halt(state: &mut State, reason: String) -> PlannerReply {
    let payload = DonePayload::new(&reason, DoneOutcome::Incomplete);
    state.pending = None;
    state.halted = Some(reason);
    PlannerReply::done_with(payload)
}

/// The reason a step that cannot be carried out here hands back. Closed text with the step number.
fn needs_repair(step: u32) -> String {
    format!("recipe step {step} needs repair")
}

/// The reason a step whose outcome did not verify hands back. Closed text with the step number.
fn did_not_verify(step: u32) -> String {
    format!("recipe step {step} did not verify")
}

/// The observe that asks for a snapshot before a semantic step. The loop fills in the id.
fn observe_action(grant_id: &str, step: u32) -> BrowserAction {
    BrowserAction {
        action_id: format!("recipe-observe-{step}"),
        grant_id: grant_id.to_owned(),
        action_type: ActionType::Observe,
        selector: String::new(),
        value: String::new(),
        url: String::new(),
        max_wait_ms: 5000,
        target: None,
        reason: format!("recipe step {step} needs a snapshot"),
        risk_category: None,
    }
}

/// Whether the loop dispatched the action that was proposed. The loop replaces the id and the
/// grant, clamps a wait and trims a key, so those fields are not compared.
fn same_action(proposed: &BrowserAction, dispatched: &BrowserAction) -> bool {
    proposed.action_type == dispatched.action_type
        && proposed.selector == dispatched.selector
        && proposed.value.trim() == dispatched.value.trim()
        && proposed.url == dispatched.url
        && proposed.target == dispatched.target
}

/// The top-level frame of a snapshot: the one with no parent.
fn root_frame_id(snapshot: &WireBrowserSnapshot) -> Option<&str> {
    snapshot
        .frames
        .iter()
        .find(|frame| frame.parent_frame_id.is_none())
        .map(|frame| frame.frame_id.as_str())
}
