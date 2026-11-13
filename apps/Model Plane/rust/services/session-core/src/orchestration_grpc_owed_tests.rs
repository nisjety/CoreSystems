//! Owed `ApprovalRuleProposalService` edge tests [W2-WS13-S7-proposal-store-b], a test-only child
//! module of `orchestration_grpc`: the resolution and `policy_id` validation, the switch gate that
//! holds CONFIRMING and CONFIRMED only, the refusal mapping, and the waived-caller refusal before
//! any resolve. Pure: no database, no network.

use super::*;

#[test]
fn only_confirming_confirmed_and_dismissed_are_resolutions() {
    use proto::ApprovalRuleProposalState as S;
    assert_eq!(
        rule_proposal_resolution(S::Confirming as i32, "").unwrap(),
        store::RuleProposalResolution::Claim
    );
    assert_eq!(
        rule_proposal_resolution(S::Dismissed as i32, "").unwrap(),
        store::RuleProposalResolution::Dismissed
    );
    assert_eq!(
        rule_proposal_resolution(S::Confirmed as i32, "tpol_0b3c-9f").unwrap(),
        store::RuleProposalResolution::Confirmed {
            policy_id: "tpol_0b3c-9f"
        }
    );
    for refused in [S::Unspecified as i32, S::Proposed as i32, 99, -1] {
        assert_eq!(
            rule_proposal_resolution(refused, "").unwrap_err().code(),
            tonic::Code::InvalidArgument,
            "{refused}"
        );
    }
}

#[test]
fn a_policy_id_rides_only_a_confirmed_resolution_and_must_be_a_tool_policy_id() {
    use proto::ApprovalRuleProposalState as S;
    for state in [S::Confirming, S::Dismissed] {
        assert_eq!(
            rule_proposal_resolution(state as i32, "tpol_1")
                .unwrap_err()
                .code(),
            tonic::Code::InvalidArgument,
            "{state:?} carries no policy id"
        );
    }
    let longest = format!("tpol_{}", "a".repeat(100));
    let too_long = format!("tpol_{}", "a".repeat(101));
    for (policy, ok) in [
        ("tpol_1", true),
        ("tpol_123e4567-e89b-12d3-a456-426614174000", true),
        (longest.as_str(), true),
        ("", false),
        ("tpol_", false),
        ("pol_1", false),
        ("tpol_a b", false),
        ("tpol_a/b", false),
        ("tpol_a_b", false),
        (too_long.as_str(), false),
    ] {
        assert_eq!(
            rule_proposal_resolution(S::Confirmed as i32, policy).is_ok(),
            ok,
            "{policy:?}"
        );
    }
}

#[test]
fn the_switch_holds_a_confirm_and_never_a_dismiss() {
    let claim = store::RuleProposalResolution::Claim;
    let confirmed = store::RuleProposalResolution::Confirmed {
        policy_id: "tpol_1",
    };
    let dismissed = store::RuleProposalResolution::Dismissed;
    for held in [claim, confirmed] {
        let refused = rule_proposal_switch_gate(held, false).unwrap_err();
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(refused.message(), "rule_proposals_disabled");
        assert!(rule_proposal_switch_gate(held, true).is_ok());
    }
    assert!(
        rule_proposal_switch_gate(dismissed, false).is_ok(),
        "narrowing works off"
    );
    assert!(rule_proposal_switch_gate(dismissed, true).is_ok());
}

#[test]
fn a_store_refusal_is_a_failed_precondition_carrying_its_code() {
    for refusal in [
        store::RuleProposalRefusal::Resolved,
        store::RuleProposalRefusal::Stale,
        store::RuleProposalRefusal::Busy,
        store::RuleProposalRefusal::NotClaimed,
    ] {
        let status = rule_proposal_status(&anyhow::Error::from(refusal));
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(status.message(), refusal.code());
    }
    let other = rule_proposal_status(&anyhow::anyhow!("connection reset"));
    assert_eq!(other.code(), tonic::Code::Internal);
}

#[test]
fn a_resolve_refuses_zdr_and_waived_callers_before_anything_is_read() {
    let zdr = VerifiedIdentity::user_for_test_with_zdr("org-1", "u1", true);
    assert!(refuse_zdr_or_waived_question_caller(&zdr).is_err());
    let waived = VerifiedIdentity::user_for_test_with_waiver("org-1", "u1", "conn-1", "ack-1");
    assert_eq!(
        refuse_zdr_or_waived_question_caller(&waived)
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied,
        "a waived run counts as ZDR for a durable write (DEC-32), with no switch"
    );
    let persistent = VerifiedIdentity::user_for_test("org-1", "u1");
    assert!(refuse_zdr_or_waived_question_caller(&persistent).is_ok());
}

