//! F-f10-28: the scheduled step verifier recomputes the stored contract's digest and expiry on every step whose
//! Control decision names a contract and whose run carries one, whatever the switch says. It binds the decision to the
//! run's contract under the switch, and a run that carries no contract at all is left to the switch (P2-ec-07-rel1).
//!
//! The decision module is crate-private, so this test includes its source the way `provenance.rs` includes
//! `screening_client.rs`. The module's own unit tests compile into this binary too.

#[allow(dead_code)]
#[path = "../src/scheduled_step_decision.rs"]
mod scheduled_step_decision;

use chrono::{DateTime, Duration, Utc};
use mp_contracts::model_plane::v1::ScheduledStepContext;
use scheduled_step_decision::{
    check_run_contract, verify_stored_contract, VerifiedStep, REFUSAL_CONTRACT_EXPIRED,
    REFUSAL_CONTRACT_INVALID, REFUSAL_CONTRACT_MISMATCH, REFUSAL_CONTRACT_REQUIRED,
};

/// The contract text a run stores in these tests. Only its digest and its expiry are checked here.
const BODY: &str = r#"{"v":1,"tools":["read_file"]}"#;

/// A run holding `body`, recorded under the digest of its exact text and under `expires_at`.
fn held_run(body: &str, expires_at: DateTime<Utc>) -> ScheduledStepContext {
    ScheduledStepContext {
        run_id: "run-1".to_owned(),
        thread_id: "thread-1".to_owned(),
        org_id: "org-1".to_owned(),
        goal: "summarise the week".to_owned(),
        status: "running".to_owned(),
        contract_json: body.to_owned(),
        contract_digest: mp_contracts::scheduled_contract::digest(body),
        contract_expires_at: expires_at.to_rfc3339(),
        ..Default::default()
    }
}

/// A run that holds no contract, as Session Core serves the run of a schedule made without one.
fn legacy_run() -> ScheduledStepContext {
    ScheduledStepContext {
        run_id: "run-1".to_owned(),
        thread_id: "thread-1".to_owned(),
        org_id: "org-1".to_owned(),
        goal: "summarise the week".to_owned(),
        status: "running".to_owned(),
        ..Default::default()
    }
}

/// The facts a Control decision binds when it signed over `body`, live until `expires_at`.
fn signed(body: &str, expires_at: DateTime<Utc>) -> VerifiedStep {
    VerifiedStep {
        contract_digest: Some(mp_contracts::scheduled_contract::digest(body)),
        contract_expires_at: Some(expires_at),
    }
}

/// The closed token a refused check answers with.
fn refusal(result: Result<(), tonic::Status>) -> String {
    result
        .expect_err("the step is refused")
        .message()
        .to_owned()
}

/// Binds `decision` to `run` in the order the step lane uses: the switch-gated binding first, then the stored
/// contract's own checks.
fn bind(
    decision: &VerifiedStep,
    run: &ScheduledStepContext,
    required: bool,
    now: DateTime<Utc>,
) -> Result<(), tonic::Status> {
    check_run_contract(
        decision.contract_digest.as_deref(),
        !run.contract_json.is_empty(),
        &run.contract_digest,
        required,
    )?;
    verify_stored_contract(
        decision.contract_digest.as_deref(),
        decision.contract_expires_at,
        run,
        now,
        required,
    )
}

#[test]
fn scheduled_step_contract_accepts_matching() {
    let now = Utc::now();
    let expires_at = now + Duration::hours(1);
    let run = held_run(BODY, expires_at);
    let decision = signed(BODY, expires_at);
    bind(&decision, &run, true, now).expect("a decision naming the stored contract is accepted");
    bind(&decision, &run, false, now).expect("and so with the switch off");
}

#[test]
fn scheduled_step_contract_rejects_digest_mismatch() {
    let now = Utc::now();
    let expires_at = now + Duration::hours(1);

    // The stored text is not the text the decision signed, though the run records the signed digest.
    let mut tampered = held_run(BODY, expires_at);
    tampered.contract_json = r#"{"v":1,"tools":["shell"]}"#.to_owned();
    let decision = signed(BODY, expires_at);
    assert_eq!(
        refusal(bind(&decision, &tampered, false, now)),
        REFUSAL_CONTRACT_INVALID
    );
    assert_eq!(
        refusal(bind(&decision, &tampered, true, now)),
        REFUSAL_CONTRACT_INVALID
    );

    // A decision that signed another digest than the run holds: the switch refuses it as a mismatch, and with the
    // switch off the binding stays silent, so the stored-contract check refuses the other digest as invalid.
    let other = format!("sha256:{}", "e".repeat(64));
    let other_decision = VerifiedStep {
        contract_digest: Some(other),
        contract_expires_at: Some(expires_at),
    };
    let held = held_run(BODY, expires_at);
    assert_eq!(
        refusal(bind(&other_decision, &held, true, now)),
        REFUSAL_CONTRACT_MISMATCH
    );
    assert_eq!(
        refusal(bind(&other_decision, &held, false, now)),
        REFUSAL_CONTRACT_INVALID
    );
}

#[test]
fn scheduled_step_contract_rejects_expired() {
    let now = Utc::now();
    let expires_at = now - Duration::seconds(1);
    let run = held_run(BODY, expires_at);
    let decision = signed(BODY, expires_at);
    // Expiry is checked whatever the switch says: the claims carry a contract, so it is recomputed.
    assert_eq!(
        refusal(bind(&decision, &run, true, now)),
        REFUSAL_CONTRACT_EXPIRED
    );
    assert_eq!(
        refusal(bind(&decision, &run, false, now)),
        REFUSAL_CONTRACT_EXPIRED
    );
}

