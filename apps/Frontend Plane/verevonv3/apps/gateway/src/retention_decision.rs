//! The BFF's retention-decision client and the one `turn_retention` entry (ZDR.md section 5.3,
//! task Z-19a).
//!
//! The BFF asks, Auth Core decides. For a subscription chat turn in a Zero Data Retention
//! organisation, whether the turn runs as ZDR, runs persistent, or runs under a signed per-connection
//! waiver is Auth Core's answer (`POST /api/retention/zdr-decision`), never the gateway's. This module
//! only builds the question and relays the answer:
//!
//!   * [`policy_token`] mints the org-bound `retention:zdr-decide` policy token the question needs,
//!     from the registered `verevon-gateway-retention-decision` principal.
//!   * [`decide`] asks the question with that token and the user's own session cookie.
//!   * [`turn_retention`] is the one entry a handler calls. It reads only the raw request body and
//!     headers, so it can run before any bearer of the turn is minted.
//!   * [`continuation_retention`] is the entry a continuation handler (approve, decide, resume,
//!     continue, cancel, plan approval, todo status, approval preview) calls (ZDR-Z-19e, D-ZDR-15).
//!     A continuation never inherits a waiver: only a fresh decision for the connection the SPA names
//!     waives its bearers, and session-core checks that waiver against the run's stored receipt.
//!
//! Dark: with `ZDR_WAIVER_UI_ENABLED` off (`AppState::zdr_waiver_ui_enabled`) [`turn_retention`]
//! answers [`TurnRetention::Unchanged`] and makes no call. The chat stream and JSON handlers call it
//! first and mint every bearer of the turn from its answer (`chat::shared::mint_turn_bearers`, Z-19b).
//!
//! # ZDR
//!
//! The BFF never writes `zdr: false` and cannot create a waiver: only Auth Core's signed,
//! session-bound decision can. Every doubt refuses before anything is sent upstream: an unconfigured
//! credential, an org whose posture cannot be read, a timeout, an unreadable answer. Nothing is
//! stored. Logs carry ids, statuses and closed codes only, never the policy token or the decision
//! token. The decision is never asked for speech, dictation, browser, scheduled, inbox or support AI
//! routes, and never for a Space-scoped turn.

use std::time::Duration;

use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    audience_tokens::CachedToken,
    config::AppState,
    domains::spaces::human_posts::{space_scope_for_thread, SpaceThreadScope},
    envelope::error,
    middleware::AuthenticatedUser,
    zdr::{self, OrgPosture, ZDR_DECISION_UNAVAILABLE},
};

/// The registered Auth Core service principal this module mints its policy token as.
const SERVICE_ID: &str = "verevon-gateway-retention-decision";
/// The one scope the policy token asks for.
const DECIDE_SCOPE: &str = "retention:zdr-decide";
/// The audit reason Auth Core records on the policy token.
const TOKEN_REASON: &str = "retention_decision";
/// Ceiling on each Auth Core call (policy token and decision).
const CALL_TIMEOUT: Duration = Duration::from_secs(3);
/// A cached policy token is dropped this long before Auth Core says it expires.
const TOKEN_EXPIRY_MARGIN_SECS: u64 = 15;
/// The provider string of a subscription turn.
const SUBSCRIPTION_PROVIDER: &str = "openai-codex-subscription";
/// Cache namespace of the policy token; the token is org-bound, so the key carries the org.
const TOKEN_CACHE_NAMESPACE: &str = "retention-decision";

/// The refusal codes Auth Core's decision endpoint can answer (a subset of the ZDR catalogue). A 412
/// or 503 carrying any other code is not relayed.
pub(crate) const DECISION_REFUSAL_CODES: [&str; 6] = [
    "zdr_connection_not_acknowledged",
    "zdr_connection_not_supported",
    "zdr_explicit_request_not_supported_by_connection",
    "zdr_decision_unavailable",
    "zdr_waiver_route_mismatch",
    "zdr_posture_mismatch",
];

/// Body keys that name a Space or room. A turn carrying any of them is a Space turn.
const SPACE_SELECTION_KEYS: [&str; 8] = [
    "space_ref",
    "spaceRef",
    "space_id",
    "spaceId",
    "room_ref",
    "roomRef",
    "room_id",
    "roomId",
];

