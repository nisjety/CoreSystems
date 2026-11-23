//! `RunService.CancelRunAsRoomManager` (build plan WS09-S8): a room manager or owner stops another
//! member's live agent run.
//!
//! The run's owner can already stop it (`CancelRun`, owner-bound). A room manager is not the owner,
//! and nothing in model-gateway or the gateway may decide that they may stop someone else's turn:
//! Session Core is the only verifier. The authority is a Control `model.run.control` decision
//! (WS09-S7: role manager or owner, the org's `run_control_entitled` bit, current audience), and
//! this function checks it before it changes a row.
//!
//! Checks, in order, each one failing closed:
//!
//! 1. The caller is a user principal, or a service holding `session:write`. A zero-retention
//!    credential may stop a run (D23): the `RUN_CANCELLED` receipt names ids and a reason, never turn
//!    content, so the check is [`authorize_run_stop`].
//! 2. The Control decision verifies (signature, key, audience `model-plane`, subject = caller, org,
//!    Space, action `model.run.control`, permissions exactly `["run:cancel"]`, digest bound to this
//!    `run_id` and the operation `cancel`, the request's idempotency key, window) through
//!    [`crate::grpc::verify_run_control_space_decision`]. A decision issued under zero retention
//!    verifies for a stop too (D23).
//! 3. The caller holds no row in `space_membership_revocations` for this Space.
//! 4. The run is a ROOT run of a thread of THIS Space and org, inside the decision's audience
//!    ceiling. A run anywhere else, or beyond the ceiling, is `NotFound`: there is no oracle for a
//!    run id the caller may not see.
//! 5. The run's retention class is `persistent`, `zdr` or `zdr_waived` (D23); any other class is
//!    refused with the typed refusal.
//!
//! Then the same transaction as `CancelRun` flips the run and writes the `RUN_CANCELLED` receipt,
//! which names `cancelled_by` (the manager), `acting_as: "room_manager"` and the decision
//! reference. The event key `run_cancelled:{run_id}` de-duplicates a retried or racing stop.

#![allow(clippy::result_large_err)]

use chrono::Utc;
use mp_contracts::model_plane::v1 as pb;
use mp_contracts::zdr_refusal::Refusal;
use sqlx::PgPool;
use tonic::Status;

use crate::auth::VerifiedIdentity;
use crate::run_service_grpc::{authorize_run_stop, cancel_run_terminalize, ActingAs};

/// Longest id accepted on the request.
const MAX_ID_BYTES: usize = 128;

/// The reason recorded when the request names none, and the only words a request may name.
const DEFAULT_REASON: &str = "room_manager_stop";

/// What `acting_as` reads on the receipt of a manager's stop.
const ACTING_AS_ROOM_MANAGER: &str = "room_manager";

fn zdr_refusal() -> Status {
    Refusal::DURABLE_MUTATION_FORBIDDEN.grpc_status(tonic::Code::FailedPrecondition)
}

/// Whether a stop may proceed on a run of this retention class (D23). The stop writes no turn content,
/// so a zero-retention or waived run stops like any other; a class this crate does not know is refused.
fn stop_admits_retention(retention_class: &str) -> bool {
    matches!(retention_class, "persistent" | "zdr" | "zdr_waived")
}

/// The receipt reason: a short token of lowercase letters, digits and underscores, else the
/// default. A free-text reason never reaches a durable event.
fn receipt_reason(requested: &str) -> &str {
    let requested = requested.trim();
    let token_ok = !requested.is_empty()
        && requested.len() <= 40
        && requested
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if token_ok {
        requested
    } else {
        DEFAULT_REASON
    }
}

/// One run joined to its thread, as the stop check needs it.
#[derive(sqlx::FromRow)]
struct StopRow {
    owner_org_id: String,
    owner_user_id: String,
    retention_class: String,
}

