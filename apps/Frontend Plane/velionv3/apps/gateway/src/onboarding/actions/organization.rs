use axum::{extract::State, http::HeaderMap, http::StatusCode, response::IntoResponse, Json};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    auth::actor_from_request,
    config::AppState,
    contracts::CreateOrganizationRequest,
    envelope::error,
    upstream::{browser_origin, proxy_json},
    utils::{slugify, trim_opt},
};

/// Create (or re-activate) the caller's onboarding organization, with Better Auth
/// as the source of truth for org identity and membership.
///
/// Flow (each step depends on the previous):
///   1. DEDUPE — list the caller's Better Auth orgs; if one already exists, set it
///      active and return it. This stops the duplicate-org accumulation where every
///      onboarding revisit minted a fresh org.
///   2. CREATE via Better Auth (`/organization/create`) so BA records the org AND an
///      owner membership for the caller. The id BA returns is canonical.
///   3. SET-ACTIVE that BA id on the session — now succeeds because membership exists
///      (the old bug: set-active 403'd USER_IS_NOT_A_MEMBER on an org-core-only org).
///   4. MIRROR to org-core under the SAME id (idempotent) so org-core + the user-core
///      membership it publishes share BA's id. Mirror failures are best-effort and
///      never fail onboarding; set-active, however, must have succeeded by here.
pub(crate) async fn create_organization(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateOrganizationRequest>,
) -> impl IntoResponse {
    let name = input.name.trim().to_owned();
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_name", "Organization name is required.")),
        );
    }

    let cookie = cookie_header(&headers);
    if cookie.is_empty() {
        // No session cookie ⇒ no identity to attach the org to. Better Auth org
        // membership is the whole point of this flow, so fail clearly rather than
        // silently minting an unowned org.
        return (
            StatusCode::UNAUTHORIZED,
            Json(error(
                "unauthenticated",
                "A signed-in session is required to create an organization.",
            )),
        );
    }

    // Better Auth's organization endpoints enforce an Origin/CSRF check, so the
    // browser origin must travel with every server-to-server hop below (same as
    // the sign-in proxy) — without it BA rejects create/set-active with 403.
    let origin = browser_origin(&headers);

    // 1. Dedupe: reuse an existing membership instead of creating a duplicate.
    if let Some(existing) = existing_org(&state, &cookie, origin.as_deref()).await {
        if let Some(org_id) = ba_org_id(&existing) {
            set_active_org(&state, &cookie, origin.as_deref(), &org_id).await;
            return (StatusCode::OK, Json(normalize_org(&existing)));
        }
    }

    let slug = slugify(&name);
    let plan = input.plan.unwrap_or_else(|| "trial".into());
    let org_number = trim_opt(input.org_number);

    // 2. Create via Better Auth. Org-specific attributes that BA does not model
    // natively (plan, Brreg, the SPA's onboarding metadata) ride along in
    // `metadata` so they survive into the canonical org record + the mirror.
    let metadata = build_metadata(
        input.metadata,
        &plan,
        org_number.as_deref(),
        &input.brreg_data,
    );
    let create_body = json!({
        "name": name,
        "slug": slug,
        "metadata": metadata,
    });
    let (ba_status, ba_org) =
        match ba_post(&state, &cookie, origin.as_deref(), "create", create_body).await {
            Some(result) => result,
            None => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(error(
                        "upstream_unavailable",
                        "Could not reach the identity service to create the organization.",
                    )),
                );
            }
        };
    if !ba_status.is_success() {
        let code = if ba_status.as_u16() == 403 {
            "forbidden"
        } else {
            "create_failed"
        };
        return (
            ba_status,
            Json(error(code, ba_error_message(&ba_org, ba_status))),
        );
    }

    let Some(org_id) = ba_org_id(&ba_org) else {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "create_failed",
                "Identity service did not return an organization id.",
            )),
        );
    };

    // 3. Set-active — membership now exists, so this must succeed.
    set_active_org(&state, &cookie, origin.as_deref(), &org_id).await;

    // 4. Mirror to org-core under the SAME id (best-effort, never fatal).
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let mirror_body = json!({
        "id": org_id,
        "name": name,
        "slug": slug,
        "plan": plan,
        "org_number": org_number,
        "brreg_data": input.brreg_data,
        "metadata": metadata,
    });
    let _ = proxy_json(
        &state,
        Method::POST,
        &format!("{}/orgs", state.org_core_url),
        Some(mirror_body),
        None,
        Some(&actor),
        None,
    )
    .await;

    (StatusCode::CREATED, Json(normalize_org(&ba_org)))
}

