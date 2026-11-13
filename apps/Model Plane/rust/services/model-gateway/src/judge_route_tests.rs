//! W3-EP-7-S6-a: the unattended goal judge route answers 404 `goal_unattended_disabled` while its switches are
//! off, before it looks at the caller, the body or any credential, so a deployment without the feature answers
//! the same for every caller and makes no upstream call. The test process never sets
//! `MODEL_GATEWAY_GOAL_UNATTENDED_ENABLED` or `MODEL_GATEWAY_GOAL_JUDGE_ENABLED`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde_json::json;

use crate::auth::Claims;
use crate::goal_unattended::{judge_goal_turn_route, JudgeTurnBody};
use crate::state::AppState;

fn claims(principal_type: &str, service_id: Option<&str>, zdr: bool) -> Claims {
    Claims {
        key_id: None,
        acting_user_id: None,
        sub: service_id.map_or_else(|| "user-1".to_owned(), |id| format!("service:{id}")),
        iss: "test-issuer".to_owned(),
        exp: i64::MAX,
        org_id: "org-1".to_owned(),
        user_id: "user-1".to_owned(),
        nbf: None,
        aud: Some("model-gateway".to_owned()),
        scopes: vec!["model:goal_judge".to_owned()],
        zdr,
        sovereign: None,
        org_zdr: None,
        zdr_waiver: None,
        principal_type: Some(principal_type.to_owned()),
        service_id: service_id.map(str::to_owned),
        reason: None,
    }
}

fn body() -> JudgeTurnBody {
    serde_json::from_value(json!({
        "run_id": "run-1",
        "org_id": "org-1",
        "schedule_id": "sched-1",
        "fire_key": "fire-1",
    }))
    .expect("an ids-only body")
}

#[tokio::test]
async fn the_judge_route_is_a_404_for_every_caller_while_its_switches_are_off() {
    assert!(
        !(crate::goal_unattended::enabled() && crate::goal_judge::goal_judge_enabled()),
        "the test process must run with the judge switches off"
    );
    let state = AppState::new();
    for caller in [
        claims("service", Some("orchestrator-core"), false),
        claims("service", Some("model-gateway"), false),
        claims("user", None, false),
        claims("user", None, true),
    ] {
        let (status, Json(error)) = judge_goal_turn_route(
            State(state.clone()),
            Extension(caller),
            None,
            Path("goal_01HZY".to_owned()),
            Json(body()),
        )
        .await
        .expect_err("off answers 404");
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error["error"]["code"], "goal_unattended_disabled");
        assert_eq!(error["error"]["details"], json!({}));
    }
}
