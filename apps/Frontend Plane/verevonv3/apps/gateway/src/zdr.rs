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
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let org_id = org_id.trim();
    if org_id.is_empty() {
        // No authorized org scope: there is no org-scoped store to write to
        // either, and every caller of this rejects such a request on its own
        // scope check first. Treat as ZDR rather than inventing a posture.
        return true;
    }

    let key = cache_key("org-zdr", &[org_id]);
    if let Some(cached) = state.cache.lookup_within(&key, ORG_ZDR_CACHE_SECS).await {
        if let Some(value) = cached.as_bool() {
            return value;
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
        return true;
    }

    let enabled = read_zdr_flag(&organization);
    state
        .cache
        .store_for_secs(&key, &Value::Bool(enabled), ORG_ZDR_CACHE_SECS)
        .await;
    enabled
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
}
