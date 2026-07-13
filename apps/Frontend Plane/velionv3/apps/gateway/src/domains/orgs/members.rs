use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    envelope::error,
    upstream::{browser_origin, proxy_auth},
};

use super::shared::cookie_header;

fn auth_membership_url(auth_core_url: &str, operation: &str) -> String {
    format!(
        "{}/api/auth/organization/{}",
        auth_core_url.trim_end_matches('/'),
        operation
    )
}

fn auth_cookie(headers: &HeaderMap) -> Result<String, Box<Response>> {
    let cookie = cookie_header(headers);
    if cookie.trim().is_empty() {
        return Err(Box::new(
            (
                StatusCode::UNAUTHORIZED,
                Json(error("unauthorized", "Authentication required.")),
            )
                .into_response(),
        ));
    }
    Ok(cookie)
}

pub(super) async fn list_members(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let cookie = match auth_cookie(&headers) {
        Ok(cookie) => cookie,
        Err(response) => return *response,
    };
    let url = format!(
        "{}?organizationId={}&limit=1000&offset=0",
        auth_membership_url(&state.auth_core_url, "list-members"),
        urlencoding::encode(id.trim())
    );
    proxy_auth(
        &state,
        Method::GET,
        &url,
        None,
        Some(&cookie),
        browser_origin(&headers).as_deref(),
    )
    .await
}

pub(super) async fn invite_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let cookie = match auth_cookie(&headers) {
        Ok(cookie) => cookie,
        Err(response) => return *response,
    };
    let email = body
        .get("email")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let role = body
        .get("role")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("member");
    if email.is_none() || !matches!(role, "member" | "admin") {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "validation_error",
                "A valid email and member/admin role are required.",
            )),
        )
            .into_response();
    }

    proxy_auth(
        &state,
        Method::POST,
        &auth_membership_url(&state.auth_core_url, "invite-member"),
        Some(json!({
            "email": email,
            "role": role,
            "organizationId": id.trim(),
            "resend": false,
        })),
        Some(&cookie),
        browser_origin(&headers).as_deref(),
    )
    .await
}

async fn canonical_member_id(
    state: &AppState,
    headers: &HeaderMap,
    organization_id: &str,
    user_id: &str,
) -> Result<String, Box<Response>> {
    let cookie = auth_cookie(headers)?;
    let url = format!(
        "{}?organizationId={}&limit=1000&offset=0",
        auth_membership_url(&state.auth_core_url, "list-members"),
        urlencoding::encode(organization_id)
    );
    let response = state
        .client
        .get(url)
        .header("cookie", cookie)
        .send()
        .await
        .map_err(|_| {
            Box::new(
                (
                    StatusCode::BAD_GATEWAY,
                    Json(error(
                        "upstream_unavailable",
                        "Membership authority is unavailable.",
                    )),
                )
                    .into_response(),
            )
        })?;
    if !response.status().is_success() {
        let status =
            StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        return Err(Box::new(
            (
                status,
                Json(error(
                    "membership_lookup_failed",
                    "Unable to resolve the canonical organization member.",
                )),
            )
                .into_response(),
        ));
    }

    let payload = response.json::<Value>().await.unwrap_or(Value::Null);
    let members = payload
        .get("members")
        .or_else(|| payload.get("data").and_then(|data| data.get("members")))
        .and_then(Value::as_array);
    let member_id = members.and_then(|members| {
        members.iter().find_map(|member| {
            let candidate_user_id = member
                .get("userId")
                .or_else(|| member.get("user_id"))
                .or_else(|| member.get("user").and_then(|user| user.get("id")))
                .and_then(Value::as_str)?;
            if candidate_user_id != user_id {
                return None;
            }
            member.get("id").and_then(Value::as_str).map(str::to_owned)
        })
    });

    member_id.ok_or_else(|| {
        Box::new(
            (
                StatusCode::NOT_FOUND,
                Json(error("member_not_found", "Organization member not found.")),
            )
                .into_response(),
        )
    })
}

pub(super) async fn remove_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, user_id)): Path<(String, String)>,
) -> Response {
    let member_id = match canonical_member_id(&state, &headers, id.trim(), user_id.trim()).await {
        Ok(member_id) => member_id,
        Err(response) => return *response,
    };
    let cookie = match auth_cookie(&headers) {
        Ok(cookie) => cookie,
        Err(response) => return *response,
    };
    proxy_auth(
        &state,
        Method::POST,
        &auth_membership_url(&state.auth_core_url, "remove-member"),
        Some(json!({
            "memberIdOrEmail": member_id,
            "organizationId": id.trim(),
        })),
        Some(&cookie),
        browser_origin(&headers).as_deref(),
    )
    .await
}

pub(super) async fn update_member_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, user_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let role = body
        .get("role")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|role| matches!(*role, "member" | "admin"));
    let Some(role) = role else {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "validation_error",
                "Role must be member or admin; owner transfer uses a separate flow.",
            )),
        )
            .into_response();
    };
    let member_id = match canonical_member_id(&state, &headers, id.trim(), user_id.trim()).await {
        Ok(member_id) => member_id,
        Err(response) => return *response,
    };
    let cookie = match auth_cookie(&headers) {
        Ok(cookie) => cookie,
        Err(response) => return *response,
    };
    proxy_auth(
        &state,
        Method::POST,
        &auth_membership_url(&state.auth_core_url, "update-member-role"),
        Some(json!({
            "memberId": member_id,
            "role": role,
            "organizationId": id.trim(),
        })),
        Some(&cookie),
        browser_origin(&headers).as_deref(),
    )
    .await
}

pub(super) async fn accept_invitation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(invitation_id): Path<String>,
) -> Response {
    let cookie = match auth_cookie(&headers) {
        Ok(cookie) => cookie,
        Err(response) => return *response,
    };
    proxy_auth(
        &state,
        Method::POST,
        &auth_membership_url(&state.auth_core_url, "accept-invitation"),
        Some(json!({ "invitationId": invitation_id.trim() })),
        Some(&cookie),
        browser_origin(&headers).as_deref(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_mutations_target_canonical_auth_endpoints() {
        assert_eq!(
            auth_membership_url("http://auth-core:3011", "invite-member"),
            "http://auth-core:3011/api/auth/organization/invite-member"
        );
        assert_eq!(
            auth_membership_url("http://auth-core:3011/", "remove-member"),
            "http://auth-core:3011/api/auth/organization/remove-member"
        );
        assert_eq!(
            auth_membership_url("http://auth-core:3011", "update-member-role"),
            "http://auth-core:3011/api/auth/organization/update-member-role"
        );
    }
}
