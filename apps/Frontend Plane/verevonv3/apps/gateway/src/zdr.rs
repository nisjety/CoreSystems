//! Server-side Zero Data Retention posture for the same-origin BFF.
//!
//! ZDR has to be decided HERE, on the server, for the same reason model-gateway
//! decides it in `sse.rs` rather than trusting the request body: a gate that
//! lives in the SPA is not a gate. The browser's own temporary-chat marking is
//! an in-memory `Set` — it is gone on reload, absent from a replayed request,
//! absent from any non-SPA client, and one regression away from being wrong.
//! Every BFF path that would persist user content must therefore resolve the
//! posture from something the client cannot take away.
//!
//! Two independent sources, OR-ed — a caller can raise the posture, never lower
//! it, exactly like `claims.effective_zdr(req.zdr)` upstream:
//!
//!   * [`request_zdr`] — the `x-zdr` header or a `zdr: true` body field on this
//!     request. Same normalization the streaming proxy already applies in
//!     `domains::chat::shared::normalized_model_body`.
//!   * [`org_zdr_enabled`] — the organisation's standing posture, read from
//!     org-core (`metadata.interactiveRetention.zdr`), which is where the
//!     workspace ZDR toggle writes it. No client input reaches this at all.
//!
//! A per-thread marker for "this conversation has already had a ZDR turn" lives
//! in `domains::chat::history` instead, because it is keyed by the chat scope
//! that module owns.

use axum::http::HeaderMap;
use reqwest::Method;
use serde_json::Value;

use crate::{
    cache::cache_key, config::AppState, contracts::ActionActor, middleware::AuthenticatedUser,
    upstream::proxy_json,
};

/// How long a successful org-posture lookup is reused. Short on purpose: turning
/// ZDR ON must take effect promptly, and this sits on the write path of chat
/// history and page previews, not on a hot loop.
const ORG_ZDR_CACHE_SECS: u64 = 60;

