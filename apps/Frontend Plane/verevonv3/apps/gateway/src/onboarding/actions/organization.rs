use axum::{
    extract::State,
    http::{header::SET_COOKIE, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    auth::actor_from_request,
    config::AppState,
    contracts::CreateOrganizationRequest,
    envelope::error,
    middleware::AuthenticatedUser,
    upstream::{browser_origin, proxy_json},
    utils::{slugify, trim_opt},
};

/// Create (or re-activate) the caller's onboarding organization, with Better Auth
/// as the source of truth for org identity and membership.
///
/// Flow (each step depends on the previous):
///   1. DEDUPE — list the caller's Better Auth orgs; if one already exists, reuse it.
///      Reuse never skips reconciliation: the org-core mirror + owner membership
///      must be healthy before the org can become active.
///   2. CREATE via Better Auth (`/organization/create`) so BA records the org AND an
///      owner membership for the caller. The id BA returns is canonical.
///   3. PROVISION org-core under the SAME id, atomically with the first owner.
///   4. SET-ACTIVE only after provisioning succeeds. A partial control-plane write
///      fails closed and remains retryable instead of advancing onboarding.
pub(crate) async fn create_organization(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<CreateOrganizationRequest>,
) -> Response {
    let name = input.name.trim().to_owned();
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_name", "Organization name is required.")),
        )
            .into_response();
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
        )
            .into_response();
    }

    // Better Auth's organization endpoints enforce an Origin/CSRF check, so the
    // browser origin must travel with every server-to-server hop below (same as
    // the sign-in proxy) — without it BA rejects create/set-active with 403.
    let origin = browser_origin(&headers);
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );

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
    let (ba_status, ba_org, full_org) = match existing_owned_org(
        &state,
        &cookie,
        origin.as_deref(),
        actor.user_id.as_str(),
    )
    .await
    {
        Ok(Some((existing, full))) => (StatusCode::OK, existing, full),
        Ok(None) => {
            match ba_post(&state, &cookie, origin.as_deref(), "create", create_body).await {
                Some((status, body)) if status.is_success() => {
                    let Some(org_id) = ba_org_id(&body) else {
                        return (
                            StatusCode::BAD_GATEWAY,
                            Json(error(
                                "create_failed",
                                "Identity service did not return an organization id.",
                            )),
                        )
                            .into_response();
                    };
                    let Some(full) =
                        full_organization(&state, &cookie, origin.as_deref(), &org_id).await
                    else {
                        return (
                            StatusCode::BAD_GATEWAY,
                            Json(error(
                                "organization_membership_unavailable",
                                "The organization membership could not be verified. Please retry.",
                            )),
                        )
                            .into_response();
                    };
                    (StatusCode::CREATED, body, full)
                }
                Some((status, body)) => return create_error_response(status, &body),
                None => {
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(error(
                            "upstream_unavailable",
                            "Could not reach the identity service to create the organization.",
                        )),
                    )
                        .into_response();
                }
            }
        }
        Err(()) => {
            return (
                    StatusCode::BAD_GATEWAY,
                    Json(error(
                        "organization_lookup_failed",
                        "Existing organizations could not be verified. Please retry; no new organization was created.",
                    )),
                )
                    .into_response();
        }
    };

    let Some(org_id) = ba_org_id(&ba_org) else {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "create_failed",
                "Identity service did not return an organization id.",
            )),
        )
            .into_response();
    };

    if canonical_owner_id(&full_org).as_deref() != Some(actor.user_id.as_str()) {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "organization_owner_required",
                "Only the organization owner can provision this workspace. Ask the owner for an invitation or choose another organization.",
            )),
        )
            .into_response();
    }

    // 3. Provision the org-core projection + first owner before the session is
    // allowed to activate the organization. The endpoint is idempotent, so this
    // also repairs a previous partial attempt on retry.
    let mirror_body = json!({
        "id": org_id,
        "name": name,
        "slug": slug,
        "plan": "free",
        "org_number": org_number,
        "brreg_data": input.brreg_data,
        "metadata": metadata,
        "primary_domain": verified_company_domain(&user),
    });
    let (mirror_status, _) = proxy_json(
        &state,
        Method::POST,
        &format!("{}/orgs", state.org_core_url),
        Some(mirror_body),
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;

    if !mirror_status.is_success() {
        let status = if mirror_status == StatusCode::CONFLICT {
            StatusCode::CONFLICT
        } else {
            StatusCode::BAD_GATEWAY
        };
        return (
            status,
            Json(error(
                "organization_provisioning_failed",
                "Your organization could not be safely provisioned. No onboarding progress was lost; please retry.",
            )),
        )
            .into_response();
    }

    // 4. Set-active only after both organization authorities agree. Capture the
    // refreshed Better Auth session cookie so the browser cookie-cache reflects
    // the new active org immediately.
    let active_cookies = match set_active_org(&state, &cookie, origin.as_deref(), &org_id).await {
        Ok(cookies) => cookies,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "organization_activation_failed",
                    "Your organization was created, but the session could not activate it. Please retry.",
                )),
            )
                .into_response();
        }
    };

    respond_json(ba_status, normalize_org(&ba_org), active_cookies)
}

