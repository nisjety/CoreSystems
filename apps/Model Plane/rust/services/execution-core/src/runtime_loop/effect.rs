//! Effect classification and the ledger calls around a dispatch (build plan WS03 S5).
//!
//! The ledger records what a call may change, and the loop that resumes a run
//! decides whether it may run a call again from that class. The class is derived
//! from the same per-call classifier the autonomy ladder already uses, never from a
//! second table: two tables would let a call be an effect to one gate and a read
//! to the other, and that only shows up as a duplicated side effect. WS05 will
//! replace this with declared tool classes; until then this is the stub.
//!
//! The ledger is off unless `EXECUTION_EFFECT_LEDGER_ENABLED` is `true`. When it is
//! on, an effectful call is reserved before it is dispatched and finalized after,
//! and a call that cannot be reserved is not run: a call whose effect cannot be
//! recorded ahead of time is not one to run blind.

use mp_contracts::effect_ledger::EffectClass;
use mp_contracts::model_plane::v1::{self as pb, session_core_client::SessionCoreClient, AutonomyRung};
use tonic::transport::Channel;

const LEDGER_ENV: &str = "EXECUTION_EFFECT_LEDGER_ENABLED";

#[cfg(test)]
thread_local! {
    /// A test's override of the switch. `#[tokio::test]` runs its future on the
    /// test's own thread, so a thread-local cannot leak into a parallel test the
    /// way a process-wide environment variable would.
    static TEST_SWITCH: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Holds the switch at a fixed value for the current test thread, then restores it.
#[cfg(test)]
pub(crate) struct LedgerSwitch;

#[cfg(test)]
impl LedgerSwitch {
    pub(crate) fn set(on: bool) -> Self {
        TEST_SWITCH.with(|cell| cell.set(Some(on)));
        Self
    }
}

#[cfg(test)]
impl Drop for LedgerSwitch {
    fn drop(&mut self) {
        TEST_SWITCH.with(|cell| cell.set(None));
    }
}

/// Whether the effect ledger is switched on. Off by default.
pub(crate) fn ledger_enabled() -> bool {
    #[cfg(test)]
    if let Some(forced) = TEST_SWITCH.with(std::cell::Cell::get) {
        return forced;
    }
    std::env::var(LEDGER_ENV).is_ok_and(|value| value.trim().eq_ignore_ascii_case("true"))
}

/// The effect class of one call, from the tool and its actual arguments.
///
/// The rung a call needs says how far it reaches: the widest rung is a
/// destructive or outbound action, which can leave the platform (`External`); the
/// middle rung is a durable write inside the run's own context (`Write`); anything
/// else is a read. A call the classifier cannot place is already treated as the
/// widest rung, so an unrecognised tool is never mistaken for a read.
pub(crate) fn effect_class_of(tool_name: &str, tool_input: &str) -> EffectClass {
    match crate::permission::rung_required_for(tool_name, tool_input) {
        AutonomyRung::DangerFullAccess => EffectClass::External,
        AutonomyRung::WorkspaceWrite => EffectClass::Write,
        _ => EffectClass::Read,
    }
}

/// The digest the ledger stores for a call's arguments, or `None` when they are not
/// JSON. Only the digest is stored; the arguments never leave the loop.
pub(crate) fn args_digest_of(tool_input: &str) -> Option<String> {
    mp_contracts::effect_ledger::args_digest(tool_input)
}

/// The ledger's action id for a step: the step id itself when the ledger will accept
/// it, otherwise a digest of it. A step id embeds the model's call id, which the
/// ledger does not bound the way it bounds its own tokens.
pub(crate) fn action_id_for(step_id: &str) -> String {
    let plain = !step_id.is_empty()
        && step_id.len() <= 128
        && step_id == step_id.trim()
        && !step_id.chars().any(char::is_control);
    if plain {
        step_id.to_owned()
    } else {
        format!("act-{}", &blake3::hash(step_id.as_bytes()).to_hex()[..40])
    }
}

/// The outcome the ledger records for a step that ended with `status`.
///
/// A call the gate stopped before its body ran did not run. A call that finished
/// reports its own result. Anything else, a timeout above all, is an outcome nobody
/// observed: the honest record is that it is unknown.
pub(crate) fn final_outcome_for(status: &str) -> &'static str {
    match status {
        "completed" => "completed",
        "failed" => "failed",
        "awaiting_approval" | "permission_denied" => "not_run",
        _ => "unknown_outcome",
    }
}

