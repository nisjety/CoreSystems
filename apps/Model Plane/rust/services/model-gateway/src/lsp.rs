//! Wave 10c — LSP (language server protocol) bridge.
//!
//! Forwards `LspQuery` RPCs to an externally-running bridge addressed
//! by `LSP_BRIDGE_URL`. The bridge owns:
//!   - LSP server lifecycle per language (TypeScript / Go / Python / …)
//!   - File-to-language detection
//!   - LSP JSON-RPC wire protocol
//!
//! The gateway is intentionally just a typed pass-through. Why an
//! external bridge instead of porting v2's LSPManager:
//!   - LSP servers (tsserver, gopls, rust-analyzer) are stateful and
//!     long-lived. They need workspace-rooted setup, file-system
//!     watch, restart logic — all stuff already solved by tooling like
//!     `multilspy` or a thin Node bridge.
//!   - Putting that lifecycle inside the Rust gateway would bloat the
//!     hot path and break the gateway's "stateless front-door" mandate.
//!   - The bridge can stay in Python / TypeScript where the LSP
//!     client libs are mature.
//!
//! When `LSP_BRIDGE_URL` is unset the handler returns `Unimplemented`;
//! that's the correct response for "LSP support not wired in this
//! environment" — agents fall back to grep-and-read patterns.
//!
//! Wire format: bridge speaks JSON over POST `/lsp/query`.
//! Request:
//!   `{ "operation": "...", "file_path": "...", "line": 0, "column": 0 }`
//! Response:
//!   `{ "diagnostics": [...], "hover_text": "...", "locations": [...],
//!     "completions": [...], "error_message": "" }`
//! Bridge is expected to validate inputs; gateway just forwards.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tonic::Status;
use tracing::warn;

use mp_contracts::model_plane::v1::{
    LspCompletion, LspDiagnostic, LspLocation, LspQueryRequest, LspQueryResponse,
};

const ALLOWED_OPS: &[&str] = &["diagnostics", "hover", "definition", "completion"];
const DEFAULT_TIMEOUT_SECS: u64 = 15;

/// LSP bridge client. Cheap clone. Empty `base_url` makes the client
/// unavailable; the handler then returns Unimplemented.
#[derive(Clone, Debug)]
pub struct BridgeClient {
    base_url: String,
    http: reqwest::Client,
}

impl Default for BridgeClient {
    fn default() -> Self {
        Self::new("")
    }
}

impl BridgeClient {
    /// Build a client from a base URL. Pass empty string for "not
    /// configured" → the handler will return Unimplemented.
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http,
        }
    }

    /// Construct from `LSP_BRIDGE_URL` env var. Empty / unset → an
    /// unavailable client.
    #[must_use]
    pub fn from_env() -> Self {
        Self::new(std::env::var("LSP_BRIDGE_URL").unwrap_or_default())
    }

    #[must_use]
    pub fn available(&self) -> bool {
        !self.base_url.is_empty()
    }
}

// Wire types for the bridge JSON payload. Kept independent of the
// proto structs so the bridge contract can evolve without breaking
// the gRPC schema.
#[derive(Serialize)]
struct BridgeRequest<'a> {
    operation: &'a str,
    file_path: &'a str,
    line: i32,
    column: i32,
}

#[derive(Deserialize, Default)]
struct BridgeResponse {
    #[serde(default)]
    diagnostics: Vec<BridgeDiagnostic>,
    #[serde(default)]
    hover_text: String,
    #[serde(default)]
    locations: Vec<BridgeLocation>,
    #[serde(default)]
    completions: Vec<BridgeCompletion>,
    #[serde(default)]
    error_message: String,
}

#[derive(Deserialize, Default)]
struct BridgeDiagnostic {
    #[serde(default)]
    severity: String,
    #[serde(default)]
    line: i32,
    #[serde(default)]
    column: i32,
    #[serde(default)]
    end_line: i32,
    #[serde(default)]
    end_column: i32,
    #[serde(default)]
    message: String,
    #[serde(default)]
    code: String,
}

#[derive(Deserialize, Default)]
struct BridgeLocation {
    #[serde(default)]
    file_path: String,
    #[serde(default)]
    line: i32,
    #[serde(default)]
    column: i32,
}