/// Stop one room run under a verified Control run-control decision.
pub(crate) async fn cancel_run_as_room_manager(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: &pb::CancelRunAsRoomManagerRequest,
) -> Result<pb::CancelRunAsRoomManagerResponse, Status> {
    // A zero-retention credential may stop a run (D23): the receipt is content-free. Only the
    // service-scope check applies, as for the starter's CancelRun.
    authorize_run_stop(caller)?;
    let run_id = req.run_id.trim();
    let space_id = req.space_id.trim();
    let idempotency_key = req.idempotency_key.trim();
    if run_id.is_empty() || run_id.len() > MAX_ID_BYTES {
        return Err(Status::invalid_argument("run_id is required"));
    }
    if space_id.is_empty() || space_id.len() > MAX_ID_BYTES {
        return Err(Status::invalid_argument("space_id is required"));
    }
    if idempotency_key.is_empty() || idempotency_key.len() > MAX_ID_BYTES {
        return Err(Status::invalid_argument("idempotency_key is required"));
    }
    let Some(user_id) = caller.user_id().filter(|id| !id.trim().is_empty()) else {
        return Err(Status::permission_denied(
            "a room run stop requires an authenticated user subject",
        ));
    };

    let now = Utc::now();
    let decision = crate::grpc::verify_run_control_space_decision(
        &req.space_control_decision_token,
        &req.space_control_decision_ref,
        caller.org_id(),
        user_id,
        space_id,
        run_id,
        idempotency_key,
        now,
    )?;
    // A decision issued under zero retention verifies for a stop too (D23): nothing new is retained.

    // A removed member is refused even while the decision they hold has not expired.
    let (revoked,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM space_membership_revocations \
         WHERE space_ref = $1 AND subject_id = $2)",
    )
    .bind(space_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    if revoked {
        return Err(Status::permission_denied(
            "Space membership required for this run",
        ));
    }

    let row: StopRow = sqlx::query_as(
        "SELECT r.org_id AS owner_org_id, r.user_id AS owner_user_id, \
                r.retention_class AS retention_class \
         FROM runs r JOIN threads t ON t.id = r.thread_id \
         WHERE r.id = $1 AND r.org_id = $2 AND t.org_id = $2 AND t.space_id = $3 \
           AND r.parent_run_id IS NULL \
           AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $4)",
    )
    .bind(run_id)
    .bind(caller.org_id())
    .bind(space_id)
    .bind(decision.recipient_audience_revision)
    .fetch_optional(pool)
    .await
    .map_err(|error| Status::internal(error.to_string()))?
    .ok_or_else(|| Status::not_found("run not found"))?;

    // A stop is the safe direction under every posture this crate knows (D23).
    if !stop_admits_retention(&row.retention_class) {
        return Err(zdr_refusal());
    }

    let acting = ActingAs {
        acting_as: ACTING_AS_ROOM_MANAGER,
        decision_ref: req.space_control_decision_ref.trim(),
    };
    let cancellation = cancel_run_terminalize(
        pool,
        run_id,
        receipt_reason(&req.reason),
        user_id,
        Some(&acting),
    )
    .await?;
    Ok(pb::CancelRunAsRoomManagerResponse {
        cancelled: cancellation.cancelled,
        receipt_id: cancellation.receipt_id,
        owner_org_id: row.owner_org_id,
        owner_user_id: row.owner_user_id,
    })
}

