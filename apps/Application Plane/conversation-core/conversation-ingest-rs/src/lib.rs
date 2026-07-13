use axum::{
    body::{to_bytes, Body},
    extract::{Extension, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    sync::atomic::{AtomicU64, Ordering},
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub conversation_core_url: String,
    pub ingest_service_token: String,
    pub conversation_core_service_token: String,
    pub delegation_replays: ReplayCache,
    pub client: reqwest::Client,
}

pub type ReplayCache = Arc<Mutex<HashMap<String, DateTime<Utc>>>>;

pub fn new_replay_cache() -> ReplayCache {
    Arc::new(Mutex::new(HashMap::new()))
}

#[derive(Clone, Debug)]
struct VerifiedIngestPrincipal {
    organization_id: String,
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
    #[error("email ingress accepts inbound direction only")]
    InvalidDirection,
}

pub fn build_router(state: AppState) -> Router {
    // Delegation auth runs as MIDDLEWARE (not in the handler) so it precedes
    // body deserialization: an unauthenticated caller gets 401 even for
    // malformed payloads, and never exercises the parse path.
    let ingest = Router::new()
        .route("/internal/ingest/email", post(ingest_email))
        .route("/internal/ingest/normalized-email", post(ingest_email))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_ingest_delegation,
        ));
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(health))
        .merge(ingest)
        .with_state(state)
}

async fn require_ingest_delegation(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let (mut parts, body) = request.into_parts();
    let body = match to_bytes(body, 2 << 20).await {
        Ok(body) => body,
        Err(_) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(serde_json::json!({"error": {"code": "payload_too_large"}})),
            )
                .into_response()
        }
    };
    let principal = match verify_ingest_delegation(&state, &parts, &body).await {
        Ok(principal) => principal,
        Err(()) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": {"code": "unauthorized"}})),
            )
                .into_response()
        }
    };
    parts.extensions.insert(principal);
    next.run(axum::http::Request::from_parts(parts, Body::from(body)))
        .await
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok", "service": "conversation-ingest-rs"}))
}

async fn ingest_email(
    State(state): State<AppState>,
    Extension(principal): Extension<VerifiedIngestPrincipal>,
    Json(raw): Json<RawEmailEvent>,
) -> impl IntoResponse {
    if raw.org_id.trim() != principal.organization_id {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": {"code": "organization_scope_mismatch"}})),
        );
    }
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

    let url = format!(
        "{}/internal/conversation-events",
        state.conversation_core_url
    );
    if !conversation_target_is_configured_origin(&url, &state.conversation_core_url) {
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": {"code": "conversation_core_target_rejected"}})),
        );
    }
    let body = match serde_json::to_vec(&canonical) {
        Ok(body) => body,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": {"code": "event_serialization_failed"}})),
            )
        }
    };
    let timestamp = Utc::now();
    let nonce = delegation_nonce(timestamp);
    let headers = conversation_delegation_headers(
        &state.conversation_core_service_token,
        "POST",
        &url,
        &body,
        &canonical.org_id,
        timestamp,
        &nonce,
    );
    let mut request = state
        .client
        .post(url)
        .header("content-type", "application/json")
        .body(body)
        .timeout(Duration::from_secs(8));
    for (name, value) in headers {
        request = request.header(name, value);
    }
    let response = request.send().await;

    match response {
        Ok(response) => {
            let status =
                StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let response_body = response.bytes().await.unwrap_or_default();
            if status == StatusCode::ACCEPTED {
                if let Some(payload) = parse_conversation_core_ack(status.as_u16(), &response_body)
                {
                    return (StatusCode::ACCEPTED, Json(payload));
                }
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({"error": {"code": "invalid_conversation_core_ack"}})),
                );
            }
            let payload = serde_json::from_slice::<serde_json::Value>(&response_body)
                .unwrap_or_else(
                    |_| serde_json::json!({"error": {"code": "conversation_core_failed"}}),
                );
            (status, Json(payload))
        }
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": {"code": "conversation_core_unavailable"}})),
        ),
    }
}

