//! Q3-E3.2-e3-gw: the Frontend Plane's door to the viewer's own per-origin browser consent
//! decisions, so the settings view can list them and revoke one.
//!
//! user-core (Control Plane) stores the decisions per person and serves them at
//! `/api/v1/settings/browser-consent` (list) and `/api/v1/settings/browser-consent/origins`
//! (record and revoke). The generic `/api/v1/settings/{key}` route does not reach that sub-path,
//! so this module is the BFF's only door to it. A decision is made from the approval card in the
//! Model Plane, so there is no PUT here: the viewer can read what was stored and forget a row.
//!
//! Identity: the person is the session's own user. The forwarded identity headers are minted from
//! the verified session by `proxy_json`; no user id is read from the path, the query or a body.
//!
//! The revoke route forwards only the four known query keys (`startHost`, `scope`,
//! `conversationId`, `turnId`), each checked here, and refuses any other key or a repeated key
//! with the typed envelope instead of dropping it. user-core still owns every semantic rule (the
//! reserved host suffixes, the scope bindings), so its typed answers pass through unchanged.
//!
//! Off by default: the routes exist only when `AppState.browser_consent_api` is set, which is read
//! once from `BROWSER_CONSENT_API` (exactly `true`) when the state is built. user-core has its own
//! switch and answers `browser_consent_disabled` while that is off.
//!
//! Zero data retention: only hosts and ids pass through here, and nothing is stored or logged.

use axum::{
    extract::{Extension, RawQuery, State},
    routing::{delete, get},
    Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::error_with_details,
    middleware::{require_session, AuthenticatedUser},
    rate_limit::{with_rate_class, RateClass},
    upstream::proxy_json,
};

/// Route path of the list read.
pub(crate) const CONSENT_PATH: &str = "/api/v1/browser/consent";
/// Route path of the revoke.
pub(crate) const CONSENT_ORIGINS_PATH: &str = "/api/v1/browser/consent/origins";

/// The only query keys the revoke forwards.
const QUERY_KEYS: [&str; 4] = ["startHost", "scope", "conversationId", "turnId"];
/// The scopes a decision can be bound to.
const SCOPES: [&str; 3] = ["turn", "conversation", "always"];
/// The DNS limit for a whole hostname.
const MAX_HOST_CHARS: usize = 253;
/// The DNS limit for one label.
const MAX_LABEL_CHARS: usize = 63;
/// Mirrors user-core's `browserConsentMaxIDLength`.
const MAX_ID_CHARS: usize = 128;
/// The longest query string read before it is refused, well above the four keys' own limits.
const MAX_QUERY_BYTES: usize = 1024;

type GatewayJsonResponse = (axum::http::StatusCode, axum::Json<Value>);

/// The consent routes, or none at all when the switch is off. Merged in `main.rs`, and the
/// session layer sits on these routes alone.
pub(crate) fn router(state: AppState) -> Router<AppState> {
    let enabled = state.browser_consent_api;
    router_with(state, enabled)
}

