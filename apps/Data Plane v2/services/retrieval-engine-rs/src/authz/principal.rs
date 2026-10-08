//! Principal-aware admission for the HTTP surface (QB-T03).
//!
//! Two kinds of verified caller reach this engine, and they are admitted by
//! different authorities:
//!
//!   * a USER (`principal_type=user`, or a legacy token with none) is admitted
//!     by Control's membership decision exactly as before: the `x-org-id`
//!     header must equal the token's org, then `PolicyClient::resolve`.
//!   * a SERVICE (`principal_type=service`, `sub == service_id ==
//!     "service:<name>"`) has no membership row to look up (Auth Core answers
//!     `not_member`, which is what 403'd every quarry-edge retrieve). It is
//!     admitted by the default-deny route table in [`super::service_routes`]:
//!     the exact `(method, matched route)` and the scope the route names.
//!
//! The whole decision lives in [`admit`], a plain function over verified claims,
//! so the crate's integration tests drive the REAL rules (the real validator in
//! [`super::jwt`], the real `HttpPolicyClient`, this function) rather than a
//! permissive double. [`guard_middleware`] is the axum wrapper that builds the
//! request context, writes the audit row and runs the handler.
//!
//! Anything ambiguous is refused with 401, never guessed at: a service token
//! that also names a user, an unknown principal type, a `service:` subject that
//! does not say it is a service, a user token that carries a service id.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::{MatchedPath, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use super::context::{AuthContext, AuthMethod, Claims, EffectiveAcl};
use super::jwt::{bearer_token, ClaimsVerifier};
use super::policy::PolicyClient;
use super::service_routes::{self, ServiceRouteDecision, SERVICE_SUBJECT_PREFIX};
use super::visibility::VisibilityClient;
use crate::audit::{AccessEvent, AuditSink};

/// Who is calling, as established by verified claims.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Principal {
    /// An interactive user, admitted by Control membership.
    #[default]
    User,
    /// A first-party service, admitted by the route table.
    Service { service_id: String },
}

impl Principal {
    #[must_use]
    pub const fn is_service(&self) -> bool {
        matches!(self, Self::Service { .. })
    }

    #[must_use]
    pub fn service_id(&self) -> Option<&str> {
        match self {
            Self::Service { service_id } => Some(service_id),
            Self::User => None,
        }
    }
}

/// Why a verified token's identity claims could not be classified. Callers map
/// every variant to 401: the token is genuine but says something contradictory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalShapeError {
    /// `principal_type` is present and neither `user` nor `service`.
    UnknownPrincipalType,
    /// A `service` token whose `service_id`/`sub` do not agree on one
    /// `service:<name>` identity.
    AmbiguousServiceIdentity,
    /// A `service` token that also names a user or an acting user.
    ServiceCarriesUserIdentity,
    /// A `service` token with no usable scope.
    ServiceWithoutScopes,
    /// A `user` (or untyped) token that looks like a service, or that carries a
    /// delegation claim only a service token can carry.
    UserCarriesServiceIdentity,
}

/// Classify a verified token. This is the only place `principal_type` is read.
pub fn classify_principal(claims: &Claims) -> Result<Principal, PrincipalShapeError> {
    let subject_is_service = claims.sub.starts_with(SERVICE_SUBJECT_PREFIX);
    match claims.principal_type.as_deref() {
        Some("service") => {
            let service_id = claims
                .service_id
                .as_deref()
                .ok_or(PrincipalShapeError::AmbiguousServiceIdentity)?;
            // `service:<name>` with a non-empty, printable name.
            service_id
                .strip_prefix(SERVICE_SUBJECT_PREFIX)
                .filter(|name| {
                    !name.is_empty() && !name.chars().any(|c| c.is_whitespace() || c.is_control())
                })
                .ok_or(PrincipalShapeError::AmbiguousServiceIdentity)?;
            if !subject_is_service || service_id != claims.sub {
                return Err(PrincipalShapeError::AmbiguousServiceIdentity);
            }
            if claims.user_id.is_some() || claims.acting_user_id.is_some() {
                return Err(PrincipalShapeError::ServiceCarriesUserIdentity);
            }
            if claims.scopes.is_empty()
                || claims
                    .scopes
                    .iter()
                    .any(|scope| scope.is_empty() || scope.trim() != scope)
            {
                return Err(PrincipalShapeError::ServiceWithoutScopes);
            }
            Ok(Principal::Service {
                service_id: service_id.to_owned(),
            })
        }
        Some("user") | None => {
            if subject_is_service || claims.service_id.is_some() || claims.acting_user_id.is_some()
            {
                return Err(PrincipalShapeError::UserCarriesServiceIdentity);
            }
            Ok(Principal::User)
        }
        Some(_) => Err(PrincipalShapeError::UnknownPrincipalType),
    }
}