fn parse_conversation_core_ack(status: u16, body: &[u8]) -> Option<serde_json::Value> {
    if status != StatusCode::ACCEPTED.as_u16() || body.is_empty() || body.len() > (1 << 20) {
        return None;
    }
    let payload = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    let data = payload.get("data")?;
    let conversation_id = data.get("detail")?.get("id")?.as_str()?.trim();
    let message_id = data.get("message")?.get("id")?.as_str()?.trim();
    data.get("created")?.as_bool()?;
    if conversation_id.is_empty() || message_id.is_empty() {
        return None;
    }
    Some(payload)
}

static DELEGATION_NONCE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn delegation_nonce(now: DateTime<Utc>) -> String {
    let counter = DELEGATION_NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let seed = format!(
        "{}:{}:{}",
        now.timestamp_nanos_opt().unwrap_or_default(),
        std::process::id(),
        counter
    );
    URL_SAFE_NO_PAD.encode(Sha256::digest(seed.as_bytes()))
}

#[allow(clippy::too_many_arguments)]
fn conversation_delegation_headers(
    service_token: &str,
    method: &str,
    url: &str,
    body: &[u8],
    organization_id: &str,
    timestamp: DateTime<Utc>,
    nonce: &str,
) -> BTreeMap<String, String> {
    type HmacSha256 = Hmac<Sha256>;

    let timestamp = timestamp.to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    let uri = reqwest::Url::parse(url)
        .map(|parsed| match parsed.query() {
            Some(query) => format!("{}?{query}", parsed.path()),
            None => parsed.path().to_owned(),
        })
        .unwrap_or_default();
    let organization_id = organization_id.trim();
    let body_digest = URL_SAFE_NO_PAD.encode(Sha256::digest(body));
    let canonical = [
        "v2",
        "conversation-ingest",
        "conversation-core",
        timestamp.as_str(),
        nonce,
        method,
        uri.as_str(),
        "",
        organization_id,
        "",
        body_digest.as_str(),
    ]
    .join("\n");
    let mut mac = HmacSha256::new_from_slice(service_token.as_bytes())
        .expect("HMAC accepts arbitrary key lengths");
    mac.update(canonical.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());

    BTreeMap::from([
        ("x-service-id".to_owned(), "conversation-ingest".to_owned()),
        ("x-org-id".to_owned(), organization_id.to_owned()),
        ("x-delegation-timestamp".to_owned(), timestamp),
        ("x-delegation-nonce".to_owned(), nonce.to_owned()),
        ("x-delegation-body-sha256".to_owned(), body_digest),
        ("x-delegation-signature".to_owned(), signature),
    ])
}

fn conversation_target_is_configured_origin(target: &str, configured_base: &str) -> bool {
    let (Ok(target), Ok(base)) = (
        reqwest::Url::parse(target),
        reqwest::Url::parse(configured_base),
    ) else {
        return false;
    };
    target.scheme() == base.scheme()
        && target.host_str() == base.host_str()
        && target.port_or_known_default() == base.port_or_known_default()
}