fn router_with(state: AppState, enabled: bool) -> Router<AppState> {
    if !enabled {
        return Router::new();
    }
    let routes = Router::new()
        .route(CONSENT_PATH, get(list_consent))
        .route(CONSENT_ORIGINS_PATH, delete(revoke_consent));
    with_rate_class(routes, &state.rate_limiter, RateClass::Interactive)
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// GET /api/v1/browser/consent: the caller's stored origin decisions, as user-core lists them.
pub(crate) async fn list_consent(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> GatewayJsonResponse {
    let url = format!("{}/api/v1/settings/browser-consent", state.user_core_url);
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// DELETE /api/v1/browser/consent/origins?startHost=&scope=&conversationId=&turnId=: forgets one
/// stored decision, so the next browse to that origin asks again. The key is rebuilt from the
/// validated values, so nothing the client sent beyond the four keys can reach user-core.
pub(crate) async fn revoke_consent(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    RawQuery(raw): RawQuery,
) -> GatewayJsonResponse {
    let key = match parse_key(raw.as_deref().unwrap_or("")) {
        Ok(key) => key,
        Err(response) => return response,
    };
    let url = format!(
        "{}/api/v1/settings/browser-consent/origins?{}",
        state.user_core_url,
        key.query_string()
    );
    proxy_json(
        &state,
        Method::DELETE,
        &url,
        None,
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await
}

fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Query validation
// ---------------------------------------------------------------------------

/// The validated identity of one stored decision.
#[derive(Debug, PartialEq, Eq)]
struct ConsentKey {
    start_host: String,
    scope: String,
    conversation_id: Option<String>,
    turn_id: Option<String>,
}

impl ConsentKey {
    /// The query user-core is called with: only the four known keys, percent-encoded, with an id
    /// left out when the client did not send one.
    fn query_string(&self) -> String {
        let mut query = format!(
            "startHost={}&scope={}",
            urlencoding::encode(&self.start_host),
            urlencoding::encode(&self.scope)
        );
        if let Some(id) = &self.conversation_id {
            query.push_str(&format!("&conversationId={}", urlencoding::encode(id)));
        }
        if let Some(id) = &self.turn_id {
            query.push_str(&format!("&turnId={}", urlencoding::encode(id)));
        }
        query
    }
}

/// Reads the raw query into a key, or the typed refusal. Unknown keys and repeated keys are
/// refused rather than dropped: a typo such as `convesationId` would otherwise be forwarded as
/// "no conversation" and answer with a binding error that points at the wrong field.
fn parse_key(raw: &str) -> Result<ConsentKey, GatewayJsonResponse> {
    if raw.len() > MAX_QUERY_BYTES {
        return Err(invalid(
            "The query string is too long.",
            json!({ "field": "query" }),
        ));
    }
    let mut start_host: Option<String> = None;
    let mut scope: Option<String> = None;
    let mut conversation_id: Option<String> = None;
    let mut turn_id: Option<String> = None;
    for (name, value) in url::form_urlencoded::parse(raw.as_bytes()) {
        let slot = match name.as_ref() {
            "startHost" => &mut start_host,
            "scope" => &mut scope,
            "conversationId" => &mut conversation_id,
            "turnId" => &mut turn_id,
            _ => {
                return Err(invalid(
                    "Only startHost, scope, conversationId and turnId are accepted.",
                    json!({ "allowed": QUERY_KEYS }),
                ));
            }
        };
        if slot.is_some() {
            return Err(invalid(
                "Each query key may be given once.",
                json!({ "allowed": QUERY_KEYS }),
            ));
        }
        *slot = Some(value.into_owned());
    }

    let start_host = start_host.unwrap_or_default();
    if !valid_canonical_host(&start_host) {
        return Err(invalid(
            "startHost must be a canonical lowercase host name.",
            json!({ "field": "startHost" }),
        ));
    }
    let scope = scope.unwrap_or_default();
    if !SCOPES.contains(&scope.as_str()) {
        return Err(invalid(
            "scope must be turn, conversation or always.",
            json!({ "field": "scope", "allowed": SCOPES }),
        ));
    }
    for (field, id) in [("conversationId", &conversation_id), ("turnId", &turn_id)] {
        if id.as_deref().is_some_and(|id| !valid_id(id)) {
            return Err(invalid(
                "The id must be 1 to 128 characters of letters, digits, underscore or hyphen.",
                json!({ "field": field }),
            ));
        }
    }
    Ok(ConsentKey {
        start_host,
        scope,
        conversation_id,
        turn_id,
    })
}

/// A canonical host as user-core stores it: lower case, no trailing dot, at least two labels of
/// letters, digits and inner hyphens. IP literals, ports and paths fail on their characters or
/// their numeric last label. The reserved private suffixes are user-core's rule and are answered
/// by it.
fn valid_canonical_host(host: &str) -> bool {
    if host.is_empty() || host.len() > MAX_HOST_CHARS {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let labels_ok = labels.iter().all(|label| {
        !label.is_empty()
            && label.len() <= MAX_LABEL_CHARS
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    });
    let numeric_tld = labels
        .last()
        .is_some_and(|tld| tld.bytes().all(|byte| byte.is_ascii_digit()));
    labels_ok && !numeric_tld
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_CHARS
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn invalid(message: &'static str, details: Value) -> GatewayJsonResponse {
    (
        axum::http::StatusCode::BAD_REQUEST,
        axum::Json(error_with_details("validation_error", message, details)),
    )
}