#[derive(Deserialize, Default)]
struct BridgeCompletion {
    #[serde(default)]
    label: String,
    #[serde(default)]
    detail: String,
    #[serde(default)]
    kind: i32,
}

pub async fn handle_lsp_query(
    client: &BridgeClient,
    req: LspQueryRequest,
) -> Result<LspQueryResponse, Status> {
    if !client.available() {
        return Err(Status::unimplemented("LSP_BRIDGE_URL not configured"));
    }
    if !ALLOWED_OPS.contains(&req.operation.as_str()) {
        return Err(Status::invalid_argument(format!(
            "operation must be one of {:?}, got {:?}",
            ALLOWED_OPS, req.operation
        )));
    }
    if req.file_path.trim().is_empty() {
        return Err(Status::invalid_argument("file_path is required"));
    }

    let endpoint = format!("{}/lsp/query", client.base_url);
    let body = BridgeRequest {
        operation: &req.operation,
        file_path: &req.file_path,
        line: req.line,
        column: req.column,
    };

    let resp = client
        .http
        .post(&endpoint)
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            warn!(error = %e, endpoint = %endpoint, "lsp bridge request failed");
            Status::unavailable(format!("lsp bridge: {e}"))
        })?;

    let status = resp.status();
    if !status.is_success() {
        // Distinguish "this file/language isn't supported by the
        // bridge" (404) from "the bridge itself is broken" (5xx).
        let body = resp.text().await.unwrap_or_default();
        if status.as_u16() == 404 {
            return Ok(LspQueryResponse {
                request_id: req.request_id,
                error_message: format!("not supported by bridge: {}", req.file_path),
                ..Default::default()
            });
        }
        return Err(Status::unavailable(format!(
            "lsp bridge HTTP {}: {}",
            status,
            truncate(&body, 200)
        )));
    }

    let parsed: BridgeResponse = resp
        .json()
        .await
        .map_err(|e| Status::internal(format!("lsp bridge decode: {e}")))?;

    Ok(LspQueryResponse {
        request_id: req.request_id,
        diagnostics: parsed
            .diagnostics
            .into_iter()
            .map(|d| LspDiagnostic {
                severity: d.severity,
                line: d.line,
                column: d.column,
                end_line: d.end_line,
                end_column: d.end_column,
                message: d.message,
                code: d.code,
            })
            .collect(),
        hover_text: parsed.hover_text,
        locations: parsed
            .locations
            .into_iter()
            .map(|l| LspLocation {
                file_path: l.file_path,
                line: l.line,
                column: l.column,
            })
            .collect(),
        completions: parsed
            .completions
            .into_iter()
            // The bridge caps at 20 but be defensive — a misbehaving
            // bridge shouldn't blow our response size.
            .take(20)
            .map(|c| LspCompletion {
                label: c.label,
                detail: c.detail,
                kind: c.kind,
            })
            .collect(),
        error_message: parsed.error_message,
    })
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut end = n;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_base_url_is_unavailable() {
        let c = BridgeClient::new("");
        assert!(!c.available());
    }

    #[tokio::test]
    async fn unavailable_returns_unimplemented() {
        let c = BridgeClient::new("");
        let r = handle_lsp_query(
            &c,
            LspQueryRequest {
                request_id: "t".into(),
                org_id: "".into(),
                operation: "hover".into(),
                file_path: "/x".into(),
                line: 0,
                column: 0,
            },
        )
        .await
        .expect_err("must be unimplemented");
        assert_eq!(r.code(), tonic::Code::Unimplemented);
    }

    #[tokio::test]
    async fn rejects_unknown_operation() {
        let c = BridgeClient::new("http://stub");
        let r = handle_lsp_query(
            &c,
            LspQueryRequest {
                request_id: "t".into(),
                org_id: "".into(),
                operation: "rename".into(),
                file_path: "/x".into(),
                line: 0,
                column: 0,
            },
        )
        .await
        .expect_err("must reject");
        assert_eq!(r.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn rejects_empty_file_path() {
        let c = BridgeClient::new("http://stub");
        let r = handle_lsp_query(
            &c,
            LspQueryRequest {
                request_id: "t".into(),
                org_id: "".into(),
                operation: "diagnostics".into(),
                file_path: "   ".into(),
                line: 0,
                column: 0,
            },
        )
        .await
        .expect_err("must reject");
        assert_eq!(r.code(), tonic::Code::InvalidArgument);
    }
}