/// What the turn's retention is, as far as the BFF asks.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum TurnRetention {
    /// The BFF did not ask (switch off, not a subscription turn, a Space turn, or the org is not
    /// under ZDR). Every other rule of the turn is exactly what it was.
    Unchanged,
    /// Auth Core decided the turn runs as Zero Data Retention.
    Zdr,
    /// Auth Core decided the turn runs persistent.
    Persistent,
    /// Auth Core signed a per-connection waiver for this turn. The token is Auth Core's signature;
    /// the BFF only carries it.
    Waived { decision_token: String },
}

impl std::fmt::Debug for TurnRetention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unchanged => f.write_str("Unchanged"),
            Self::Zdr => f.write_str("Zdr"),
            Self::Persistent => f.write_str("Persistent"),
            Self::Waived { .. } => f.write_str("Waived { decision_token: <redacted> }"),
        }
    }
}

/// A refusal the handler answers as is: an HTTP status and the typed error body.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Refusal {
    pub(crate) status: StatusCode,
    pub(crate) body: Value,
}

impl Refusal {
    fn unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            body: zdr::zdr_refusal_body(ZDR_DECISION_UNAVAILABLE),
        }
    }
}

/// The policy token could not be had (unconfigured, refused, unreachable, unreadable). The cause is
/// logged as a status only; the caller refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Unavailable;

/// What [`decide`] sends to Auth Core.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DecisionInput<'a> {
    pub(crate) connection_id: Option<&'a str>,
    pub(crate) provider: Option<&'a str>,
    pub(crate) explicit_zdr: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyTokenResponse {
    token: String,
    #[serde(alias = "expiresIn")]
    expires_in_seconds: Option<u64>,
}

fn token_cache_key(org_id: &str) -> String {
    format!("{TOKEN_CACHE_NAMESPACE}\u{1f}{org_id}")
}

/// The org-bound `retention:zdr-decide` policy token for `org_id`, from Auth Core's
/// `control-policy` internal-token route, as the registered gateway principal.
///
/// Cached per org in the shared audience token cache for the returned TTL minus 15 seconds; the
/// token is bound to one org, so the key carries the org and a token is never shared across orgs.
/// A response without a usable TTL is used once and not cached. Any failure is [`Unavailable`].
pub(crate) async fn policy_token(state: &AppState, org_id: &str) -> Result<String, Unavailable> {
    let org_id = org_id.trim();
    if org_id.is_empty() {
        return Err(Unavailable);
    }
    let Some(api_key) = state.retention_decision_service_api_key.as_deref() else {
        tracing::warn!("retention decision credential is not configured");
        return Err(Unavailable);
    };
    let key = token_cache_key(org_id);
    {
        let cache = state.audience_token_cache.lock().await;
        if let Some(token) = cache.get(&key).and_then(CachedToken::live_token) {
            return Ok(token.to_owned());
        }
    }

    let response = state
        .client
        .post(format!(
            "{}/api/control-policy/internal-token",
            state.auth_core_url
        ))
        .header("x-service-id", SERVICE_ID)
        .header("x-service-api-key", api_key)
        .timeout(CALL_TIMEOUT)
        .json(&json!({
            "orgId": org_id,
            "scopes": [DECIDE_SCOPE],
            "reason": TOKEN_REASON,
        }))
        .send()
        .await
        .map_err(|_| {
            tracing::warn!(
                org_id,
                "retention decision policy token: auth-core unreachable"
            );
            Unavailable
        })?;
    let status = response.status();
    if !status.is_success() {
        tracing::warn!(org_id, %status, "retention decision policy token refused");
        return Err(Unavailable);
    }
    let minted = response.json::<PolicyTokenResponse>().await.map_err(|_| {
        tracing::warn!(
            org_id,
            "retention decision policy token: unreadable response"
        );
        Unavailable
    })?;
    let token = minted.token.trim().to_owned();
    if token.is_empty() {
        tracing::warn!(org_id, "retention decision policy token: empty token");
        return Err(Unavailable);
    }
    let ttl_secs = minted
        .expires_in_seconds
        .unwrap_or(0)
        .saturating_sub(TOKEN_EXPIRY_MARGIN_SECS);
    if ttl_secs > 0 {
        let mut cache = state.audience_token_cache.lock().await;
        cache.insert(
            key,
            CachedToken::expiring_in(token.clone(), Duration::from_secs(ttl_secs)),
        );
    }
    Ok(token)
}

