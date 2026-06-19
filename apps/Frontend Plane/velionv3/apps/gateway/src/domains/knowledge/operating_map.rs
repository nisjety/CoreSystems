use axum::{
    body::Body,
    extract::{Extension, Path, State},
    http::{header::CONTENT_TYPE, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState, domains::knowledge::shared, envelope::unwrap_data,
    middleware::AuthenticatedUser, upstream::proxy_json,
};

pub(super) async fn get_operating_map(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let org_id = shared::org_id_from_headers(&headers);
    let url = format!("{}/v1/wiki/operating-map", state.wiki_store_url);
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn generate_operating_map(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> impl IntoResponse {
    let org_id = shared::org_id_from_headers(&headers);
    body["requested_by"] = Value::String(user.user_id.clone());
    if body.get("generated_from").is_none() {
        body["generated_from"] = json!({
            "source": "knowledge-workspace",
            "capability": "operating_map.generate",
        });
    }
    let url = format!("{}/v1/wiki/operating-map/refresh", state.wiki_store_url);
    proxy_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn operating_map_run_events(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> Response {
    let org_id = shared::org_id_from_headers(&headers);
    let url = format!("{}/v1/wiki/operating-map", state.wiki_store_url);
    let (upstream_status, Json(body)) = proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await;
    let status = if upstream_status.is_success() {
        operating_map_run_event_payload(&run_id, &unwrap_data(&body))
    } else {
        json!({
            "runId": run_id,
            "status": "error",
            "detail": "Operating Map run status could not be loaded.",
        })
    };
    let body = format!(
        "event: status\ndata: {}\n\nevent: done\ndata: {{}}\n\n",
        status
    );
    (
        StatusCode::OK,
        [(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))],
        Body::from(body),
    )
        .into_response()
}

fn operating_map_run_event_payload(run_id: &str, snapshot: &Value) -> Value {
    let proposal = snapshot
        .get("proposals")
        .and_then(Value::as_array)
        .and_then(|proposals| {
            proposals.iter().find(|proposal| {
                proposal.get("generated_by_run_id").and_then(Value::as_str) == Some(run_id)
            })
        });
    if let Some(proposal) = proposal {
        let proposal_id = proposal
            .get("proposal_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let status = proposal
            .get("proposal_status")
            .and_then(Value::as_str)
            .unwrap_or("pending");
        return json!({
            "runId": run_id,
            "proposalId": proposal_id,
            "status": "proposal_created",
            "proposalStatus": status,
            "detail": format!("Operating Map proposal {proposal_id} is ready for review."),
        });
    }
    json!({
        "runId": run_id,
        "status": "completed",
        "detail": "Operating Map proposal is ready for review.",
    })
}

pub(super) async fn review_operating_map_proposal(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(proposal_id): Path<String>,
    Json(mut body): Json<Value>,
) -> impl IntoResponse {
    let org_id = shared::org_id_from_headers(&headers);
    body["reviewed_by"] = Value::String(user.user_id.clone());
    let url = format!(
        "{}/v1/wiki/operating-map/proposals/{}/review",
        state.wiki_store_url,
        urlencoding::encode(&proposal_id)
    );
    proxy_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_event_payload_reports_matching_proposal() {
        let payload = operating_map_run_event_payload(
            "run-1",
            &json!({
                "proposals": [{
                    "proposal_id": "proposal-1",
                    "generated_by_run_id": "run-1",
                    "proposal_status": "pending"
                }]
            }),
        );

        assert_eq!(payload["status"], "proposal_created");
        assert_eq!(payload["proposalId"], "proposal-1");
        assert_eq!(payload["proposalStatus"], "pending");
    }
}