/// What the admission decision needs to know about the HTTP request.
pub struct AdmissionRequest<'a> {
    /// HTTP method, upper case (`Method::as_str`).
    pub method: &'a str,
    /// The router's route PATTERN (axum `MatchedPath`), not the raw path.
    pub matched_path: Option<&'a str>,
    /// The `x-org-id` header, if the caller sent one.
    pub org_header: Option<&'a str>,
}

/// A request the guard let through, with the authority it carries.
#[derive(Debug, Clone)]
pub struct Admitted {
    pub principal: Principal,
    pub acl: EffectiveAcl,
    pub scopes: Vec<String>,
}

/// A request the guard refused.
#[derive(Debug, Clone)]
pub struct Denial {
    pub status: StatusCode,
    /// Recorded in `access_audit_log.cause`.
    pub audit_cause: String,
    /// The `dpv2_authz_denials_total` `cause` label. Differs from `audit_cause`
    /// only for the header mismatch, whose label predates the `denied:` prefix.
    pub metric_cause: String,
    /// Stable machine code returned as `{"error": ..}`. `None` keeps the
    /// pre-existing empty-body status the user path always returned.
    pub error: Option<&'static str>,
    /// Known only when identity classification succeeded.
    pub principal: Option<Principal>,
}

impl Denial {
    fn new(status: StatusCode, cause: impl Into<String>) -> Self {
        let cause = cause.into();
        Self {
            status,
            metric_cause: cause.clone(),
            audit_cause: cause,
            error: None,
            principal: None,
        }
    }

    fn with_error(mut self, error: &'static str) -> Self {
        self.error = Some(error);
        self
    }

