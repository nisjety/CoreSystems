use axum::{
    body::{to_bytes, Body},
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    envelope::{error, ok},
    middleware::{AuthenticatedUser, AuthorizedMembership},
    upstream::{browser_origin, proxy_auth},
};

const MEMBERSHIP_ERROR_BODY_LIMIT: usize = 64 * 1024;

use super::shared::cookie_header;

fn auth_membership_url(auth_core_url: &str, operation: &str) -> String {
    format!(
        "{}/api/auth/organization/{}",
        auth_core_url.trim_end_matches('/'),
        operation
    )
}

fn auth_invitation_acceptance_url(auth_core_url: &str, invitation_id: &str) -> String {
    format!(
        "{}/api/v1/organization/invitations/{}/accept",
        auth_core_url.trim_end_matches('/'),
        urlencoding::encode(invitation_id)
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

fn membership_path_allowed(
    membership: Option<&AuthorizedMembership>,
    organization_id: &str,
    require_admin: bool,
) -> bool {
    membership.is_some_and(|membership| {
        membership.organization_id == organization_id.trim()
            && (!require_admin || matches!(membership.role.as_str(), "owner" | "admin"))
    })
}

fn authorize_membership_path(
    user: &AuthenticatedUser,
    organization_id: &str,
    require_admin: bool,
) -> Result<(), Box<Response>> {
    if membership_path_allowed(
        user.authorized_membership.as_ref(),
        organization_id,
        require_admin,
    ) {
        return Ok(());
    }
    Err(Box::new(
        (
            StatusCode::FORBIDDEN,
            Json(error(
                "organization_access_denied",
                "The path organization must match the live active membership and required role.",
            )),
        )
            .into_response(),
    ))
}

async fn normalize_duplicate_invite(
    response: Response,
    normalized_email: &str,
    role: &str,
) -> Response {
    if !matches!(
        response.status(),
        StatusCode::BAD_REQUEST | StatusCode::CONFLICT
    ) {
        return response;
    }

    let (parts, body) = response.into_parts();
    let bytes = match to_bytes(body, MEMBERSHIP_ERROR_BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "membership_response_invalid",
                    "Membership authority returned an invalid response.",
                )),
            )
                .into_response()
        }
    };
    let payload = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
    let code = payload
        .get("code")
        .or_else(|| payload.pointer("/error/code"))
        .and_then(Value::as_str);
    if matches!(
        code,
        Some(
            "USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION"
                | "USER_IS_ALREADY_A_MEMBER_OF_THIS_ORGANIZATION"
        )
    ) {
        return (
            StatusCode::OK,
            Json(ok(json!({
                "email": normalized_email,
                "role": role,
                "invitation_created": false,
            }))),
        )
            .into_response();
    }

    Response::from_parts(parts, Body::from(bytes))
}

fn valid_member_email(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 320
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return false;
    }
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty() && !domain.is_empty() && !domain.contains('@')
}

fn valid_member_subject_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn invalid_member_subject_response() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error(
            "validation_error",
            "Member user id must be a bounded opaque identifier.",
        )),
    )
        .into_response()
}

pub(super) async fn list_members(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_membership_path(&user, &id, false) {
        return *response;
    }
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

pub(crate) async fn invite_member(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if let Err(response) = authorize_membership_path(&user, &id, true) {
        return *response;
    }
    let cookie = match auth_cookie(&headers) {
        Ok(cookie) => cookie,
        Err(response) => return *response,
    };
    let raw_email = body.get("email").and_then(Value::as_str);
    let email = raw_email.map(str::trim).map(str::to_ascii_lowercase);
    let role = body
        .get("role")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("member");
    let email_valid = raw_email
        .is_some_and(|value| value.len() <= 320 && !value.chars().any(char::is_control))
        && email.as_deref().is_some_and(valid_member_email);
    if !email_valid || !matches!(role, "member" | "admin") {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "validation_error",
                "A valid email and member/admin role are required.",
            )),
        )
            .into_response();
    }

    let response = proxy_auth(
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
    .await;
    normalize_duplicate_invite(
        response,
        email.as_deref().expect("email validated above"),
        role,
    )
    .await
}

#[derive(Debug)]
struct CanonicalMember {
    id: String,
    role: Option<String>,
}