fn cookie_header(headers: &HeaderMap) -> String {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_owned()
}

/// Merge the SPA's onboarding metadata with the org-level attributes BA does not
/// model natively, so a single canonical metadata blob travels to BA + org-core.
fn build_metadata(
    base: Option<Value>,
    plan: &str,
    org_number: Option<&str>,
    brreg_data: &Option<Value>,
) -> Value {
    let mut map = match base {
        Some(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    map.insert("plan".into(), json!(plan));
    if let Some(org_number) = org_number {
        map.insert("org_number".into(), json!(org_number));
    }
    if let Some(brreg) = brreg_data {
        if !brreg.is_null() {
            map.insert("brreg".into(), brreg.clone());
        }
    }
    Value::Object(map)
}

/// GET the caller's Better Auth orgs and return the first one, if any. `None` on
/// any transport/parse failure or an empty list — the caller then creates a new org.
async fn existing_org(state: &AppState, cookie: &str, origin: Option<&str>) -> Option<Value> {
    let url = format!("{}/api/auth/organization/list", state.auth_core_url);
    let mut request = state.client.get(&url).header("cookie", cookie);
    if let Some(origin) = origin.filter(|value| !value.trim().is_empty()) {
        request = request.header("origin", origin);
    }
    let response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = response.json::<Value>().await.ok()?;
    first_org(&body)
}

/// Better Auth's `organization/list` returns an array (sometimes wrapped under
/// `data`/`organizations`). Return the first entry that carries an id.
fn first_org(body: &Value) -> Option<Value> {
    let array = body
        .as_array()
        .or_else(|| body.get("data").and_then(Value::as_array))
        .or_else(|| body.get("organizations").and_then(Value::as_array))?;
    array.iter().find(|item| ba_org_id(item).is_some()).cloned()
}

/// POST to a Better Auth organization sub-route with the caller cookie.
/// Returns `(status, body)` or `None` when the request never reached auth-core.
async fn ba_post(
    state: &AppState,
    cookie: &str,
    origin: Option<&str>,
    action: &str,
    body: Value,
) -> Option<(StatusCode, Value)> {
    let url = format!("{}/api/auth/organization/{}", state.auth_core_url, action);
    let mut request = state.client.post(&url).header("cookie", cookie).json(&body);
    if let Some(origin) = origin.filter(|value| !value.trim().is_empty()) {
        request = request.header("origin", origin);
    }
    let response = request.send().await.ok()?;
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let parsed = response.json::<Value>().await.unwrap_or(Value::Null);
    Some((status, parsed))
}

/// Set the session's active organization via Better Auth. Best-effort: the response
/// is ignored, the active org is persisted on the session row server-side.
async fn set_active_org(state: &AppState, cookie: &str, origin: Option<&str>, org_id: &str) {
    let _ = ba_post(
        state,
        cookie,
        origin,
        "set-active",
        json!({ "organizationId": org_id }),
    )
    .await;
    // The session-context cache is keyed by active org; the next scoped call re-reads
    // a fresh session, so no explicit invalidation is required here.
}

/// Extract a Better Auth organization id from a create/list entry. BA returns the
/// org object at the root (create) or as array entries (list); a `/data/id` wrapper
/// is tolerated defensively.
fn ba_org_id(value: &Value) -> Option<String> {
    ["/id", "/data/id", "/organization/id"]
        .into_iter()
        .find_map(|ptr| value.pointer(ptr).and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

/// Normalize a Better Auth org object into the SPA's expected
/// `{ id, name, slug, plan }` shape. `plan` is pulled from metadata when present.
fn normalize_org(value: &Value) -> Value {
    let org = value.get("data").unwrap_or(value);
    let metadata = parse_metadata(org.get("metadata"));
    json!({
        "id": ba_org_id(value).unwrap_or_default(),
        "name": org.get("name").and_then(Value::as_str).unwrap_or_default(),
        "slug": org.get("slug").and_then(Value::as_str),
        "plan": metadata.get("plan").and_then(Value::as_str),
    })
}

/// Better Auth may store `metadata` as a JSON object or as a serialized JSON string.
/// Return it as an object either way (empty object on absence / parse failure).
fn parse_metadata(value: Option<&Value>) -> Value {
    match value {
        Some(Value::Object(_)) => value.cloned().unwrap(),
        Some(Value::String(raw)) => serde_json::from_str(raw).unwrap_or_else(|_| json!({})),
        _ => json!({}),
    }
}

fn ba_error_message(body: &Value, status: StatusCode) -> String {
    body.get("message")
        .or_else(|| body.pointer("/error/message"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Identity service returned {}", status.as_u16()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ba_org_id_reads_root_and_wrapped() {
        assert_eq!(ba_org_id(&json!({ "id": "org_1" })), Some("org_1".into()));
        assert_eq!(
            ba_org_id(&json!({ "data": { "id": "org_2" } })),
            Some("org_2".into())
        );
        assert_eq!(ba_org_id(&json!({ "name": "no id" })), None);
    }

    #[test]
    fn first_org_handles_array_and_wrappers() {
        let bare = json!([{ "id": "org_a" }, { "id": "org_b" }]);
        assert_eq!(ba_org_id(&first_org(&bare).unwrap()), Some("org_a".into()));

        let wrapped = json!({ "data": [{ "id": "org_c" }] });
        assert_eq!(
            ba_org_id(&first_org(&wrapped).unwrap()),
            Some("org_c".into())
        );

        assert!(first_org(&json!([])).is_none());
        assert!(first_org(&json!({ "data": [] })).is_none());
        // An entry with no id is skipped.
        assert!(first_org(&json!([{ "name": "x" }])).is_none());
    }

    #[test]
    fn build_metadata_merges_plan_and_brreg() {
        let meta = build_metadata(
            Some(json!({ "onboarding_branding": { "site_name": "Acme" } })),
            "trial",
            Some("123456789"),
            &Some(json!({ "navn": "ACME AS" })),
        );
        assert_eq!(meta.pointer("/plan").and_then(Value::as_str), Some("trial"));
        assert_eq!(
            meta.pointer("/org_number").and_then(Value::as_str),
            Some("123456789")
        );
        assert_eq!(
            meta.pointer("/brreg/navn").and_then(Value::as_str),
            Some("ACME AS")
        );
        assert_eq!(
            meta.pointer("/onboarding_branding/site_name")
                .and_then(Value::as_str),
            Some("Acme")
        );
    }

    #[test]
    fn build_metadata_tolerates_non_object_base() {
        let meta = build_metadata(None, "starter", None, &None);
        assert_eq!(
            meta.pointer("/plan").and_then(Value::as_str),
            Some("starter")
        );
        assert!(meta.get("org_number").is_none());
        assert!(meta.get("brreg").is_none());
    }

    #[test]
    fn normalize_org_extracts_plan_from_object_metadata() {
        let org = json!({
            "id": "org_x",
            "name": "Acme",
            "slug": "acme",
            "metadata": { "plan": "trial" }
        });
        let out = normalize_org(&org);
        assert_eq!(out.pointer("/id").and_then(Value::as_str), Some("org_x"));
        assert_eq!(out.pointer("/name").and_then(Value::as_str), Some("Acme"));
        assert_eq!(out.pointer("/slug").and_then(Value::as_str), Some("acme"));
        assert_eq!(out.pointer("/plan").and_then(Value::as_str), Some("trial"));
    }

    #[test]
    fn normalize_org_parses_stringified_metadata() {
        let org = json!({
            "id": "org_y",
            "name": "Beta",
            "metadata": "{\"plan\":\"starter\"}"
        });
        let out = normalize_org(&org);
        assert_eq!(
            out.pointer("/plan").and_then(Value::as_str),
            Some("starter")
        );
    }
}