/// Whether `body` has a non-empty string under `key`.
fn non_empty_str<'a>(body: &'a Value, key: &str) -> Option<&'a str> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn cookie_header(headers: &HeaderMap) -> &str {
    headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
}

/// Ask Auth Core for this turn's retention decision.
///
/// POSTs `{connection_id, provider, explicit_zdr}` to `/api/retention/zdr-decision` with the org-bound
/// policy token as the bearer and the user's own cookie (Auth Core requires both factors and that
/// they agree on the org), under a 3 second timeout. A `zdr`, `persistent` or `waived` answer maps
/// to [`TurnRetention`]. A 412 or 503 whose body carries a catalogue `error.code` is relayed with its
/// status and body unchanged. Anything else (a timeout, another status, a body that is not the
/// envelope, a waiver without a token) is a 503 `zdr_decision_unavailable`.
pub(crate) async fn decide(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: DecisionInput<'_>,
) -> Result<TurnRetention, Refusal> {
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let token = policy_token(state, &org_id)
        .await
        .map_err(|Unavailable| Refusal::unavailable())?;

    let response = state
        .client
        .post(format!(
            "{}/api/retention/zdr-decision",
            state.auth_core_url
        ))
        .header("authorization", format!("Bearer {token}"))
        .header("cookie", cookie_header(headers))
        .timeout(CALL_TIMEOUT)
        .json(&json!({
            "connection_id": input.connection_id,
            "provider": input.provider,
            "explicit_zdr": input.explicit_zdr,
        }))
        .send()
        .await
        .map_err(|_| {
            tracing::warn!(org_id = %org_id, "retention decision: auth-core unreachable");
            Refusal::unavailable()
        })?;
    let status = response.status();
    let body = response.json::<Value>().await.map_err(|_| {
        tracing::warn!(org_id = %org_id, %status, "retention decision: unreadable response");
        Refusal::unavailable()
    })?;

    if status == StatusCode::OK {
        return interpret_decision(&body).ok_or_else(|| {
            tracing::warn!(org_id = %org_id, "retention decision: unrecognised answer");
            Refusal::unavailable()
        });
    }
    if matches!(
        status,
        StatusCode::PRECONDITION_FAILED | StatusCode::SERVICE_UNAVAILABLE
    ) {
        let code = body
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_str);
        if let Some(code) = code.filter(|code| DECISION_REFUSAL_CODES.contains(code)) {
            tracing::info!(org_id = %org_id, %status, code, "retention decision refused");
            return Err(Refusal { status, body });
        }
    }
    tracing::warn!(org_id = %org_id, %status, "retention decision: unexpected answer");
    Err(Refusal::unavailable())
}

/// Read the `{ data: { decision, decision_token? } }` envelope. `None` for anything else.
fn interpret_decision(body: &Value) -> Option<TurnRetention> {
    let data = body.get("data")?;
    match data.get("decision").and_then(Value::as_str)? {
        "zdr" => Some(TurnRetention::Zdr),
        "persistent" => Some(TurnRetention::Persistent),
        "waived" => {
            let token = non_empty_str(data, "decision_token")?;
            Some(TurnRetention::Waived {
                decision_token: token.to_owned(),
            })
        }
        _ => None,
    }
}

/// A subscription turn: the subscription provider with a non-empty `subscription_connection_id`
/// (the same test model-gateway's `normalize.rs` applies).
fn subscription_connection(body: &Value) -> Option<&str> {
    if non_empty_str(body, "provider") != Some(SUBSCRIPTION_PROVIDER) {
        return None;
    }
    non_empty_str(body, "subscription_connection_id")
}

/// True when the body names a Space or room, by the browser's selection or by injected authority.
fn names_a_space(body: &Value) -> bool {
    SPACE_SELECTION_KEYS
        .iter()
        .any(|key| non_empty_str(body, key).is_some())
        || crate::domains::chat::shared::is_space_scoped_turn(body)
}

/// The body (or query) key that names the connection a continuation's run was waived for.
const CONTINUATION_CONNECTION_KEY: &str = "subscription_connection_id";
/// The body (or query) key that names the provider of that connection.
const CONTINUATION_PROVIDER_KEY: &str = "provider";
/// The longest connection id or provider a continuation may name; the same bound the chat turn's
/// connection ids obey.
const CONTINUATION_NAME_MAX_CHARS: usize = 128;

