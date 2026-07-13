use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

pub const REBUILD_SCOPE: &str = "data:search:rebuild";
pub const GLOBAL_REBUILD_SCOPE: &str = "data:search:rebuild:global";
pub const APPROVE_REBUILD_SCOPE: &str = "data:search:rebuild:approve";
pub const APPROVE_GLOBAL_REBUILD_SCOPE: &str = "data:search:rebuild:approve:global";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminClaims {
    #[serde(rename = "user_id")]
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(rename = "service_id", default)]
    pub service_id: Option<String>,
    #[serde(rename = "principal_type")]
    pub principal_type: String,
    #[serde(rename = "org_id")]
    pub org_id: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(rename = "iss")]
    pub issuer: String,
    #[serde(rename = "aud")]
    pub audience: String,
    #[serde(rename = "sub")]
    pub subject: String,
    #[serde(rename = "exp")]
    pub expires_at: u64,
}

impl AdminClaims {
    pub(crate) fn has_scope(&self, required: &str) -> bool {
        self.scopes.iter().any(|scope| scope == required)
    }
}

pub struct AdminVerifier {
    key: DecodingKey,
    validation: Validation,
}

impl AdminVerifier {
    pub fn from_pem(public_key_pem: &[u8], audience: &str, issuer: &str) -> anyhow::Result<Self> {
        let audience = audience.trim();
        let issuer = issuer.trim();
        anyhow::ensure!(!audience.is_empty(), "admin JWT audience is required");
        anyhow::ensure!(!issuer.is_empty(), "admin JWT issuer is required");

        let key = DecodingKey::from_rsa_pem(public_key_pem)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[audience]);
        validation.set_issuer(&[issuer]);
        validation.leeway = 30;
        validation.validate_exp = true;

        Ok(Self { key, validation })
    }

    pub fn verify_authorization(
        &self,
        authorization: Option<&str>,
    ) -> Result<AdminClaims, AuthError> {
        let header = authorization.ok_or(AuthError::Unauthorized)?;
        let (scheme, token) = header.split_once(' ').ok_or(AuthError::Unauthorized)?;
        if !scheme.eq_ignore_ascii_case("bearer") || token.trim().is_empty() {
            return Err(AuthError::Unauthorized);
        }

        let claims = decode::<AdminClaims>(token.trim(), &self.key, &self.validation)
            .map_err(|_| AuthError::Unauthorized)?
            .claims;
        let identity_valid = match claims.principal_type.as_str() {
            "user" => claims.user_id.as_deref().is_some_and(|user_id| {
                !user_id.trim().is_empty()
                    && user_id == user_id.trim()
                    && user_id == claims.subject
                    && claims.service_id.is_none()
            }),
            "service" => claims.service_id.as_deref().is_some_and(|service_id| {
                !service_id.trim().is_empty()
                    && service_id == service_id.trim()
                    && service_id == claims.subject
                    && claims.user_id.is_none()
            }),
            _ => false,
        };
        if !identity_valid
            || claims.subject.trim().is_empty()
            || claims.org_id.trim().is_empty()
            || claims.org_id != claims.org_id.trim()
        {
            return Err(AuthError::Unauthorized);
        }
        Ok(claims)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    Unauthorized,
    Forbidden,
    BadRequest,
}

#[derive(Debug, Clone)]
pub struct RebuildIntent {
    pub requested_org_id: Option<String>,
    pub global: bool,
    pub break_glass: bool,
    pub dry_run: bool,
    pub clear: bool,
    pub approval_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebuildAuthorization {
    Org {
        org_id: String,
        dry_run: bool,
        clear: bool,
    },
    Global {
        dry_run: bool,
        clear: bool,
    },
}

pub fn authorize_rebuild(
    claims: &AdminClaims,
    intent: &RebuildIntent,
) -> Result<RebuildAuthorization, AuthError> {
    if !claims.has_scope(REBUILD_SCOPE) {
        return Err(AuthError::Forbidden);
    }

    if intent
        .requested_org_id
        .as_ref()
        .is_some_and(|value| value.trim().is_empty() || value != value.trim() || value.len() > 128)
    {
        return Err(AuthError::BadRequest);
    }

    if !intent.dry_run
        && (!valid_admin_metadata(&intent.approval_id, 200)
            || !valid_admin_metadata(&intent.idempotency_key, 200)
            || !valid_admin_reason(&intent.reason))
    {
        return Err(AuthError::BadRequest);
    }

    if intent.global {
        if !claims.has_scope(GLOBAL_REBUILD_SCOPE)
            || !intent.break_glass
            || intent
                .requested_org_id
                .as_ref()
                .is_some_and(|org| !org.trim().is_empty())
        {
            return Err(AuthError::Forbidden);
        }
        return Ok(RebuildAuthorization::Global {
            dry_run: intent.dry_run,
            clear: intent.clear,
        });
    }

    let org_id = claims.org_id.trim();
    if intent
        .requested_org_id
        .as_ref()
        .is_some_and(|requested| requested.trim() != org_id)
    {
        return Err(AuthError::Forbidden);
    }
    Ok(RebuildAuthorization::Org {
        org_id: org_id.to_string(),
        dry_run: intent.dry_run,
        clear: intent.clear,
    })
}

fn valid_admin_reason(value: &Option<String>) -> bool {
    valid_admin_metadata(value, 500)
        && value
            .as_deref()
            .is_some_and(|reason| reason.trim().len() >= 3)
}

pub fn authorize_approval(
    claims: &AdminClaims,
    job_org_id: Option<&str>,
    global: bool,
    break_glass: bool,
) -> Result<(), AuthError> {
    if !claims.has_scope(APPROVE_REBUILD_SCOPE) {
        return Err(AuthError::Forbidden);
    }
    if global {
        if !claims.has_scope(APPROVE_GLOBAL_REBUILD_SCOPE) || !break_glass {
            return Err(AuthError::Forbidden);
        }
    } else if job_org_id != Some(claims.org_id.as_str()) {
        return Err(AuthError::Forbidden);
    }
    Ok(())
}

fn valid_admin_metadata(value: &Option<String>, max_len: usize) -> bool {
    value.as_ref().is_some_and(|value| {
        let normalized = value.trim();
        !normalized.is_empty()
            && normalized.len() <= max_len
            && normalized.chars().all(|character| !character.is_control())
    })
}
