use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use thiserror::Error;

#[derive(Clone)]
pub struct AppState {
    pub conversation_core_url: String,
    pub internal_api_key: String,
    pub client: reqwest::Client,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct Participant {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct Attachment {
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub size_bytes: i64,
    #[serde(default)]
    pub storage_ref: String,
    #[serde(default)]
    pub provider_ref: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RawEmailEvent {
    #[serde(default)]
    pub idempotency_key: String,
    pub org_id: String,
    #[serde(default)]
    pub connection_id: String,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default)]
    pub provider_event_id: String,
    #[serde(default)]
    pub provider_message_id: String,
    #[serde(default)]
    pub provider_thread_id: String,
    #[serde(default)]
    pub message_id_header: String,
    #[serde(default)]
    pub references_header: String,
    #[serde(default)]
    pub in_reply_to_header: String,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub subject: String,
    pub from: Participant,
    #[serde(default)]
    pub to: Vec<Participant>,
    #[serde(default)]
    pub body_text: String,
    #[serde(default)]
    pub body_html: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub occurred_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct CanonicalEvent {
    pub idempotency_key: String,
    pub org_id: String,
    pub connection_id: String,
    pub provider: String,
    pub provider_event_id: String,
    pub provider_message_id: String,
    pub provider_thread_id: String,
    pub direction: String,
    pub subject: String,
    pub from: Participant,
    pub to: Vec<Participant>,
    pub body_text: String,
    pub body_html: String,
    pub attachments: Vec<Attachment>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum NormalizeError {
    #[error("org_id is required")]
    MissingOrg,
    #[error("sender is required")]
    MissingSender,
    #[error("body_text or body_html is required")]
    MissingBody,
}

pub fn build_router(state: AppState) -> Router {
    // Internal auth runs as MIDDLEWARE (not in the handler) so it precedes
    // body deserialization: an unauthenticated caller gets 401 even for
    // malformed payloads, and never exercises the parse path.
    let ingest = Router::new()
        .route("/internal/ingest/email", post(ingest_email))
        .route("/internal/ingest/normalized-email", post(ingest_email))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_internal_key,
        ));
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(health))
        .merge(ingest)
        .with_state(state)
}

async fn require_internal_key(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if !internal_key_matches(&state.internal_api_key, request.headers()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(
                serde_json::json!({"error": {"code": "unauthorized", "message": "missing or invalid x-internal-api-key"}}),
            ),
        )
            .into_response();
    }
    next.run(request).await
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok", "service": "conversation-ingest-rs"}))
}

async fn ingest_email(
    State(state): State<AppState>,
    Json(raw): Json<RawEmailEvent>,
) -> impl IntoResponse {
    let canonical = match normalize_email_event(raw) {
        Ok(canonical) => canonical,
        Err(error) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(
                    serde_json::json!({"error": {"code": "validation_error", "message": error.to_string()}}),
                ),
            );
        }
    };

    let response = state
        .client
        .post(format!(
            "{}/internal/conversation-events",
            state.conversation_core_url
        ))
        .header("x-internal-api-key", state.internal_api_key)
        .header("x-org-id", canonical.org_id.clone())
        .json(&canonical)
        .timeout(Duration::from_secs(8))
        .send()
        .await;

    match response {
        Ok(response) if response.status().is_success() => {
            let payload = response
                .json::<serde_json::Value>()
                .await
                .unwrap_or_else(|_| serde_json::json!({"data": {"ok": true}}));
            (StatusCode::ACCEPTED, Json(payload))
        }
        Ok(response) => {
            let status =
                StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let payload = response.json::<serde_json::Value>().await.unwrap_or_else(
                |_| serde_json::json!({"error": {"code": "conversation_core_failed"}}),
            );
            (status, Json(payload))
        }
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(
                serde_json::json!({"error": {"code": "conversation_core_unavailable", "message": error.to_string()}}),
            ),
        ),
    }
}