fn invalid_membership_lookup_response() -> Box<Response> {
    Box::new(
        (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "membership_lookup_invalid",
                "Membership authority returned an invalid response.",
            )),
        )
            .into_response(),
    )
}

async fn canonical_member(
    state: &AppState,
    headers: &HeaderMap,
    organization_id: &str,
    user_id: &str,
) -> Result<Option<CanonicalMember>, Box<Response>> {
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

    let payload = response
        .json::<Value>()
        .await
        .map_err(|_| invalid_membership_lookup_response())?;
    let members = payload
        .get("members")
        .or_else(|| payload.get("data").and_then(|data| data.get("members")))
        .and_then(Value::as_array)
        .ok_or_else(invalid_membership_lookup_response)?;

    for member in members {
        let candidate_user_id = member
            .get("userId")
            .or_else(|| member.get("user_id"))
            .or_else(|| member.get("user").and_then(|user| user.get("id")))
            .and_then(Value::as_str);
        if candidate_user_id != Some(user_id) {
            continue;
        }
        let id = member
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(invalid_membership_lookup_response)?;
        let role = member
            .get("role")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        return Ok(Some(CanonicalMember {
            id: id.to_owned(),
            role,
        }));
    }

    Ok(None)
}

pub(crate) async fn remove_member(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((id, user_id)): Path<(String, String)>,
) -> Response {
    if let Err(response) = authorize_membership_path(&user, &id, true) {
        return *response;
    }
    if !valid_member_subject_id(&user_id) {
        return invalid_member_subject_response();
    }
    let user_id = user_id.as_str();
    let member_id = match canonical_member(&state, &headers, id.trim(), user_id).await {
        Ok(Some(member)) => member.id,
        Ok(None) => {
            return (
                StatusCode::OK,
                Json(ok(json!({
                    "user_id": user_id,
                    "removed": false,
                }))),
            )
                .into_response()
        }
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

pub(crate) async fn update_member_role(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((id, user_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    if let Err(response) = authorize_membership_path(&user, &id, true) {
        return *response;
    }
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
    if !valid_member_subject_id(&user_id) {
        return invalid_member_subject_response();
    }
    let user_id = user_id.as_str();
    let member = match canonical_member(&state, &headers, id.trim(), user_id).await {
        Ok(Some(member)) => member,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(error("member_not_found", "Organization member not found.")),
            )
                .into_response()
        }
        Err(response) => return *response,
    };
    if member.role.is_none() {
        return *invalid_membership_lookup_response();
    }
    let member_id = member.id;
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

fn valid_invitation_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub(super) async fn accept_invitation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(invitation_id): Path<String>,
) -> Response {
    let invitation_id = invitation_id.trim();
    if !valid_invitation_id(invitation_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "validation_error",
                "Invitation id must be a bounded opaque identifier.",
            )),
        )
            .into_response();
    }
    let cookie = match auth_cookie(&headers) {
        Ok(cookie) => cookie,
        Err(response) => return *response,
    };
    proxy_auth(
        &state,
        Method::POST,
        &auth_invitation_acceptance_url(&state.auth_core_url, invitation_id),
        None,
        Some(&cookie),
        browser_origin(&headers).as_deref(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use wiremock::{
        matchers::{method as wm_method, path as wm_path},
        Mock, MockServer, ResponseTemplate,
    };

    fn admin_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "owner-user".to_owned(),
            user_email: "owner@example.invalid".to_owned(),
            user_name: "Owner".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("owner".to_owned()),
            active_org_id: Some("org-active".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-active".to_owned(),
                role: "owner".to_owned(),
            }),
        }
    }

    fn cookie_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("cookie", "better-auth.session_token=owner".parse().unwrap());
        headers
    }

    fn state_for(auth_core: &MockServer) -> AppState {
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth_core.uri();
        state
    }

    async fn response_json(response: Response) -> Value {
        serde_json::from_slice(
            &response
                .into_body()
                .collect()
                .await
                .expect("response body")
                .to_bytes(),
        )
        .expect("JSON response")
    }

    async fn mock_auth_response(
        method_name: &str,
        request_path: &str,
        status: u16,
        body: Value,
    ) -> MockServer {
        let auth = MockServer::start().await;
        Mock::given(wm_method(method_name))
            .and(wm_path(request_path))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&auth)
            .await;
        auth
    }

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
        assert_eq!(
            auth_invitation_acceptance_url("http://auth-core:3011", "inv_123"),
            "http://auth-core:3011/api/v1/organization/invitations/inv_123/accept"
        );
    }

    #[test]
    fn invitation_ids_are_bounded_opaque_identifiers() {
        assert!(valid_invitation_id("inv_123-abc"));
        assert!(!valid_invitation_id(""));
        assert!(!valid_invitation_id("../invite"));
        assert!(!valid_invitation_id(&"a".repeat(257)));
    }

    #[test]
    fn invite_email_is_bounded_and_rejects_ambiguous_or_injectable_values() {
        assert!(valid_member_email("invitee@example.com"));
        for value in [
            "invitee.example.com",
            "@example.com",
            "invitee@",
            "invitee@@example.com",
            "invitee @example.com",
            "invitee@example.com\n",
        ] {
            assert!(!valid_member_email(value), "{value:?}");
        }
        assert!(!valid_member_email(&format!(
            "{}@example.com",
            "a".repeat(309)
        )));
    }

    #[test]
    fn member_subject_ids_are_bounded_opaque_path_identifiers() {
        assert!(valid_member_subject_id("user_123-abc"));
        assert!(!valid_member_subject_id(""));
        assert!(!valid_member_subject_id("   "));
        assert!(!valid_member_subject_id("../user"));
        assert!(!valid_member_subject_id(&"a".repeat(257)));
    }

    #[tokio::test]
    async fn malformed_member_subject_is_400_while_legitimate_absence_is_a_noop() {
        let auth = mock_auth_response(
            "GET",
            "/api/auth/organization/list-members",
            200,
            json!({ "members": [] }),
        )
        .await;
        let state = state_for(&auth);

        for user_id in ["   ".to_owned(), "a".repeat(257)] {
            let removed = remove_member(
                State(state.clone()),
                Extension(admin_user()),
                cookie_headers(),
                Path(("org-active".to_owned(), user_id.clone())),
            )
            .await;
            assert_eq!(removed.status(), StatusCode::BAD_REQUEST);

            let updated = update_member_role(
                State(state.clone()),
                Extension(admin_user()),
                cookie_headers(),
                Path(("org-active".to_owned(), user_id)),
                Json(json!({ "role": "member" })),
            )
            .await;
            assert_eq!(updated.status(), StatusCode::BAD_REQUEST);
        }

        let absent = remove_member(
            State(state),
            Extension(admin_user()),
            cookie_headers(),
            Path(("org-active".to_owned(), "absent-user".to_owned())),
        )
        .await;
        assert_eq!(absent.status(), StatusCode::OK);
        assert_eq!(
            response_json(absent).await.pointer("/data/removed"),
            Some(&json!(false))
        );
    }

    #[test]
    fn membership_paths_are_pinned_to_the_live_active_org_and_admin_role_for_mutations() {
        let owner = crate::middleware::AuthorizedMembership {
            organization_id: "org-active".to_owned(),
            role: "owner".to_owned(),
        };
        let member = crate::middleware::AuthorizedMembership {
            organization_id: "org-active".to_owned(),
            role: "member".to_owned(),
        };

        assert!(membership_path_allowed(Some(&owner), "org-active", true));
        assert!(!membership_path_allowed(Some(&owner), "org-other", true));
        assert!(!membership_path_allowed(Some(&member), "org-active", true));
        assert!(membership_path_allowed(Some(&member), "org-active", false));
        assert!(!membership_path_allowed(None, "org-active", false));
    }

    #[test]
    fn non_admin_mutation_is_denied_with_stable_error_envelope() {
        let user = AuthenticatedUser {
            user_id: "user-member".to_owned(),
            user_email: "member@example.invalid".to_owned(),
            user_name: "Member".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-active".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-active".to_owned(),
                role: "member".to_owned(),
            }),
        };
        let response = authorize_membership_path(&user, "org-active", true)
            .expect_err("non-admin mutation must fail");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn duplicate_invite_is_the_only_auth_error_normalized_to_success() {
        let duplicate = mock_auth_response(
            "POST",
            "/api/auth/organization/invite-member",
            400,
            json!({ "code": "USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION" }),
        )
        .await;
        let response = invite_member(
            State(state_for(&duplicate)),
            Extension(admin_user()),
            cookie_headers(),
            Path("org-active".to_owned()),
            Json(json!({ "email": " Invitee@Example.com ", "role": "member" })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response_json(response).await,
            json!({
                "data": {
                    "email": "invitee@example.com",
                    "role": "member",
                    "invitation_created": false
                }
            })
        );

        let rejected = mock_auth_response(
            "POST",
            "/api/auth/organization/invite-member",
            400,
            json!({ "code": "INVITATION_RATE_LIMITED", "message": "retry later" }),
        )
        .await;
        let response = invite_member(
            State(state_for(&rejected)),
            Extension(admin_user()),
            cookie_headers(),
            Path("org-active".to_owned()),
            Json(json!({ "email": "invitee@example.com", "role": "admin" })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_json(response).await,
            json!({ "code": "INVITATION_RATE_LIMITED", "message": "retry later" })
        );
    }

    #[tokio::test]
    async fn canonical_lookup_treats_only_a_valid_empty_list_as_absent() {
        let absent = mock_auth_response(
            "GET",
            "/api/auth/organization/list-members",
            200,
            json!({ "members": [] }),
        )
        .await;
        assert!(canonical_member(
            &state_for(&absent),
            &cookie_headers(),
            "org-active",
            "absent-user"
        )
        .await
        .expect("valid empty member list")
        .is_none());

        let malformed = mock_auth_response(
            "GET",
            "/api/auth/organization/list-members",
            200,
            json!({ "data": {} }),
        )
        .await;
        let malformed = canonical_member(
            &state_for(&malformed),
            &cookie_headers(),
            "org-active",
            "absent-user",
        )
        .await
        .expect_err("malformed success must fail closed");
        assert_eq!(malformed.status(), StatusCode::BAD_GATEWAY);

        let unavailable = mock_auth_response(
            "GET",
            "/api/auth/organization/list-members",
            503,
            json!({ "error": "down" }),
        )
        .await;
        let unavailable = canonical_member(
            &state_for(&unavailable),
            &cookie_headers(),
            "org-active",
            "absent-user",
        )
        .await
        .expect_err("authority failure must not become absence");
        assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn every_role_request_reaches_canonical_auth_serialization() {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/organization/list-members"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "members": [{ "id": "member-record", "userId": "target-user", "role": "member" }]
            })))
            .mount(&auth)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/auth/organization/update-member-role"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "role": "admin" })))
            .expect(2)
            .mount(&auth)
            .await;
        let state = state_for(&auth);

        let same = update_member_role(
            State(state.clone()),
            Extension(admin_user()),
            cookie_headers(),
            Path(("org-active".to_owned(), "target-user".to_owned())),
            Json(json!({ "role": "member" })),
        )
        .await;
        assert_eq!(same.status(), StatusCode::OK);
        assert_eq!(response_json(same).await, json!({ "role": "admin" }));

        let changed = update_member_role(
            State(state),
            Extension(admin_user()),
            cookie_headers(),
            Path(("org-active".to_owned(), "target-user".to_owned())),
            Json(json!({ "role": "admin" })),
        )
        .await;
        assert_eq!(changed.status(), StatusCode::OK);
        assert_eq!(response_json(changed).await, json!({ "role": "admin" }));
    }

    #[tokio::test]
    async fn list_and_invitation_acceptance_proxy_only_valid_authenticated_requests() {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/organization/list-members"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "members": [] })))
            .expect(1)
            .mount(&auth)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/organization/invitations/inv_123/accept"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "accepted": true })))
            .expect(1)
            .mount(&auth)
            .await;
        let state = state_for(&auth);

        let listed = list_members(
            State(state.clone()),
            Extension(admin_user()),
            cookie_headers(),
            Path("org-active".to_owned()),
        )
        .await;
        assert_eq!(listed.status(), StatusCode::OK);

        let accepted = accept_invitation(
            State(state.clone()),
            cookie_headers(),
            Path("inv_123".to_owned()),
        )
        .await;
        assert_eq!(accepted.status(), StatusCode::OK);

        let invalid =
            accept_invitation(State(state), cookie_headers(), Path("../escape".to_owned())).await;
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    }
}