/// What the ledger records about one call: ids and a class, never arguments.
///
/// session-core binds a reservation to ALL of these (run, action, tool, data category,
/// zdr, class and digest): a second reserve of the same action that differs in any of them
/// is refused as an identity conflict, not answered as a replay. So an entry is built one
/// way, by [`LedgerEntry::new`], whoever reserves (a dispatch in phase 2, or the release's
/// pre-reserve), and two entries for one call are equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LedgerEntry<'a> {
    pub run_id: &'a str,
    pub action_id: String,
    pub tool: &'a str,
    pub data_category: &'a str,
    pub zdr: bool,
    pub class: EffectClass,
    pub args_digest: Option<String>,
}

impl<'a> LedgerEntry<'a> {
    /// The one builder. `step_id` becomes the ledger's action id ([`action_id_for`]).
    // Called only by `entry_for_call` and the release's entry, both dead until the in-place
    // resume lands (WS03 S7b step 9 removes this allow).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(
        run_id: &'a str,
        step_id: &str,
        tool: &'a str,
        data_category: &'a str,
        zdr: bool,
        class: EffectClass,
        args_digest: Option<String>,
    ) -> Self {
        Self {
            run_id,
            action_id: action_id_for(step_id),
            tool,
            data_category,
            zdr,
            class,
            args_digest,
        }
    }
}

/// The entry for a call about to be dispatched: its class and digest come from the tool
/// and its actual arguments. The loop's phase 2 builds the entry of every effectful call
/// this way.
// WS03 S7b step 7/9 switches phase 2 to this builder; until then the loop spells the same
// fields inline and only the tests call it (step 9 removes this allow).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn entry_for_call<'a>(
    run_id: &'a str,
    step_id: &str,
    tool: &'a str,
    tool_input: &str,
    data_category: &'a str,
    zdr: bool,
) -> LedgerEntry<'a> {
    LedgerEntry::new(
        run_id,
        step_id,
        tool,
        data_category,
        zdr,
        effect_class_of(tool, tool_input),
        args_digest_of(tool_input),
    )
}

/// What a reservation did. The ledger answers `created: false` for an exact
/// replay of an earlier reservation, and that answer is not "go ahead": it says an
/// earlier attempt, possibly of a loop that has since died, already claimed this
/// action and may have started it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReserveOutcome {
    /// This call made the reservation: nothing has started the action before.
    Created,
    /// The reservation already existed. An earlier attempt may have run the call,
    /// so it is never dispatched again; it is closed as an unknown outcome.
    AlreadyReserved,
}

impl ReserveOutcome {
    /// From the response's `created` flag.
    pub(crate) const fn from_created(created: bool) -> Self {
        if created {
            Self::Created
        } else {
            Self::AlreadyReserved
        }
    }
}

/// Reserve a call before it is dispatched. The error is the reason, for the log.
pub(crate) async fn reserve(
    channel: &Channel,
    bearer: Option<&str>,
    entry: &LedgerEntry<'_>,
) -> Result<ReserveOutcome, String> {
    let bearer = bearer.ok_or_else(|| "no verified session credential".to_owned())?;
    let request = super::authenticated_session_request(
        pb::ReserveToolActionRequest {
            run_id: entry.run_id.to_owned(),
            action_id: entry.action_id.clone(),
            // The run is the request: a run's agent loop has no per-call gateway request.
            request_id: entry.run_id.to_owned(),
            tool: entry.tool.to_owned(),
            data_category: entry.data_category.to_owned(),
            zdr: entry.zdr,
            args_digest: entry.args_digest.clone().unwrap_or_default(),
            effect_class: entry.class.as_str().to_owned(),
        },
        bearer,
    )?;
    SessionCoreClient::new(channel.clone())
        .reserve_tool_action(request)
        .await
        .map(|response| ReserveOutcome::from_created(response.into_inner().created))
        .map_err(|status| status.message().to_owned())
}

/// Record how a reserved call ended. A failure leaves the row `reserved`, which the
/// session-core sweep closes as `unknown_outcome`: honest, and never a replay.
pub(crate) async fn finalize(
    channel: &Channel,
    bearer: Option<&str>,
    run_id: &str,
    action_id: &str,
    outcome: &str,
) -> Result<(), String> {
    let bearer = bearer.ok_or_else(|| "no verified session credential".to_owned())?;
    let request = super::authenticated_session_request(
        pb::FinalizeToolActionRequest {
            run_id: run_id.to_owned(),
            action_id: action_id.to_owned(),
            outcome: outcome.to_owned(),
            receipt_id: String::new(),
        },
        bearer,
    )?;
    SessionCoreClient::new(channel.clone())
        .finalize_tool_action(request)
        .await
        .map(|_| ())
        .map_err(|status| status.message().to_owned())
}

