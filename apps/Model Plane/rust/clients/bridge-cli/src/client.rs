//! Thin HTTP client for bridge-core's session API (matrix P6). The CLI is the
//! operator-facing shell; bridge-core (Go) owns sessions/channels/voice. This
//! consumes its `/api/v1/sessions` surface and adds NO backend — no duplication.

use anyhow::{bail, Context, Result};
use base64::prelude::*;

/// Client for bridge-core's session API, bound to one org/user identity.
pub struct BridgeClient {
    base_url: String,
    org_id: String,
    user_id: String,
    http: reqwest::Client,
}

impl BridgeClient {
    pub fn new(
        base_url: impl Into<String>,
        org_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            org_id: org_id.into(),
            user_id: user_id.into(),
            http: reqwest::Client::new(),
        }
    }

    fn sessions_url(&self) -> String {
        format!("{}/api/v1/sessions", self.base_url)
    }
    fn session_url(&self, id: &str) -> String {
        format!("{}/api/v1/sessions/{id}", self.base_url)
    }
    fn ingest_url(&self, id: &str) -> String {
        format!("{}/api/v1/sessions/{id}/ingest", self.base_url)
    }

    /// Register a session (`POST /api/v1/sessions`).
    pub async fn new_session(&self, channel: &str) -> Result<String> {
        let body = serde_json::json!({
            "org_id": self.org_id,
            "user_id": self.user_id,
            "channel": channel,
        });
        self.send(self.http.post(self.sessions_url()).json(&body))
            .await
    }

    /// List sessions (`GET /api/v1/sessions`).
    pub async fn list_sessions(&self) -> Result<String> {
        self.send(self.http.get(self.sessions_url())).await
    }

    /// Show one session (`GET /api/v1/sessions/{id}`).
    pub async fn get_session(&self, id: &str) -> Result<String> {
        self.send(self.http.get(self.session_url(id))).await
    }

    /// Send input to a session (`POST /api/v1/sessions/{id}/ingest`).
    /// bridge-core's ingest body is `{"payload": <bytes>}`; Go marshals/decodes
    /// `[]byte` as a base64 string, so the operator text is base64-encoded here.
    pub async fn ingest(&self, id: &str, text: &str) -> Result<String> {
        let body = serde_json::json!({ "payload": BASE64_STANDARD.encode(text.as_bytes()) });
        self.send(self.http.post(self.ingest_url(id)).json(&body))
            .await
    }

    /// Close a session (`DELETE /api/v1/sessions/{id}`).
    pub async fn close_session(&self, id: &str) -> Result<String> {
        self.send(self.http.delete(self.session_url(id))).await
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<String> {
        let resp = req.send().await.context("bridge-core request failed")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("bridge-core returned {status}: {text}");
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_canonical_urls() {
        let c = BridgeClient::new("http://localhost:8091", "o", "u");
        assert_eq!(c.sessions_url(), "http://localhost:8091/api/v1/sessions");
        assert_eq!(
            c.session_url("s-1"),
            "http://localhost:8091/api/v1/sessions/s-1"
        );
        assert_eq!(
            c.ingest_url("s-1"),
            "http://localhost:8091/api/v1/sessions/s-1/ingest"
        );
    }

    #[test]
    fn ingest_payload_matches_go_byte_base64() {
        // Go json marshals []byte as standard base64; "hi" -> "aGk=". This is
        // what bridge-core's ingestRequest.Payload []byte expects to decode.
        assert_eq!(BASE64_STANDARD.encode(b"hi"), "aGk=");
    }
}
