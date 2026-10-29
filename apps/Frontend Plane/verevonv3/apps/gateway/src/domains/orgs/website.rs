use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState, envelope::error, middleware::AuthenticatedUser,
    public_url::normalize_public_http_url, upstream::proxy_json,
};

use super::shared::{actor_for, require_active_org, GatewayJsonResponse};

/// Records the organisation's own website in org-core. The organisation is the session's active one, and
/// the address is checked in the gateway's form before it leaves: only a public web address is forwarded.
pub(super) async fn update_org_website(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    if let Err(response) = require_active_org(&user, &id) {
        return response;
    }
    let forwarded = match website_body(&body) {
        Ok(forwarded) => forwarded,
        Err(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error("invalid_url", message)),
            )
        }
    };
    let url = format!(
        "{}/api/v1/organizations/{}/website",
        state.org_core_url,
        urlencoding::encode(&id)
    );
    proxy_json(
        &state,
        Method::PATCH,
        &url,
        Some(forwarded),
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// The body org-core receives: the website in the gateway's form, and nothing else from the browser.
pub(super) fn website_body(body: &Value) -> Result<Value, String> {
    let raw = body
        .get("website_url")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let website = normalize_public_http_url(raw)?;
    Ok(json!({ "website_url": website }))
}

#[cfg(test)]
mod tests {
    use super::website_body;
    use serde_json::json;

    #[test]
    fn the_website_is_forwarded_in_the_gateway_form_and_nothing_else() {
        let forwarded = website_body(&json!({"website_url": " https://Firma.NO/om-oss ", "role": "owner"}))
            .expect("a public web address is accepted");
        assert_eq!(forwarded, json!({"website_url": "https://firma.no/om-oss"}));
    }

    #[test]
    fn a_private_or_non_web_address_is_refused_before_it_leaves() {
        assert!(website_body(&json!({"website_url": "http://127.0.0.1/"})).is_err());
        assert!(website_body(&json!({"website_url": "http://10.0.0.5"})).is_err());
        assert!(website_body(&json!({"website_url": "ftp://firma.no"})).is_err());
        assert!(website_body(&json!({"website_url": "https://user:pw@firma.no"})).is_err());
        assert!(website_body(&json!({})).is_err());
    }
}