/// The most action ids one read of the ledger may name (session-core's bound).
const LIST_ACTIONS_MAX_IDS: usize = 64;

/// One ledger row as the loop that resumes a run reads it: the tool, the state and
/// the digest, never the arguments. `effect_class` and `args_digest` are `None` for
/// a row whose writer did not record them.
// Read only by the in-place resume (WS03 S7b), which is itself off by default; until
// it lands nothing in the loop constructs or reads a row.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LedgerRow {
    pub tool: String,
    pub status: mp_contracts::loop_fold::LedgerStatus,
    pub effect_class: Option<EffectClass>,
    pub args_digest: Option<String>,
}

/// Why a ledger read failed, by what the loop that resumes a run must do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LedgerReadFailure {
    /// `PermissionDenied` or `Unauthenticated`: the caller may not read this run (a missing
    /// scope, not the owner, a revoked Space membership). Asking again, or running the
    /// call another way, is not allowed: the resume holds.
    Denied,
    /// The ZDR catalogue refused: the run keeps no durable state. The resume holds.
    NotPersistent,
    /// Anything else: the read could not be made or could not be trusted. The resume may
    /// fall back to the stage that needs no read.
    Other,
}

/// A failed ledger read: what kind of failure, and the reason for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LedgerReadError {
    pub kind: LedgerReadFailure,
    /// The server's reason, or the plumbing's. Content-free by construction (a closed
    /// token or a fixed sentence); it is only ever logged.
    pub reason: String,
}

impl LedgerReadError {
    /// Classify a refused `ListToolActions`. The ZDR catalogue trailer is read first,
    /// whatever the code, exactly as the fold store's statuses are.
    fn from_status(status: &tonic::Status) -> Self {
        use tonic::Code;
        let kind = if mp_contracts::zdr_refusal::Refusal::from_status(status).is_some() {
            LedgerReadFailure::NotPersistent
        } else if matches!(status.code(), Code::PermissionDenied | Code::Unauthenticated) {
            LedgerReadFailure::Denied
        } else {
            LedgerReadFailure::Other
        };
        Self {
            kind,
            reason: status.message().to_owned(),
        }
    }
}

impl From<String> for LedgerReadError {
    /// A local failure (no credential, an answer that is not the answer to the question).
    fn from(reason: String) -> Self {
        Self {
            kind: LedgerReadFailure::Other,
            reason,
        }
    }
}

impl std::fmt::Display for LedgerReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

/// Compares the reason, so a test that asked "what was the reason" keeps asking it.
impl PartialEq<&str> for LedgerReadError {
    fn eq(&self, other: &&str) -> bool {
        self.reason == *other
    }
}

/// Read one wire row. A value outside the shared vocabulary is an error: a status,
/// class or digest the loop cannot read must not be guessed at, because a guess
/// could take a call that may have run for one that did not.
#[allow(dead_code)] // S7b
fn row_from_state(state: &pb::ToolActionState) -> Result<LedgerRow, String> {
    let status = mp_contracts::effect_ledger::ledger_status_from_row(&state.status)
        .ok_or_else(|| "the ledger returned an unknown status".to_owned())?;
    let effect_class = if state.effect_class.is_empty() {
        None
    } else {
        Some(
            EffectClass::parse(&state.effect_class)
                .ok_or_else(|| "the ledger returned an unknown effect class".to_owned())?,
        )
    };
    let args_digest = if state.args_digest.is_empty() {
        None
    } else if mp_contracts::effect_ledger::is_args_digest(&state.args_digest) {
        Some(state.args_digest.clone())
    } else {
        return Err("the ledger returned a malformed args digest".to_owned());
    };
    Ok(LedgerRow {
        tool: state.tool.clone(),
        status,
        effect_class,
        args_digest,
    })
}

/// The rows of an answer, by action id. The answer must be to the question: a row
/// for an action that was not asked, or two rows for one action, is refused.
#[allow(dead_code)] // S7b
fn rows_from_states(
    asked: &[String],
    states: Vec<pb::ToolActionState>,
) -> Result<std::collections::BTreeMap<String, LedgerRow>, String> {
    let mut rows = std::collections::BTreeMap::new();
    for state in states {
        if !asked.contains(&state.action_id) {
            return Err("the ledger answered an action that was not asked".to_owned());
        }
        let row = row_from_state(&state)?;
        if rows.insert(state.action_id, row).is_some() {
            return Err("the ledger answered one action twice".to_owned());
        }
    }
    Ok(rows)
}