/// True when this request explicitly carries a ZDR posture.
///
/// Header OR body, never AND: the header is set by the gateway's own outbound
/// path and by trusted callers, the body field is what the SPA sends for a
/// temporary chat. Either one alone means "retain nothing".
pub(crate) fn request_zdr(headers: &HeaderMap, body: Option<&Value>) -> bool {
    if crate::domains::chat::shared::zdr_flag(headers) {
        return true;
    }
    body.and_then(|body| body.get("zdr"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The organisation's standing ZDR posture, from org-core.
///
/// # Fails CLOSED
///
/// An unreachable or erroring org-core yields `true` — "assume ZDR". Callers use
/// this to decide whether they may write user content to Frontend-Plane storage,
/// and the asymmetry between the two failure modes is total: guessing `false`
/// during an outage durably persists content the org may have forbidden us to
/// keep, while guessing `true` merely skips a cache write. Nothing a caller does
/// with this value is load-bearing for the answer itself — the chat turn still
/// streams, the page is still fetched — only the local copy is dropped.
///
/// A failed lookup is deliberately NOT cached, so the posture self-heals on the
/// next request instead of pinning a whole minute of refusals to one blip.
pub(crate) async fn org_zdr_enabled(state: &AppState, user: &AuthenticatedUser) -> bool {
    org_zdr_lookup(state, user).await.treat_as_zdr()
}

/// Tri-state org ZDR posture. `Unknown` means the lookup failed or there is no authorized org
/// scope: it is NOT "not ZDR". Persisting callers must go through [`Self::treat_as_zdr`] (which
/// is true for `Unknown`) so a failed lookup fails closed; callers that need to tell an outage
/// from a real posture (to answer a typed refusal instead of silently skipping a write) match it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OrgZdrPosture {
    Enabled,
    Disabled,
    Unknown,
}

impl OrgZdrPosture {
    /// Fail-closed collapse: only a successful lookup that says "not ZDR" permits persisting.
    pub(crate) fn treat_as_zdr(self) -> bool {
        !matches!(self, OrgZdrPosture::Disabled)
    }
}

/// The lookup behind [`org_zdr_enabled`], with the failure case kept distinguishable. Same
/// cache, same org-core call, same non-caching of failures.
pub(crate) async fn org_zdr_lookup(state: &AppState, user: &AuthenticatedUser) -> OrgZdrPosture {
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let org_id = org_id.trim();
    if org_id.is_empty() {
        // No authorized org scope: there is no org-scoped store to write to
        // either, and every caller of this rejects such a request on its own
        // scope check first. No posture to report.
        return OrgZdrPosture::Unknown;
    }

    let key = cache_key("org-zdr", &[org_id]);
    if let Some(cached) = state.cache.lookup_within(&key, ORG_ZDR_CACHE_SECS).await {
        if let Some(value) = cached.as_bool() {
            return OrgZdrPosture::from_flag(value);
        }
    }

    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!(
        "{}/api/v1/organizations/{}",
        state.org_core_url,
        urlencoding::encode(org_id)
    );
    let (status, axum::Json(organization)) = proxy_json(
        state,
        Method::GET,
        &url,
        None,
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !status.is_success() {
        tracing::warn!(
            org_id,
            %status,
            "org ZDR posture unavailable; assuming Zero Data Retention and skipping local persistence"
        );
        return OrgZdrPosture::Unknown;
    }

    let enabled = read_zdr_flag(&organization);
    state
        .cache
        .store_for_secs(&key, &Value::Bool(enabled), ORG_ZDR_CACHE_SECS)
        .await;
    OrgZdrPosture::from_flag(enabled)
}

impl OrgZdrPosture {
    fn from_flag(zdr: bool) -> Self {
        if zdr {
            OrgZdrPosture::Enabled
        } else {
            OrgZdrPosture::Disabled
        }
    }
}

/// Pull `metadata.interactiveRetention.zdr` out of an org-core organization
/// record, tolerating both the bare record and the `{ data: … }` envelope.
///
/// A record that simply has no `interactiveRetention` block predates the toggle
/// and is not ZDR — unlike a *failed lookup*, this is a successful answer that
/// says "no posture set", and the ZDR add-on is opt-in and off by default.
fn read_zdr_flag(organization: &Value) -> bool {
    organization
        .get("data")
        .unwrap_or(organization)
        .get("metadata")
        .and_then(|metadata| metadata.get("interactiveRetention"))
        .and_then(|retention| retention.get("zdr"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The catalogue refusal a durable Space write answers when the caller's posture is Zero Data
/// Retention (`zdr_durable_mutation_forbidden`) or cannot be established
/// (`zdr_decision_unavailable`). Body shape `{error:{code,message,retryable,action}}`, with the
/// message, retryable flag and action copied from the Model Plane catalogue
/// (`mp-contracts` `zdr_refusal.rs`, pinned for the SPA in `contracts/zdr-refusals.json`) so the
/// browser renders the same text whichever plane refused. Any other code falls back to the
/// stricter durable-mutation refusal.
pub(crate) fn zdr_refusal_body(code: &str) -> Value {
    let (code, message, retryable, action) = if code == ZDR_DECISION_UNAVAILABLE {
        (
            ZDR_DECISION_UNAVAILABLE,
            "The zero data retention settings cannot be confirmed right now. The request was not sent to any model. Try again shortly.",
            true,
            "retry",
        )
    } else {
        (
            ZDR_DURABLE_MUTATION_FORBIDDEN,
            "Zero Data Retention credentials cannot create, change, or delete durable state.",
            false,
            "none",
        )
    };
    serde_json::json!({
        "error": {
            "code": code,
            "message": message,
            "retryable": retryable,
            "action": action,
        }
    })
}

pub(crate) const ZDR_DURABLE_MUTATION_FORBIDDEN: &str = "zdr_durable_mutation_forbidden";
pub(crate) const ZDR_DECISION_UNAVAILABLE: &str = "zdr_decision_unavailable";

/// A durable write of stored content is refused for a Zero Data Retention org, typed, before any
/// Control or model call: the guard behind every `DurableWrite` row of the Space surface registry
/// (`space-surface.v1.json`). `Some` is the response to send.
///
/// A request-level flag (the header or a body `zdr`) counts as ZDR. The org posture comes from
/// org-core and fails closed in its own words: an outage is the typed, retryable
/// `zdr_decision_unavailable` (503), never a guess in either direction.
pub(crate) async fn durable_write_refusal(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: Option<&HeaderMap>,
    body: Option<&Value>,
) -> Option<(axum::http::StatusCode, axum::Json<Value>)> {
    use axum::http::StatusCode;
    let forbidden = || {
        (
            StatusCode::PRECONDITION_FAILED,
            axum::Json(zdr_refusal_body(ZDR_DURABLE_MUTATION_FORBIDDEN)),
        )
    };
    let no_headers = HeaderMap::new();
    if request_zdr(headers.unwrap_or(&no_headers), body) {
        return Some(forbidden());
    }
    match org_zdr_lookup(state, user).await {
        OrgZdrPosture::Enabled => Some(forbidden()),
        OrgZdrPosture::Unknown => Some((
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(zdr_refusal_body(ZDR_DECISION_UNAVAILABLE)),
        )),
        OrgZdrPosture::Disabled => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};
    use serde_json::json;

    fn headers_with(zdr: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-zdr", HeaderValue::from_str(zdr).unwrap());
        headers
    }

    /// The whole point: a caller may RAISE the posture from either side, and
    /// nothing a caller sends can lower it.
    #[test]
    fn either_the_header_or_the_body_alone_establishes_zdr() {
        assert!(request_zdr(&headers_with("true"), None));
        assert!(request_zdr(&headers_with("1"), None));
        assert!(request_zdr(
            &HeaderMap::new(),
            Some(&json!({ "zdr": true }))
        ));
        // A body that says `false` cannot cancel the header.
        assert!(request_zdr(
            &headers_with("true"),
            Some(&json!({ "zdr": false }))
        ));
    }

    #[test]
    fn absent_or_false_on_both_sides_is_not_zdr() {
        assert!(!request_zdr(&HeaderMap::new(), None));
        assert!(!request_zdr(&HeaderMap::new(), Some(&json!({}))));
        assert!(!request_zdr(
            &headers_with("false"),
            Some(&json!({ "zdr": false }))
        ));
        // A non-boolean `zdr` is not an assertion of ZDR; the model boundary
        // rejects the malformed value on its own path.
        assert!(!request_zdr(
            &HeaderMap::new(),
            Some(&json!({ "zdr": "true" }))
        ));
    }

    #[test]
    fn org_posture_is_read_through_both_record_shapes() {
        let bare = json!({ "metadata": { "interactiveRetention": { "zdr": true } } });
        let enveloped = json!({ "data": bare.clone() });
        assert!(read_zdr_flag(&bare));
        assert!(read_zdr_flag(&enveloped));
    }

    /// A successful lookup with no posture recorded means the opt-in add-on is
    /// off — distinct from a FAILED lookup, which `org_zdr_enabled` treats as
    /// ZDR without ever reaching this function.
    #[test]
    fn an_org_with_no_recorded_posture_is_not_zdr() {
        assert!(!read_zdr_flag(&json!({})));
        assert!(!read_zdr_flag(&json!({ "metadata": {} })));
        assert!(!read_zdr_flag(
            &json!({ "metadata": { "interactiveRetention": {} } })
        ));
        assert!(!read_zdr_flag(
            &json!({ "metadata": { "interactiveRetention": { "zdr": false } } })
        ));
    }

    #[test]
    fn treat_as_zdr_is_true_for_enabled_and_unknown_and_false_only_for_disabled() {
        assert!(OrgZdrPosture::Enabled.treat_as_zdr());
        assert!(OrgZdrPosture::Unknown.treat_as_zdr());
        assert!(!OrgZdrPosture::Disabled.treat_as_zdr());
    }

    fn test_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "User One".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    /// A lookup against an unreachable org-core is `Unknown` (an outage, not a posture) and the
    /// fail-closed collapse reads it as ZDR.
    #[tokio::test]
    async fn an_unreachable_org_core_is_unknown_and_fails_closed() {
        let state = AppState::for_tests();
        let user = test_user();
        assert_eq!(org_zdr_lookup(&state, &user).await, OrgZdrPosture::Unknown);
        assert!(org_zdr_enabled(&state, &user).await);
    }

    /// No authorized org scope is also `Unknown`, without any call.
    #[tokio::test]
    async fn no_authorized_org_scope_is_unknown() {
        let state = AppState::for_tests();
        let mut user = test_user();
        user.authorized_membership = None;
        assert_eq!(org_zdr_lookup(&state, &user).await, OrgZdrPosture::Unknown);
    }

    #[test]
    fn refusal_bodies_use_the_catalogue_shape() {
        let forbidden = zdr_refusal_body(ZDR_DURABLE_MUTATION_FORBIDDEN);
        assert_eq!(forbidden["error"]["code"], "zdr_durable_mutation_forbidden");
        assert_eq!(forbidden["error"]["retryable"], false);
        assert_eq!(forbidden["error"]["action"], "none");
        let unavailable = zdr_refusal_body(ZDR_DECISION_UNAVAILABLE);
        assert_eq!(unavailable["error"]["code"], "zdr_decision_unavailable");
        assert_eq!(unavailable["error"]["retryable"], true);
        assert_eq!(unavailable["error"]["action"], "retry");
        // Any other code falls back to the stricter refusal.
        assert_eq!(
            zdr_refusal_body("something_else")["error"]["code"],
            "zdr_durable_mutation_forbidden"
        );
    }
}
