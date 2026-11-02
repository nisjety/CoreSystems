use axum::{
    extract::{DefaultBodyLimit, Extension, Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::{post, MethodRouter},
    Json,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    config::AppState,
    domains::chat::shared,
    middleware::AuthenticatedUser,
    upstream::{proxy_sse_stream_with_browser, proxy_sse_stream_with_session},
};

/// Default request body ceiling of the chat stream route, and only that route.
///
/// Attachments ride the body as base64, so axum's 2 MiB default refused two
/// images the model-gateway attachment policy admits. This matches
/// model-gateway's `CHAT_STREAM_BODY_LIMIT_BYTES` default; the policy it
/// declares on `/api/v1/models` is the number the client reads, this is only
/// the transport room for it.
pub(super) const CHAT_STREAM_BODY_LIMIT_BYTES: usize = 12 * 1024 * 1024;
/// Room for the conversation around the attachments in one request body.
const STREAM_BODY_HEADROOM_BYTES: usize = 4 * 1024 * 1024;

/// The body limit of the stream route. The same two variables model-gateway reads
/// give the same number in both hops: the explicit limit, raised when the
/// declared attachment total (as base64) plus the conversation would not fit, so
/// a raised cap never fails as a bare 413 in the wrong hop.
fn stream_body_limit_bytes() -> usize {
    stream_body_limit_with(&|key| std::env::var(key).ok())
}

fn stream_body_limit_with(get: &dyn Fn(&str) -> Option<String>) -> usize {
    let positive = |key: &str| {
        get(key)
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
    };
    let configured =
        positive("CHAT_STREAM_BODY_LIMIT_BYTES").unwrap_or(CHAT_STREAM_BODY_LIMIT_BYTES);
    match positive("CHAT_ATTACHMENT_MAX_TOTAL_BYTES") {
        Some(total) => configured.max(
            total
                .div_ceil(3)
                .saturating_mul(4)
                .saturating_add(STREAM_BODY_HEADROOM_BYTES),
        ),
        None => configured,
    }
}

/// The stream route with its own body limit, so tests drive the same wiring.
pub(super) fn stream_route() -> MethodRouter<AppState> {
    post(stream_chat).layer(DefaultBodyLimit::max(stream_body_limit_bytes()))
}

async fn stream_chat(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let token = shared::model_token(&state, &user, &headers).await;
    let data_plane_token = shared::data_plane_token(&state, &user, &headers).await;
    let ingestion_token = shared::ingestion_token(&state, &user, &headers).await;
    // F-14 (apps/verevon-web/plans/system-audits/CHAT_PARITY_AUDIT_2026-09-15.md §3.9): best-effort, not
    // `required_*` — a mint failure must degrade the PII/injection-defense
    // policy lookup (model-gateway fails closed and redacts), not the turn.
    let capability_token = shared::best_effort_capability_token(&state, &user, &headers).await;
    let inference_token = match shared::required_inference_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error).into_response(),
    };
    let execution_token = match shared::required_execution_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error).into_response(),
    };
    let cost_token = match shared::required_cost_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error).into_response(),
    };
    let session_token = match shared::required_session_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error).into_response(),
    };
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let org_name = crate::domains::auth::resolve_org_name(&state, &user, &org_id).await;
    let url = format!("{}/v1/invoke/stream", state.model_gateway_url);
    let outbound_body = shared::normalized_model_body(body, &headers);
    let mut outbound_body = super::support::enrich_model_body(&state, &user, outbound_body).await;
    if let Err(message) = super::history::enforce_support_thread_policy(&mut outbound_body) {
        return shared::invalid_chat_request(message).into_response();
    }
    // Must run before `inject_personal_thread_context`, which is the call
    // that removes the Space reference fields this one still needs to read.
    if let Err((status, body)) = crate::domains::spaces::inject_mentioned_space_agent_persona(
        &state,
        &user,
        &org_id,
        &mut outbound_body,
    )
    .await
    {
        return (status, body).into_response();
    }
    // ADR-0003 — same ordering requirement as the mention persona above: this
    // only peeks at `space_ref` for the Space layer, so it must run before
    // `inject_personal_thread_context` consumes it.
    if let Err((status, body)) = crate::domains::spaces::inject_authored_instructions(
        &state,
        &user,
        &org_id,
        &mut outbound_body,
    )
    .await
    {
        return (status, body).into_response();
    }
    // Persona spec — user layer, independent of org/Space resolution.
    if let Err((status, body)) =
        crate::domains::spaces::inject_persona_instructions(&state, &user, &mut outbound_body)
            .await
    {
        return (status, body).into_response();
    }
    if let Err((status, body)) = crate::domains::spaces::inject_personal_thread_context(
        &state,
        &user,
        &org_id,
        &mut outbound_body,
    )
    .await
    {
        return (status, body).into_response();
    }
    let outbound_body =
        shared::with_identity_context(outbound_body, &user.user_name, &org_name);
    let mut outbound_body =
        shared::with_viewer_locale(outbound_body, state.space_features.chat_locale_hint);
    // Before the turn goes out, not after: a ZDR turn that fails midway is still
    // a ZDR turn, so the posture has to be settled on the body that is sent.
    shared::apply_org_zdr_posture(&state, &user, &mut outbound_body).await;
    // Read the EFFECTIVE posture off that body rather than the header. The
    // header alone misses both the SPA's body flag and the org's standing
    // posture, so this argument used to disagree with the `zdr` field sitting
    // in the very payload it accompanies.
    let effective_zdr = outbound_body
        .get("zdr")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // After `inject_personal_thread_context`, not with the other bearers above:
    // it is only minted once Control has actually scoped this turn to a Space.
    let sandbox_token = shared::sandbox_token(&state, &user, &headers, &outbound_body).await;
    // The browser credential of a Do turn that may use the agent browser: the same rule as the
    // sandbox credential, and never on a zero-retention turn (WS21-S2).
    let browser_token =
        shared::browser_token(&state, &user, &headers, &outbound_body, effective_zdr).await;
    proxy_sse_stream_with_browser(
        &state,
        Method::POST,
        &url,
        Some(outbound_body),
        token.as_deref(),
        data_plane_token.as_deref(),
        capability_token.as_deref(),
        Some(&inference_token),
        Some(&execution_token),
        Some(&cost_token),
        Some(&session_token),
        ingestion_token.as_deref(),
        sandbox_token.as_deref(),
        browser_token.as_deref(),
        None,
        Some((&user.user_id, org_id.as_str())),
        effective_zdr,
        &[],
    )
    .await
}

