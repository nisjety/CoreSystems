//! Reading the claims of a Visma Connect token. The token comes straight from the token endpoint over TLS,
//! so its signature is not checked here: the claims are used to label the sign-in (who, which company,
//! which scopes, until when). They are never used to decide what a person may do; Visma decides that when
//! the token is used.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::Value;

use crate::error::{Result, SignInError};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Claims {
    pub sub: Option<String>,
    pub tenant_id: Option<String>,
    pub scopes: Vec<String>,
    pub exp: Option<i64>,
    pub name: Option<String>,
    pub email: Option<String>,
}

/// The claims of a JWT, without verifying it.
pub fn claims(jwt: &str) -> Result<Claims> {
    let payload = jwt
        .split('.')
        .nth(1)
        .filter(|part| !part.is_empty())
        .ok_or_else(|| SignInError::BadToken("not a JWT".into()))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|_| SignInError::BadToken("the payload is not base64url".into()))?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| SignInError::BadToken("the payload is not JSON".into()))?;
    let text = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_string);
    let scopes = match value.get("scope") {
        Some(Value::String(s)) => s.split_whitespace().map(str::to_string).collect(),
        Some(Value::Array(list)) => list
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    };
    Ok(Claims {
        sub: text("sub"),
        tenant_id: text("tenant_id"),
        scopes,
        exp: value.get("exp").and_then(Value::as_i64),
        name: text("name"),
        email: text("email"),
    })
}

#[cfg(test)]
pub(crate) fn test_jwt(payload: &Value) -> String {
    format!(
        "{}.{}.signature",
        URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
        URL_SAFE_NO_PAD.encode(payload.to_string())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_claims_that_label_a_sign_in_are_read() {
        let jwt = test_jwt(&json!({
            "sub": "user-1", "tenant_id": "t-1", "scope": ["a:read", "b:write"], "exp": 1_900_000_000,
            "name": "Ima", "email": "ima@example.com"
        }));
        let c = claims(&jwt).unwrap();
        assert_eq!(c.sub.as_deref(), Some("user-1"));
        assert_eq!(c.tenant_id.as_deref(), Some("t-1"));
        assert_eq!(c.scopes, vec!["a:read", "b:write"]);
        assert_eq!(c.exp, Some(1_900_000_000));
        assert_eq!(
            (c.name.as_deref(), c.email.as_deref()),
            (Some("Ima"), Some("ima@example.com"))
        );
    }

    #[test]
    fn the_scope_may_be_one_space_separated_string() {
        let c = claims(&test_jwt(
            &json!({"scope": "openid  offline_access x:read"}),
        ))
        .unwrap();
        assert_eq!(c.scopes, vec!["openid", "offline_access", "x:read"]);
    }

    #[test]
    fn missing_claims_are_none_not_errors() {
        assert_eq!(claims(&test_jwt(&json!({}))).unwrap(), Claims::default());
    }

    #[test]
    fn what_is_not_a_jwt_is_refused_without_echoing_it() {
        for bad in ["", "abc", "a..c", "a.!!!.c", "a.bm90LWpzb24.c"] {
            let e = claims(bad).unwrap_err().to_string();
            assert!(e.contains("could not be read"), "{bad}: {e}");
            assert!(!e.contains("bm90LWpzb24"), "{e}");
        }
    }
}