/// Read the ledger's rows for the named actions of a run: what the loop that resumes
/// a run needs to close a dangling call honestly. A run that keeps no ledger rows
/// answers an empty map. The error is the reason, for the log.
///
/// # Absence is not proof
/// An action id that is missing from the returned map is UNKNOWN, never "not run".
/// It can mean the id was never reserved, that a concurrent writer reserved it and has
/// not committed yet, or that the row is one the read hides (a zero-retention row, or a
/// run that keeps no ledger). The read cannot tell these apart and this function never
/// invents a row for an absent id. The S7b rule: an effectful call that is absent from
/// this read is UNKNOWN and must be closed as such, never re-dispatched as "not run,
/// safe to dispatch". Only a row with status `not_run` (or a `completed` row with its
/// output) licenses a re-dispatch decision.
// Called only by the in-place resume (WS03 S7b), which is itself off by default.
#[allow(dead_code)]
pub(crate) async fn list_actions(
    channel: &Channel,
    bearer: Option<&str>,
    run_id: &str,
    action_ids: &[String],
) -> Result<std::collections::BTreeMap<String, LedgerRow>, LedgerReadError> {
    list_actions_with(bearer, run_id, action_ids, |request| {
        let mut client = SessionCoreClient::new(channel.clone());
        async move { client.list_tool_actions(request).await }
    })
    .await
}

