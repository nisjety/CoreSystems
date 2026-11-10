//! The retention receipt of a run: the zero-retention posture that applied when
//! the run was created, recorded once on the run and never changed.
//!
//! A run is one of three classes. `zdr` is a zero-retention run: its content is
//! not persisted. `persistent` is an ordinary run. `zdr_waived` is a persistent
//! run under a connection-bound waiver: the org is ZDR, the session user
//! acknowledged that one connection cannot give ZDR, and this run is kept on
//! that connection with ZDR off. Session Core stamps the class from the
//! caller's verified claims when the run starts. Later decisions about the run
//! read this receipt, not the live org posture, which stays owned by Control
//! Plane: the learning review and the proof bundle read the run's own receipt.
//! The external memory mirror of a completed answer follows the run's receipt,
//! and the mirror of an appended message follows the appending caller's posture
//! (a waiver token carries its waiver). Background dreaming excludes any thread
//! that hosts a waived run, because messages carry no run id.
//!
//! A child run inherits its parent's receipt. A caller whose posture does not
//! match its parent's is refused, so a child can never leave the posture its
//! parent was started under.
//!
//! The classes and their strings are the matrix's `retention_classes` in
//! `api/contracts/zdr-posture-matrix.json`.

use chrono::{DateTime, Utc};
use mp_contracts::zdr_refusal::Refusal;
use serde_json::{json, Value};
use sqlx::{PgPool, Postgres, Transaction};
use tonic::{Code, Status};

use crate::auth::VerifiedIdentity;

/// Audit event name of a waived run's start. audit-core accepts it on the
/// subject whose last token is this name (`eventAuthorityMatches`).
pub(crate) const RUN_STARTED_ZDR_WAIVED_EVENT: &str = "run_started_zdr_waived";

/// Subject family of Session Core's flat audit-core bodies. The outbox accepts
/// only rows under this prefix on the audit route (see `audit_publisher`).
const AUDIT_SUBJECT_PREFIX: &str = "verevon.audit.v2.model.session-core.";

/// The retention class of one run. Spelled as `runs.retention_class`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetentionClass {
    Zdr,
    Persistent,
    ZdrWaived,
}

impl RetentionClass {
    /// The stored spelling.
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Zdr => "zdr",
            Self::Persistent => "persistent",
            Self::ZdrWaived => "zdr_waived",
        }
    }

    /// The class a stored value names, or `None` for any other value.
    #[must_use]
    pub(crate) fn from_storage(value: &str) -> Option<Self> {
        match value {
            "zdr" => Some(Self::Zdr),
            "persistent" => Some(Self::Persistent),
            "zdr_waived" => Some(Self::ZdrWaived),
            _ => None,
        }
    }
}

/// The acknowledgement and the one connection a waived run is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WaiverReceipt {
    pub(crate) ack_id: String,
    pub(crate) connection_id: String,
}

/// What a run's receipt says about its retention. `waiver` is present exactly
/// when the class is `zdr_waived`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunRetention {
    pub(crate) class: RetentionClass,
    pub(crate) waiver: Option<WaiverReceipt>,
}

impl RunRetention {
    /// A zero-retention run.
    #[must_use]
    pub(crate) const fn zdr() -> Self {
        Self {
            class: RetentionClass::Zdr,
            waiver: None,
        }
    }

    /// An ordinary persistent run.
    #[must_use]
    pub(crate) const fn persistent() -> Self {
        Self {
            class: RetentionClass::Persistent,
            waiver: None,
        }
    }

    /// A persistent run under the waiver `ack_id` for `connection_id`.
    #[must_use]
    pub(crate) fn waived(ack_id: &str, connection_id: &str) -> Self {
        Self {
            class: RetentionClass::ZdrWaived,
            waiver: Some(WaiverReceipt {
                ack_id: ack_id.to_owned(),
                connection_id: connection_id.to_owned(),
            }),
        }
    }

    /// The receipt the verified caller's own claims give. A ZDR credential is
    /// zero retention; a persistent credential that carries a waiver is waived;
    /// any other persistent credential is ordinary.
    #[must_use]
    pub(crate) fn from_caller(caller: &VerifiedIdentity) -> Self {
        if caller.zdr() {
            return Self::zdr();
        }
        match caller.zdr_waiver() {
            Some(waiver) => Self::waived(waiver.ack_id(), waiver.connection_id()),
            None => Self::persistent(),
        }
    }

    /// Read a stored receipt. The columns must agree with the class: a waived
    /// row names its acknowledgement and connection, and no other row names
    /// either. A row that disagrees is not a receipt this code wrote.
    ///
    /// # Errors
    ///
    /// `internal` when the class is unknown or the waiver columns disagree with
    /// it. Never a silently defaulted class.
    #[allow(clippy::result_large_err)]
    pub(crate) fn from_storage(
        class: &str,
        ack_id: Option<String>,
        connection_id: Option<String>,
    ) -> Result<Self, Status> {
        let class = RetentionClass::from_storage(class)
            .ok_or_else(|| Status::internal("run retention class is invalid"))?;
        match (class, ack_id, connection_id) {
            (RetentionClass::ZdrWaived, Some(ack_id), Some(connection_id)) => Ok(Self {
                class,
                waiver: Some(WaiverReceipt {
                    ack_id,
                    connection_id,
                }),
            }),
            (RetentionClass::ZdrWaived, _, _) => Err(Status::internal(
                "run retention waiver receipt is incomplete",
            )),
            (_, None, None) => Ok(Self {
                class,
                waiver: None,
            }),
            _ => Err(Status::internal(
                "run retention waiver columns disagree with the class",
            )),
        }
    }

    /// The receipt a child run takes. With no parent it is this one. With a
    /// parent it must be the parent's exactly: a child of a waived run is waived
    /// on the same connection and acknowledgement, and a child of a persistent
    /// run is persistent. Any other combination is refused, so a caller can
    /// never start a child that leaves its parent's posture.
    ///
    /// # Errors
    ///
    /// `permission_denied` when the caller's posture differs from the parent's.
    #[allow(clippy::result_large_err)]
    pub(crate) fn inherit_from(self, parent: Option<&Self>) -> Result<Self, Status> {
        match parent {
            None => Ok(self),
            Some(parent) if *parent == self => Ok(self),
            Some(_) => Err(Refusal::POSTURE_MISMATCH.grpc_status(Code::PermissionDenied)),
        }
    }

    /// The acknowledgement a waived run rests on.
    #[must_use]
    pub(crate) fn ack_id(&self) -> Option<&str> {
        self.waiver.as_ref().map(|waiver| waiver.ack_id.as_str())
    }

    /// The connection a waived run is bound to.
    #[must_use]
    pub(crate) fn connection_id(&self) -> Option<&str> {
        self.waiver
            .as_ref()
            .map(|waiver| waiver.connection_id.as_str())
    }
}

/// The posture gate of a durable continuation (owner decision 17). A human caller advances stored work
/// (an approval or rejection, a todo update, a plan decision, a run resume or cancel, a continuation of a
/// stored run) only when its posture is the stored posture of the object it changes, exactly.
///
/// * A ZDR credential is refused as a durable mutation whatever the object holds: a ZDR run keeps no plan,
///   todo or approval to advance, and Session Core refuses every ZDR write.
/// * A persistent or waived credential is refused as a posture mismatch unless its receipt equals the
///   object's receipt. A waiver is valid only for the run started under that same waiver: same
///   connection and same acknowledgement.
/// * A service workload is not held to a user's posture. It acts for the run's owner under its own scope,
///   and its system writes (the terminal outcome of a ZDR run among them) must keep working.
///
/// Every refusal comes before any state change, so the caller checks this before it writes.
///
/// # Errors
/// `failed_precondition` with the durable-mutation refusal for a ZDR credential; `permission_denied` with
/// the posture-mismatch refusal for any other difference.
#[allow(clippy::result_large_err)]
pub(crate) fn require_stored_posture(
    caller: &VerifiedIdentity,
    stored: &RunRetention,
) -> Result<(), Status> {
    if caller.is_service() {
        return Ok(());
    }
    if caller.zdr() {
        return Err(Refusal::DURABLE_MUTATION_FORBIDDEN.grpc_status(Code::FailedPrecondition));
    }
    if RunRetention::from_caller(caller) != *stored {
        return Err(Refusal::POSTURE_MISMATCH.grpc_status(Code::PermissionDenied));
    }
    Ok(())
}