async fn verify_ingest_delegation(
    state: &AppState,
    parts: &axum::http::request::Parts,
    body: &[u8],
) -> Result<VerifiedIngestPrincipal, ()> {
    type HmacSha256 = Hmac<Sha256>;

    let header = |name: &str| {
        parts
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .unwrap_or_default()
    };
    if header("x-service-id") != "integration-email-worker" {
        return Err(());
    }
    let organization_id = header("x-org-id");
    if organization_id.is_empty() {
        return Err(());
    }
    let timestamp_text = header("x-delegation-timestamp");
    let timestamp = DateTime::parse_from_rfc3339(timestamp_text)
        .map_err(|_| ())?
        .with_timezone(&Utc);
    let now = Utc::now();
    if timestamp < now - chrono::Duration::minutes(2)
        || timestamp > now + chrono::Duration::minutes(2)
    {
        return Err(());
    }
    let nonce = header("x-delegation-nonce");
    if !(16..=128).contains(&nonce.len())
        || !nonce
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(());
    }
    let digest = URL_SAFE_NO_PAD.encode(Sha256::digest(body));
    if header("x-delegation-body-sha256") != digest {
        return Err(());
    }
    let request_uri = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or(parts.uri.path());
    let canonical = [
        "v2",
        "integration-email-worker",
        "conversation-ingest",
        timestamp_text,
        nonce,
        parts.method.as_str(),
        request_uri,
        "",
        organization_id,
        "",
        digest.as_str(),
    ]
    .join("\n");
    let provided_signature = URL_SAFE_NO_PAD
        .decode(header("x-delegation-signature"))
        .map_err(|_| ())?;
    let mut mac =
        HmacSha256::new_from_slice(state.ingest_service_token.as_bytes()).map_err(|_| ())?;
    mac.update(canonical.as_bytes());
    mac.verify_slice(&provided_signature).map_err(|_| ())?;

    let replay_key = format!("integration-email-worker\0{nonce}");
    let mut replays = state.delegation_replays.lock().await;
    replays.retain(|_, expires_at| *expires_at > now);
    if replays.contains_key(&replay_key) {
        return Err(());
    }
    replays.insert(replay_key, timestamp + chrono::Duration::minutes(2));
    Ok(VerifiedIngestPrincipal {
        organization_id: organization_id.to_owned(),
    })
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
    raw.direction = normalize_direction(&raw.direction)?;
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