#[test]
fn scheduled_step_contract_rejects_missing_body() {
    let now = Utc::now();
    let expires_at = now + Duration::hours(1);
    // The run keeps its digest and expiry, but the text itself is gone. A missing view never falls back to allow, so the
    // stored check refuses it whatever the switch says.
    let mut run = held_run(BODY, expires_at);
    run.contract_json.clear();
    let decision = signed(BODY, expires_at);
    for required in [false, true] {
        assert_eq!(
            refusal(verify_stored_contract(
                decision.contract_digest.as_deref(),
                decision.contract_expires_at,
                &run,
                now,
                required,
            )),
            REFUSAL_CONTRACT_INVALID
        );
    }
    // With the switch on the run holds no contract, so a decision that names one is refused as a mismatch first.
    assert_eq!(
        refusal(bind(&decision, &run, true, now)),
        REFUSAL_CONTRACT_MISMATCH
    );
}

#[test]
fn scheduled_step_contract_rejects_an_expiry_the_decision_does_not_name() {
    let now = Utc::now();
    let expires_at = now + Duration::hours(1);
    // The run records a later expiry than the one the decision signed.
    let run = held_run(BODY, expires_at + Duration::minutes(1));
    let decision = signed(BODY, expires_at);
    assert_eq!(
        refusal(bind(&decision, &run, false, now)),
        REFUSAL_CONTRACT_INVALID
    );
}

#[test]
fn scheduled_step_contract_recomputes_whatever_the_switch_says() {
    let now = Utc::now();
    let expires_at = now + Duration::hours(1);
    let mut run = held_run(BODY, expires_at);
    run.contract_json = r#"{"v":2}"#.to_owned();
    let decision = signed(BODY, expires_at);
    // With the switch off nothing is bound, but the run carries a contract, so the stored-contract check still runs and
    // refuses.
    verify_stored_contract(
        decision.contract_digest.as_deref(),
        decision.contract_expires_at,
        &run,
        now,
        false,
    )
    .expect_err("a run that carries a contract is re-hashed whatever the switch says");
    check_run_contract(
        decision.contract_digest.as_deref(),
        true,
        &run.contract_digest,
        false,
    )
    .expect("the binding itself is off");
}

#[test]
fn legacy_step_unchanged() {
    let now = Utc::now();
    let legacy_decision = VerifiedStep {
        contract_digest: None,
        contract_expires_at: None,
    };
    // A legacy decision for a legacy run passes whatever the switch says.
    for required in [false, true] {
        bind(&legacy_decision, &legacy_run(), required, now)
            .expect("a legacy step for a legacy run is unchanged");
    }
    // A legacy decision for a run that holds a contract is not bound with the switch off, and is refused as
    // required with it on.
    let held = held_run(BODY, now + Duration::hours(1));
    bind(&legacy_decision, &held, false, now)
        .expect("with the switch off a legacy decision is not bound to the run");
    assert_eq!(
        refusal(bind(&legacy_decision, &held, true, now)),
        REFUSAL_CONTRACT_REQUIRED
    );
}

/// P2-ec-07-rel1: an execution-core build deployed before Session Core serves the stored contract sees a run that carries
/// no contract at all, though the Control decision names one. With the switch off that step is not refused by the stored
/// check, so it takes the legacy lane as it did before the check existed. With the switch on the binding refuses it, as
/// it always did, and the stored check refuses a run that carries no contract as invalid.
#[test]
fn a_contract_bearing_step_is_not_refused_for_a_run_that_carries_no_contract_with_the_switch_off() {
    let now = Utc::now();
    let expires_at = now + Duration::hours(1);
    let decision = signed(BODY, expires_at);
    let unserved = legacy_run();
    bind(&decision, &unserved, false, now)
        .expect("switch off: a run that carries no contract is left to the switch, not refused");
    verify_stored_contract(
        decision.contract_digest.as_deref(),
        decision.contract_expires_at,
        &unserved,
        now,
        false,
    )
    .expect("the stored check is switch-gated for a run that carries no contract");
    assert_eq!(
        refusal(verify_stored_contract(
            decision.contract_digest.as_deref(),
            decision.contract_expires_at,
            &unserved,
            now,
            true,
        )),
        REFUSAL_CONTRACT_INVALID
    );
    assert_eq!(
        refusal(bind(&decision, &unserved, true, now)),
        REFUSAL_CONTRACT_MISMATCH
    );
}

/// A run that carries only part of its stored contract is refused whatever the switch says. The switch governs a run that
/// carries no contract at all; a missing text, digest or expiry is a view that never falls back to allow.
#[test]
fn a_partly_served_contract_is_refused_whatever_the_switch_says() {
    let now = Utc::now();
    let expires_at = now + Duration::hours(1);
    let decision = signed(BODY, expires_at);

    let mut no_text = held_run(BODY, expires_at);
    no_text.contract_json.clear();
    let mut no_digest = held_run(BODY, expires_at);
    no_digest.contract_digest.clear();
    let mut no_expiry = held_run(BODY, expires_at);
    no_expiry.contract_expires_at.clear();

    for run in [&no_text, &no_digest, &no_expiry] {
        for required in [false, true] {
            assert_eq!(
                refusal(verify_stored_contract(
                    decision.contract_digest.as_deref(),
                    decision.contract_expires_at,
                    run,
                    now,
                    required,
                )),
                REFUSAL_CONTRACT_INVALID
            );
        }
    }
}
