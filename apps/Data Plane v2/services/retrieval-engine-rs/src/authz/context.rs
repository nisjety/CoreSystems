//! `AuthContext` + JWT claims decoding. Owns no business logic — pure data.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[allow(dead_code)] // variants surface in audit logs + future code paths; serde-tagged
pub enum AuthMethod {
    /// `INTERNAL_API_KEY` shared secret. No identity attached.
    ApiKey,
    /// Bearer JWT verified RS256 against `JWT_PUBLIC_KEY_PEM`.
    Jwt,
    /// No credential. Only `/health` / `/readyz` / `/metrics` reach here.
    Anonymous,
}

impl AuthMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthMethod::ApiKey => "api_key",
            AuthMethod::Jwt => "jwt",
            AuthMethod::Anonymous => "anonymous",
        }
    }
}

/// Claims we expect in a Data Plane v2 JWT. Mirror of the `auth-service`
/// (Better Auth) issuance shape. Unknown claims are ignored — `serde_json::Value`
/// would let us pass them through but pollutes the type.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // JWT fields decoded from auth-service; not all are read today
pub struct Claims {
    pub sub: String, // user_id (RFC 7519 conventional)
    #[serde(default)]
    pub org_id: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub exp: Option<usize>,
    #[serde(default)]
    pub iss: Option<String>,
    #[serde(default)]
    pub aud: Option<String>,
}

/// What this user is effectively allowed to see in this org, as
/// returned by `org-core.GetUserPermissions(user_id, org_id)`.
/// Empty vectors mean "no restriction at that axis"; the caller still
/// gets full org-scoped data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EffectiveAcl {
    #[serde(default)]
    pub workspaces: Vec<String>,
    #[serde(default)]
    pub collections: Vec<String>,
    #[serde(default)]
    pub acl_tags: Vec<String>,
    #[serde(default)]
    pub can_read: bool,
    #[serde(default)]
    pub can_write: bool,
    #[serde(default)]
    pub can_delete: bool,
}

impl EffectiveAcl {
    /// Default-allow ACL used when Control Plane enforcement is `off`.
    /// Keeps v2.3 backward compatibility — caller's own filters still apply.
    pub fn allow_all() -> Self {
        Self {
            workspaces: vec![],
            collections: vec![],
            acl_tags: vec![],
            can_read: true,
            can_write: true,
            can_delete: true,
        }
    }
}

/// Populated by the auth middleware at request entry; consumed by the
/// retrieval pipeline + audit-log writer. Cheap to clone (small vectors).
#[derive(Clone, Serialize)]
pub struct AuthContext {
    pub user_id: Option<String>,
    pub org_id: String,
    pub auth_method: AuthMethod,
    pub scopes: Vec<String>,
    pub acl: EffectiveAcl,
    /// Echoed back to the caller in `X-Request-Id` and persisted in
    /// `access_audit_log.request_id` for cross-service correlation.
    pub request_id: String,
    /// Original bearer retained only in memory after cryptographic verification
    /// so Control can independently prove an end-user delegation. Never log or
    /// serialize this value.
    #[serde(skip_serializing)]
    pub verified_bearer: Option<String>,
}

impl AuthContext {
    /// Build a context that grants org-scoped access only (no per-user ACL).
    /// Used when `CONTROL_PLANE_ENFORCEMENT=off` or for `API_KEY`-only calls
    /// where there is no user identity to enforce against.
    #[allow(dead_code)]
    pub fn org_scoped(org_id: impl Into<String>, method: AuthMethod, request_id: String) -> Self {
        Self {
            user_id: None,
            org_id: org_id.into(),
            auth_method: method,
            scopes: vec![],
            acl: EffectiveAcl::allow_all(),
            request_id,
            verified_bearer: None,
        }
    }

    /// Intersect the caller's requested filters with the effective ACL.
    /// Returns a permitted-filter set: empty vector at any axis means "no
    /// restriction", non-empty means "must subset". When the caller asks
    /// for something they're not allowed to see, that item is silently
    /// dropped (not an error — informs the audit log).
    pub fn intersect_filter<'a>(
        &self,
        requested: &'a [String],
        axis: AclAxis,
    ) -> std::borrow::Cow<'a, [String]> {
        let allowed: &[String] = match axis {
            AclAxis::Workspaces => &self.acl.workspaces,
            AclAxis::Collections => &self.acl.collections,
            AclAxis::AclTags => &self.acl.acl_tags,
        };

        // No ACL restriction → caller's filter passes through unchanged.
        if allowed.is_empty() {
            return std::borrow::Cow::Borrowed(requested);
        }
        // Caller didn't ask for anything specific → enforce the ACL exactly.
        if requested.is_empty() {
            return std::borrow::Cow::Owned(allowed.to_vec());
        }
        // Both sides have constraints → intersection.
        let allowed_set: std::collections::HashSet<&String> = allowed.iter().collect();
        let intersected: Vec<String> = requested
            .iter()
            .filter(|r| allowed_set.contains(r))
            .cloned()
            .collect();
        std::borrow::Cow::Owned(intersected)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AclAxis {
    Workspaces,
    Collections,
    AclTags,
}