#[test]
fn the_projection_maps_every_state_word_and_unknown_to_unspecified() {
    use proto::ApprovalRuleProposalState as S;
    for (word, state) in [
        ("proposed", S::Proposed),
        ("confirming", S::Confirming),
        ("confirmed", S::Confirmed),
        ("dismissed", S::Dismissed),
        ("garbage", S::Unspecified),
    ] {
        assert_eq!(
            approval_rule_proposal_state_from_str(word),
            state as i32,
            "{word}"
        );
    }
}

// ---- input pause: prepare_question and the pause reference [W2-WS14-S5-b2] -------------------

fn single_choice() -> proto::QuestionItem {
    proto::QuestionItem {
        id: "scope".to_owned(),
        kind: "single".to_owned(),
        prompt: "Which one?".to_owned(),
        options: vec![
            proto::QuestionOption {
                id: "a".to_owned(),
                label: "A".to_owned(),
                description: String::new(),
            },
            proto::QuestionOption {
                id: "b".to_owned(),
                label: "B".to_owned(),
                description: String::new(),
            },
        ],
        allow_other: false,
    }
}

fn resume_request(step_id: &str, key: &str) -> proto::CreateRunQuestionRequest {
    proto::CreateRunQuestionRequest {
        run_id: "run-1".to_owned(),
        org_id: "org-1".to_owned(),
        step_id: step_id.to_owned(),
        idempotency_key: key.to_owned(),
        delivery: user_question::DELIVERY_RESUME.to_owned(),
        items: vec![single_choice()],
        ..Default::default()
    }
}

#[test]
fn a_resume_question_is_prepared_only_with_the_switch_on() {
    let req = resume_request("step-1", "run-1:step-1:input");
    {
        let _on = AskResumeSwitch::set(true);
        let prepared = prepare_question(&req).expect("on: a resume question is accepted");
        assert_eq!(prepared.delivery, user_question::DELIVERY_RESUME);
        let ceiling = user_question::delivery_ttl_secs(user_question::DELIVERY_RESUME)
            .expect("resume has a lifetime");
        assert_eq!(
            prepared.expires_in,
            chrono::Duration::seconds(i64::from(ceiling))
        );
        // A next_turn question is unchanged by the switch.
        let mut next = req.clone();
        next.delivery = String::new();
        assert_eq!(
            prepare_question(&next).expect("valid").delivery,
            user_question::DELIVERY_NEXT_TURN
        );
    }
    {
        let _off = AskResumeSwitch::set(false);
        let refused = prepare_question(&req).expect_err("off: refused as before");
        assert_eq!(refused.code(), tonic::Code::InvalidArgument);
        assert_eq!(refused.message(), "question_delivery_unavailable");
    }
}

#[test]
fn a_resume_question_must_name_its_own_input_pause() {
    let model = user_question::ORIGIN_MODEL;
    assert!(
        require_input_pause_ref(&resume_request("step-1", "run-1:step-1:input"), model).is_ok()
    );
    for (step, key, why) in [
        ("step-1", "run-1:step-1", "an approval pause's reference"),
        ("step-1", "", "no reference"),
        ("step-1", "run-1:step-2:input", "another step's input pause"),
        ("step-1", "run-2:step-1:input", "another run's input pause"),
        ("", "run-1::input", "no step"),
        (
            "step-1",
            "run-1:step-1:elicit:1",
            "an elicitation reference for a model question",
        ),
    ] {
        let refused = require_input_pause_ref(&resume_request(step, key), model).expect_err(why);
        assert_eq!(refused.code(), tonic::Code::InvalidArgument, "{why}");
        assert_eq!(refused.message(), "question_pause_ref_mismatch", "{why}");
    }
    let elicitation = user_question::ORIGIN_MCP_ELICITATION;
    assert!(require_input_pause_ref(
        &resume_request("step-1", "run-1:step-1:elicit:1"),
        elicitation
    )
    .is_ok());
    assert!(
        require_input_pause_ref(&resume_request("step-1", "run-1:step-1:input"), elicitation)
            .is_err()
    );
}