/// The connection the SPA says a continuation's run was waived for (ZDR.md section 5.4, D-ZDR-15).
///
/// It is only a question put to Auth Core, never an authority: the run's own receipt, checked by
/// session-core, decides whether a waiver for this connection can advance the run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ContinuationConnection {
    /// The subscription connection id, 1 to 128 characters with no whitespace.
    pub(crate) connection_id: String,
    /// The connection's provider, 1 to 128 characters with no whitespace.
    pub(crate) provider: String,
}

impl ContinuationConnection {
    /// The connection when both values are present and valid (non-empty, at most 128 characters, no
    /// whitespace); otherwise `None`, and the continuation is treated as naming no connection.
    fn parse(connection_id: Option<&str>, provider: Option<&str>) -> Option<Self> {
        let valid = |value: &str| {
            !value.is_empty()
                && value.chars().count() <= CONTINUATION_NAME_MAX_CHARS
                && !value.chars().any(char::is_whitespace)
        };
        let connection_id = connection_id.filter(|value| valid(*value))?;
        let provider = provider.filter(|value| valid(*value))?;
        Some(Self {
            connection_id: connection_id.to_owned(),
            provider: provider.to_owned(),
        })
    }

    /// The pair [`continuation_retention`] takes: connection id, then provider.
    pub(crate) fn as_pair(&self) -> (&str, &str) {
        (self.connection_id.as_str(), self.provider.as_str())
    }
}

/// Read the connection a continuation names from its JSON body, and remove both keys so neither is
/// ever forwarded upstream. With `ZDR_WAIVER_UI_ENABLED` off the body is left untouched and `None`
/// is returned, so the switch-off path is byte-identical to before. A body that is not an object,
/// or that names an incomplete or invalid pair, answers `None`.
pub(crate) fn take_continuation_connection(
    state: &AppState,
    body: &mut Value,
) -> Option<ContinuationConnection> {
    if !state.zdr_waiver_ui_enabled {
        return None;
    }
    let object = body.as_object_mut()?;
    let connection_id = object.remove(CONTINUATION_CONNECTION_KEY);
    let provider = object.remove(CONTINUATION_PROVIDER_KEY);
    ContinuationConnection::parse(
        connection_id.as_ref().and_then(Value::as_str),
        provider.as_ref().and_then(Value::as_str),
    )
}

/// The query string of a GET or SSE continuation route (`subscription_connection_id`, `provider`).
/// Every other query key is ignored.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ContinuationQuery {
    /// The connection id the SPA names.
    #[serde(default)]
    pub(crate) subscription_connection_id: Option<String>,
    /// The provider of that connection.
    #[serde(default)]
    pub(crate) provider: Option<String>,
}

impl ContinuationQuery {
    /// The connection the query names, under the same validation as the body form. `None` with the
    /// switch off, or when the pair is incomplete or invalid.
    pub(crate) fn connection(&self, state: &AppState) -> Option<ContinuationConnection> {
        if !state.zdr_waiver_ui_enabled {
            return None;
        }
        ContinuationConnection::parse(
            self.subscription_connection_id.as_deref(),
            self.provider.as_deref(),
        )
    }
}