pub(super) async fn resume_stream(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Response {
    let token = shared::model_token(&state, &user, &headers).await;
    let session_token = shared::session_token(&state, &user, &headers).await;
    // F-14 (apps/verevon-web/plans/system-audits/CHAT_PARITY_AUDIT_2026-09-15.md §3.9): see stream_chat above.
    let capability_token = shared::best_effort_capability_token(&state, &user, &headers).await;
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let url = format!(
        "{}/v1/invoke/resume/{}",
        state.model_gateway_url,
        urlencoding::encode(&request_id),
    );
    proxy_sse_stream_with_session(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        session_token.as_deref(),
        capability_token.as_deref(),
        last_event_id.as_deref(),
        Some((&user.user_id, org_id.as_str())),
        false,
    )
    .await
}

pub(super) async fn run_events_stream(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> Response {
    let token = shared::model_token(&state, &user, &headers).await;
    let session_token = shared::session_token(&state, &user, &headers).await;
    // F-14 (apps/verevon-web/plans/system-audits/CHAT_PARITY_AUDIT_2026-09-15.md §3.9): see stream_chat above.
    let capability_token = shared::best_effort_capability_token(&state, &user, &headers).await;
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    // Phase 2: forward `last-event-id` like `resume_stream` already does — a
    // reconnecting client (e.g. a durable browser-agent run) must resume via
    // `after_event_id` on the upstream replay buffer, not silently restart
    // from the live tail. Pre-existing gap on a route Phase 2 newly depends
    // on for browser-agent runs; not new scope.
    //
    // Phase 5: this is a raw byte-level SSE proxy (see `proxy_sse_stream`
    // below) with no event-name allowlist, so model-gateway's two new
    // browser-approval event kinds (`browser_action_approval_required`,
    // `browser_action_decided` — carrying action type/url/selector/risk
    // category/decision) already flow through unchanged; nothing here needed
    // to change for them to reach the SPA.
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let url = format!(
        "{}/v1/runs/{}/events",
        state.model_gateway_url,
        urlencoding::encode(&run_id),
    );
    proxy_sse_stream_with_session(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        session_token.as_deref(),
        capability_token.as_deref(),
        last_event_id.as_deref(),
        Some((&user.user_id, org_id.as_str())),
        false,
    )
    .await
}

#[derive(Debug, Deserialize)]
pub(super) struct RunEventsReplayQuery {
    #[serde(default)]
    after_event_id: Option<String>,
    limit: Option<u32>,
}

/// Read-only durable browser-event projection for a run. The live SSE route
/// above remains the only tail transport; this proxy simply carries the
/// caller's verified model/session audiences to Model Gateway.
pub(super) async fn run_events_replay(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Query(query): Query<RunEventsReplayQuery>,
) -> Response {
    let token = shared::model_token(&state, &user, &headers).await;
    let session_token = match shared::required_session_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error).into_response(),
    };
    // F-14 (apps/verevon-web/plans/system-audits/CHAT_PARITY_AUDIT_2026-09-15.md §3.9): see stream_chat above.
    let capability_token = shared::best_effort_capability_token(&state, &user, &headers).await;
    let mut url = format!(
        "{}/v1/runs/{}/events/replay",
        state.model_gateway_url,
        urlencoding::encode(&run_id),
    );
    let mut params = Vec::new();
    if let Some(cursor) = query
        .after_event_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        params.push(format!("after_event_id={}", urlencoding::encode(cursor)));
    }
    if let Some(limit) = query.limit {
        params.push(format!("limit={limit}"));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }
    let (status, body) = shared::proxy_model_json_with_session_and_capability(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&session_token),
        capability_token.as_deref(),
        &user,
    )
    .await;
    (status, body).into_response()
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        extract::{Extension, Request},
        http::StatusCode,
        Router,
    };
    use tower::ServiceExt;

    use super::{stream_body_limit_with, stream_route, CHAT_STREAM_BODY_LIMIT_BYTES};
    use crate::middleware::{AuthenticatedUser, AuthorizedMembership};

    fn member() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "Chat User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    fn app() -> Router {
        Router::new()
            .route("/api/v1/chat/stream", stream_route())
            .layer(Extension(member()))
            .with_state(crate::tests::test_state(false))
    }

    fn post_json(bytes: usize) -> Request {
        let padding = "A".repeat(bytes);
        Request::builder()
            .method("POST")
            .uri("/api/v1/chat/stream")
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"content":"x","pad":"{padding}"}}"#)))
            .expect("request")
    }

    #[tokio::test]
    async fn the_chat_stream_takes_a_5_mib_body_and_refuses_one_over_its_limit() {
        // Past axum's 2 MiB default and under the stream limit: the body limit lets it
        // through, so whatever answers (the test state has no upstreams) is not a 413.
        let accepted = app().oneshot(post_json(5 * 1024 * 1024)).await.expect("response");
        assert_ne!(accepted.status(), StatusCode::PAYLOAD_TOO_LARGE);

        let refused = app()
            .oneshot(post_json(CHAT_STREAM_BODY_LIMIT_BYTES + 1))
            .await
            .expect("response");
        assert_eq!(refused.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn the_limit_holds_the_declared_attachment_total_as_base64() {
        // 3 MiB of attachments (the model-gateway default) is 4 MiB of base64.
        assert!(CHAT_STREAM_BODY_LIMIT_BYTES > (3 * 1024 * 1024 / 3) * 4);
    }

    fn limit(total: Option<&str>, explicit: Option<&str>) -> usize {
        stream_body_limit_with(&|key| match key {
            "CHAT_ATTACHMENT_MAX_TOTAL_BYTES" => total.map(str::to_owned),
            "CHAT_STREAM_BODY_LIMIT_BYTES" => explicit.map(str::to_owned),
            _ => None,
        })
    }

    #[test]
    fn the_default_limit_is_unchanged_when_nothing_is_raised() {
        assert_eq!(limit(None, None), CHAT_STREAM_BODY_LIMIT_BYTES);
        // Today's declared total (3 MiB) already fits under the default.
        assert_eq!(limit(Some("3145728"), None), CHAT_STREAM_BODY_LIMIT_BYTES);
        assert_eq!(limit(Some("0"), Some("junk")), CHAT_STREAM_BODY_LIMIT_BYTES);
    }

    #[test]
    fn a_raised_attachment_total_raises_the_limit_to_hold_it_as_base64() {
        let total = 24 * 1024 * 1024;
        let derived = limit(Some(&total.to_string()), None);
        assert!(derived >= total / 3 * 4 + 4 * 1024 * 1024);
        assert!(derived > CHAT_STREAM_BODY_LIMIT_BYTES);
    }

    #[test]
    fn an_explicit_limit_above_the_derived_one_wins() {
        assert_eq!(limit(Some("3145728"), Some("33554432")), 32 * 1024 * 1024);
    }
}