#[cfg(test)]
mod tests {
    //! `room_stop_*` (WS09-S8). The refusals that come before the first query run against a lazy
    //! pool nothing ever reaches; the tests that read or write rows are `#[ignore]`d and need a
    //! disposable Postgres (`space::test_support::connect`).
    use super::*;
    use crate::grpc::space_w1_tests::{mint, signing_key, StreamKeysEnv, DECISION_REF, KEY_ID};
    use crate::space::test_support;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use ed25519_dalek::Signer as _;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};

    /// The Control `model.run.control` payload digest over `claims`, bound to `run_id` and
    /// `operation`. Written out here rather than called from `grpc.rs`, so the contract is pinned
    /// from the outside: the positive control in each test fails if either side drifts.
    fn run_control_digest(claims: &Value, run_id: &str, operation: &str) -> String {
        let text = |key: &str| claims[key].as_str().unwrap_or_default().to_owned();
        let mut digest = Sha256::new();
        digest.update(b"model.run.control\0v1\0");
        for (name, value) in [
            ("org_id", text("org_id")),
            ("user_id", text("subject_id")),
            ("space_id", text("space_ref")),
            ("space_decision_ref", text("decision_ref")),
            ("recipient_audience_ref", text("recipient_audience_ref")),
            ("recipient_audience_hash", text("recipient_audience_hash")),
            ("privacy_policy_ref", text("privacy_policy_ref")),
            ("resource_authorization_ref", text("resource_authorization_ref")),
            ("run_id", run_id.to_owned()),
            ("operation", operation.to_owned()),
            ("action_schema_hash", "sha256:run-control-v1".to_owned()),
            ("idempotency_key", text("idempotency_key")),
        ] {
            digest.update(name.as_bytes());
            digest.update([0]);
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value.as_bytes());
        }
        digest.update(b"authority_revision\0");
        digest.update(claims["authority_revision"].as_u64().expect("revision").to_be_bytes());
        digest.update(b"recipient_audience_revision\0");
        digest.update(
            claims["recipient_audience_revision"]
                .as_u64()
                .expect("revision")
                .to_be_bytes(),
        );
        format!("sha256:{:x}", digest.finalize())
    }

    /// A Control `model.run.control` decision for `user` in `space` under idempotency key `key`,
    /// valid now for two minutes, with its digest bound to `bound` = (run id, operation). `edit`
    /// changes a claim before the digest is computed.
    fn control_token(
        org: &str,
        user: &str,
        space: &str,
        key: &str,
        bound: (&str, &str),
        edit: impl FnOnce(&mut Value),
    ) -> String {
        let now = Utc::now();
        let mut claims = json!({
            "decision_ref": DECISION_REF,
            "org_id": org,
            "space_ref": space,
            "subject_id": user,
            "service_audience": crate::grpc::CONTROL_SPACE_DECISION_AUDIENCE,
            "action_id": "model.run.control",
            "action_schema_hash": "sha256:run-control-v1",
            "payload_digest": "",
            "idempotency_key": key,
            "recipient_audience_ref": "audience:space-room:4",
            "recipient_audience_hash": "sha256:test-audience",
            "privacy_policy_ref": "privacy:org-1:5",
            "resource_authorization_ref": "control:space-room:run-control:7",
            "purpose": "assistant_collaboration",
            "lawful_basis": "contract",
            "privacy_class": "internal",
            "third_party_processing_allowed": false,
            "retention_class": "standard",
            "residency": "swedencentral",
            "deletion_scope": "space",
            "zero_data_retention": false,
            "nonce": "nonce-1",
            "authority_revision": 7,
            "membership_revision": 4,
            "privacy_revision": 5,
            "recipient_audience_revision": 4,
            "entitlement_revision": 3,
            "permissions": ["run:cancel"],
            "issued_at": now.to_rfc3339(),
            "expires_at": (now + chrono::Duration::minutes(2)).to_rfc3339(),
        });
        edit(&mut claims);
        claims["payload_digest"] = json!(run_control_digest(&claims, bound.0, bound.1));
        let key_part = URL_SAFE_NO_PAD.encode(KEY_ID.as_bytes());
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims"));
        let signing_input = format!(
            "{}.{key_part}.{payload}",
            crate::grpc::CONTROL_SPACE_DECISION_VERSION
        );
        let signature = signing_key().sign(signing_input.as_bytes());
        format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        )
    }

    fn stop_request(run: &str, space: &str, key: &str, token: String) -> pb::CancelRunAsRoomManagerRequest {
        pb::CancelRunAsRoomManagerRequest {
            run_id: run.to_owned(),
            space_id: space.to_owned(),
            space_control_decision_ref: DECISION_REF.to_owned(),
            space_control_decision_token: token,
            idempotency_key: key.to_owned(),
            reason: String::new(),
        }
    }

    fn lazy_pool() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://unreachable.invalid/session")
            .expect("lazy pool")
    }

    fn manager() -> VerifiedIdentity {
        VerifiedIdentity::user_for_test("org-1", "manager-1")
    }

    /// The control every refusal below is measured against: the well-formed decision verifies for
    /// the manager, the Space, the run and the key it was issued for.
    fn assert_well_formed_decision_verifies() {
        let good = control_token("org-1", "manager-1", "space-1", "stop-1", ("run-1", "cancel"), |_| {});
        crate::grpc::verify_run_control_space_decision(
            &good,
            DECISION_REF,
            "org-1",
            "manager-1",
            "space-1",
            "run-1",
            "stop-1",
            Utc::now(),
        )
        .expect("a well-formed run-control decision verifies");
    }

    /// The reason on the receipt is a short closed token, never free text, so a person's words
    /// never reach a durable event; the receipt's `acting_as` word is the one the plan names.
    #[test]
    fn room_stop_the_receipt_reason_is_a_closed_token_and_never_free_text() {
        assert_eq!(ACTING_AS_ROOM_MANAGER, "room_manager");
        assert_eq!(receipt_reason(""), "room_manager_stop");
        assert_eq!(receipt_reason("   "), "room_manager_stop");
        assert_eq!(receipt_reason("wrong_room"), "wrong_room");
        assert_eq!(receipt_reason("  wrong_room  "), "wrong_room");
        assert_eq!(receipt_reason("stop_2"), "stop_2");
        for free_text in ["Wrong Room", "wrong-room", "this person's message said x", "ÆØÅ", "a\nb"] {
            assert_eq!(receipt_reason(free_text), "room_manager_stop", "{free_text:?}");
        }
        let longest = "a".repeat(40);
        assert_eq!(receipt_reason(&longest), longest);
        assert_eq!(receipt_reason(&"a".repeat(41)), "room_manager_stop");
    }

    /// D23: a zero-retention credential passes the caller gate. The empty decision then refuses the
    /// request before any row is read, so the refusal is the decision's, never the ZDR one.
    #[tokio::test]
    async fn room_stop_a_zero_retention_credential_passes_the_caller_gate() {
        let zdr = VerifiedIdentity::user_for_test_with_zdr("org-1", "manager-1", true);
        let refused = cancel_run_as_room_manager(
            &lazy_pool(),
            &zdr,
            &stop_request("run-1", "space-1", "stop-1", String::new()),
        )
        .await
        .expect_err("an empty decision stops nothing");
        assert_ne!(Refusal::from_status(&refused), Some(Refusal::DURABLE_MUTATION_FORBIDDEN), "{refused:?}");
        assert_ne!(refused.code(), tonic::Code::Internal, "no row was read: {refused:?}");
    }

    /// D23: a decision issued under zero retention verifies for a stop, so the claim alone refuses nothing.
    #[test]
    fn room_stop_a_decision_with_the_zero_retention_claim_verifies_for_a_stop() {
        let _keys = StreamKeysEnv::publish();
        assert_well_formed_decision_verifies();
        let token = control_token("org-1", "manager-1", "space-1", "stop-1", ("run-1", "cancel"), |claims| {
            claims["zero_data_retention"] = json!(true);
        });
        let decision = crate::grpc::verify_run_control_space_decision(
            &token,
            DECISION_REF,
            "org-1",
            "manager-1",
            "space-1",
            "run-1",
            "stop-1",
            Utc::now(),
        )
        .expect("a stop may be decided under zero retention (D23)");
        assert!(decision.zero_data_retention);
    }

    #[tokio::test]
    async fn room_stop_expired_foreign_subject_wrong_run_wrong_operation_and_other_permission_decisions_are_refused()
    {
        let _keys = StreamKeysEnv::publish();
        assert_well_formed_decision_verifies();
        let now = Utc::now();
        let minute = chrono::Duration::minutes(1);
        let does_not_authorize = "Control Space decision does not authorize this run control";
        let cases = [
            (
                "an expired decision",
                control_token("org-1", "manager-1", "space-1", "stop-1", ("run-1", "cancel"), |claims| {
                    claims["issued_at"] = json!((now - minute * 10).to_rfc3339());
                    claims["expires_at"] = json!((now - minute * 5).to_rfc3339());
                }),
                "Control Space run-control decision is expired or incomplete",
            ),
            (
                "another member's decision",
                control_token("org-1", "someone-else", "space-1", "stop-1", ("run-1", "cancel"), |_| {}),
                does_not_authorize,
            ),
            (
                "a decision whose digest is bound to another run",
                control_token("org-1", "manager-1", "space-1", "stop-1", ("run-other", "cancel"), |_| {}),
                does_not_authorize,
            ),
            (
                "a decision whose digest is bound to another operation",
                control_token("org-1", "manager-1", "space-1", "stop-1", ("run-1", "delete"), |_| {}),
                does_not_authorize,
            ),
            (
                "a read decision (thread:read)",
                mint(
                    crate::grpc::CONTROL_SPACE_DECISION_AUDIENCE,
                    &["thread:read"],
                    false,
                    |claims| {
                        claims["org_id"] = json!("org-1");
                        claims["subject_id"] = json!("manager-1");
                        claims["space_ref"] = json!("space-1");
                        claims["idempotency_key"] = json!("stop-1");
                        claims["issued_at"] = json!(now.to_rfc3339());
                        claims["expires_at"] = json!((now + minute * 2).to_rfc3339());
                    },
                    false,
                ),
                does_not_authorize,
            ),
            (
                "a control decision that also carries a thread permission",
                control_token("org-1", "manager-1", "space-1", "stop-1", ("run-1", "cancel"), |claims| {
                    claims["permissions"] = json!(["run:cancel", "thread:append"]);
                }),
                does_not_authorize,
            ),
        ];
        for (what, token, message) in cases {
            let refused = cancel_run_as_room_manager(
                &lazy_pool(),
                &manager(),
                &stop_request("run-1", "space-1", "stop-1", token),
            )
            .await
            .expect_err(what);
            assert_eq!(refused.code(), tonic::Code::PermissionDenied, "{what}");
            assert_eq!(refused.message(), message, "{what}");
        }
    }

    /// D23: every retention class this crate knows stops; an unknown class is refused.
    #[test]
    fn room_stop_admits_every_known_retention_class_and_no_other() {
        for class in ["persistent", "zdr", "zdr_waived"] {
            assert!(stop_admits_retention(class), "{class}");
        }
        for class in ["", "standard", "Persistent", "zdr_unknown"] {
            assert!(!stop_admits_retention(class), "{class}");
        }
    }

    /// One Space thread with one persistent root run, in fresh org and Space ids.
    struct Room {
        pool: PgPool,
        org: String,
        space: String,
        thread: String,
        run: String,
    }

    async fn room(tag: &str) -> Option<Room> {
        let pool = test_support::connect().await?;
        let sfx = mp_ids::new_ulid();
        let (org, space) = (format!("rs-{tag}-org-{sfx}"), format!("rs-{tag}-space-{sfx}"));
        let (thread, run) = (format!("rs-{tag}-thread-{sfx}"), format!("rs-{tag}-run-{sfx}"));
        test_support::seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        test_support::seed_space_run(&pool, &org, &space, &thread, &run, "queued", "persistent").await;
        Some(Room {
            pool,
            org,
            space,
            thread,
            run,
        })
    }

    /// The manager's stop of `run`, under a decision bound to that run, in `space`.
    async fn stop(room: &Room, org: &str, space: &str, run: &str) -> Result<pb::CancelRunAsRoomManagerResponse, Status> {
        let token = control_token(org, "manager-1", space, "stop-1", (run, "cancel"), |_| {});
        cancel_run_as_room_manager(
            &room.pool,
            &VerifiedIdentity::user_for_test(org, "manager-1"),
            &stop_request(run, space, "stop-1", token),
        )
        .await
    }

    async fn status_of(pool: &PgPool, run: &str) -> String {
        sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(run)
            .fetch_one(pool)
            .await
            .expect("run status")
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn room_stop_wrong_space_foreign_org_beyond_ceiling_and_child_run_are_not_found() {
        let Some(room) = room("nf").await else {
            return;
        };
        let _keys = StreamKeysEnv::publish();
        // A run that is not in the decision's Space.
        let other_space = format!("{}-other", room.space);
        let wrong_space = stop(&room, &room.org, &other_space, &room.run)
            .await
            .expect_err("another Space's decision reaches no run of this one");
        // A member of another org, holding that org's own decision.
        let foreign_org = stop(&room, "org-elsewhere", &room.space, &room.run)
            .await
            .expect_err("another org reaches no run of this one");
        // A thread whose audience is newer than the decision's ceiling (the decision is at revision 4).
        let (late_thread, late_run) = (format!("{}-late", room.thread), format!("{}-late", room.run));
        test_support::seed_space_thread(&room.pool, &room.org, &room.space, &late_thread, "owner-1").await;
        test_support::seed_space_run(&room.pool, &room.org, &room.space, &late_thread, &late_run, "queued", "persistent")
            .await;
        sqlx::query("UPDATE threads SET recipient_audience_revision = 9 WHERE id = $1")
            .bind(&late_thread)
            .execute(&room.pool)
            .await
            .expect("a newer audience");
        let beyond_ceiling = stop(&room, &room.org, &room.space, &late_run)
            .await
            .expect_err("a thread beyond the audience ceiling is not visible");
        // A child run: only a root run is stopped through the room.
        let child_run = format!("{}-child", room.run);
        test_support::seed_space_run(&room.pool, &room.org, &room.space, &room.thread, &child_run, "queued", "persistent")
            .await;
        sqlx::query("UPDATE runs SET parent_run_id = $1 WHERE id = $2")
            .bind(&room.run)
            .bind(&child_run)
            .execute(&room.pool)
            .await
            .expect("make it a child run");
        let child = stop(&room, &room.org, &room.space, &child_run)
            .await
            .expect_err("a child run is not stopped through the room");

        for (what, refused) in [
            ("wrong space", wrong_space),
            ("foreign org", foreign_org),
            ("beyond the ceiling", beyond_ceiling),
            ("child run", child),
        ] {
            assert_eq!(refused.code(), tonic::Code::NotFound, "{what}");
            assert_eq!(refused.message(), "run not found", "{what}: no oracle for the run id");
        }
        for run in [&room.run, &late_run, &child_run] {
            assert_eq!(status_of(&room.pool, run).await, "queued", "nothing was cancelled");
        }
        test_support::clean_space(&room.pool, &room.org, &room.space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn room_stop_a_revoked_member_is_refused() {
        let Some(room) = room("rv").await else {
            return;
        };
        let _keys = StreamKeysEnv::publish();
        sqlx::query(
            "INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) VALUES ($1, $2, $3) \
             ON CONFLICT (space_ref, subject_id) DO UPDATE SET revoked_at = NOW()",
        )
        .bind(&room.space)
        .bind(&room.org)
        .bind("manager-1")
        .execute(&room.pool)
        .await
        .expect("remove the member");
        // The decision is still inside its window, and the removed member stops nothing.
        let refused = stop(&room, &room.org, &room.space, &room.run)
            .await
            .expect_err("a removed member stops nothing");
        assert_eq!(refused.code(), tonic::Code::PermissionDenied);
        assert_eq!(refused.message(), "Space membership required for this run");
        assert_eq!(status_of(&room.pool, &room.run).await, "queued");

        sqlx::query("DELETE FROM space_membership_revocations WHERE space_ref = $1")
            .bind(&room.space)
            .execute(&room.pool)
            .await
            .ok();
        test_support::clean_space(&room.pool, &room.org, &room.space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn room_stop_zero_retention_and_waived_runs_stop_like_any_other_run() {
        let Some(room) = room("zr").await else {
            return;
        };
        let _keys = StreamKeysEnv::publish();
        let (zdr_run, waived_run) = (format!("{}-zdr", room.run), format!("{}-waived", room.run));
        test_support::seed_space_run(&room.pool, &room.org, &room.space, &room.thread, &zdr_run, "queued", "zdr").await;
        test_support::seed_space_run(&room.pool, &room.org, &room.space, &room.thread, &waived_run, "queued", "zdr_waived")
            .await;
        for run in [&zdr_run, &waived_run] {
            let stopped = stop(&room, &room.org, &room.space, run)
                .await
                .expect("a stop writes only ids and a reason, so zero retention does not refuse it (D23)");
            assert!(stopped.cancelled, "{run}");
        }
        test_support::clean_space(&room.pool, &room.org, &room.space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn room_stop_a_valid_stop_cancels_once_and_records_who_stopped_it_on_whose_authority() {
        let Some(room) = room("ok").await else {
            return;
        };
        let _keys = StreamKeysEnv::publish();
        let manager = VerifiedIdentity::user_for_test(&room.org, "manager-1");
        let token = control_token(&room.org, "manager-1", &room.space, "stop-1", (&room.run, "cancel"), |_| {});
        let mut request = stop_request(&room.run, &room.space, "stop-1", token);
        request.reason = "wrong_room".to_owned();

        let first = cancel_run_as_room_manager(&room.pool, &manager, &request)
            .await
            .expect("a manager stops the run");
        assert!(first.cancelled);
        assert!(!first.receipt_id.is_empty());
        // The run owner's identity comes back so the caller can stop the owner's live producer.
        assert_eq!(first.owner_org_id, room.org);
        assert_eq!(first.owner_user_id, "owner-1");
        assert_eq!(status_of(&room.pool, &room.run).await, "cancelled");

        // The receipt names who stopped the run, as whom, and on which decision.
        let payload: Value = sqlx::query_scalar("SELECT payload FROM events WHERE id = $1")
            .bind(&first.receipt_id)
            .fetch_one(&room.pool)
            .await
            .expect("the RUN_CANCELLED receipt");
        assert_eq!(
            payload,
            json!({
                "run_id": room.run,
                "reason": "wrong_room",
                "cancelled_by": "manager-1",
                "acting_as": "room_manager",
                "decision_ref": DECISION_REF,
            })
        );

        // The same stop again changes nothing and answers the same receipt.
        let second = cancel_run_as_room_manager(&room.pool, &manager, &request)
            .await
            .expect("a repeated stop is idempotent");
        assert!(!second.cancelled);
        assert_eq!(second.receipt_id, first.receipt_id);
        assert_eq!(second.owner_org_id, room.org);
        assert_eq!(second.owner_user_id, "owner-1");
        let receipts: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM events WHERE run_id = $1 AND event_type = 'RUN_CANCELLED'",
        )
        .bind(&room.run)
        .fetch_one(&room.pool)
        .await
        .expect("count receipts");
        assert_eq!(receipts, 1, "one receipt for one stop");
        test_support::clean_space(&room.pool, &room.org, &room.space).await;
    }
}