/// The ZDR refusal of a human caller for a durable continuation, decided before any lookup, so a refused
/// call reads and writes nothing of the object it names.
///
/// # Errors
/// `failed_precondition` with the durable-mutation refusal for a ZDR credential.
#[allow(clippy::result_large_err)]
pub(crate) fn refuse_zdr_continuation(caller: &VerifiedIdentity) -> Result<(), Status> {
    if !caller.is_service() && caller.zdr() {
        return Err(Refusal::DURABLE_MUTATION_FORBIDDEN.grpc_status(Code::FailedPrecondition));
    }
    Ok(())
}

/// Whether `run_id` is `user_id`'s run in `org_id` and, for a human caller, whether the caller's posture
/// is that run's receipt. A run that is not the caller's answers `false` and its receipt is never read,
/// so the answer does not disclose another owner's run.
///
/// # Errors
/// `unavailable` when the lookup fails; `permission_denied` with the posture-mismatch refusal when the
/// caller owns the run under another posture.
#[allow(clippy::result_large_err)]
pub(crate) async fn run_owner_matches(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    run_id: &str,
    org_id: &str,
    user_id: &str,
) -> Result<bool, Status> {
    let row: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT retention_class, waiver_ack_id, waiver_connection_id
         FROM runs
         WHERE id = $1 AND org_id = $2 AND user_id = $3",
    )
    .bind(run_id)
    .bind(org_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "durable run ownership lookup failed");
        Status::unavailable("run ownership unavailable")
    })?;
    let Some((class, ack_id, connection_id)) = row else {
        return Ok(false);
    };
    let stored = RunRetention::from_storage(&class, ack_id, connection_id)?;
    require_stored_posture(caller, &stored)?;
    Ok(true)
}

/// The receipt of a stored run, by id. Callers reach this after their own owner check, so a missing run
/// is a fault, not a refusal.
///
/// # Errors
/// `not_found` when no such run exists; `internal` when the lookup fails or the receipt is malformed.
#[allow(clippy::result_large_err)]
pub(crate) async fn stored_run_retention(
    pool: &PgPool,
    run_id: &str,
) -> Result<RunRetention, Status> {
    let row: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT retention_class, waiver_ack_id, waiver_connection_id FROM runs WHERE id = $1",
    )
    .bind(run_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "run retention lookup failed");
        Status::internal("run retention persistence failed")
    })?;
    let (class, ack_id, connection_id) = row.ok_or_else(|| Status::not_found("run not found"))?;
    RunRetention::from_storage(&class, ack_id, connection_id)
}

/// The stored posture of a plan: its run's receipt. A plan with no run carries no receipt and is
/// persistent. A ZDR credential never creates durable state, so a plan without a run was made under
/// ordinary retention.
///
/// # Errors
/// As [`stored_run_retention`].
#[allow(clippy::result_large_err)]
pub(crate) async fn stored_plan_retention(
    pool: &PgPool,
    run_id: Option<&str>,
) -> Result<RunRetention, Status> {
    match run_id.map(str::trim).filter(|id| !id.is_empty()) {
        None => Ok(RunRetention::persistent()),
        Some(run_id) => stored_run_retention(pool, run_id).await,
    }
}

/// The stored posture of a todo: the receipt of the run its plan belongs to, or persistent when the plan
/// has no run.
///
/// # Errors
/// `not_found` when no such todo exists; `internal` when the lookup fails or the receipt is malformed.
#[allow(clippy::result_large_err)]
pub(crate) async fn stored_todo_retention(
    pool: &PgPool,
    todo_id: &str,
) -> Result<RunRetention, Status> {
    let row: Option<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT r.retention_class, r.waiver_ack_id, r.waiver_connection_id
         FROM todos AS t
         JOIN plans AS p ON p.id = t.plan_id
         LEFT JOIN runs AS r ON r.id = p.run_id
         WHERE t.id = $1",
    )
    .bind(todo_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "todo retention lookup failed");
        Status::internal("run retention persistence failed")
    })?;
    let (class, ack_id, connection_id) = row.ok_or_else(|| Status::not_found("todo not found"))?;
    match class {
        None => Ok(RunRetention::persistent()),
        Some(class) => RunRetention::from_storage(&class, ack_id, connection_id),
    }
}

/// The stored posture of an approval: the receipt of its run (an approval always names one). `None` when
/// no approval of `org_id` has that id, or, for a human caller, when the approval is another user's. The
/// caller's own lookup then answers, so another user's run never discloses its posture.
///
/// # Errors
/// `internal` when the lookup fails or the receipt is malformed.
#[allow(clippy::result_large_err)]
pub(crate) async fn stored_approval_retention(
    pool: &PgPool,
    approval_id: &str,
    org_id: &str,
    user_id: Option<&str>,
) -> Result<Option<RunRetention>, Status> {
    let row: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT r.retention_class, r.waiver_ack_id, r.waiver_connection_id
         FROM approvals AS a
         JOIN runs AS r ON r.id = a.run_id
         WHERE a.id = $1 AND a.org_id = $2 AND ($3::text IS NULL OR a.user_id = $3)",
    )
    .bind(approval_id)
    .bind(org_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "approval retention lookup failed");
        Status::internal("run retention persistence failed")
    })?;
    row.map(|(class, ack_id, connection_id)| {
        RunRetention::from_storage(&class, ack_id, connection_id)
    })
    .transpose()
}

/// The durable audit row's identity for a waived run's start. Stable per run,
/// so a replayed start writes no second row.
#[must_use]
pub(crate) fn run_started_zdr_waived_event_id(run_id: &str) -> String {
    format!("{RUN_STARTED_ZDR_WAIVED_EVENT}:session-core:{run_id}")
}

/// The outbox subject of the waived-start audit event.
#[must_use]
pub(crate) fn run_started_zdr_waived_subject() -> String {
    format!("{AUDIT_SUBJECT_PREFIX}{RUN_STARTED_ZDR_WAIVED_EVENT}")
}

/// The flat audit body of a waived run's start, or `None` for any other class.
///
/// Content-free: ids, the class and the time only. The body names the run, the
/// acknowledgement and the connection, which is what the design's audit trail
/// needs. It carries no prompt, goal or message text.
#[must_use]
pub(crate) fn run_started_zdr_waived_body(
    retention: &RunRetention,
    run_id: &str,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
    occurred_at: DateTime<Utc>,
) -> Option<Value> {
    if retention.class != RetentionClass::ZdrWaived {
        return None;
    }
    Some(json!({
        "event_id": run_started_zdr_waived_event_id(run_id),
        "occurred_at": occurred_at.to_rfc3339(),
        "org_id": org_id,
        "user_id": user_id,
        "plane": "model",
        "producer": "session-core",
        "event": RUN_STARTED_ZDR_WAIVED_EVENT,
        "subject": run_id,
        "resource_id": retention.connection_id().unwrap_or_default(),
        "outcome": "ok",
        "details": {
            "retention_class": RetentionClass::ZdrWaived.as_str(),
            "waiver_ack_id": retention.ack_id().unwrap_or_default(),
            "thread_id": thread_id,
        },
    }))
}

