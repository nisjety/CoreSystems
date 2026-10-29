use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizeRequest {
    pub provider_key: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body_base64: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedWebhook {
    pub schema_version: u16,
    pub provider_key: String,
    pub event_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub organization_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub signature_hash: String,
    pub event_id: String,
    pub replay_key: String,
    pub body_sha256: String,
    pub payload: Map<String, Value>,
    pub normalized_by: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Error)]
pub enum NormalizeError {
    #[error("bodyBase64 is invalid: {0}")]
    InvalidBase64(String),
    #[error("webhook payload must be a JSON object: {0}")]
    InvalidJson(String),
}

pub fn normalize(request: NormalizeRequest) -> Result<NormalizedWebhook, NormalizeError> {
    let provider_key = normalize_provider_key(&request.provider_key);
    let body = if request.body_base64.trim().is_empty() {
        Vec::new()
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(request.body_base64.trim())
            .map_err(|err| NormalizeError::InvalidBase64(err.to_string()))?
    };

    let payload = if body.is_empty() {
        Map::new()
    } else {
        serde_json::from_slice::<Map<String, Value>>(&body)
            .map_err(|err| NormalizeError::InvalidJson(err.to_string()))?
    };

    let body_sha256 = sha256_hex(&body);
    let signature_hash = hash_first_header(
        &request.headers,
        &[
            "Stripe-Signature",
            "X-Slack-Signature",
            "X-Hub-Signature-256",
            "X-GitHub-Delivery",
            "X-Shopify-Hmac-Sha256",
            "X-Webhook-Signature",
        ],
    );
    let event_type = first_non_empty(&[
        header(&request.headers, "X-Event-Type"),
        header(&request.headers, "X-GitHub-Event"),
        header(&request.headers, "X-Shopify-Topic"),
        string_from_value(payload.get("type")),
        string_from_value(payload.get("event")),
        "provider.webhook".to_string(),
    ]);
    let organization_id = first_non_empty(&[
        header(&request.headers, "X-Org-ID"),
        string_from_value(payload.get("organizationId")),
        string_from_value(payload.get("organization_id")),
    ]);
    let replay_key = first_non_empty(&[
        string_from_value(payload.get("id")),
        string_from_value(payload.get("eventId")),
        string_from_value(payload.get("event_id")),
        string_from_value(payload.get("deliveryId")),
        string_from_value(payload.get("delivery_id")),
        header(&request.headers, "X-GitHub-Delivery"),
        signature_hash.clone(),
        body_sha256.clone(),
    ]);
    let event_id = webhook_event_id(&provider_key, &event_type, &replay_key);

    Ok(NormalizedWebhook {
        schema_version: 1,
        provider_key,
        event_type,
        organization_id,
        signature_hash,
        event_id,
        replay_key,
        body_sha256,
        payload,
        normalized_by: "rust-webhook-normalizer".to_string(),
        warnings: Vec::new(),
    })
}

fn normalize_provider_key(input: &str) -> String {
    input.trim().to_lowercase().replace('_', "-")
}

fn header(headers: &BTreeMap<String, String>, name: &str) -> String {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

fn hash_first_header(headers: &BTreeMap<String, String>, names: &[&str]) -> String {
    names
        .iter()
        .map(|name| header(headers, name))
        .find(|value| !value.is_empty())
        .map(|value| sha256_hex(value.as_bytes()))
        .unwrap_or_default()
}

fn first_non_empty(values: &[String]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn string_from_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.trim().to_string(),
        _ => String::new(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn webhook_event_id(provider_key: &str, event_type: &str, replay_key: &str) -> String {
    let input = format!("{provider_key}|{event_type}|{replay_key}");
    let digest = sha256_hex(input.as_bytes());
    format!("wh_{provider_key}_{}", &digest[..32])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    fn encoded(body: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(body.as_bytes())
    }

    #[test]
    fn normalizes_stripe_event_with_stable_id() {
        let body = r#"{"id":"evt_1","type":"account.updated","organizationId":"org-1"}"#;
        let request = NormalizeRequest {
            provider_key: "Stripe".to_string(),
            headers: BTreeMap::from([(
                "Stripe-Signature".to_string(),
                "t=1,v1=signature".to_string(),
            )]),
            body_base64: encoded(body),
        };

        let normalized = normalize(request).expect("normalize");
        assert_eq!(normalized.provider_key, "stripe");
        assert_eq!(normalized.event_type, "account.updated");
        assert_eq!(normalized.organization_id, "org-1");
        assert_eq!(normalized.replay_key, "evt_1");
        assert!(normalized.event_id.starts_with("wh_stripe_"));
        assert_eq!(normalized.schema_version, 1);
        assert_eq!(normalized.normalized_by, "rust-webhook-normalizer");
    }

    #[test]
    fn uses_provider_headers_for_github() {
        let request = NormalizeRequest {
            provider_key: "github".to_string(),
            headers: BTreeMap::from([
                ("x-github-delivery".to_string(), "delivery-1".to_string()),
                ("X-GitHub-Event".to_string(), "push".to_string()),
                ("X-Org-ID".to_string(), "org-1".to_string()),
            ]),
            body_base64: encoded(r#"{"repository":{"name":"demo"}}"#),
        };

        let normalized = normalize(request).expect("normalize");
        assert_eq!(normalized.event_type, "push");
        assert_eq!(normalized.organization_id, "org-1");
        assert_eq!(normalized.replay_key, "delivery-1");
    }

    #[test]
    fn rejects_non_object_payloads() {
        let request = NormalizeRequest {
            provider_key: "stripe".to_string(),
            headers: BTreeMap::new(),
            body_base64: encoded("[1,2,3]"),
        };

        assert!(matches!(
            normalize(request),
            Err(NormalizeError::InvalidJson(_))
        ));
    }

    #[test]
    fn matches_shared_fixtures() {
        let fixtures: Vec<WebhookFixture> =
            serde_json::from_str(include_str!("../testdata/webhook_fixtures.json"))
                .expect("fixtures");

        for fixture in fixtures {
            let normalized = normalize(NormalizeRequest {
                provider_key: fixture.provider_key,
                headers: fixture.headers,
                body_base64: encoded(&fixture.body),
            })
            .expect("normalize");

            assert_eq!(
                normalized.schema_version, fixture.expected.schema_version,
                "{} schema version",
                fixture.name
            );
            assert_eq!(
                normalized.provider_key, fixture.expected.provider_key,
                "{} provider",
                fixture.name
            );
            assert_eq!(
                normalized.event_type, fixture.expected.event_type,
                "{} event type",
                fixture.name
            );
            assert_eq!(
                normalized.organization_id, fixture.expected.organization_id,
                "{} org",
                fixture.name
            );
            assert_eq!(
                normalized.replay_key, fixture.expected.replay_key,
                "{} replay key",
                fixture.name
            );
            assert_eq!(
                normalized.event_id, fixture.expected.event_id,
                "{} event id",
                fixture.name
            );
        }
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WebhookFixture {
        name: String,
        provider_key: String,
        headers: BTreeMap<String, String>,
        body: String,
        expected: WebhookFixtureExpected,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WebhookFixtureExpected {
        schema_version: u16,
        provider_key: String,
        event_type: String,
        organization_id: String,
        replay_key: String,
        event_id: String,
    }
}
