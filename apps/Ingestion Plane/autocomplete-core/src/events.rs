use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct QuarryEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestCommand {
    QueryIssued {
        org_id: String,
        user_id: Option<String>,
        query: String,
        provider: Option<String>,
        result_count: Option<i64>,
    },
    HostDiscovered {
        org_id: String,
        user_id: Option<String>,
        host: String,
        seed_url: Option<String>,
    },
    Unsupported,
}

impl QuarryEvent {
    pub fn into_command(self) -> IngestCommand {
        match self.event_type.as_str() {
            "search_issued" => search_issued_command(self.payload),
            "host_discovered" => host_discovered_command(self.payload),
            _ => IngestCommand::Unsupported,
        }
    }
}

fn search_issued_command(payload: Value) -> IngestCommand {
    let org_id = payload
        .get("org_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let query = payload
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    if org_id.is_empty() || query.is_empty() {
        return IngestCommand::Unsupported;
    }

    IngestCommand::QueryIssued {
        org_id,
        query,
        user_id: payload
            .get("user_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        provider: payload
            .get("provider")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        result_count: payload.get("result_count").and_then(Value::as_i64),
    }
}

fn host_discovered_command(payload: Value) -> IngestCommand {
    let org_id = payload
        .get("org_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let host = payload
        .get("host")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    if org_id.is_empty() || host.is_empty() {
        return IngestCommand::Unsupported;
    }

    IngestCommand::HostDiscovered {
        org_id,
        host,
        user_id: payload
            .get("user_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        seed_url: payload
            .get("seed_url")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_search_issued_payload() {
        let event = QuarryEvent {
            event_type: "search_issued".to_string(),
            idempotency_key: "search:1".to_string(),
            payload: json!({
                "org_id": "org_a",
                "user_id": "user_a",
                "query": "Find me a spa",
                "provider": "brave",
                "result_count": 3
            }),
        };

        assert_eq!(
            event.into_command(),
            IngestCommand::QueryIssued {
                org_id: "org_a".to_string(),
                user_id: Some("user_a".to_string()),
                query: "Find me a spa".to_string(),
                provider: Some("brave".to_string()),
                result_count: Some(3),
            }
        );
    }
}