/// The receipt of `parent_run_id`, which must be a run of `org_id` owned by
/// `user_id`. A run outside that scope is not found, so a caller cannot probe
/// another tenant's run ids.
///
/// # Errors
///
/// `not_found` when no such run exists in that scope; `internal` when the
/// lookup fails or the stored receipt is malformed.
#[allow(clippy::result_large_err)]
pub(crate) async fn load_parent_retention(
    connection: &mut sqlx::PgConnection,
    org_id: &str,
    user_id: &str,
    parent_run_id: &str,
) -> Result<RunRetention, Status> {
    let row: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT retention_class, waiver_ack_id, waiver_connection_id
         FROM runs
         WHERE id = $1 AND org_id = $2 AND user_id = $3",
    )
    .bind(parent_run_id)
    .bind(org_id)
    .bind(user_id)
    .fetch_optional(connection)
    .await
    .map_err(|error| {
        tracing::error!(%error, "run retention parent lookup failed");
        Status::internal("run retention persistence failed")
    })?;
    let (class, ack_id, connection_id) =
        row.ok_or_else(|| Status::not_found("parent run not found"))?;
    RunRetention::from_storage(&class, ack_id, connection_id)
}

/// Write the durable audit row of a waived run's start on the caller's open
/// transaction, so the row commits with the run or not at all. A row that
/// cannot be written fails the start: a waived run that is not audited is not
/// started.
///
/// # Errors
///
/// `internal` when the row cannot be written.
#[allow(clippy::result_large_err)]
pub(crate) async fn enqueue_run_started_zdr_waived(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    body: &Value,
) -> Result<(), Status> {
    sqlx::query(
        "INSERT INTO session_audit_outbox (event_id, subject, payload)
         VALUES ($1, $2, $3)",
    )
    .bind(run_started_zdr_waived_event_id(run_id))
    .bind(run_started_zdr_waived_subject())
    .bind(body)
    .execute(&mut **transaction)
    .await
    .map_err(|error| {
        tracing::error!(%error, "run_started_zdr_waived audit row could not be written");
        Status::internal("run retention persistence failed")
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit_publisher::{audit_subject_for_event, classify_outbox_subject, OutboxRoute};
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 10, 12, 0, 0).unwrap()
    }

    #[test]
    fn class_strings_are_the_matrix_retention_classes() {
        assert_eq!(RetentionClass::Zdr.as_str(), "zdr");
        assert_eq!(RetentionClass::Persistent.as_str(), "persistent");
        assert_eq!(RetentionClass::ZdrWaived.as_str(), "zdr_waived");
        for class in [
            RetentionClass::Zdr,
            RetentionClass::Persistent,
            RetentionClass::ZdrWaived,
        ] {
            assert_eq!(RetentionClass::from_storage(class.as_str()), Some(class));
        }
        assert_eq!(RetentionClass::from_storage("waived"), None);
        assert_eq!(RetentionClass::from_storage(""), None);
    }

    #[test]
    fn a_caller_gives_its_own_receipt() {
        assert_eq!(
            RunRetention::from_caller(&VerifiedIdentity::user_for_test("org-1", "user-1")),
            RunRetention::persistent()
        );
        assert_eq!(
            RunRetention::from_caller(&VerifiedIdentity::user_for_test_with_zdr(
                "org-1", "user-1", true
            )),
            RunRetention::zdr()
        );
        assert_eq!(
            RunRetention::from_caller(&VerifiedIdentity::user_for_test_with_waiver(
                "org-1", "user-1", "conn-1", "ack-1"
            )),
            RunRetention::waived("ack-1", "conn-1")
        );
    }

    #[test]
    fn a_waived_receipt_names_its_acknowledgement_and_connection_and_nothing_else_names_them() {
        let waived =
            RunRetention::from_storage("zdr_waived", Some("ack-1".into()), Some("conn-1".into()))
                .expect("a complete waived row reads back");
        assert_eq!(waived, RunRetention::waived("ack-1", "conn-1"));
        assert_eq!(waived.ack_id(), Some("ack-1"));
        assert_eq!(waived.connection_id(), Some("conn-1"));

        assert!(RunRetention::from_storage("zdr_waived", None, Some("conn-1".into())).is_err());
        assert!(RunRetention::from_storage("zdr_waived", Some("ack-1".into()), None).is_err());
        assert!(RunRetention::from_storage("persistent", Some("ack-1".into()), None).is_err());
        assert!(RunRetention::from_storage("zdr", None, Some("conn-1".into())).is_err());
        assert_eq!(
            RunRetention::from_storage("persistent", None, None).expect("an ordinary row"),
            RunRetention::persistent()
        );
        assert_eq!(
            RunRetention::from_storage("zdr", None, None).expect("a ZDR row"),
            RunRetention::zdr()
        );
        assert!(RunRetention::from_storage("unknown", None, None).is_err());
    }

    /// The waiver-binding rule: a child is only ever started under its parent's
    /// exact receipt. Each mismatch is a refusal, never a silent re-class.
    #[test]
    fn a_child_run_inherits_its_parents_exact_receipt_and_nothing_else() {
        let waived_a = RunRetention::waived("ack-a", "conn-a");
        let waived_b = RunRetention::waived("ack-b", "conn-b");
        let persistent = RunRetention::persistent();

        assert_eq!(
            waived_a
                .clone()
                .inherit_from(Some(&waived_a))
                .expect("same waiver inherits"),
            waived_a
        );
        assert_eq!(
            persistent
                .clone()
                .inherit_from(Some(&persistent))
                .expect("persistent inherits"),
            persistent
        );
        assert_eq!(
            waived_a
                .clone()
                .inherit_from(None)
                .expect("no parent keeps its own"),
            waived_a
        );

        for (own, parent) in [
            (waived_b.clone(), &waived_a),
            (persistent.clone(), &waived_a),
            (waived_a.clone(), &persistent),
            (RunRetention::zdr(), &waived_a),
        ] {
            assert_eq!(
                own.clone()
                    .inherit_from(Some(parent))
                    .expect_err("a different posture is refused")
                    .code(),
                tonic::Code::PermissionDenied,
                "{own:?} under {parent:?}"
            );
        }
    }

    #[test]
    fn the_waived_start_audit_body_is_content_free_and_pins_the_event_identity() {
        let waived = RunRetention::waived("ack-1", "conn-1");
        let body =
            run_started_zdr_waived_body(&waived, "run-1", "org-1", "user-1", "thread-1", at())
                .expect("a waived start has an audit body");

        assert_eq!(body["event"], "run_started_zdr_waived");
        assert_eq!(body["plane"], "model");
        assert_eq!(body["producer"], "session-core");
        assert_eq!(
            body["event_id"],
            "run_started_zdr_waived:session-core:run-1"
        );
        assert_eq!(body["subject"], "run-1");
        assert_eq!(body["resource_id"], "conn-1");
        assert_eq!(body["details"]["retention_class"], "zdr_waived");
        assert_eq!(body["details"]["waiver_ack_id"], "ack-1");
        assert_eq!(body["details"]["thread_id"], "thread-1");
        let serialized = body.to_string();
        assert!(
            !serialized.contains("goal"),
            "no goal text in the audit body"
        );
        assert!(
            !serialized.contains("prompt"),
            "no prompt in the audit body"
        );

        for class in [RunRetention::zdr(), RunRetention::persistent()] {
            assert!(
                run_started_zdr_waived_body(&class, "run-1", "org-1", "user-1", "thread-1", at())
                    .is_none(),
                "only a waived start is audited under this event"
            );
        }
    }

    /// The row must be published by the drainer, not refused as outside its
    /// authority: the subject classifies as the audit route and the wire subject
    /// the drainer derives from the body's event is the subject the row stores.
    #[test]
    fn the_waived_start_row_is_routed_and_published_on_its_own_subject() {
        let subject = run_started_zdr_waived_subject();
        assert!(matches!(
            classify_outbox_subject(&subject),
            Some(OutboxRoute::Audit)
        ));
        assert_eq!(
            audit_subject_for_event(RUN_STARTED_ZDR_WAIVED_EVENT).as_deref(),
            Some(subject.as_str())
        );
        let event_id = run_started_zdr_waived_event_id("run-1");
        assert!(
            event_id.len() <= 128,
            "the outbox event_id column is 128 bytes"
        );
        assert!(
            event_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte)),
            "audit-core's stable event id pattern"
        );
    }

    /// A waived run keeps its reads and its own local rows, as an ordinary run
    /// does. Its content never reaches the external memory service, and a ZDR run
    /// reaches neither.
    #[test]
    fn a_waived_run_keeps_reads_and_local_rows_but_never_external_indexing() {
        use crate::grpc::MemoryRetention;
        let waived =
            VerifiedIdentity::user_for_test_with_waiver("org-1", "user-1", "conn-1", "ack-1");
        let retention = MemoryRetention::of(&waived);
        assert_eq!(retention, MemoryRetention::WaivedRun);
        assert!(retention.permits_durable_memory());
        assert!(!retention.permits_external_memory_indexing());

        let ordinary = MemoryRetention::of(&VerifiedIdentity::user_for_test("org-1", "user-1"));
        assert!(ordinary.permits_durable_memory());
        assert!(ordinary.permits_external_memory_indexing());

        let zdr = MemoryRetention::of(&VerifiedIdentity::user_for_test_with_zdr(
            "org-1", "user-1", true,
        ));
        assert!(!zdr.permits_durable_memory());
        assert!(!zdr.permits_external_memory_indexing());
    }

    /// The refusal a status carries, read the way a client reads it.
    fn refusal_code(status: &Status) -> Option<&'static str> {
        mp_contracts::zdr_refusal::Refusal::from_status(status).map(|refusal| refusal.code())
    }

    /// Owner decision 17: a human caller advances durable work on a stored object only when its
    /// posture is the object's receipt exactly. A ZDR credential is refused as a durable mutation
    /// whatever the object holds. A waiver is valid only for the run started under that same waiver.
    #[test]
    fn a_human_caller_must_hold_exactly_the_stored_posture() {
        use mp_contracts::zdr_refusal::code;
        let persistent_user = VerifiedIdentity::user_for_test("org-1", "user-1");
        let zdr_user = VerifiedIdentity::user_for_test_with_zdr("org-1", "user-1", true);
        let waived_user =
            VerifiedIdentity::user_for_test_with_waiver("org-1", "user-1", "conn-1", "ack-1");
        let stored_persistent = RunRetention::persistent();
        let stored_zdr = RunRetention::zdr();
        let stored_waived = RunRetention::waived("ack-1", "conn-1");

        assert!(require_stored_posture(&persistent_user, &stored_persistent).is_ok());
        assert!(require_stored_posture(&waived_user, &stored_waived).is_ok());

        for stored in [&stored_persistent, &stored_zdr, &stored_waived] {
            let status = require_stored_posture(&zdr_user, stored)
                .expect_err("a ZDR credential never advances durable work");
            assert_eq!(status.code(), Code::FailedPrecondition);
            assert_eq!(
                refusal_code(&status),
                Some(code::DURABLE_MUTATION_FORBIDDEN)
            );
        }

        let persistent_against_zdr = require_stored_posture(&persistent_user, &stored_zdr)
            .expect_err("a persistent credential may not attach to a ZDR run");
        assert_eq!(persistent_against_zdr.code(), Code::PermissionDenied);
        assert_eq!(
            refusal_code(&persistent_against_zdr),
            Some(code::POSTURE_MISMATCH)
        );

        let persistent_against_waived = require_stored_posture(&persistent_user, &stored_waived)
            .expect_err("a persistent credential may not attach to a waived run");
        assert_eq!(
            refusal_code(&persistent_against_waived),
            Some(code::POSTURE_MISMATCH)
        );

        let waiver_against_persistent = require_stored_posture(&waived_user, &stored_persistent)
            .expect_err("a waiver may not attach itself to a persistent run");
        assert_eq!(
            refusal_code(&waiver_against_persistent),
            Some(code::POSTURE_MISMATCH)
        );

        let other_connection = RunRetention::waived("ack-1", "conn-2");
        let waiver_on_another_connection = require_stored_posture(&waived_user, &other_connection)
            .expect_err("a waiver for one connection may not continue a run of another");
        assert_eq!(
            refusal_code(&waiver_on_another_connection),
            Some(code::POSTURE_MISMATCH)
        );

        let other_acknowledgement = RunRetention::waived("ack-2", "conn-1");
        let waiver_under_another_ack = require_stored_posture(&waived_user, &other_acknowledgement)
            .expect_err("a waiver is bound to its acknowledgement too");
        assert_eq!(
            refusal_code(&waiver_under_another_ack),
            Some(code::POSTURE_MISMATCH)
        );
    }

    /// A service workload acts for the run's owner under its own scope. Its system writes, such as
    /// the terminal outcome of a ZDR run, are not held to a user's posture.
    #[test]
    fn a_service_workload_is_not_held_to_a_users_posture() {
        let service = VerifiedIdentity::service_for_test("org-1", &["session:write"], false);
        assert!(require_stored_posture(&service, &RunRetention::zdr()).is_ok());
        assert!(require_stored_posture(&service, &RunRetention::waived("ack-1", "conn-1")).is_ok());
        assert!(require_stored_posture(&service, &RunRetention::persistent()).is_ok());
    }

    /// A child of a run under another posture is refused as a typed posture mismatch, not as an
    /// untyped permission denial.
    #[test]
    fn a_child_under_another_posture_is_a_typed_posture_refusal() {
        let parent = RunRetention::persistent();
        let status = RunRetention::zdr()
            .inherit_from(Some(&parent))
            .expect_err("a child may not leave its parent's posture");
        assert_eq!(status.code(), Code::PermissionDenied);
        assert_eq!(
            refusal_code(&status),
            Some(mp_contracts::zdr_refusal::code::POSTURE_MISMATCH)
        );
        assert!(RunRetention::persistent()
            .inherit_from(Some(&parent))
            .is_ok());
    }
}