/// Constant-time-ish comparison of the inbound `x-internal-api-key` header
/// against the configured key. An EMPTY configured key rejects everything —
/// the binary refuses to boot without a key unless the operator explicitly
/// sets `ALLOW_INSECURE_DEV_DEFAULTS=1` (see main.rs), and that dev override
/// keeps auth open rather than silently disabling it here.
pub fn internal_key_matches(expected: &str, headers: &HeaderMap) -> bool {
    if expected.is_empty() {
        // Dev-override mode (boot allowed the empty key): accept, matching
        // the pre-hardening behavior only when explicitly opted into.
        return std::env::var("ALLOW_INSECURE_DEV_DEFAULTS").as_deref() == Ok("1");
    }
    let Some(provided) = headers
        .get("x-internal-api-key")
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    constant_time_eq(provided.as_bytes(), expected.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn normalize_email_event(mut raw: RawEmailEvent) -> Result<CanonicalEvent, NormalizeError> {
    raw.org_id = raw.org_id.trim().to_owned();
    if raw.org_id.is_empty() {
        return Err(NormalizeError::MissingOrg);
    }

    raw.provider = normalize_token(&raw.provider);
    raw.connection_id = raw.connection_id.trim().to_owned();
    raw.provider_event_id = raw.provider_event_id.trim().to_owned();
    raw.provider_message_id = raw.provider_message_id.trim().to_owned();
    raw.provider_thread_id = raw.provider_thread_id.trim().to_owned();
    raw.message_id_header = raw.message_id_header.trim().to_owned();
    raw.references_header = raw.references_header.trim().to_owned();
    raw.in_reply_to_header = raw.in_reply_to_header.trim().to_owned();
    raw.direction = normalize_direction(&raw.direction);
    raw.subject = normalize_subject(&raw.subject);
    raw.from.name = raw.from.name.trim().to_owned();
    raw.from.email = raw.from.email.trim().to_lowercase();
    if raw.from.name.is_empty() && raw.from.email.is_empty() {
        return Err(NormalizeError::MissingSender);
    }

    raw.to = raw
        .to
        .into_iter()
        .map(|mut participant| {
            participant.name = participant.name.trim().to_owned();
            participant.email = participant.email.trim().to_lowercase();
            participant
        })
        .filter(|participant| !participant.name.is_empty() || !participant.email.is_empty())
        .collect();

    raw.body_text = raw.body_text.trim().to_owned();
    raw.body_html = raw.body_html.trim().to_owned();
    if raw.body_text.is_empty() && !raw.body_html.is_empty() {
        raw.body_text = html_to_text(&raw.body_html);
    }
    if raw.body_text.is_empty() && raw.body_html.is_empty() {
        return Err(NormalizeError::MissingBody);
    }

    let provider_message_id = first_non_empty(&[
        raw.provider_message_id.as_str(),
        raw.message_id_header.as_str(),
        raw.provider_event_id.as_str(),
    ]);
    let provider_thread_id = first_non_empty(&[
        raw.provider_thread_id.as_str(),
        raw.references_header.as_str(),
        raw.in_reply_to_header.as_str(),
        provider_message_id.as_str(),
    ]);
    let idempotency_key = if raw.idempotency_key.trim().is_empty() {
        digest_key(&[
            raw.org_id.as_str(),
            raw.provider.as_str(),
            raw.connection_id.as_str(),
            raw.provider_event_id.as_str(),
            provider_message_id.as_str(),
            provider_thread_id.as_str(),
        ])
    } else {
        raw.idempotency_key.trim().to_owned()
    };

    Ok(CanonicalEvent {
        idempotency_key,
        org_id: raw.org_id,
        connection_id: raw.connection_id,
        provider: raw.provider,
        provider_event_id: raw.provider_event_id,
        provider_message_id,
        provider_thread_id,
        direction: raw.direction,
        subject: raw.subject,
        from: raw.from,
        to: raw.to,
        body_text: raw.body_text,
        body_html: raw.body_html,
        attachments: raw.attachments,
        occurred_at: raw.occurred_at.unwrap_or_else(Utc::now),
    })
}

fn default_provider() -> String {
    "email".into()
}

fn normalize_token(value: &str) -> String {
    let value = value.trim().to_lowercase();
    if value.is_empty() {
        "email".into()
    } else {
        value
    }
}

fn normalize_direction(value: &str) -> String {
    match value.trim().to_lowercase().as_str() {
        "outbound" => "outbound".into(),
        _ => "inbound".into(),
    }
}

fn normalize_subject(value: &str) -> String {
    let subject = value.trim();
    if subject.is_empty() {
        "(no subject)".into()
    } else {
        subject.into()
    }
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or("")
        .to_owned()
}

fn digest_key(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.trim().as_bytes());
        hasher.update([0]);
    }
    format!("email:{}", hex_lower(&hasher.finalize()))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn html_to_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                output.push(' ');
            }
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_event() -> RawEmailEvent {
        RawEmailEvent {
            idempotency_key: String::new(),
            org_id: "org_1".into(),
            connection_id: "conn_1".into(),
            provider: "Email".into(),
            provider_event_id: "evt_1".into(),
            provider_message_id: String::new(),
            provider_thread_id: String::new(),
            message_id_header: "<msg@example.com>".into(),
            references_header: "<root@example.com>".into(),
            in_reply_to_header: String::new(),
            direction: String::new(),
            subject: " Need help ".into(),
            from: Participant {
                name: " Ada ".into(),
                email: "ADA@example.COM ".into(),
            },
            to: vec![],
            body_text: String::new(),
            body_html: "<p>Hello&nbsp;there</p>".into(),
            attachments: vec![],
            occurred_at: None,
        }
    }

    #[test]
    fn normalize_email_event_derives_keys_and_text() {
        let event = normalize_email_event(raw_event()).expect("normalize");
        assert_eq!(event.provider, "email");
        assert_eq!(event.direction, "inbound");
        assert_eq!(event.from.email, "ada@example.com");
        assert_eq!(event.provider_message_id, "<msg@example.com>");
        assert_eq!(event.provider_thread_id, "<root@example.com>");
        assert_eq!(event.body_text, "Hello there");
        assert!(event.idempotency_key.starts_with("email:"));
    }

    #[test]
    fn normalize_email_event_rejects_missing_body() {
        let mut raw = raw_event();
        raw.body_html.clear();
        let error = normalize_email_event(raw).expect_err("missing body");
        assert!(matches!(error, NormalizeError::MissingBody));
    }

    #[test]
    fn normalize_email_event_is_deterministic_for_same_refs() {
        let left = normalize_email_event(raw_event()).expect("left");
        let right = normalize_email_event(raw_event()).expect("right");
        assert_eq!(left.idempotency_key, right.idempotency_key);
    }

    fn test_state() -> AppState {
        AppState {
            conversation_core_url: "http://conversation-core-go.invalid".into(),
            internal_api_key: "fleet-key".into(),
            client: reqwest::Client::new(),
        }
    }

    async fn post_ingest(router: Router, key: Option<&str>, body: serde_json::Value) -> StatusCode {
        use tower::util::ServiceExt;
        let mut request = axum::http::Request::builder()
            .method("POST")
            .uri("/internal/ingest/email")
            .header("content-type", "application/json");
        if let Some(key) = key {
            request = request.header("x-internal-api-key", key);
        }
        let request = request
            .body(axum::body::Body::from(body.to_string()))
            .expect("request");
        router.oneshot(request).await.expect("response").status()
    }

    #[tokio::test]
    async fn ingest_email_rejects_missing_internal_key() {
        let status = post_ingest(build_router(test_state()), None, serde_json::json!({})).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn ingest_email_rejects_wrong_internal_key() {
        let status = post_ingest(
            build_router(test_state()),
            Some("wrong-key"),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn ingest_email_with_valid_key_reaches_validation() {
        // Correct key + invalid payload → the request passes auth and fails
        // VALIDATION (422), proving auth no longer blocks legitimate callers
        // without needing a live conversation-core.
        let status = post_ingest(
            build_router(test_state()),
            Some("fleet-key"),
            serde_json::json!({"org_id": "", "from": {"name": "", "email": ""}}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
}