fn verified_company_domain(user: &AuthenticatedUser) -> Option<String> {
    if !user.email_verified {
        return None;
    }
    let normalized_email = user.user_email.trim().to_lowercase();
    let (_, domain) = normalized_email.rsplit_once('@')?;
    let domain = domain.trim_end_matches('.');
    if domain.is_empty()
        || matches!(
            domain,
            "gmail.com"
                | "googlemail.com"
                | "hotmail.com"
                | "outlook.com"
                | "live.com"
                | "icloud.com"
                | "me.com"
                | "yahoo.com"
                | "proton.me"
                | "protonmail.com"
                | "privaterelay.appleid.com"
        )
    {
        return None;
    }
    Some(domain.to_owned())
}

/// Build a JSON response, replaying any captured `Set-Cookie` headers (the
/// refreshed Better Auth session-data cookie from set-active) onto it so the
/// browser's cookie-cache reflects the new active org immediately instead of
/// reading a stale value until the cache TTL elapses.
fn respond_json(status: StatusCode, value: Value, set_cookies: Vec<Vec<u8>>) -> Response {
    let mut response = (status, Json(value)).into_response();
    let headers = response.headers_mut();
    for cookie in set_cookies {
        if let Ok(value) = HeaderValue::from_bytes(&cookie) {
            headers.append(SET_COOKIE, value);
        }
    }
    response
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
async fn existing_owned_org(
    state: &AppState,
    cookie: &str,
    origin: Option<&str>,
    user_id: &str,
) -> Result<Option<(Value, Value)>, ()> {
    let url = format!("{}/api/auth/organization/list", state.auth_core_url);
    let mut request = state.client.get(&url).header("cookie", cookie);
    if let Some(origin) = origin.filter(|value| !value.trim().is_empty()) {
        request = request.header("origin", origin);
    }
    let response = request.send().await.map_err(|_| ())?;
    if !response.status().is_success() {
        return Err(());
    }
    let body = response.json::<Value>().await.map_err(|_| ())?;
    let organizations = organization_entries(&body).ok_or(())?;
    for organization in organizations {
        let Some(org_id) = ba_org_id(organization) else {
            continue;
        };
        let full = full_organization(state, cookie, origin, &org_id)
            .await
            .ok_or(())?;
        if canonical_owner_id(&full).as_deref() == Some(user_id) {
            return Ok(Some((organization.clone(), full)));
        }
    }
    Ok(None)
}

async fn full_organization(
    state: &AppState,
    cookie: &str,
    origin: Option<&str>,
    org_id: &str,
) -> Option<Value> {
    let url = format!(
        "{}/api/auth/organization/get-full-organization?organizationId={}",
        state.auth_core_url,
        urlencoding::encode(org_id),
    );
    let mut request = state.client.get(&url).header("cookie", cookie);
    if let Some(origin) = origin.filter(|value| !value.trim().is_empty()) {
        request = request.header("origin", origin);
    }
    let response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<Value>().await.ok()
}

fn canonical_owner_id(full_org: &Value) -> Option<String> {
    let members = full_org
        .get("members")
        .or_else(|| full_org.pointer("/data/members"))
        .and_then(Value::as_array)?;
    members.iter().find_map(|member| {
        let is_owner = member
            .get("role")
            .and_then(Value::as_str)
            .is_some_and(|role| role.split(',').any(|part| part.trim() == "owner"));
        is_owner
            .then(|| {
                member
                    .get("userId")
                    .or_else(|| member.get("user_id"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .flatten()
    })
}

/// Better Auth's `organization/list` returns an array (sometimes wrapped under
/// `data`/`organizations`). Return the first entry that carries an id.
#[cfg(test)]
fn first_org(body: &Value) -> Option<Value> {
    organization_entries(body)?
        .iter()
        .find(|item| ba_org_id(item).is_some())
        .cloned()
}

fn organization_entries(body: &Value) -> Option<&Vec<Value>> {
    body.as_array()
        .or_else(|| body.get("data").and_then(Value::as_array))
        .or_else(|| body.get("organizations").and_then(Value::as_array))
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

/// Set the session's active organization via Better Auth and return the
/// refreshed `Set-Cookie` headers from that response. Better Auth re-issues the
/// session-data (cookie-cache) cookie carrying the new `activeOrganizationId`;
/// the caller replays those onto its own response so the browser reflects the
/// active org immediately rather than serving a stale cache for the TTL window.
/// Best-effort: returns an empty Vec on any failure — the active org is still
/// persisted server-side and scoped calls fall back to membership resolution.
async fn set_active_org(
    state: &AppState,
    cookie: &str,
    origin: Option<&str>,
    org_id: &str,
) -> Result<Vec<Vec<u8>>, StatusCode> {
    let url = format!("{}/api/auth/organization/set-active", state.auth_core_url);
    let mut request = state
        .client
        .post(&url)
        .header("cookie", cookie)
        .json(&json!({ "organizationId": org_id }));
    if let Some(origin) = origin.filter(|value| !value.trim().is_empty()) {
        request = request.header("origin", origin);
    }
    let response = request.send().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if !status.is_success() {
        return Err(status);
    }
    Ok(response
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .map(|value| value.as_bytes().to_vec())
        .collect())
}

fn create_error_response(status: StatusCode, body: &Value) -> Response {
    let message = ba_error_message(body, status);
    let normalized = message.to_ascii_lowercase();
    if normalized.contains("organization already exists") || normalized.contains("slug already") {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "organization_conflict",
                "This organization already exists. Sign in with an invited administrator account or ask an owner for access.",
            )),
        )
            .into_response();
    }

    let code = if status == StatusCode::FORBIDDEN {
        "forbidden"
    } else {
        "create_failed"
    };
    (status, Json(error(code, message))).into_response()
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

    #[test]
    fn canonical_owner_must_match_the_authenticated_creator() {
        let full_org = json!({
            "id": "org_x",
            "members": [
                { "userId": "member-1", "role": "member" },
                { "userId": "owner-1", "role": "owner" }
            ]
        });

        assert_eq!(canonical_owner_id(&full_org), Some("owner-1".into()));
        assert_ne!(canonical_owner_id(&full_org).as_deref(), Some("member-1"));
    }

    #[test]
    fn company_domain_requires_a_verified_non_public_email() {
        let user = |email: &str, verified| AuthenticatedUser {
            user_id: "user-1".into(),
            user_email: email.into(),
            user_name: "User".into(),
            user_image: None,
            email_verified: verified,
            auth_role: None,
            active_org_id: None,
            authorized_membership: None,
        };
        assert_eq!(
            verified_company_domain(&user("Ima@Coresystem.com", true)).as_deref(),
            Some("coresystem.com")
        );
        assert!(verified_company_domain(&user("ima@coresystem.com", false)).is_none());
        assert!(verified_company_domain(&user("ima@gmail.com", true)).is_none());
    }
}