impl AuthContext {
    /// Wave-3.1 §15-C completion. Overwrite the caller's filter axes in
    /// `req.filters.{workspaces, collections, acl_tags}` with the intersection
    /// of what they asked for and what the org-core ACL permits. Also pins
    /// `req.org_id` and stamps `req.user_id` from the authenticated principal.
    ///
    /// Idempotent. Called from HTTP retrieval handlers immediately before
    /// `pipeline.retrieve(req)`.
    pub fn apply_to_request(&self, req: &mut crate::pipeline::types::RetrievalRequest) {
        let workspaces = self
            .intersect_filter(&req.filters.workspaces, AclAxis::Workspaces)
            .into_owned();
        let collections = self
            .intersect_filter(&req.filters.collections, AclAxis::Collections)
            .into_owned();
        let acl_tags = self
            .intersect_filter(&req.filters.acl_tags, AclAxis::AclTags)
            .into_owned();
        req.filters.workspaces = workspaces;
        req.filters.collections = collections;
        req.filters.acl_tags = acl_tags;

        if !self.org_id.is_empty() {
            req.org_id = self.org_id.clone();
        }
        // The authenticated principal's user_id OVERRIDES any caller-supplied
        // body value — a client must never be able to set the viewer identity
        // the ownership post-filter enforces against by putting a different
        // user_id in the request JSON.
        if self.user_id.is_some() {
            req.user_id = self.user_id.clone();
        }
        req.verified_bearer = self.verified_bearer.clone();

        // Org-admin super-visibility derives ONLY from a verified scope. This is
        // reached only on the JWT HTTP path; the api-key/agent path carries no
        // scopes, so admin bypass can never leak into agent grounding.
        req.admin_read_all = self.scopes.iter().any(|s| s == "org:data:read_all");
    }
}

/// Pin `org_id` from a verified [`AuthContext`] (Phase-1 GAP-1). Auxiliary read
/// handlers (graph / wiki / contradictions / timeline / semantic-cache) use
/// bespoke request structs rather than `RetrievalRequest`, so they can't call
/// [`AuthContext::apply_to_request`]; this gives them the same org pin. The
/// An absent body org is populated from claims. A conflicting body org is
/// rejected before data access so spoof attempts remain observable and receive
/// a deterministic 403 instead of being silently rewritten.
/// No-op when there is no context (mirrors the `/v1/retrieve` Option pattern).
#[derive(Debug, thiserror::Error)]
#[error("authenticated tenant does not match requested tenant")]
pub struct OrgScopeMismatch;

pub fn pin_org_from_ctx(
    ctx: Option<&AuthContext>,
    org_id: &mut String,
) -> Result<(), OrgScopeMismatch> {
    if let Some(ctx) = ctx {
        if !ctx.org_id.is_empty() {
            if org_id.is_empty() {
                *org_id = ctx.org_id.clone();
            } else if *org_id != ctx.org_id {
                return Err(OrgScopeMismatch);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with_acl(workspaces: Vec<&str>) -> AuthContext {
        AuthContext {
            user_id: Some("u1".into()),
            org_id: "org-a".into(),
            auth_method: AuthMethod::Jwt,
            scopes: vec![],
            acl: EffectiveAcl {
                workspaces: workspaces.into_iter().map(String::from).collect(),
                ..Default::default()
            },
            request_id: "req".into(),
            verified_bearer: None,
        }
    }

    #[test]
    fn verified_bearer_is_never_serialized() {
        let mut ctx = ctx_with_acl(vec![]);
        ctx.verified_bearer = Some("sensitive.jwt.proof".into());
        let encoded = serde_json::to_string(&ctx).expect("serialize auth context");
        assert!(!encoded.contains("sensitive.jwt.proof"));
        assert!(!encoded.contains("verified_bearer"));
    }

    #[test]
    fn no_restriction_passes_caller_filter_through() {
        let c = ctx_with_acl(vec![]);
        let requested = vec!["a".to_string(), "b".to_string()];
        let out = c.intersect_filter(&requested, AclAxis::Workspaces);
        assert_eq!(out.as_ref(), &["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn empty_caller_filter_yields_acl() {
        let c = ctx_with_acl(vec!["w1", "w2"]);
        let out = c.intersect_filter(&[], AclAxis::Workspaces);
        assert_eq!(out.as_ref(), &["w1".to_string(), "w2".to_string()]);
    }

    #[test]
    fn intersect_drops_unauthorized_filter_items() {
        let c = ctx_with_acl(vec!["w1", "w2"]);
        let requested = vec!["w1".to_string(), "w3".to_string()];
        let out = c.intersect_filter(&requested, AclAxis::Workspaces);
        assert_eq!(out.as_ref(), &["w1".to_string()]);
    }

    #[test]
    fn allow_all_grants_full_access() {
        let acl = EffectiveAcl::allow_all();
        assert!(acl.can_read && acl.can_write && acl.can_delete);
        assert!(acl.workspaces.is_empty());
    }

    #[test]
    fn pin_org_rejects_body_org_mismatch() {
        let ctx = AuthContext::org_scoped("org-a", AuthMethod::Jwt, "req".into());
        let mut body_org = "org-b".to_string(); // attacker-supplied body org
        assert!(pin_org_from_ctx(Some(&ctx), &mut body_org).is_err());
        assert_eq!(
            body_org, "org-b",
            "mismatched request must not be rewritten"
        );
    }

    #[test]
    fn pin_org_populates_an_absent_body_org() {
        let ctx = AuthContext::org_scoped("org-a", AuthMethod::Jwt, "req".into());
        let mut body_org = String::new();
        pin_org_from_ctx(Some(&ctx), &mut body_org).expect("claim should populate empty org");
        assert_eq!(body_org, "org-a");
    }

    #[test]
    fn pin_org_is_noop_without_ctx() {
        let mut body_org = "org-b".to_string();
        pin_org_from_ctx(None, &mut body_org).expect("no context remains a no-op");
        assert_eq!(
            body_org, "org-b",
            "no ctx → unchanged (same Option semantics as /v1/retrieve)"
        );
    }
}
