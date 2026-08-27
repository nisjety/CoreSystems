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
    /// Signed Zero Data Retention posture. Required at deserialization so an
    /// absent, null, or non-boolean claim fails authentication closed.
    pub zdr: bool,
    /// Signed sovereign-infrastructure posture: this org/user requires
    /// processing to stay on sovereign (currently Norwegian) infrastructure.
    /// A distinct axis from `zdr`: ZDR is a promise about data RETENTION
    /// (does the provider keep it), sovereignty is a promise about
    /// JURISDICTION (which soil the processing happens on). A provider can
    /// satisfy one without the other.
    ///
    /// Unlike `zdr`, this is NOT hard-required at deserialization —
    /// deliberately, and only because of a real constraint verified against
    /// the live system: as of 2026-08-22, auth-core's `issuePlaneToken` (the
    /// only real minter of these tokens) has never heard of this claim and
    /// emits none. Making it hard-required the way `zdr` is would reject
    /// every token in production the moment this ships — a full
    /// authentication outage, not a compliance improvement.
    ///
    /// `None` means ABSENT — genuinely unknown, not "required". That
    /// distinction is load bearing, and getting it wrong took the dense arm
    /// down: this field previously defaulted an absent claim to `true` on the
    /// reasoning that the strictest posture is always the safe one. It is not,
    /// because "sovereignty required" is UNSATISFIABLE by the configured
    /// embedding provider — Cohere Embed v4 is Azure-hosted, so
    /// `EMBEDDING_PROVIDER=cohere` plus a blanket `true` made every query
    /// embedding fail closed and returned HTTP 500 for every dense retrieval
    /// in the deployment. Since auth-core emits no claim at all, that applied
    /// to literally every token. A default that makes the primary retrieval
    /// path unreachable is an outage wearing a compliance costume.
    ///
    /// The posture ladder is therefore:
    ///   * `Some(true)`  — signed floor. Cannot be relaxed by any request field.
    ///   * `Some(false)` — signed as not required; a caller may still opt IN.
    ///   * `None`        — unknown, so the CALLER's `sovereign_required`
    ///                     governs; a caller that declares nothing still fails
    ///                     closed, because `pipeline::retrieve` treats its own
    ///                     absent value as `true`.
    ///
    /// Nothing here fails open: silence at BOTH levels is still strict. What
    /// changed is that an authenticated caller can once again declare the
    /// posture of its own data, which is exactly what the request field is for.
    /// A PRESENT-but-malformed claim (wrong type) still fails closed via
    /// ordinary deserialization — only true absence yields `None`.
    /// Revisit requiring this once auth-core mints it for real.
    #[serde(default)]
    pub sovereign: Option<bool>,
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
    /// Cryptographically verified request-retention posture from the JWT.
    pub zdr: bool,
    /// Cryptographically verified sovereign-infrastructure posture from the
    /// JWT. See [`Claims::sovereign`] for why this is a separate axis from
    /// `zdr` rather than folded into it, and why `None` (absent claim) means
    /// unknown rather than required.
    pub sovereign: Option<bool>,
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
    /// Merge caller-requested posture with the cryptographically verified
    /// authority posture monotonically. `reject` remains stricter than
    /// `ephemeral`; a signed `zdr=true` can never become durable/disabled.
    #[must_use]
    pub const fn effective_zdr_mode(
        &self,
        requested: Option<crate::pipeline::types::ZdrMode>,
    ) -> Option<crate::pipeline::types::ZdrMode> {
        use crate::pipeline::types::ZdrMode;

        if self.zdr {
            match requested {
                Some(ZdrMode::Reject) => Some(ZdrMode::Reject),
                _ => Some(ZdrMode::Ephemeral),
            }
        } else {
            requested
        }
    }

    /// Merge caller-requested sovereignty with the cryptographically verified
    /// authority posture monotonically — same shape as [`Self::effective_zdr_mode`],
    /// simpler because sovereignty has no `Reject`/`Ephemeral` gradation: a
    /// query either may only touch sovereign infrastructure, or it may not.
    /// A signed `sovereign=true` can never be relaxed by an unsigned request
    /// field; a signed `sovereign=false` still lets a caller opt IN to a
    /// stricter posture for one particular request.
    ///
    /// An ABSENT claim (`None`) defers to the caller rather than imposing the
    /// strict posture. It has to: the strict posture is unsatisfiable by an
    /// Azure-hosted embedding provider, so imposing it on an absent claim —
    /// and auth-core emits none — made every dense query fail closed. Deferring
    /// is not failing open, because a caller that requests nothing either still
    /// ends up strict (`pipeline::retrieve` reads its own `None` as `true`).
    /// See [`Claims::sovereign`] for the full ladder.
    #[must_use]
    pub const fn effective_sovereign_required(&self, requested: Option<bool>) -> Option<bool> {
        match self.sovereign {
            Some(true) => Some(true),
            Some(false) | None => requested,
        }
    }

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
            zdr: true,
            // Strictest default, matching `zdr: true` above: absent a verified
            // claim either way, assume the stricter posture rather than the
            // permissive one (`Residency::classify`'s doctrine — unproven is
            // not the same claim as proven-safe).
            sovereign: Some(true),
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
        req.zdr_mode = self.effective_zdr_mode(req.zdr_mode);
        req.sovereign_required = self.effective_sovereign_required(req.sovereign_required);

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
    use crate::pipeline::types::{RetrievalRequest, ZdrMode};

    fn ctx_with_acl(workspaces: Vec<&str>) -> AuthContext {
        AuthContext {
            user_id: Some("u1".into()),
            org_id: "org-a".into(),
            auth_method: AuthMethod::Jwt,
            scopes: vec![],
            zdr: false,
            sovereign: Some(false),
            acl: EffectiveAcl {
                workspaces: workspaces.into_iter().map(String::from).collect(),
                ..Default::default()
            },
            request_id: "req".into(),
            verified_bearer: None,
        }
    }

    fn retrieval_request(zdr_mode: Option<ZdrMode>) -> RetrievalRequest {
        let mut value = serde_json::json!({
            "org_id": "body-org",
            "query": "synthetic boundary test"
        });
        if let Some(mode) = zdr_mode {
            value["zdr_mode"] = serde_json::to_value(mode).expect("serialize mode");
        }
        serde_json::from_value(value).expect("valid retrieval request")
    }

    #[test]
    fn signed_zdr_forces_http_request_to_ephemeral_without_downgrading_reject() {
        let ctx = AuthContext {
            zdr: true,
            ..ctx_with_acl(vec![])
        };

        for requested in [None, Some(ZdrMode::Disabled), Some(ZdrMode::Ephemeral)] {
            let mut request = retrieval_request(requested);
            ctx.apply_to_request(&mut request);
            assert_eq!(request.zdr_mode, Some(ZdrMode::Ephemeral));
        }

        let mut reject = retrieval_request(Some(ZdrMode::Reject));
        ctx.apply_to_request(&mut reject);
        assert_eq!(reject.zdr_mode, Some(ZdrMode::Reject));
    }

    #[test]
    fn signed_non_zdr_preserves_any_stricter_caller_posture() {
        let ctx = ctx_with_acl(vec![]);
        for requested in [
            None,
            Some(ZdrMode::Disabled),
            Some(ZdrMode::Reject),
            Some(ZdrMode::Ephemeral),
        ] {
            let mut request = retrieval_request(requested);
            ctx.apply_to_request(&mut request);
            assert_eq!(request.zdr_mode, requested);
        }
    }

    fn retrieval_request_sovereign(sovereign_required: Option<bool>) -> RetrievalRequest {
        let mut value = serde_json::json!({
            "org_id": "body-org",
            "query": "synthetic boundary test"
        });
        if let Some(v) = sovereign_required {
            value["sovereign_required"] = serde_json::json!(v);
        }
        serde_json::from_value(value).expect("valid retrieval request")
    }

    /// Mirrors `signed_zdr_forces_http_request_to_ephemeral_without_downgrading_reject`.
    /// No `Reject`-style exception here — sovereignty has no gradation to
    /// preserve, a signed `sovereign=true` always wins outright.
    #[test]
    fn signed_sovereign_forces_every_request_to_required_regardless_of_caller() {
        let ctx = AuthContext {
            sovereign: Some(true),
            ..ctx_with_acl(vec![])
        };

        for requested in [None, Some(false), Some(true)] {
            let mut request = retrieval_request_sovereign(requested);
            ctx.apply_to_request(&mut request);
            assert_eq!(request.sovereign_required, Some(true));
        }
    }

    #[test]
    fn signed_non_sovereign_preserves_any_stricter_caller_posture() {
        let ctx = ctx_with_acl(vec![]);
        for requested in [None, Some(false), Some(true)] {
            let mut request = retrieval_request_sovereign(requested);
            ctx.apply_to_request(&mut request);
            assert_eq!(request.sovereign_required, requested);
        }
    }

    /// The regression this whole tri-state exists for.
    ///
    /// auth-core mints no `sovereign` claim, so every real token arrives with
    /// `None`. When `None` was promoted to "required", `sovereign_required`
    /// came out `Some(true)` for every request no matter what the caller asked
    /// — and because Azure-hosted Cohere Embed v4 can never satisfy
    /// sovereignty, every dense query returned HTTP 500. An absent claim must
    /// therefore defer to the caller, NOT impose the strict posture.
    #[test]
    fn an_absent_sovereign_claim_defers_to_the_caller() {
        let ctx = AuthContext {
            sovereign: None,
            ..ctx_with_acl(vec![])
        };

        for requested in [None, Some(false), Some(true)] {
            let mut request = retrieval_request_sovereign(requested);
            ctx.apply_to_request(&mut request);
            assert_eq!(
                request.sovereign_required, requested,
                "an unknown claim must not override the caller's declaration"
            );
        }
    }

    /// Deferring is not failing open: a caller that declares nothing is still
    /// strict, because `pipeline::retrieve` reads its own `None` as `true`.
    /// This pins the half of that contract that lives here — the merge leaves
    /// `None` alone rather than rewriting it to `Some(false)`.
    #[test]
    fn an_absent_claim_and_a_silent_caller_stay_undeclared_for_the_pipeline() {
        let ctx = AuthContext {
            sovereign: None,
            ..ctx_with_acl(vec![])
        };

        let mut request = retrieval_request_sovereign(None);
        ctx.apply_to_request(&mut request);
        assert_eq!(
            request.sovereign_required, None,
            "must stay None so the pipeline's fail-closed default applies"
        );
        // The pipeline's rule, restated here so the two halves cannot drift.
        assert!(
            request.sovereign_required.unwrap_or(true),
            "a silent caller must still resolve to sovereignty required"
        );
    }

    /// A signed floor still cannot be relaxed — the fix must not have widened
    /// anything for a token that actually asserts the requirement.
    #[test]
    fn a_signed_sovereign_floor_still_beats_an_opt_out() {
        let ctx = AuthContext {
            sovereign: Some(true),
            ..ctx_with_acl(vec![])
        };

        let mut request = retrieval_request_sovereign(Some(false));
        ctx.apply_to_request(&mut request);
        assert_eq!(request.sovereign_required, Some(true));
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