fn normalize_direction(value: &str) -> Result<String, NormalizeError> {
    match value.trim().to_lowercase().as_str() {
        "" | "inbound" => Ok("inbound".into()),
        _ => Err(NormalizeError::InvalidDirection),
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
    fn normalize_email_event_rejects_outbound_direction() {
        let mut raw = raw_event();
        raw.direction = "outbound".into();
        let error =
            normalize_email_event(raw).expect_err("outbound email must not enter inbound contract");
        assert!(matches!(error, NormalizeError::InvalidDirection));
    }

    #[test]
    fn normalize_email_event_is_deterministic_for_same_refs() {
        let left = normalize_email_event(raw_event()).expect("left");
        let right = normalize_email_event(raw_event()).expect("right");
        assert_eq!(left.idempotency_key, right.idempotency_key);
    }

    #[test]
    fn conversation_delegation_matches_go_cross_language_contract() {
        let body = br#"{"org_id":"org-1"}"#;
        let timestamp = chrono::DateTime::parse_from_rfc3339("2026-07-13T12:00:00+00:00")
            .expect("timestamp")
            .with_timezone(&Utc);
        let headers = conversation_delegation_headers(
            "0123456789abcdef0123456789abcdef",
            "POST",
            "http://conversation-core:3160/internal/conversation-events",
            body,
            "org-1",
            timestamp,
            "fixed-nonce-1234567890",
        );

        assert_eq!(
            headers.get("x-delegation-body-sha256").map(String::as_str),
            Some("YqOazKjaPktfdxVznPrrhX7qEbel9X3ciCClxMerpjg")
        );
        assert_eq!(
            headers.get("x-delegation-signature").map(String::as_str),
            Some("MnM-PPg7tt5ZMVu_SDZkUagwlheYeCoiZw7GHv035ok")
        );
        assert_eq!(
            headers.get("x-service-id").map(String::as_str),
            Some("conversation-ingest")
        );
        assert!(!headers.contains_key("x-internal-api-key"));
    }

    #[test]
    fn conversation_signing_rejects_origin_mismatch() {
        assert!(conversation_target_is_configured_origin(
            "http://conversation-core:3160/internal/conversation-events",
            "http://conversation-core:3160"
        ));
        assert!(!conversation_target_is_configured_origin(
            "http://conversation-core.attacker:3160/internal/conversation-events",
            "http://conversation-core:3160"
        ));
    }

    #[test]
    fn conversation_core_ack_requires_exact_persisted_result_contract() {
        let valid =
            br#"{"data":{"detail":{"id":"conv-1"},"message":{"id":"msg-1"},"created":true}}"#;
        assert!(parse_conversation_core_ack(202, valid).is_some());
        assert!(parse_conversation_core_ack(200, valid).is_none());
        assert!(parse_conversation_core_ack(202, b"").is_none());
        assert!(parse_conversation_core_ack(202, br#"{"data":{"ok":true}}"#).is_none());
    }

    fn test_state() -> AppState {
        AppState {
            conversation_core_url: "http://conversation-core-go.invalid".into(),
            ingest_service_token: "integration-email-worker-test-secret-at-least-32-bytes".into(),
            conversation_core_service_token: "ingest-test-secret-at-least-32-bytes-1".into(),
            delegation_replays: new_replay_cache(),
            client: reqwest::Client::new(),
        }
    }

    enum IngestAuth<'a> {
        None,
        Legacy,
        Signed(&'a str),
    }

    async fn post_ingest(
        router: &Router,
        auth: IngestAuth<'_>,
        body: serde_json::Value,
    ) -> StatusCode {
        use tower::util::ServiceExt;
        let body = body.to_string();
        let mut request = axum::http::Request::builder()
            .method("POST")
            .uri("/internal/ingest/email")
            .header("content-type", "application/json");
        match auth {
            IngestAuth::None => {}
            IngestAuth::Legacy => {
                request = request.header("x-internal-api-key", "fleet-key");
            }
            IngestAuth::Signed(nonce) => {
                let timestamp = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
                let organization_id = serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("org_id")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                    })
                    .unwrap_or_default();
                let digest = URL_SAFE_NO_PAD.encode(Sha256::digest(body.as_bytes()));
                let canonical = [
                    "v2",
                    "integration-email-worker",
                    "conversation-ingest",
                    timestamp.as_str(),
                    nonce,
                    "POST",
                    "/internal/ingest/email",
                    "",
                    organization_id.as_str(),
                    "",
                    digest.as_str(),
                ]
                .join("\n");
                let mut mac = Hmac::<Sha256>::new_from_slice(
                    b"integration-email-worker-test-secret-at-least-32-bytes",
                )
                .expect("test hmac");
                mac.update(canonical.as_bytes());
                request = request
                    .header("x-service-id", "integration-email-worker")
                    .header("x-org-id", organization_id)
                    .header("x-delegation-timestamp", timestamp)
                    .header("x-delegation-nonce", nonce)
                    .header("x-delegation-body-sha256", digest)
                    .header(
                        "x-delegation-signature",
                        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()),
                    );
            }
        }
        let request = request.body(axum::body::Body::from(body)).expect("request");
        router
            .clone()
            .oneshot(request)
            .await
            .expect("response")
            .status()
    }

    #[tokio::test]
    async fn ingest_email_rejects_missing_and_legacy_shared_key_auth() {
        let router = build_router(test_state());
        let status = post_ingest(&router, IngestAuth::None, serde_json::json!({})).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let status = post_ingest(&router, IngestAuth::Legacy, serde_json::json!({})).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn ingest_email_with_valid_delegation_reaches_validation_and_replay_fails() {
        let router = build_router(test_state());
        // Correct delegation + invalid payload → the request passes auth and fails
        // VALIDATION (422), proving auth no longer blocks legitimate callers
        // without needing a live conversation-core.
        let status = post_ingest(
            &router,
            IngestAuth::Signed("fixed-nonce-1234567890"),
            serde_json::json!({"org_id": "org-1", "from": {"name": "", "email": ""}}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

        let replay = post_ingest(
            &router,
            IngestAuth::Signed("fixed-nonce-1234567890"),
            serde_json::json!({"org_id": "org-1", "from": {"name": "", "email": ""}}),
        )
        .await;
        assert_eq!(replay, StatusCode::UNAUTHORIZED);
    }
}