// Verification plan 3.4: the parsers of the switches this branch added to session-core. Each is
// off for anything but `true`; the strict repo parser (`switch_from`) also takes surrounding space
// and any case, the two literal readers take only the exact word.

const OFF_WORDS: [&str; 9] = ["", "1", "yes", "on", "false", "0", "ture", "true1", "t rue"];

#[test]
fn the_resume_question_switch_takes_only_the_exact_literal_true() {
    assert_eq!(ASK_RESUME_ENV, "AGENT_ASK_USER_RESUME_ENABLED");
    assert!(ask_resume_switch(Some("true")));
    assert!(!ask_resume_switch(None));
    for off in OFF_WORDS
        .into_iter()
        .chain(["TRUE", "True", " true", "true "])
    {
        assert!(!ask_resume_switch(Some(off)), "{off:?} must leave it off");
    }
}

#[test]
fn the_proof_walkthrough_switch_takes_only_true_with_surrounding_space() {
    assert_eq!(PROOF_WALKTHROUGH_ENV, "SESSION_PROOF_WALKTHROUGH_ENABLED");
    assert!(!proof_walkthrough_switch_from(None));
    for on in ["true", " true", "true\n"] {
        assert!(proof_walkthrough_switch_from(Some(on)), "{on:?}");
    }
    for off in OFF_WORDS.into_iter().chain(["TRUE", "True"]) {
        assert!(
            !proof_walkthrough_switch_from(Some(off)),
            "{off:?} must leave it off"
        );
    }
}

#[test]
fn the_strict_parser_switches_take_true_and_nothing_else() {
    assert_eq!(APPROVAL_WAIVER_REFUSE_ENV, "APPROVAL_WAIVER_REFUSE");
    assert_eq!(APPROVAL_EDIT_ENV, "APPROVAL_EDIT_ENABLED");
    assert_eq!(
        crate::auth::SKILL_ORG_ADMIN_CHECK_ENV,
        "SKILL_ORG_ADMIN_CHECK"
    );
    assert_eq!(
        crate::retention_receipt::THREAD_RECEIPT_ENFORCE_ENV,
        "SESSION_THREAD_RECEIPT_ENFORCE"
    );
    let parsers: [(&str, fn(Option<&str>) -> bool); 2] = [
        (
            "SKILL_ORG_ADMIN_CHECK",
            crate::auth::skill_org_admin_check_enabled,
        ),
        (
            "SESSION_THREAD_RECEIPT_ENFORCE",
            crate::retention_receipt::thread_receipt_enforce_from,
        ),
    ];
    for (name, parse) in parsers {
        assert!(!parse(None), "{name}: unset is off");
        for on in ["true", " true ", "TRUE"] {
            assert!(parse(Some(on)), "{name}: {on:?}");
        }
        for off in OFF_WORDS {
            assert!(!parse(Some(off)), "{name}: {off:?} must leave it off");
        }
    }
}

/// Off, an approval create, decide or content-free check treats a waived caller as before: on the
/// approval paths the waiver is consulted only paired with `APPROVAL_WAIVER_REFUSE`, and the decide
/// gate goes through the same waiver gate. The one unconditional use is the question gate, a store
/// that is new on this branch (DEC-32: a waived run counts as ZDR for every new store).
#[test]
fn the_approval_waiver_refusal_is_only_added_behind_its_switch() {
    let source = include_str!("orchestration_grpc.rs").replace("\r\n", "\n");
    let production = &source[..source
        .find("#[cfg(test)]\nmod tests")
        .unwrap_or(source.len())];
    let body = |signature: &str| -> String {
        let start = production.find(signature).expect(signature);
        let rest = &production[start..];
        rest[..rest.find("\n}\n").expect("end of the function")].to_owned()
    };
    assert_eq!(
        production
            .matches("approval_waiver_refuse_enabled() && caller.zdr_waiver().is_some()")
            .count(),
        2,
        "the content-free check and the approval waiver gate pair the waiver with the switch"
    );
    assert_eq!(production.matches("zdr_waiver()").count(), 3);
    assert!(body("fn refuse_zdr_or_waived_question_caller(")
        .contains("if caller.zdr_waiver().is_some()"));
    assert!(body("fn refuse_zdr_or_waived_approval_caller(")
        .contains("refuse_waived_approval_caller(caller)"));
}