/// Postgres-backed contract tests for the receipt. Each one applies the real
/// migrator, then drives the real start, learning and dreaming paths against a
/// scratch database. They are `#[ignore]`d so a local `cargo test` never needs a
/// database; the session-core receipts CI job runs them with `--ignored` and
/// `REQUIRE_CONTRACT=1`, where a missing `DATABASE_URL` fails rather than skips.
#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::dreaming;
    use crate::grpc::start_run_inner;
    use crate::learning_events;
    use crate::orchestration_store;
    use crate::terminalization::start_managed_run_with_retention_inner;
    use chrono::Utc;
    use mp_contracts::model_plane::v1 as pb;
    use mp_ids::new_ulid;
    use sqlx::PgPool;
    use tonic::Code;

    /// The scratch Postgres these tests write to. Without `DATABASE_URL` they
    /// skip locally; when `REQUIRE_CONTRACT` or `CI` is set they fail instead. The
    /// database name must say it is a test database, because these tests write rows.
    async fn scratch_pool() -> Option<PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            assert!(
                std::env::var_os("REQUIRE_CONTRACT").is_none() && std::env::var_os("CI").is_none(),
                "DATABASE_URL must name a scratch Postgres when REQUIRE_CONTRACT or CI is set"
            );
            eprintln!("skipping: DATABASE_URL unset");
            return None;
        };
        let database = url
            .rsplit('/')
            .next()
            .and_then(|tail| tail.split('?').next())
            .unwrap_or_default();
        assert!(
            database.contains("test"),
            "these tests write rows: DATABASE_URL must name a scratch database whose name contains \"test\""
        );
        let pool = PgPool::connect(&url)
            .await
            .expect("scratch Postgres is reachable");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("session-core migrations apply in order");
        Some(pool)
    }

    struct Seeded {
        org: String,
        user: String,
        thread: String,
    }

    async fn seeded_thread(pool: &PgPool, tag: &str) -> Seeded {
        let suffix = new_ulid();
        let seeded = Seeded {
            org: format!("receipt-{tag}-org-{suffix}"),
            user: format!("receipt-{tag}-user-{suffix}"),
            thread: new_ulid(),
        };
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id, created_at)
             VALUES ($1, $2, $3, $4, now())",
        )
        .bind(&seeded.thread)
        .bind(format!("receipt-{suffix}"))
        .bind(&seeded.org)
        .bind(&seeded.user)
        .execute(pool)
        .await
        .expect("thread seeded");
        seeded
    }

    fn managed_request(
        seeded: &Seeded,
        start_key: &str,
        parent_run_id: &str,
    ) -> pb::StartManagedRunRequest {
        pb::StartManagedRunRequest {
            wake: None,
            thread_id: seeded.thread.clone(),
            parent_run_id: parent_run_id.to_owned(),
            agent_id: "general-v1".to_owned(),
            goal: "receipt goal".to_owned(),
            mode: "execute".to_owned(),
            org_id: seeded.org.clone(),
            user_id: seeded.user.clone(),
            start_key: start_key.to_owned(),
            terminal_source: pb::ManagedRunSource::GatewayDirect as i32,
            agent_version_pin: None,
        }
    }

    async fn receipt_of(pool: &PgPool, run_id: &str) -> (String, Option<String>, Option<String>) {
        sqlx::query_as(
            "SELECT retention_class, waiver_ack_id, waiver_connection_id FROM runs WHERE id = $1",
        )
        .bind(run_id)
        .fetch_one(pool)
        .await
        .expect("the run row exists")
    }

    async fn waiver_audit_rows(pool: &PgPool, run_id: &str) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM session_audit_outbox WHERE event_id = $1 AND subject = $2",
        )
        .bind(run_started_zdr_waived_event_id(run_id))
        .bind(run_started_zdr_waived_subject())
        .fetch_one(pool)
        .await
        .expect("audit rows counted")
    }

    fn waived_receipt(ack: &str, connection: &str) -> (String, Option<String>, Option<String>) {
        (
            "zdr_waived".to_owned(),
            Some(ack.to_owned()),
            Some(connection.to_owned()),
        )
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\" (session-core receipts CI job)"]
    async fn zdr_receipt_waived_start_persists_thread_receipt_and_audit_row() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let seeded = seeded_thread(&pool, "waived").await;
        let started = start_managed_run_with_retention_inner(
            &pool,
            managed_request(&seeded, "start-waived-1", ""),
            RunRetention::waived("ack-1", "conn-1"),
        )
        .await
        .expect("a waived start is accepted");

        assert_eq!(
            started.thread_id, seeded.thread,
            "a waived run persists in the user's own thread"
        );
        assert!(!started.already_started);
        assert_eq!(
            receipt_of(&pool, &started.run_id).await,
            waived_receipt("ack-1", "conn-1"),
            "the receipt is recorded on the run"
        );
        assert_eq!(
            waiver_audit_rows(&pool, &started.run_id).await,
            1,
            "exactly one durable audit row for the waived start"
        );
        let payload: serde_json::Value =
            sqlx::query_scalar("SELECT payload FROM session_audit_outbox WHERE event_id = $1")
                .bind(run_started_zdr_waived_event_id(&started.run_id))
                .fetch_one(&pool)
                .await
                .expect("the audit payload");
        assert_eq!(payload["plane"], "model");
        assert_eq!(payload["details"]["waiver_ack_id"], "ack-1");
        assert!(
            !payload.to_string().contains("receipt goal"),
            "the audit body is content-free"
        );

        let (configured,): (String,) = sqlx::query_as(
            "SELECT configured_source FROM managed_run_terminalization_outbox WHERE run_id = $1",
        )
        .bind(&started.run_id)
        .fetch_one(&pool)
        .await
        .expect("the terminalization obligation is installed as for any run");
        assert_eq!(configured, "gateway_direct");

        let provenance =
            orchestration_store::get_run_provenance_for_org(&pool, &started.run_id, &seeded.org)
                .await
                .expect("provenance reads")
                .expect("the run is visible to its own org");
        assert_eq!(provenance.retention_class, "zdr_waived");
        assert_eq!(provenance.waiver_ack_id.as_deref(), Some("ack-1"));
        assert_eq!(provenance.waiver_connection_id.as_deref(), Some("conn-1"));
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\" (session-core receipts CI job)"]
    async fn zdr_receipt_a_waiver_cannot_adopt_another_connections_run_or_start_a_child_of_it() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let seeded = seeded_thread(&pool, "adopt").await;
        let first = start_managed_run_with_retention_inner(
            &pool,
            managed_request(&seeded, "start-adopt", ""),
            RunRetention::waived("ack-1", "conn-1"),
        )
        .await
        .expect("the first waived start");

        // A retry under another connection's waiver, or none, is not the same start.
        for other in [
            RunRetention::waived("ack-2", "conn-2"),
            RunRetention::persistent(),
        ] {
            let error = start_managed_run_with_retention_inner(
                &pool,
                managed_request(&seeded, "start-adopt", ""),
                other.clone(),
            )
            .await
            .expect_err("a different receipt must not adopt the run");
            assert_eq!(error.code(), Code::AlreadyExists, "{other:?}");
        }
        // The exact retry is the same run, not a second one.
        let retry = start_managed_run_with_retention_inner(
            &pool,
            managed_request(&seeded, "start-adopt", ""),
            RunRetention::waived("ack-1", "conn-1"),
        )
        .await
        .expect("the exact retry");
        assert!(retry.already_started);
        assert_eq!(retry.run_id, first.run_id);

        // A child of that run cannot be started under another connection's
        // waiver, nor by a persistent caller: it would leave the parent's posture.
        for caller in [
            RunRetention::waived("ack-2", "conn-2"),
            RunRetention::persistent(),
        ] {
            let key = format!("child-{}", caller.class.as_str());
            let error = start_managed_run_with_retention_inner(
                &pool,
                managed_request(&seeded, &key, &first.run_id),
                caller.clone(),
            )
            .await
            .expect_err("a child may not leave its parent's posture");
            assert_eq!(error.code(), Code::PermissionDenied, "{caller:?}");
        }
        let (refused,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM managed_run_terminalization_outbox
             WHERE org_id = $1 AND start_key LIKE 'child-%'",
        )
        .bind(&seeded.org)
        .fetch_one(&pool)
        .await
        .expect("refused children counted");
        assert_eq!(
            refused, 0,
            "a refused child writes no run and no obligation"
        );

        // The same waiver inherits: the child carries the parent's receipt.
        let child = start_managed_run_with_retention_inner(
            &pool,
            managed_request(&seeded, "child-waived", &first.run_id),
            RunRetention::waived("ack-1", "conn-1"),
        )
        .await
        .expect("the same waiver starts a child");
        assert_eq!(
            receipt_of(&pool, &child.run_id).await,
            waived_receipt("ack-1", "conn-1"),
            "a child inherits its parent's receipt"
        );

        // A parent in another org is not found: a caller cannot probe other tenants' runs.
        let other = seeded_thread(&pool, "elsewhere").await;
        let foreign = start_managed_run_with_retention_inner(
            &pool,
            managed_request(&other, "foreign-parent", ""),
            RunRetention::persistent(),
        )
        .await
        .expect("a run in another org");
        let error = start_managed_run_with_retention_inner(
            &pool,
            managed_request(&seeded, "probe", &foreign.run_id),
            RunRetention::persistent(),
        )
        .await
        .expect_err("another org's run is not a parent");
        assert_eq!(error.code(), Code::NotFound);
        // The same org's run owned by another user is not a parent either: the
        // lookup is scoped to the caller's user as well as to the org.
        let peer = Seeded {
            org: seeded.org.clone(),
            user: format!("receipt-adopt-peer-{}", new_ulid()),
            thread: new_ulid(),
        };
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id, created_at)
             VALUES ($1, $2, $3, $4, now())",
        )
        .bind(&peer.thread)
        .bind(format!("receipt-peer-{}", peer.thread))
        .bind(&peer.org)
        .bind(&peer.user)
        .execute(&pool)
        .await
        .expect("peer thread seeded");
        let peer_run = start_managed_run_with_retention_inner(
            &pool,
            managed_request(&peer, "peer-parent", ""),
            RunRetention::persistent(),
        )
        .await
        .expect("a peer run in the same org");
        let error = start_managed_run_with_retention_inner(
            &pool,
            managed_request(&seeded, "probe-peer", &peer_run.run_id),
            RunRetention::persistent(),
        )
        .await
        .expect_err("another user's run in the same org is not a parent");
        assert_eq!(error.code(), Code::NotFound);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\" (session-core receipts CI job)"]
    async fn zdr_receipt_zdr_start_stays_metadata_only_and_records_the_zdr_class() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let seeded = seeded_thread(&pool, "zdr").await;
        let mut request = managed_request(&seeded, "start-zdr", "");
        // A ZDR start names no thread: Session Core creates a fresh metadata-only one.
        request.thread_id = String::new();
        let started = start_managed_run_with_retention_inner(&pool, request, RunRetention::zdr())
            .await
            .expect("a ZDR start is accepted");

        assert_ne!(
            started.thread_id, seeded.thread,
            "a ZDR run gets a fresh metadata-only thread"
        );
        assert_eq!(
            receipt_of(&pool, &started.run_id).await,
            ("zdr".to_owned(), None, None),
            "a ZDR run's receipt names no waiver"
        );
        assert_eq!(waiver_audit_rows(&pool, &started.run_id).await, 0);
        let (zdr,): (bool,) =
            sqlx::query_as("SELECT zdr FROM managed_run_terminalization_outbox WHERE run_id = $1")
                .bind(&started.run_id)
                .fetch_one(&pool)
                .await
                .expect("the obligation");
        assert!(zdr, "the terminalization obligation keeps the ZDR flag");

        let error = start_managed_run_with_retention_inner(
            &pool,
            managed_request(&seeded, "start-zdr-threaded", ""),
            RunRetention::zdr(),
        )
        .await
        .expect_err("a ZDR start that names a thread is refused");
        assert_eq!(error.code(), Code::FailedPrecondition);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\" (session-core receipts CI job)"]
    async fn zdr_receipt_legacy_start_run_records_the_waiver_receipt_and_its_audit_row() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let seeded = seeded_thread(&pool, "legacy").await;
        let request = || pb::StartRunRequest {
            thread_id: seeded.thread.clone(),
            parent_run_id: String::new(),
            agent_id: "general-v1".to_owned(),
            goal: "legacy receipt".to_owned(),
            mode: "execute".to_owned(),
            org_id: seeded.org.clone(),
            user_id: seeded.user.clone(),
        };
        let response = start_run_inner(
            &pool,
            request(),
            None,
            RunRetention::waived("ack-legacy", "conn-legacy"),
        )
        .await
        .expect("the legacy StartRun records the receipt too")
        .into_inner();
        assert_eq!(
            receipt_of(&pool, &response.run_id).await,
            waived_receipt("ack-legacy", "conn-legacy")
        );
        assert_eq!(waiver_audit_rows(&pool, &response.run_id).await, 1);

        // A requested run id is the persistent scheduled lane only.
        let error = start_run_inner(
            &pool,
            request(),
            Some(new_ulid()),
            RunRetention::waived("ack-legacy", "conn-legacy"),
        )
        .await
        .expect_err("a waived run may not claim a scheduled run id");
        assert_eq!(error.code(), Code::PermissionDenied);
    }

    async fn count_outbox(pool: &PgPool, event_id: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM session_audit_outbox WHERE event_id = $1")
            .bind(event_id)
            .fetch_one(pool)
            .await
            .expect("outbox rows counted")
    }

    async fn count_dream_runs(pool: &PgPool, thread_id: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM dream_runs WHERE thread_id = $1")
            .bind(thread_id)
            .fetch_one(pool)
            .await
            .expect("dream runs counted")
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\" (session-core receipts CI job)"]
    async fn zdr_receipt_waived_run_is_withheld_from_learning_and_dreaming() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let waived_thread = seeded_thread(&pool, "withheld").await;
        let waived = start_managed_run_with_retention_inner(
            &pool,
            managed_request(&waived_thread, "withheld-start", ""),
            RunRetention::waived("ack-w", "conn-w"),
        )
        .await
        .expect("the waived start");
        let ordinary_thread = seeded_thread(&pool, "announced").await;
        let ordinary = start_managed_run_with_retention_inner(
            &pool,
            managed_request(&ordinary_thread, "announced-start", ""),
            RunRetention::persistent(),
        )
        .await
        .expect("the ordinary start");
        for thread in [&waived_thread, &ordinary_thread] {
            sqlx::query(
                "INSERT INTO messages (id, thread_id, role, content) VALUES ($1, $2, 'user', $3)",
            )
            .bind(new_ulid())
            .bind(&thread.thread)
            .bind("Husk at jeg foretrekker mørk modus.")
            .execute(&pool)
            .await
            .expect("message appended");
        }

        // Learning: a waived run's RUN_COMPLETED is withheld; the ordinary run's is announced.
        for run in [&waived.run_id, &ordinary.run_id] {
            let mut tx = pool.begin().await.expect("transaction");
            learning_events::enqueue_run_completed(&mut tx, run, "evt-terminal", Utc::now()).await;
            tx.commit().await.expect("commit");
        }
        assert_eq!(
            count_outbox(
                &pool,
                &learning_events::run_completed_event_id(&waived.run_id)
            )
            .await,
            0,
            "a waived run is never announced to the review path"
        );
        assert_eq!(
            count_outbox(
                &pool,
                &learning_events::run_completed_event_id(&ordinary.run_id)
            )
            .await,
            1,
            "the ordinary control run is announced"
        );

        // Dreaming: the waived thread is not derived at all; the ordinary thread is.
        dreaming::dream_once(&pool, None, None, None, 10_000)
            .await
            .expect("a dream cycle runs");
        assert_eq!(
            count_dream_runs(&pool, &waived_thread.thread).await,
            0,
            "no background derivation runs on a waived thread"
        );
        assert!(
            count_dream_runs(&pool, &ordinary_thread.thread).await >= 1,
            "the ordinary control thread is still dreamt"
        );
    }

    /// Grounding compares a new candidate with existing memory and sends the matches to a
    /// paid classifier. A memory row that a waived run's thread wrote is never a match,
    /// whichever thread the candidate came from (decision D-B).
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\" (session-core receipts CI job)"]
    async fn zdr_receipt_grounding_pool_never_carries_a_waived_threads_memory() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let waived_seed = seeded_thread(&pool, "grounding").await;
        let waived = start_managed_run_with_retention_inner(
            &pool,
            managed_request(&waived_seed, "grounding-waived-start", ""),
            RunRetention::waived("ack-g", "conn-g"),
        )
        .await
        .expect("the waived start");
        let ordinary = Seeded {
            org: waived_seed.org.clone(),
            user: waived_seed.user.clone(),
            thread: new_ulid(),
        };
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id, created_at)
             VALUES ($1, $2, $3, $4, now())",
        )
        .bind(&ordinary.thread)
        .bind(format!("receipt-grounding-{}", ordinary.thread))
        .bind(&ordinary.org)
        .bind(&ordinary.user)
        .execute(&pool)
        .await
        .expect("ordinary thread seeded");

        let waived_memory = crate::dreaming::index_agent_memory(
            &pool,
            &waived_seed.org,
            &waived.thread_id,
            &waived_seed.user,
            "USER",
            "Waived fact: the private detail of the waived run.",
        )
        .await
        .expect("the waived thread's memory is saved locally");
        let ordinary_memory = crate::dreaming::index_agent_memory(
            &pool,
            &waived_seed.org,
            &ordinary.thread,
            &waived_seed.user,
            "USER",
            "Ordinary fact: a preference from an ordinary thread.",
        )
        .await
        .expect("the ordinary thread's memory is saved");
        assert!(!waived_memory.is_empty(), "the waived memory row exists");
        assert!(
            !ordinary_memory.is_empty(),
            "the ordinary memory row exists"
        );

        let candidate = crate::dreaming::DreamMemoryCandidate {
            scope: "user",
            session_id: None,
            key: "manual:user:new-candidate".to_owned(),
            content: "A new fact about the same user.".to_owned(),
            kind: "fact",
            confidence: 0.9,
            inferred: false,
        };
        let rows = crate::dreaming::grounding_candidate_pool(
            &pool,
            &waived_seed.org,
            &waived_seed.user,
            &candidate,
        )
        .await
        .expect("the grounding pool reads");
        let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
        assert!(
            ids.contains(&ordinary_memory),
            "an ordinary thread's memory is still grounding context"
        );
        assert!(
            !ids.contains(&waived_memory),
            "a waived thread's memory never reaches the grounding classifier"
        );
    }

    /// The migration's revert removes exactly what the migration adds. Both are
    /// checked inside transactions that are rolled back, so the shared scratch
    /// database keeps its receipts.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\" (session-core receipts CI job)"]
    async fn zdr_receipt_is_immutable_and_the_revert_is_the_inverse_of_the_migration() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let seeded = seeded_thread(&pool, "immutable").await;
        let run = start_managed_run_with_retention_inner(
            &pool,
            managed_request(&seeded, "immutable-start", ""),
            RunRetention::waived("ack-i", "conn-i"),
        )
        .await
        .expect("the waived start")
        .run_id;

        let error = sqlx::query(
            "UPDATE runs SET retention_class = 'persistent', waiver_ack_id = NULL,
                             waiver_connection_id = NULL
             WHERE id = $1",
        )
        .bind(&run)
        .execute(&pool)
        .await
        .expect_err("the receipt cannot be rewritten");
        assert!(error.to_string().contains("immutable"), "{error}");
        assert_eq!(
            receipt_of(&pool, &run).await,
            waived_receipt("ack-i", "conn-i")
        );
        sqlx::query("UPDATE runs SET goal = goal WHERE id = $1")
            .bind(&run)
            .execute(&pool)
            .await
            .expect("ordinary updates still work");

        let revert = strip_transaction_control(include_str!(
            "../migrations/revert/0044_run_retention_receipt.down.sql"
        ));
        let mut tx = pool.begin().await.expect("transaction");
        sqlx::raw_sql(&revert)
            .execute(&mut *tx)
            .await
            .expect("the revert applies");
        let (remaining,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM information_schema.columns
             WHERE table_name = 'runs'
               AND column_name IN ('retention_class', 'waiver_ack_id', 'waiver_connection_id')",
        )
        .fetch_one(&mut *tx)
        .await
        .expect("columns counted");
        assert_eq!(remaining, 0, "the revert removes every receipt column");
        tx.rollback().await.expect("the revert is rolled back");

        let mut tx = pool.begin().await.expect("transaction");
        sqlx::raw_sql(include_str!("../migrations/0044_run_retention_receipt.sql"))
            .execute(&mut *tx)
            .await
            .expect("the migration re-applies to a database it already changed");
        tx.rollback().await.expect("rolled back");
        assert_eq!(
            receipt_of(&pool, &run).await,
            waived_receipt("ack-i", "conn-i"),
            "the receipt survives both rolled-back transactions"
        );
    }

    /// The gateway completes a run with its answer through a service credential.
    /// That credential carries no waiver, so the answer's memory mirror must
    /// follow the run's own receipt: a waived run's answer is never mirrored
    /// externally, an ordinary run's is.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\" (session-core receipts CI job)"]
    async fn zdr_receipt_direct_completion_mirrors_the_answer_under_the_run_receipt() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let waived_thread = seeded_thread(&pool, "answer-waived").await;
        let waived = start_managed_run_with_retention_inner(
            &pool,
            managed_request(&waived_thread, "answer-waived-start", ""),
            RunRetention::waived("ack-a", "conn-a"),
        )
        .await
        .expect("the waived start");
        let completion =
            |run_id: &str, thread_id: &str, key: &str| pb::CompleteDirectRunWithMessageRequest {
                run_id: run_id.to_owned(),
                thread_id: thread_id.to_owned(),
                content: "Husk at jeg foretrekker mørk modus.".to_owned(),
                idempotency_key: key.to_owned(),
                ..Default::default()
            };
        let answered = crate::terminalization::complete_direct_run_with_message_inner(
            &pool,
            completion(&waived.run_id, &waived.thread_id, "answer-waived-1"),
        )
        .await
        .expect("the waived run completes with its answer");
        assert_eq!(
            answered.memory_retention,
            crate::grpc::MemoryRetention::WaivedRun,
            "a waived run's answer memories never reach the external memory service"
        );

        let ordinary_thread = seeded_thread(&pool, "answer-ordinary").await;
        let ordinary = start_managed_run_with_retention_inner(
            &pool,
            managed_request(&ordinary_thread, "answer-ordinary-start", ""),
            RunRetention::persistent(),
        )
        .await
        .expect("the ordinary start");
        let answered = crate::terminalization::complete_direct_run_with_message_inner(
            &pool,
            completion(&ordinary.run_id, &ordinary.thread_id, "answer-ordinary-1"),
        )
        .await
        .expect("the ordinary run completes with its answer");
        assert_eq!(
            answered.memory_retention,
            crate::grpc::MemoryRetention::Durable
        );
    }

    /// The migration stamps the runs that were started under zero retention before
    /// the receipt existed, and leaves every other run alone. The backfill is the
    /// statement from the migration file itself, run inside a transaction that
    /// drops the trigger first, as the migration does before it stamps. The
    /// transaction is rolled back.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\" (session-core receipts CI job)"]
    async fn zdr_receipt_backfill_stamps_pre_existing_zdr_runs_and_nothing_else() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let seeded = seeded_thread(&pool, "backfill").await;
        let zdr_run = new_ulid();
        let ordinary_run = new_ulid();
        // Seeded inside the transaction that is rolled back below. A committed seed
        // would stay in the shared scratch database and break the re-apply test.
        let mut tx = pool.begin().await.expect("transaction");
        for (run, zdr, start_key) in [
            (&zdr_run, true, "backfill-zdr"),
            (&ordinary_run, false, "backfill-ordinary"),
        ] {
            sqlx::query(
                "INSERT INTO runs (id, thread_id, goal, org_id, user_id) VALUES ($1, $2, 'backfill', $3, $4)",
            )
            .bind(run)
            .bind(&seeded.thread)
            .bind(&seeded.org)
            .bind(&seeded.user)
            .execute(&mut *tx)
            .await
            .expect("a pre-receipt run row");
            sqlx::query(
                "INSERT INTO managed_run_terminalization_outbox
                 (run_id, org_id, user_id, start_key, configured_source,
                  configured_terminal_step_id, zdr, deadline_at)
                 VALUES ($1, $2, $3, $4, 'gateway_direct',
                         'model-gateway-direct-inference-final', $5,
                         now() + interval '15 minutes')",
            )
            .bind(run)
            .bind(&seeded.org)
            .bind(&seeded.user)
            .bind(start_key)
            .bind(zdr)
            .execute(&mut *tx)
            .await
            .expect("the obligation the run was started under");
        }

        let migration = include_str!("../migrations/0044_run_retention_receipt.sql");
        let start = migration
            .find("UPDATE runs AS r")
            .expect("the migration carries the backfill");
        let end = start
            + migration[start..]
                .find(';')
                .expect("the backfill is one statement")
            + 1;
        let backfill = &migration[start..end];

        sqlx::query("DROP TRIGGER runs_retention_receipt_immutable_trg ON runs")
            .execute(&mut *tx)
            .await
            .expect("the trigger is dropped inside the transaction");
        sqlx::raw_sql(backfill)
            .execute(&mut *tx)
            .await
            .expect("the backfill applies");
        let (stamped,): (String,) =
            sqlx::query_as("SELECT retention_class FROM runs WHERE id = $1")
                .bind(&zdr_run)
                .fetch_one(&mut *tx)
                .await
                .expect("the zero-retention run row");
        let (untouched,): (String,) =
            sqlx::query_as("SELECT retention_class FROM runs WHERE id = $1")
                .bind(&ordinary_run)
                .fetch_one(&mut *tx)
                .await
                .expect("the ordinary run row");
        tx.rollback().await.expect("the backfill is rolled back");

        assert_eq!(stamped, "zdr", "a pre-receipt ZDR run is stamped zdr");
        assert_eq!(untouched, "persistent", "an ordinary run keeps the default");
    }

    /// Seed a run of `seeded` that carries the receipt `class`, and the waiver when there is one.
    async fn run_with_receipt(
        pool: &PgPool,
        seeded: &Seeded,
        class: &str,
        waiver: Option<(&str, &str)>,
    ) -> String {
        let id = new_ulid();
        let (ack, connection) = waiver.map_or((None, None), |(ack, connection)| {
            (Some(ack), Some(connection))
        });
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, retention_class,
                               waiver_ack_id, waiver_connection_id)
             VALUES ($1, $2, 'receipt goal', $3, $4, $5, $6, $7)",
        )
        .bind(&id)
        .bind(&seeded.thread)
        .bind(&seeded.org)
        .bind(&seeded.user)
        .bind(class)
        .bind(ack)
        .bind(connection)
        .execute(pool)
        .await
        .expect("a run with a receipt is seeded");
        id
    }

    /// Owner decision 17, against Postgres: ResolveRunOwner answers the owner's run only when the caller's
    /// posture is the run's receipt. A ZDR credential is refused as a durable mutation. A waiver is valid
    /// only for the run started under that same waiver. A caller that does not own the run gets `false`,
    /// and no posture is read for it. A service workload is not held to a user's posture.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres whose name contains \"test\" (session-core receipts CI job)"]
    async fn zdr_receipt_resolve_run_owner_answers_the_owners_run_under_its_receipt() {
        use mp_contracts::zdr_refusal::{code, Refusal};
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let seeded = seeded_thread(&pool, "resolve-owner").await;
        let zdr_run = run_with_receipt(&pool, &seeded, "zdr", None).await;
        let waived_run =
            run_with_receipt(&pool, &seeded, "zdr_waived", Some(("ack-1", "conn-1"))).await;
        let persistent_run = run_with_receipt(&pool, &seeded, "persistent", None).await;

        let persistent = VerifiedIdentity::user_for_test(&seeded.org, &seeded.user);
        let zdr = VerifiedIdentity::user_for_test_with_zdr(&seeded.org, &seeded.user, true);
        let waiver = |connection: &str| {
            VerifiedIdentity::user_for_test_with_waiver(
                &seeded.org,
                &seeded.user,
                connection,
                "ack-1",
            )
        };
        async fn answer(
            pool: &PgPool,
            seeded: &Seeded,
            caller: &VerifiedIdentity,
            run: &str,
        ) -> Result<bool, Status> {
            run_owner_matches(pool, caller, run, &seeded.org, &seeded.user).await
        }
        let refusal = |status: &Status| Refusal::from_status(status).map(|refusal| refusal.code());

        assert!(answer(&pool, &seeded, &persistent, &persistent_run)
            .await
            .expect("equal posture"));
        assert!(answer(&pool, &seeded, &waiver("conn-1"), &waived_run)
            .await
            .expect("the same waiver"));

        let error = answer(&pool, &seeded, &zdr, &zdr_run)
            .await
            .expect_err("a ZDR credential never advances a run");
        assert_eq!(refusal(&error), Some(code::DURABLE_MUTATION_FORBIDDEN));
        let error = answer(&pool, &seeded, &zdr, &persistent_run)
            .await
            .expect_err("a ZDR credential never advances a run");
        assert_eq!(refusal(&error), Some(code::DURABLE_MUTATION_FORBIDDEN));
        let error = answer(&pool, &seeded, &persistent, &zdr_run)
            .await
            .expect_err("a persistent credential may not attach to a ZDR run");
        assert_eq!(refusal(&error), Some(code::POSTURE_MISMATCH));
        let error = answer(&pool, &seeded, &persistent, &waived_run)
            .await
            .expect_err("a persistent credential may not attach to a waived run");
        assert_eq!(refusal(&error), Some(code::POSTURE_MISMATCH));
        let error = answer(&pool, &seeded, &waiver("conn-2"), &waived_run)
            .await
            .expect_err("a waiver for one connection may not continue another's run");
        assert_eq!(refusal(&error), Some(code::POSTURE_MISMATCH));
        let error = answer(&pool, &seeded, &waiver("conn-1"), &persistent_run)
            .await
            .expect_err("a waiver may not attach to a persistent run");
        assert_eq!(refusal(&error), Some(code::POSTURE_MISMATCH));

        let stranger = VerifiedIdentity::user_for_test_with_zdr(&seeded.org, "someone-else", true);
        let owned_by_stranger =
            run_owner_matches(&pool, &stranger, &zdr_run, &seeded.org, "someone-else")
                .await
                .expect("a caller who is not the owner is answered, not refused");
        assert!(
            !owned_by_stranger,
            "another user's run is not theirs, whatever its receipt"
        );

        let service = VerifiedIdentity::service_for_test(&seeded.org, &["session:read"], false);
        assert!(
            answer(&pool, &seeded, &service, &waived_run)
                .await
                .expect("a service is not held to a user's posture"),
            "a service workload keeps its owner answer"
        );
    }

    /// The revert script wraps itself in one transaction; a test that runs it
    /// inside its own transaction must drop the outer control statements.
    fn strip_transaction_control(script: &str) -> String {
        script
            .lines()
            .filter(|line| !matches!(line.trim(), "BEGIN;" | "COMMIT;"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