/// [`list_actions`] with the RPC passed in, so the bearer, the chunking and the checks
/// on each answer are testable without a gRPC server. `call` is given the finished
/// request (bearer attached) of one chunk and returns the served response.
///
/// Each id is asked once, in the order given (a repeat is dropped before chunking, so
/// two chunks cannot answer the same id). The read stops at the first refused or
/// unreadable chunk: a partial map is never returned.
#[allow(dead_code)] // S7b, through `list_actions`
pub(crate) async fn list_actions_with<F, Fut>(
    bearer: Option<&str>,
    run_id: &str,
    action_ids: &[String],
    mut call: F,
) -> Result<std::collections::BTreeMap<String, LedgerRow>, LedgerReadError>
where
    F: FnMut(tonic::Request<pb::ListToolActionsRequest>) -> Fut,
    Fut: std::future::Future<Output = Result<tonic::Response<pb::ListToolActionsResponse>, tonic::Status>>,
{
    if action_ids.is_empty() {
        return Ok(std::collections::BTreeMap::new());
    }
    let bearer = bearer.ok_or_else(|| "no verified session credential".to_owned())?;
    let mut seen = std::collections::BTreeSet::new();
    let unique: Vec<String> = action_ids
        .iter()
        .filter(|id| seen.insert((*id).clone()))
        .cloned()
        .collect();
    let mut rows = std::collections::BTreeMap::new();
    for chunk in unique.chunks(LIST_ACTIONS_MAX_IDS) {
        let request = super::authenticated_session_request(
            pb::ListToolActionsRequest {
                run_id: run_id.to_owned(),
                action_ids: chunk.to_vec(),
            },
            bearer,
        )?;
        let response = call(request)
            .await
            .map_err(|status| LedgerReadError::from_status(&status))?
            .into_inner();
        rows.extend(rows_from_states(chunk, response.actions)?);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_destructive_or_outbound_call_is_external() {
        for tool in ["delete_records", "send_email", "execute_provider_action", "browser_agent"] {
            assert_eq!(effect_class_of(tool, "{}"), EffectClass::External, "{tool}");
        }
    }

    #[test]
    fn a_provider_read_is_not_external_but_a_provider_write_is() {
        // `execute_provider_action` is classified by its operation, not its name.
        assert_eq!(
            effect_class_of("execute_provider_action", r#"{"operation":"pages.list"}"#),
            EffectClass::Read,
            "a catalogued read is a read"
        );
        assert_eq!(
            effect_class_of("execute_provider_action", r#"{"operation":"pages.post"}"#),
            EffectClass::External,
            "a catalogued write reaches outside the platform"
        );
        assert_eq!(
            effect_class_of("execute_provider_action", "{}"),
            EffectClass::External,
            "a call with no operation fails safe"
        );
    }

    #[test]
    fn a_memory_write_is_a_write_and_a_lookup_is_a_read() {
        assert_eq!(effect_class_of("save_memory", "{}"), EffectClass::Write);
        assert_eq!(effect_class_of("knowledge_search", "{}"), EffectClass::Read);
        assert_eq!(effect_class_of("recall_memory", "{}"), EffectClass::Read);
    }

    #[test]
    fn an_external_tool_is_never_mistaken_for_a_read() {
        assert_eq!(
            effect_class_of("mcp__crm__lookup_contact", "{}"),
            EffectClass::External
        );
    }

    #[test]
    fn the_digest_is_stable_and_only_exists_for_json_arguments() {
        let digest = args_digest_of(r#"{"b":1,"a":2}"#).expect("digest");
        assert_eq!(args_digest_of(r#"{"a":2,"b":1}"#).as_deref(), Some(digest.as_str()));
        assert_eq!(args_digest_of("not json"), None);
    }

    #[test]
    fn a_replayed_reservation_is_not_a_fresh_one() {
        assert_eq!(ReserveOutcome::from_created(true), ReserveOutcome::Created);
        assert_eq!(
            ReserveOutcome::from_created(false),
            ReserveOutcome::AlreadyReserved,
            "created: false means an earlier attempt already claimed the action"
        );
    }

    #[test]
    fn a_step_id_the_ledger_accepts_is_used_as_it_is() {
        assert_eq!(action_id_for("tool_1_call-1"), "tool_1_call-1");
    }

    #[test]
    fn a_step_id_the_ledger_would_refuse_becomes_a_digest_of_itself() {
        for hostile in [
            "x".repeat(200),
            " padded ".to_owned(),
            "has\nnewline".to_owned(),
            String::new(),
        ] {
            let id = action_id_for(&hostile);
            assert!(id.starts_with("act-"), "{hostile:?}");
            assert!(id.len() <= 128);
            assert!(!id.chars().any(char::is_control));
            // The same step id always maps to the same action id.
            assert_eq!(id, action_id_for(&hostile));
        }
        assert_ne!(action_id_for(&"a".repeat(200)), action_id_for(&"b".repeat(200)));
    }

    #[test]
    fn a_step_status_maps_to_the_honest_ledger_outcome() {
        assert_eq!(final_outcome_for("completed"), "completed");
        assert_eq!(final_outcome_for("failed"), "failed");
        // Stopped by a gate before its body ran: it did not run.
        assert_eq!(final_outcome_for("awaiting_approval"), "not_run");
        assert_eq!(final_outcome_for("permission_denied"), "not_run");
        // Nobody saw how these ended, so the record says so.
        for status in ["timed_out", "resource_exhausted", "anything_else", ""] {
            assert_eq!(final_outcome_for(status), "unknown_outcome", "{status:?}");
        }
    }

    #[test]
    fn every_outcome_the_loop_records_is_one_the_ledger_accepts() {
        for status in [
            "completed",
            "failed",
            "awaiting_approval",
            "permission_denied",
            "timed_out",
            "unknown",
        ] {
            assert!(mp_contracts::effect_ledger::is_final_outcome(final_outcome_for(status)));
        }
    }

    fn state(id: &str, status: &str, class: &str, digest: &str) -> pb::ToolActionState {
        pb::ToolActionState {
            action_id: id.to_owned(),
            tool: "send_email".to_owned(),
            status: status.to_owned(),
            effect_class: class.to_owned(),
            args_digest: digest.to_owned(),
            closed_by: String::new(),
        }
    }

    #[test]
    fn a_ledger_row_keeps_the_status_class_and_digest_the_ledger_stored() {
        let digest = args_digest_of(r#"{"to":"a"}"#).expect("digest");
        let asked = vec!["act-1".to_owned(), "act-2".to_owned(), "act-3".to_owned()];
        let rows = rows_from_states(
            &asked,
            vec![
                state("act-1", "completed", "external", &digest),
                // A row the gateway's read-only loop wrote has no class and no digest.
                state("act-2", "reserved", "", ""),
                state("act-3", "unknown_outcome", "write", &digest),
            ],
        )
        .expect("rows");
        assert_eq!(rows.len(), 3);
        let first = &rows["act-1"];
        assert_eq!(first.tool, "send_email");
        assert_eq!(first.status, mp_contracts::loop_fold::LedgerStatus::Completed);
        assert_eq!(first.effect_class, Some(EffectClass::External));
        assert_eq!(first.args_digest.as_deref(), Some(digest.as_str()));
        let second = &rows["act-2"];
        assert_eq!(second.status, mp_contracts::loop_fold::LedgerStatus::Reserved);
        assert_eq!((second.effect_class, second.args_digest.as_deref()), (None, None));
        assert_eq!(rows["act-3"].status, mp_contracts::loop_fold::LedgerStatus::Unknown);
    }

    #[test]
    fn a_row_the_loop_cannot_read_is_an_error_never_a_guess() {
        let digest = args_digest_of("{}").expect("digest");
        let asked = vec!["act-1".to_owned()];
        // A status, class or digest outside the shared vocabulary: guessing would let a
        // call that may have run be taken for one that did not.
        for bad in [
            state("act-1", "pending", "write", &digest),
            state("act-1", "", "write", &digest),
            state("act-1", "completed", "destructive", &digest),
            state("act-1", "completed", "write", "not-a-digest"),
        ] {
            assert!(rows_from_states(&asked, vec![bad.clone()]).is_err(), "{bad:?}");
        }
        // An action nobody asked about, or the same action twice, is not the answer to the question.
        assert!(rows_from_states(&asked, vec![state("act-9", "completed", "write", &digest)]).is_err());
        assert!(rows_from_states(
            &asked,
            vec![
                state("act-1", "completed", "write", &digest),
                state("act-1", "failed", "write", &digest)
            ]
        )
        .is_err());
        // No rows at all is a valid answer: the run kept none.
        assert!(rows_from_states(&asked, Vec::new()).expect("empty").is_empty());
    }

    #[tokio::test]
    async fn the_read_needs_a_credential_and_asks_nothing_when_there_is_nothing_to_ask() {
        // A channel that goes nowhere: a call that reached it would fail with a transport error.
        let channel = Channel::from_static("http://127.0.0.1:1").connect_lazy();
        let ids = vec!["act-1".to_owned()];
        assert_eq!(
            list_actions(&channel, None, "run-1", &ids).await.expect_err("no credential"),
            "no verified session credential"
        );
        assert!(list_actions(&channel, Some("bearer"), "run-1", &[])
            .await
            .expect("nothing asked")
            .is_empty());
    }

    /// What a served `ListToolActions` saw: the authorization header, the run id and the ids.
    type Seen = std::sync::Arc<std::sync::Mutex<Vec<(String, String, Vec<String>)>>>;

    fn digest_for_tests() -> String {
        args_digest_of(r#"{"to":"a"}"#).expect("digest")
    }

    /// Run `list_actions_with` against a served answer that is computed from each request,
    /// and report what the server saw. No gRPC server is involved: the call is a closure.
    async fn list_through(
        bearer: Option<&str>,
        ids: &[String],
        answer: impl Fn(&[String]) -> Result<Vec<pb::ToolActionState>, tonic::Status>,
    ) -> (
        Result<std::collections::BTreeMap<String, LedgerRow>, LedgerReadError>,
        Vec<(String, String, Vec<String>)>,
    ) {
        let seen: Seen = Seen::default();
        let recorder = std::sync::Arc::clone(&seen);
        let result = list_actions_with(bearer, "run-7", ids, |request| {
            let header = request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            let message = request.into_inner();
            recorder
                .lock()
                .expect("seen lock")
                .push((header, message.run_id.clone(), message.action_ids.clone()));
            let served = answer(&message.action_ids)
                .map(|actions| tonic::Response::new(pb::ListToolActionsResponse { actions }));
            async move { served }
        })
        .await;
        let seen = seen.lock().expect("seen lock").clone();
        (result, seen)
    }

    fn many_ids(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("act-{index:03}")).collect()
    }

    #[tokio::test]
    async fn the_bearer_and_the_run_are_forwarded_and_ids_go_in_chunks_of_sixty_four() {
        let digest = digest_for_tests();
        let ids = many_ids(130);
        let (result, seen) = list_through(Some("tok-1"), &ids, |asked| {
            // Answer only every other id: the others have no row.
            Ok(asked
                .iter()
                .filter(|id| id.ends_with(['0', '2', '4', '6', '8']))
                .map(|id| state(id, "completed", "write", &digest))
                .collect())
        })
        .await;

        let sizes: Vec<usize> = seen.iter().map(|(_, _, chunk)| chunk.len()).collect();
        assert_eq!(sizes, vec![64, 64, 2], "every chunk is asked, none over the bound");
        assert!(seen.iter().all(|(header, run, _)| header == "Bearer tok-1" && run == "run-7"));
        let asked: Vec<String> = seen.iter().flat_map(|(_, _, chunk)| chunk.clone()).collect();
        assert_eq!(asked, ids, "every id is asked once, in order");

        let rows = result.expect("merged answer");
        assert_eq!(rows.len(), 65, "rows from all three chunks are merged");
        assert!(rows.contains_key("act-000") && rows.contains_key("act-128"));
        assert!(!rows.contains_key("act-001"), "an id with no row stays absent");
    }

    #[tokio::test]
    async fn an_absent_id_stays_absent_and_is_never_reported_as_a_row() {
        // Absence is not evidence that the call never ran: the id may never have been reserved, may be
        // reserved by a writer that has not committed, or may be a row hidden from this read. The map
        // therefore has no entry for it, and S7b must treat a missing entry as UNKNOWN for an effectful call.
        let digest = digest_for_tests();
        let ids = vec!["act-a".to_owned(), "act-b".to_owned()];
        let (result, _) = list_through(Some("tok-1"), &ids, |_| {
            Ok(vec![state("act-a", "not_run", "external", &digest)])
        })
        .await;
        let rows = result.expect("answer");
        assert_eq!(rows.keys().map(String::as_str).collect::<Vec<_>>(), vec!["act-a"]);
        assert!(rows.get("act-b").is_none(), "the absent id is not in the map");
        assert_eq!(rows["act-a"].status, mp_contracts::loop_fold::LedgerStatus::NotRun);

        // An answer with no rows at all is an empty map, never a map of invented rows.
        let (empty, _) = list_through(Some("tok-1"), &ids, |_| Ok(Vec::new())).await;
        assert!(empty.expect("empty answer").is_empty());
    }

    #[tokio::test]
    async fn a_row_for_an_id_asked_in_another_chunk_is_not_the_answer_to_this_chunk() {
        let digest = digest_for_tests();
        let ids = many_ids(70);
        // The first chunk answers with an id that was only asked in the second chunk.
        let (result, seen) = list_through(Some("tok-1"), &ids, |asked| {
            if asked.first().map(String::as_str) == Some("act-000") {
                Ok(vec![state("act-069", "completed", "write", &digest)])
            } else {
                Ok(Vec::new())
            }
        })
        .await;
        assert_eq!(
            result.expect_err("an id of another chunk"),
            "the ledger answered an action that was not asked"
        );
        assert_eq!(seen.len(), 1, "a refused chunk ends the read: nothing further is asked");
    }

    #[tokio::test]
    async fn an_unknown_status_or_class_and_a_duplicate_answer_are_errors() {
        let digest = digest_for_tests();
        let ids = vec!["act-a".to_owned()];
        // Each value is refused for its own reason: the error names what was wrong with the row,
        // so a row refused by some other check (a different order of checks, an earlier error)
        // does not pass for the case it was meant to prove.
        for (bad, expected) in [
            (
                state("act-a", "pending", "write", &digest),
                "the ledger returned an unknown status",
            ),
            (
                state("act-a", "completed", "destructive", &digest),
                "the ledger returned an unknown effect class",
            ),
            (
                state("act-a", "completed", "write", "nope"),
                "the ledger returned a malformed args digest",
            ),
        ] {
            let (result, _) = list_through(Some("tok-1"), &ids, |_| Ok(vec![bad.clone()])).await;
            assert_eq!(result.expect_err(expected), expected);
        }
        let (duplicate, _) = list_through(Some("tok-1"), &ids, |_| {
            Ok(vec![
                state("act-a", "completed", "write", &digest),
                state("act-a", "completed", "write", &digest),
            ])
        })
        .await;
        assert_eq!(duplicate.expect_err("one id twice"), "the ledger answered one action twice");
    }

    #[tokio::test]
    async fn an_id_named_twice_is_asked_once_so_two_chunks_cannot_answer_it_twice() {
        let digest = digest_for_tests();
        let mut ids = many_ids(64);
        ids.push("act-000".to_owned());
        let (result, seen) = list_through(Some("tok-1"), &ids, |asked| {
            Ok(asked
                .iter()
                .map(|id| state(id, "completed", "write", &digest))
                .collect())
        })
        .await;
        assert_eq!(seen.len(), 1, "the repeat is dropped before chunking");
        assert_eq!(result.expect("answer").len(), 64);
    }

    #[tokio::test]
    async fn a_refused_call_is_the_servers_reason_and_an_empty_list_asks_nothing() {
        let ids = many_ids(3);
        let (refused, seen) = list_through(Some("tok-1"), &ids, |_| {
            Err(tonic::Status::failed_precondition("loop_checkpoint_disabled"))
        })
        .await;
        assert_eq!(refused.expect_err("refused"), "loop_checkpoint_disabled");
        assert_eq!(seen.len(), 1);

        let (none, seen) = list_through(Some("tok-1"), &[], |_| panic!("nothing was asked")).await;
        assert!(none.expect("nothing asked").is_empty());
        assert!(seen.is_empty(), "an empty id list makes no call");

        // No credential: the error is the plumbing's, and no call is made.
        let (no_bearer, seen) = list_through(None, &ids, |_| panic!("no credential")).await;
        assert_eq!(no_bearer.expect_err("no credential"), "no verified session credential");
        assert!(seen.is_empty());
    }

    /// A refusal that will not change (the caller is not allowed, or the run keeps no
    /// durable state) must not be read as "the ledger is unavailable": the loop that resumes a
    /// run holds on the first and may fall back on the second.
    #[tokio::test]
    async fn a_denied_or_zdr_ledger_read_is_told_apart_from_an_unavailable_one() {
        let ids = many_ids(3);
        for code in [tonic::Code::PermissionDenied, tonic::Code::Unauthenticated] {
            let (result, seen) = list_through(Some("tok-1"), &ids, |_| {
                Err(tonic::Status::new(code, "Space membership required for this run"))
            })
            .await;
            let error = result.expect_err("a denial");
            assert_eq!(error.kind, LedgerReadFailure::Denied, "{code:?}");
            assert_eq!(error, "Space membership required for this run", "{code:?}");
            assert_eq!(seen.len(), 1);
        }
        // The ZDR catalogue refusal carries its kind on a trailer, whatever the code, and the
        // trailer wins over the code (a `NotFound` or `InvalidArgument` is not "other").
        for code in [
            tonic::Code::FailedPrecondition,
            tonic::Code::NotFound,
            tonic::Code::InvalidArgument,
            tonic::Code::PermissionDenied,
        ] {
            let refusal = mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN.grpc_status(code);
            let (result, _) = list_through(Some("tok-1"), &ids, |_| Err(refusal.clone())).await;
            assert_eq!(
                result.expect_err("a catalogue refusal").kind,
                LedgerReadFailure::NotPersistent,
                "{code:?}"
            );
        }
        // Anything else is an unavailable read, with the server's reason kept for the log.
        for code in [
            tonic::Code::Unavailable,
            tonic::Code::DeadlineExceeded,
            tonic::Code::NotFound,
            tonic::Code::Unimplemented,
            tonic::Code::Internal,
            tonic::Code::FailedPrecondition,
        ] {
            let (result, _) = list_through(Some("tok-1"), &ids, |_| {
                Err(tonic::Status::new(code, "loop_checkpoint_disabled"))
            })
            .await;
            let error = result.expect_err("an unavailable read");
            assert_eq!(error.kind, LedgerReadFailure::Other, "{code:?}");
            assert_eq!(error, "loop_checkpoint_disabled");
        }
        // The local failures (no credential, a chunk that is not the answer) are `Other`.
        let (no_bearer, _) = list_through(None, &ids, |_| panic!("no credential")).await;
        assert_eq!(no_bearer.expect_err("no credential").kind, LedgerReadFailure::Other);

        // A refusal in a later chunk ends the read: the rows of the first chunk are never
        // returned as if they were the whole answer.
        let digest = digest_for_tests();
        let many = many_ids(70);
        let (partial, seen) = list_through(Some("tok-1"), &many, |asked| {
            if asked.first().map(String::as_str) == Some("act-000") {
                Ok(vec![state("act-000", "completed", "write", &digest)])
            } else {
                Err(tonic::Status::permission_denied("not yours"))
            }
        })
        .await;
        assert_eq!(partial.expect_err("the second chunk is denied").kind, LedgerReadFailure::Denied);
        assert_eq!(seen.len(), 2);
    }

    /// The reservation and the ledger read build their entry the same way, from the same
    /// inputs: session-core binds a reservation to run, action, tool, data category, zdr,
    /// class and digest, so two spellings of one call would be an identity conflict.
    #[test]
    fn a_ledger_entry_is_built_one_way_whoever_asks() {
        let input = r#"{"b":1,"a":2}"#;
        let entry = entry_for_call("run-1", "tool_1_c1", "delete_records", input, "public_non_personal", false);
        assert_eq!(entry.run_id, "run-1");
        assert_eq!(entry.action_id, action_id_for("tool_1_c1"));
        assert_eq!(entry.tool, "delete_records");
        assert_eq!(entry.data_category, "public_non_personal");
        assert!(!entry.zdr);
        assert_eq!(entry.class, effect_class_of("delete_records", input));
        assert_eq!(entry.args_digest, args_digest_of(input));
        assert_eq!(
            entry,
            LedgerEntry::new(
                "run-1",
                "tool_1_c1",
                "delete_records",
                "public_non_personal",
                false,
                entry.class,
                entry.args_digest.clone(),
            )
        );
        // Key order does not change the identity; a changed class or digest does.
        let reordered = entry_for_call(
            "run-1",
            "tool_1_c1",
            "delete_records",
            r#"{"a":2,"b":1}"#,
            "public_non_personal",
            false,
        );
        assert_eq!(entry, reordered);
        let other = entry_for_call("run-1", "tool_1_c1", "delete_records", r#"{"a":3}"#, "public_non_personal", false);
        assert_ne!(entry, other);
    }

    #[test]
    fn the_switch_follows_the_environment_unless_a_test_forces_it() {
        {
            let _on = LedgerSwitch::set(true);
            assert!(ledger_enabled());
        }
        {
            let _off = LedgerSwitch::set(false);
            assert!(!ledger_enabled());
        }
    }
}