/// The one entry a continuation handler calls to ask whether the run it would advance is waived
/// (D-ZDR-15). A continuation (an approval or rejection, a plan approval or rejection, a todo update,
/// a resume, a continue, a cancel) never inherits a waiver: it is waived only by a fresh decision
/// from Auth Core for the connection `connection` names, and session-core then checks that waiver
/// against the receipt of the run it would advance. Nothing is cached across requests.
///
/// `connection` is `(connection_id, provider)`.
///
/// In order:
///   1. Switch off, or no connection named: `Ok(Unchanged)`, no call. The continuation keeps today's
///      path, and model-gateway refuses a ZDR credential for durable work as it does today.
///   2. The org posture is read: `Unknown` is 503 `zdr_decision_unavailable` with no call; `NotZdr`
///      is `Unchanged` with no call; `Zdr` calls [`decide`] with `explicit_zdr` false. There is no
///      Space step: the run's receipt already fixes its Space.
///
/// Only [`TurnRetention::Waived`] changes what the handler sends; every other answer is today's path.
/// An Auth Core refusal (for example `zdr_connection_not_acknowledged` after a revoke) is returned
/// as it came, and the handler sends nothing upstream.
pub(crate) async fn continuation_retention(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    connection: Option<(&str, &str)>,
) -> Result<TurnRetention, Refusal> {
    if !state.zdr_waiver_ui_enabled {
        return Ok(TurnRetention::Unchanged);
    }
    let Some((connection_id, provider)) = connection else {
        return Ok(TurnRetention::Unchanged);
    };
    match zdr::org_posture(state, user).await {
        OrgPosture::Unknown => Err(Refusal::unavailable()),
        OrgPosture::NotZdr => Ok(TurnRetention::Unchanged),
        OrgPosture::Zdr => {
            decide(
                state,
                user,
                headers,
                DecisionInput {
                    connection_id: Some(connection_id),
                    provider: Some(provider),
                    explicit_zdr: false,
                },
            )
            .await
        }
    }
}

/// The one entry a handler calls to decide a turn's retention. Reads only the raw request body and
/// headers, so it can run before any bearer of the turn is minted and one answer serves them all.
///
/// In order:
///   1. Switch off, or not a subscription turn: `Ok(Unchanged)`, no call.
///   2. A Space-scoped body never asks: `Unchanged` (the org floor stays and model-gateway refuses
///      the subscription route as it does today).
///   3. A follow-up with a `thread_id` and no Space field: with `SPACE_THREAD_SCOPE_RESOLVE` on, the
///      thread's Space is resolved here (the lookup `resolve_thread_space_ref` uses later). A Space
///      gives `Unchanged`, no Space continues, a thread the caller cannot see is 403
///      `space_thread_not_visible`, a failed lookup is 503 `zdr_decision_unavailable`. With the flag
///      off the turn is organisation-scoped everywhere and continues.
///   4. `explicit_zdr` is read (header or body) before anything can raise it, then the org posture:
///      `Unknown` is 503 `zdr_decision_unavailable` with no call; `NotZdr` is `Unchanged` with no
///      call; `Zdr` calls [`decide`], and an unconfigured credential is 503.
pub(crate) async fn turn_retention(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    body: &Value,
) -> Result<TurnRetention, Refusal> {
    if !state.zdr_waiver_ui_enabled {
        return Ok(TurnRetention::Unchanged);
    }
    let Some(connection_id) = subscription_connection(body) else {
        return Ok(TurnRetention::Unchanged);
    };
    if names_a_space(body) {
        return Ok(TurnRetention::Unchanged);
    }
    if state.space_features.thread_scope_resolve {
        if let Some(thread_id) = non_empty_str(body, "thread_id") {
            match space_scope_for_thread(state, user, headers, thread_id).await {
                Ok(SpaceThreadScope::Space(_)) => return Ok(TurnRetention::Unchanged),
                Ok(SpaceThreadScope::NoSpace) => {}
                Ok(SpaceThreadScope::NotVisible) => {
                    tracing::warn!(
                        thread_id,
                        "retention decision: thread not visible to caller"
                    );
                    return Err(Refusal {
                        status: StatusCode::FORBIDDEN,
                        body: error(
                            "space_thread_not_visible",
                            "This conversation is not available to you.",
                        ),
                    });
                }
                Err((status, _)) => {
                    tracing::warn!(
                        thread_id,
                        upstream_status = status.as_u16(),
                        "retention decision: thread space lookup failed"
                    );
                    return Err(Refusal::unavailable());
                }
            }
        }
    }

    let explicit_zdr = zdr::request_zdr(headers, Some(body));
    match zdr::org_posture(state, user).await {
        OrgPosture::Unknown => Err(Refusal::unavailable()),
        OrgPosture::NotZdr => Ok(TurnRetention::Unchanged),
        OrgPosture::Zdr => {
            decide(
                state,
                user,
                headers,
                DecisionInput {
                    connection_id: Some(connection_id),
                    provider: Some(SUBSCRIPTION_PROVIDER),
                    explicit_zdr,
                },
            )
            .await
        }
    }
}

#[cfg(test)]
#[path = "retention_decision_tests.rs"]
mod tests;