    fn for_principal(mut self, principal: Principal) -> Self {
        self.principal = Some(principal);
        self
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        match self.error {
            Some(code) => (self.status, Json(serde_json::json!({ "error": code }))).into_response(),
            None => self.status.into_response(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Admission {
    Admitted(Admitted),
    Denied(Denial),
}

/// The ACL a service context carries: read only, and no workspace / collection /
/// tag restriction beyond what the route and the viewer predicate already apply.
fn service_acl() -> EffectiveAcl {
    EffectiveAcl {
        can_read: true,
        ..Default::default()
    }
}

/// The whole admission decision for one verified token on one request.
///
/// Order: identity shape (401) -> tenant header (403) -> principal authority
/// (user: service-only refusal, then membership; service: route table). The
/// user path below the shape check is the behaviour the engine always had.
pub async fn admit(
    claims: &Claims,
    request: &AdmissionRequest<'_>,
    policy: &dyn PolicyClient,
) -> Admission {
    let principal = match classify_principal(claims) {
        Ok(principal) => principal,
        Err(_) => {
            return Admission::Denied(Denial::new(
                StatusCode::UNAUTHORIZED,
                "denied:principal_identity_invalid",
            ));
        }
    };

    let claims_org = claims.org_id.as_deref().unwrap_or_default();
    // §15-A core check: the header MUST match the claims. An absent header is
    // interpreted as "use the claims".
    let header_org = request.org_header.unwrap_or(claims_org);
    if header_org != claims_org && !claims_org.is_empty() {
        let mut denial = Denial::new(StatusCode::FORBIDDEN, "denied:header_claims_mismatch");
        denial.metric_cause = "header_claims_mismatch".to_owned();
        return Admission::Denied(denial.for_principal(principal));
    }

    match principal {
        Principal::Service { service_id } => {
            match service_routes::decide(
                &service_id,
                request.method,
                request.matched_path,
                &claims.scopes,
            ) {
                ServiceRouteDecision::Allowed => Admission::Admitted(Admitted {
                    principal: Principal::Service { service_id },
                    acl: service_acl(),
                    scopes: service_routes::service_context_scopes(&claims.scopes),
                }),
                ServiceRouteDecision::RouteNotAllowed => Admission::Denied(
                    Denial::new(StatusCode::FORBIDDEN, "denied:service_route_not_allowed")
                        .with_error("service_principal_route_not_allowed")
                        .for_principal(Principal::Service { service_id }),
                ),
                ServiceRouteDecision::ScopeMissing => Admission::Denied(
                    Denial::new(StatusCode::FORBIDDEN, "denied:service_scope_missing")
                        .with_error("service_principal_scope_missing")
                        .for_principal(Principal::Service { service_id }),
                ),
            }
        }
        Principal::User => {
            // A route that exists for services only is refused before any
            // membership lookup: a member's role says nothing about it.
            if service_routes::is_service_only(request.matched_path) {
                return Admission::Denied(
                    Denial::new(StatusCode::FORBIDDEN, "denied:service_only_route")
                        .with_error("service_only_route")
                        .for_principal(Principal::User),
                );
            }
            // §15-B + §15-C: PolicyClient lookup.
            let decision = policy.resolve(&claims.sub, claims_org).await;
            if !decision.is_member {
                return Admission::Denied(
                    Denial::new(StatusCode::FORBIDDEN, decision.cause)
                        .for_principal(Principal::User),
                );
            }
            Admission::Admitted(Admitted {
                principal: Principal::User,
                acl: decision.acl,
                scopes: claims.scopes.clone(),
            })
        }
    }
}

/// The request context for a verified USER token (before the policy decision's
/// ACL is applied). The guard overlays the admitted principal, ACL and scopes.
pub fn auth_context_from_verified_claims(
    claims: &Claims,
    request_id: String,
    token: String,
) -> AuthContext {
    AuthContext {
        user_id: Some(claims.sub.clone()),
        org_id: claims.org_id.clone().unwrap_or_default(),
        auth_method: AuthMethod::Jwt,
        scopes: claims.scopes.clone(),
        zdr: claims.zdr,
        sovereign: claims.sovereign,
        acl: EffectiveAcl::allow_all(),
        request_id,
        verified_bearer: Some(token),
        principal: Principal::User,
    }
}

/// Resolve a viewer's EXPLICIT document grants.
///
/// A service principal has no per-user grants: it is never sent to user-core's
/// visibility facade (which would refuse its bearer and fail open to empty after
/// paying the call's latency). It sees organization-shared documents only, which
/// is also the least it can see. Users and anonymous callers behave as before.
pub async fn resolve_explicit_grants<V>(
    visibility: &V,
    org_id: &str,
    user_id: Option<&str>,
    service_principal: bool,
    verified_bearer: Option<&str>,
) -> Vec<String>
where
    V: VisibilityClient + ?Sized,
{
    if service_principal {
        return Vec::new();
    }
    match user_id {
        Some(user_id) => {
            visibility
                .visible_documents(org_id, user_id, verified_bearer)
                .await
        }
        None => Vec::new(),
    }
}

/// Everything the HTTP guard needs, injected so the crate's tests can run the
/// real guard with a real policy client and a recording audit sink.
#[derive(Clone)]
pub struct AuthGuard {
    policy: Arc<dyn PolicyClient>,
    audit: Arc<dyn AuditSink>,
    verifier: Arc<dyn ClaimsVerifier>,
}

impl AuthGuard {
    pub fn new(
        policy: Arc<dyn PolicyClient>,
        audit: Arc<dyn AuditSink>,
        verifier: Arc<dyn ClaimsVerifier>,
    ) -> Self {
        Self {
            policy,
            audit,
            verifier,
        }
    }
}

/// A rejected request has no verified posture to read: strict placeholders,
/// matching what a refusal has always recorded.
fn denial_context(claims: &Claims, request_id: &str, principal: Option<Principal>) -> AuthContext {
    AuthContext {
        user_id: Some(claims.sub.clone()),
        org_id: claims.org_id.clone().unwrap_or_default(),
        auth_method: AuthMethod::Jwt,
        scopes: vec![],
        zdr: true,
        sovereign: Some(true),
        acl: EffectiveAcl::default(),
        request_id: request_id.to_string(),
        verified_bearer: None,
        principal: principal.unwrap_or_default(),
    }
}

/// Wave 3 §15 — authentication, principal admission, `AuthContext` injection
/// and the access-audit row. Mount with `route_layer` so `MatchedPath` is set.
///
///  1. Verify the bearer (401 on any failure; nothing is audited, there is no
///     authenticated caller yet).
///  2. [`admit`]: identity shape, tenant header, then the principal's authority.
///  3. Inject the `AuthContext`; the handler and the pipeline read it.
///  4. After the response, append one row to `access_audit_log`.
pub async fn guard_middleware(
    State(guard): State<AuthGuard>,
    mut req: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    let method = req.method().as_str().to_owned();
    let endpoint_label = format!("{} {}", req.method(), req.uri().path());
    let matched_path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| matched.as_str().to_owned());

    // Stable request id for log correlation. Re-use X-Request-Id if the caller
    // set one; otherwise mint a fresh one.
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let org_header = req
        .headers()
        .get("x-org-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // A fleet-shared API key is not an identity and can no longer authorize a
    // tenant. User and service callers both use audience-scoped signed tokens.
    let Some(token) = bearer_token(req.headers()) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let claims = match guard.verifier.verify(&token).await {
        Ok(claims) => claims,
        Err(status) => return status.into_response(),
    };

    let admission = admit(
        &claims,
        &AdmissionRequest {
            method: &method,
            matched_path: matched_path.as_deref(),
            org_header: org_header.as_deref(),
        },
        guard.policy.as_ref(),
    )
    .await;

    let admitted = match admission {
        Admission::Admitted(admitted) => admitted,
        Admission::Denied(denial) => {
            let org = claims.org_id.clone().unwrap_or_default();
            crate::metrics::record_authz_denial(&org, &denial.metric_cause);
            if let Some(service_id) = denial.principal.as_ref().and_then(Principal::service_id) {
                crate::metrics::record_service_principal_request(service_id, &denial.audit_cause);
                // The one thing an operator hunts for when a newly added
                // caller is refused: who, where, and why. No token content.
                tracing::warn!(
                    service_id,
                    org_id = %org,
                    endpoint = %endpoint_label,
                    cause = %denial.audit_cause,
                    "service principal refused at admission"
                );
            }
            let ctx = denial_context(&claims, &request_id, denial.principal.clone());
            guard
                .audit
                .record(AccessEvent {
                    ctx: &ctx,
                    endpoint: &endpoint_label,
                    http_status: denial.status.as_u16(),
                    latency_ms: started.elapsed().as_millis() as u32,
                    document_ids: vec![],
                    cause: &denial.audit_cause,
                })
                .await;
            return denial.into_response();
        }
    };

    let mut auth_ctx = auth_context_from_verified_claims(&claims, request_id, token);
    auth_ctx.principal = admitted.principal;
    auth_ctx.acl = admitted.acl;
    auth_ctx.scopes = admitted.scopes;
    if let Some(service_id) = auth_ctx.principal.service_id() {
        crate::metrics::record_service_principal_request(service_id, "admitted");
    }

    // org_id must be present at this point.
    if auth_ctx.org_id.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }

    // Inject into request extensions for downstream consumers.
    let auth_for_audit = auth_ctx.clone();
    req.extensions_mut().insert(auth_ctx);

    let response = next.run(req).await;
    let status = response.status();
    let latency_ms = started.elapsed().as_millis() as u32;

    // Audit-log append (best-effort).
    let cause = if status.is_success() {
        "ok"
    } else if status.as_u16() == 403 {
        "denied:downstream_forbidden"
    } else if status.is_client_error() {
        "error:client"
    } else {
        "error:server"
    };
    guard
        .audit
        .record(AccessEvent {
            ctx: &auth_for_audit,
            endpoint: &endpoint_label,
            http_status: status.as_u16(),
            latency_ms,
            document_ids: vec![], // populated by handlers in v2.4+
            cause,
        })
        .await;

    response
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;

    fn claims(value: Value) -> Claims {
        serde_json::from_value(value).expect("claims")
    }

    fn service() -> Value {
        json!({
            "sub": "service:quarry-edge",
            "service_id": "service:quarry-edge",
            "principal_type": "service",
            "org_id": "org-a",
            "scopes": ["data:read"],
            "zdr": false,
        })
    }

    fn user() -> Value {
        json!({
            "sub": "user-1",
            "user_id": "user-1",
            "principal_type": "user",
            "org_id": "org-a",
            "scopes": ["data:read"],
            "zdr": false,
        })
    }

    fn with(mut value: Value, key: &str, new: Value) -> Value {
        value[key] = new;
        value
    }

    fn without(mut value: Value, key: &str) -> Value {
        value.as_object_mut().expect("object").remove(key);
        value
    }

    #[test]
    fn a_well_formed_service_token_is_a_service() {
        assert_eq!(
            classify_principal(&claims(service())),
            Ok(Principal::Service {
                service_id: "service:quarry-edge".into()
            })
        );
    }

    #[test]
    fn a_user_token_with_or_without_a_principal_type_is_a_user() {
        assert_eq!(classify_principal(&claims(user())), Ok(Principal::User));
        // Legacy tokens predate `principal_type`.
        let legacy = without(without(user(), "principal_type"), "user_id");
        assert_eq!(classify_principal(&claims(legacy)), Ok(Principal::User));
    }

    #[test]
    fn contradictory_identities_are_refused() {
        let cases = [
            with(service(), "service_id", json!("service:other")),
            with(service(), "user_id", json!("service:quarry-edge")),
            with(service(), "acting_user_id", json!("user-1")),
            with(
                with(service(), "sub", json!("quarry-edge")),
                "service_id",
                json!("quarry-edge"),
            ),
            with(
                with(service(), "sub", json!("service:")),
                "service_id",
                json!("service:"),
            ),
            without(service(), "service_id"),
            without(service(), "scopes"),
            with(service(), "scopes", json!([""])),
            with(service(), "scopes", json!([" data:read"])),
            with(service(), "principal_type", json!("agent")),
            without(service(), "principal_type"),
            with(service(), "principal_type", json!("user")),
            with(user(), "service_id", json!("service:quarry-edge")),
            with(user(), "acting_user_id", json!("user-2")),
            with(user(), "sub", json!("service:impostor")),
        ];
        for case in cases {
            assert!(classify_principal(&claims(case.clone())).is_err(), "{case}");
        }
    }

    #[test]
    fn principal_serializes_tagged_and_defaults_to_user() {
        assert_eq!(Principal::default(), Principal::User);
        assert_eq!(
            serde_json::to_value(Principal::Service {
                service_id: "service:quarry-edge".into()
            })
            .expect("json"),
            json!({"type": "service", "service_id": "service:quarry-edge"})
        );
        assert_eq!(
            serde_json::to_value(Principal::User).expect("json"),
            json!({"type": "user"})
        );
    }
}
