//! Provider-action tools for the agentic loop — the single typed bridge from
//! the agent runtime to every OAuth-connected provider operation.
//!
//! Backed by `integration-corev2` (Ingestion Plane), which owns the connected
//! accounts, their stored access tokens, and the per-provider action executor.
//! Rather than teach the agent runtime each provider's API, exec-core calls
//! integration-corev2's generic actions gateway: one request shape
//! (`{operation, params, body}`) reaches Meta pages/ads/WhatsApp/Messenger/
//! catalog/Threads, LinkedIn (incl. ads + lead forms + conversions), Google,
//! Microsoft, Slack, GitHub, Notion, Shopify, Stripe, and Okta. Provider
//! tokens never leave integration-corev2's OAuth broker.
//!
//! Two tools ride on this client:
//!   * `list_provider_actions` — read-only discovery: the org's live
//!     connections (from GET /api/v1/connections) crossed with the static
//!     operations catalog below. Not in `permission::is_risky_tool`, so it
//!     runs under `ask` posture without a gate.
//!   * `execute_provider_action` — runs one operation. Under `ask` posture
//!     `permission::is_risky_call` classifies it by its `operation`: a
//!     catalogued read runs without a gate, while a write or an uncatalogued
//!     operation pauses for explicit human approval first (same spine as
//!     `book_shipment`). The approval reference is then forwarded as
//!     `approvalId` in the body to satisfy integration-corev2's own
//!     write-approval check.
//!
//! Transport (Auth Core `ingestion` audience service token):
//!   GET  {INTEGRATION_COREV2_URL}/api/v1/connections?organizationId=<org>
//!   POST {INTEGRATION_COREV2_URL}/api/v1/connections/{id}/actions
//!
//! The `{success, data:{action:{providerKey,operation,result}}}` envelope and
//! provider result payloads are parsed defensively as `serde_json::Value` —
//! the wire DTOs live in a Go service and grow over time. The authoritative
//! list of operations is docs/actions-surface-operations.md; the catalog here
//! is a curated, agent-facing subset kept deliberately small.

#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use mp_contracts::provider_reads::READ_OPERATIONS;
use serde_json::Value;

use crate::attestation;

/// Default integration-corev2 address. execution-core sits on
/// model-plane-network only, so cross-plane services are dialled via the
/// host-published port (same pattern as `INFORMATION_CORE_URL`/
/// `SHIPPING_CORE_URL`). Host port 3026 → container 3026.
const DEFAULT_INTEGRATION_URL: &str = "http://host.docker.internal:3026";
const DEFAULT_AUTH_CORE_URL: &str = "http://host.docker.internal:3011";

/// Cap on the result JSON echoed back to the model — provider responses can be
/// large; the head is enough for the model to reason about, and the full
/// payload is not something it should paste verbatim.
const MAX_RESULT_CHARS: usize = 2000;

/// One entry in the static, agent-facing operations catalog. `is_write` mirrors
/// integration-corev2's approval requirement (writes are HITL-gated through the
/// gateway); it is surfaced to the model so it knows which operations pause for
/// approval before proposing them.
struct CatalogOp {
    operation: &'static str,
    is_write: bool,
    summary: &'static str,
}

/// Provider → operations the agent may call, a curated subset of
/// integration-corev2's Execute switch (see docs/actions-surface-operations.md
/// for the exhaustive list). Kept small on purpose: too many near-duplicate
/// tool names degrade model tool-selection. Operation strings here MUST match
/// the gateway's case arms exactly — they are sent verbatim.
const CATALOG: &[(&str, &[CatalogOp])] = &[
    (
        "meta",
        &[
            CatalogOp { operation: "pages.list", is_write: false, summary: "List the connected Facebook Pages (+ linked Instagram accounts)." },
            CatalogOp { operation: "pages.post", is_write: true, summary: "Publish a post to a Facebook Page feed. params.pageId, body.message." },
            CatalogOp { operation: "instagram.media.create", is_write: true, summary: "Create an Instagram media container. params.igUserId, body.image_url|video_url, caption." },
            CatalogOp { operation: "instagram.media.publish", is_write: true, summary: "Publish a prepared Instagram media container. params.igUserId, body.creation_id." },
            CatalogOp { operation: "whatsapp.messages.send", is_write: true, summary: "Send a WhatsApp Cloud API message. params.phoneNumberId, body.{to,type,text}." },
            CatalogOp { operation: "messenger.messages.send", is_write: true, summary: "Send a Messenger message. params.pageId, body.{recipient,message}." },
            CatalogOp { operation: "ads.adaccounts", is_write: false, summary: "List the connected Meta ad accounts." },
            CatalogOp { operation: "ads.campaigns", is_write: false, summary: "List campaigns for an ad account. params.adAccountId." },
            CatalogOp { operation: "ads.insights", is_write: false, summary: "Read ad performance metrics. params.adAccountId." },
            CatalogOp { operation: "catalogs.list", is_write: false, summary: "List commerce catalogs. params.businessId." },
            CatalogOp { operation: "threads.profile", is_write: false, summary: "Read the connected Threads profile." },
        ],
    ),
    (
        "linkedin",
        &[
            CatalogOp { operation: "posts.create", is_write: true, summary: "Publish a LinkedIn post. body.commentary + author." },
            CatalogOp { operation: "ads.campaigns", is_write: false, summary: "List LinkedIn ad campaigns." },
            CatalogOp { operation: "lead.forms", is_write: false, summary: "List LinkedIn lead-gen forms. params.owner (URN)." },
            CatalogOp { operation: "lead.responses", is_write: false, summary: "Read responses for a lead form. params.leadForm (URN)." },
            CatalogOp { operation: "conversions.list", is_write: false, summary: "List Conversions API rules for an ad account." },
        ],
    ),
    (
        "google",
        &[
            CatalogOp { operation: "gmail.send", is_write: true, summary: "Send an email via Gmail. body.{to,subject,bodyText}." },
            CatalogOp { operation: "gmail.messages", is_write: false, summary: "Read-only: list Gmail messages (ids and threadIds; with headers=true also subject, from, to, cc, date, snippet, labelIds). params.query (Gmail search syntax), from, since and until (YYYY-MM-DD), folder (inbox | sent | drafts | archive | deleted), unread, headers, maxResults (default 10, max 100), pageToken." },
            CatalogOp { operation: "gmail.messages.get", is_write: false, summary: "Read-only: read one Gmail message (headers and body text). params.id, the message id from gmail.messages." },
            CatalogOp { operation: "calendar.events", is_write: false, summary: "Read-only: list Google Calendar events. params.since and until (YYYY-MM-DD, or a local date-time in Europe/Oslo), query, maxResults (default 10, max 100), pageToken." },
            CatalogOp { operation: "drive.files", is_write: false, summary: "Read-only: list Google Drive files. params.query, match (name | text, default name), folderId, driveId (one shared drive), maxResults (default 10, max 100), pageToken." },
            CatalogOp { operation: "drive.file.content", is_write: false, summary: "Read-only: read one Drive file as base64. Docs, Sheets and Slides export as text with exportAs=text. params.fileId (from drive.files), mimeType, exportAs." },
            CatalogOp { operation: "profile", is_write: false, summary: "Read-only: the connected Google account profile (OpenID userinfo)." },
        ],
    ),
    (
        "microsoft",
        &[
            CatalogOp { operation: "mail.send", is_write: true, summary: "Send an Outlook email. body.{to,subject,bodyText}." },
            CatalogOp { operation: "mail.messages", is_write: false, summary: "Read-only: list recent Outlook messages (id, subject, from, receivedDateTime, webLink, bodyPreview). params.query, from, since and until (YYYY-MM-DD), folder (inbox | sent | drafts | archive | deleted), unread, maxResults (default 10, max 100), pageToken." },
            CatalogOp { operation: "mail.messages.get", is_write: false, summary: "Read-only: read one Outlook message (headers, body and attachment metadata). params.messageId, the id from mail.messages." },
            CatalogOp { operation: "drive.files", is_write: false, summary: "Read-only: list files in the connected OneDrive, or in one SharePoint library when driveId is given. params.query, folderId, driveId, maxResults (default 10, max 100), pageToken." },
            CatalogOp { operation: "drive.file.content", is_write: false, summary: "Read-only: read one OneDrive or SharePoint file as base64. params.itemId (from drive.files), driveId." },
            CatalogOp { operation: "sharepoint.sites", is_write: false, summary: "Read-only: list the SharePoint sites the user can see. params.query (default all sites), pageToken." },
            CatalogOp { operation: "sharepoint.drives", is_write: false, summary: "Read-only: list the document libraries of one SharePoint site. params.siteId (from sharepoint.sites), pageToken." },
            CatalogOp { operation: "profile", is_write: false, summary: "Read-only: the connected Microsoft account profile (id, displayName, mail)." },
            CatalogOp { operation: "calendar.events", is_write: false, summary: "Read-only: list Outlook calendar events. params.since and until (YYYY-MM-DD, or a local date-time in Europe/Oslo), query, maxResults (default 10, max 100), pageToken." },
        ],
    ),
    (
        "slack",
        &[
            CatalogOp { operation: "message.send", is_write: true, summary: "Post a Slack message. params.channel, body.text." },
            // Real executor op is `channels.list` (GET /conversations.list) — the
            // bare `channels` string matched no case arm and always failed.
            CatalogOp { operation: "channels.list", is_write: false, summary: "List Slack channels." },
        ],
    ),
    (
        "github",
        &[
            CatalogOp { operation: "issues.list", is_write: false, summary: "List repository issues. params.owner, params.repo." },
            CatalogOp { operation: "issues.create", is_write: true, summary: "Open a GitHub issue. params.owner, params.repo, body.{title,body}." },
        ],
    ),
    (
        "notion",
        // The real integration-corev2 executor implements user/databases/pages —
        // there is NO `search` op, so the previous entry was non-functional.
        // Expose the real read operations instead (docs/actions-surface-operations.md).
        &[
            CatalogOp { operation: "databases", is_write: false, summary: "List the Notion databases shared with the connection." },
            CatalogOp { operation: "pages", is_write: false, summary: "List pages; with params.databaseId, query that database's rows." },
            CatalogOp { operation: "user", is_write: false, summary: "Read the connected Notion bot user (workspace identity)." },
        ],
    ),
    (
        "shopify",
        &[
            CatalogOp { operation: "products", is_write: false, summary: "List Shopify products." },
            CatalogOp { operation: "orders", is_write: false, summary: "List Shopify orders." },
        ],
    ),
];

/// HTTP client for integration-corev2's actions gateway. Cheap to clone.
#[derive(Clone)]
pub struct IntegrationActionsClient {
    base_url: String,
    auth_core_url: String,
    service_id: String,
    service_credential: String,
    http: reqwest::Client,
    /// Signs the provider-write attestation integration-corev2 requires for
    /// write operations (see the module doc's `execute_action`). `None` when
    /// `EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY`/`_KEY_ID` are
    /// unset — a write then fails closed with an honest tool error instead of
    /// reaching integration-corev2 unattested.
    write_attestor: Option<Arc<attestation::Signer>>,
}

#[derive(serde::Deserialize)]
struct PlaneTokenResponse {
    token: String,
}

/// Result of a successful `execute_action` call: text for the model, plus
/// (when extractable) the provider's own receipt — the only thing that may
/// ever be forwarded as an authoritative `provider_receipt_id` to a durable
/// approval-continuation outcome (see `approval_delivery_worker`). A `None`
/// here means the response genuinely carried no id-like field, not that the
/// action failed.
pub struct ActionOutcome {
    pub rendered: String,
    pub provider_receipt_id: Option<String>,
}

/// A connection as surfaced to the model — the fields relevant to picking one
/// for an action, not the full stored record.
struct ConnectionSummary {
    id: String,
    provider_key: String,
    display_name: String,
    status: String,
}

impl IntegrationActionsClient {
    /// Build from the environment: `INTEGRATION_COREV2_URL` overrides the
    /// host-published compose default; `INGESTION_SERVICE_API_KEY` is sent as
    /// the `x-service-api-key` header with `INGESTION_SERVICE_ID` (default
    /// `model-execution`) as `x-service-id`. Returns `None` when the key is
    /// unset or the reqwest client cannot be built, so the caller has no
    /// provider-action client at all.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("INTEGRATION_COREV2_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_INTEGRATION_URL.to_owned());
        let auth_core_url = std::env::var("AUTH_CORE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_AUTH_CORE_URL.to_owned());
        let service_id = std::env::var("INGESTION_SERVICE_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "model-execution".to_owned());
        let service_credential = std::env::var("INGESTION_SERVICE_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(25))
            .build()
            .ok()?;
        Some(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id,
            service_credential,
            http,
            write_attestor: attestation::Signer::from_env().map(Arc::new),
        })
    }

    /// A client pointed at fake Auth Core and integration-corev2 servers, for
    /// tests of code that takes a client (the in-loop write verification).
    /// `write_attestor` is `Some` when the test runs a write: without one the
    /// client refuses the write exactly as a production client does.
    #[cfg(test)]
    pub(crate) fn new_for_test(
        auth_core_url: &str,
        integration_url: &str,
        write_attestor: Option<Arc<attestation::Signer>>,
    ) -> Self {
        Self {
            base_url: integration_url.trim_end_matches('/').to_owned(),
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id: "model-execution".to_owned(),
            service_credential: "test-only-execution-service-key".to_owned(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("build test http client"),
            write_attestor,
        }
    }

    /// GET /api/v1/connections?organizationId=<org> — the org's live provider
    /// connections. Rendered together with the static catalog so the model can
    /// see what is connected and what it can call on each.
    pub async fn list_provider_actions(
        &self,
        org_id: &str,
        session_bearer: Option<&str>,
    ) -> Result<String, String> {
        if org_id.trim().is_empty() {
            return Err("list_provider_actions requires a run org_id (tenant scope)".to_owned());
        }
        let token = self
            .resolve_ingestion_token(org_id, "integration:read", session_bearer)
            .await?;
        let resp = self
            .http
            .get(format!("{}/api/v1/connections", self.base_url))
            .query(&[("organizationId", org_id)])
            .bearer_auth(token)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("integration-corev2 /connections request failed: {e}"))?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .map_err(|e| format!("integration-corev2 /connections decode failed: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "integration-corev2 /connections returned {status}: {}",
                truncate(&value.to_string())
            ));
        }
        Ok(render_provider_actions(&parse_connections(&value)))
    }

    /// GET /api/v1/connections/{id} — the connection's own `providerKey`,
    /// needed to sign a provider-write attestation (see `execute_action`):
    /// the attestation binds `provider_key`, and only integration-corev2's
    /// own connection record is authoritative for it — the model only ever
    /// supplies `connection_id`.
    async fn provider_key_for_connection(
        &self,
        org_id: &str,
        connection_id: &str,
        session_bearer: Option<&str>,
    ) -> Result<String, String> {
        let token = self
            .resolve_ingestion_token(org_id, "integration:read", session_bearer)
            .await?;
        let resp = self
            .http
            .get(format!(
                "{}/api/v1/connections/{}",
                self.base_url, connection_id
            ))
            .bearer_auth(token)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("integration-corev2 connection lookup failed: {e}"))?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .map_err(|e| format!("integration-corev2 connection lookup decode failed: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "integration-corev2 connection lookup returned {status}: {}",
                truncate(&value.to_string())
            ));
        }
        value
            .pointer("/data/connection/providerKey")
            .and_then(Value::as_str)
            .filter(|key| !key.trim().is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                "integration-corev2 connection lookup did not return a providerKey".to_owned()
            })
    }

    /// POST /api/v1/connections/{id}/actions with `{operation, params, body}`.
    /// `approval_ref`, when present (post-HITL-approval), is injected into the
    /// body as `approvalId` so integration-corev2's write-approval check
    /// (`requireActionCapability`) passes for write operations, and the
    /// request is additionally signed as a provider-write attestation
    /// (`writeAttestation` + `idempotencyKey` fields) — integration-corev2
    /// rejects a write with neither. `actor_id` is the human whose approval
    /// authorized this specific action; it is embedded in the attestation,
    /// never trusted from the model's tool-call input.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub async fn execute_action(
        &self,
        org_id: &str,
        connection_id: &str,
        operation: &str,
        params: Value,
        mut body: Value,
        actor_id: &str,
        approval_ref: Option<&str>,
        session_bearer: Option<&str>,
    ) -> Result<ActionOutcome, String> {
        if connection_id.trim().is_empty() {
            return Err("execute_provider_action requires connection_id".to_owned());
        }
        if operation.trim().is_empty() {
            return Err("execute_provider_action requires operation".to_owned());
        }
        if let Some(approval) = approval_ref {
            if !approval.trim().is_empty() {
                if !body.is_object() {
                    body = serde_json::json!({});
                }
                if let Some(map) = body.as_object_mut() {
                    map.insert("approvalId".to_owned(), Value::String(approval.to_owned()));
                }
            }
        }
        let mut request = serde_json::json!({
            "operation": operation,
            "params": params,
            "body": body,
        });
        let approval = approval_ref
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(approval) = approval {
            let provider_key = self
                .provider_key_for_connection(org_id, connection_id, session_bearer)
                .await?;
            let payload_sha256 = attestation::payload_sha256(
                org_id,
                connection_id,
                &provider_key,
                operation,
                &request["params"],
                &request["body"],
            )?;
            let attestor = self.write_attestor.as_ref().ok_or_else(|| {
                "execute_provider_action unavailable: provider-write attestation signer is not configured".to_owned()
            })?;
            let write_attestation = attestor.sign(attestation::Authorization {
                authorization_kind: attestation::AUTHORIZATION_HUMAN_APPROVED_AI_ACTION.to_owned(),
                authorization_id: approval.to_owned(),
                approval_id: approval.to_owned(),
                action_id: approval.to_owned(),
                org_id: org_id.to_owned(),
                connection_id: connection_id.to_owned(),
                provider_key,
                operation: operation.to_owned(),
                actor_id: actor_id.to_owned(),
                payload_sha256,
                idempotency_key: approval.to_owned(),
            })?;
            if let Some(map) = request.as_object_mut() {
                map.insert(
                    "idempotencyKey".to_owned(),
                    Value::String(approval.to_owned()),
                );
                map.insert(
                    "writeAttestation".to_owned(),
                    Value::String(write_attestation),
                );
            }
        }
        let scope = if approval.is_some() {
            "integration:write"
        } else {
            "integration:read"
        };
        // Only the read branch is eligible for delegated, per-user credentials
        // (design's Gap A scope) -- a write already carries real,
        // cryptographically-bound authorization via the attestation signed
        // above, so it keeps minting the plain org-scoped credential.
        let token = if approval.is_some() {
            self.mint_ingestion_token(org_id, scope).await?
        } else {
            self.resolve_ingestion_token(org_id, scope, session_bearer)
                .await?
        };
        let mut outbound = self
            .http
            .post(format!(
                "{}/api/v1/connections/{}/actions",
                self.base_url, connection_id
            ))
            .bearer_auth(token)
            .header("content-type", "application/json")
            .json(&request);
        if let Some(approval) = approval {
            outbound = outbound.header("idempotency-key", approval);
        }
        let resp = outbound
            .send()
            .await
            .map_err(|e| format!("integration-corev2 action request failed: {e}"))?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .map_err(|e| format!("integration-corev2 action decode failed: {e}"))?;
        if !status.is_success() {
            // Surface the gateway's error envelope verbatim — it carries the
            // provider's own message (e.g. missing capability, bad params).
            let msg = value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or_else(|| value.as_str().unwrap_or(""));
            return Err(format!(
                "provider action returned {status}: {}",
                if msg.is_empty() {
                    truncate(&value.to_string())
                } else {
                    msg.to_owned()
                }
            ));
        }
        Ok(ActionOutcome {
            rendered: render_action_result(operation, &value),
            provider_receipt_id: value
                .pointer("/data/action/result")
                .and_then(extract_provider_receipt_id),
        })
    }

    /// Run a **read** operation and return integration-corev2's raw JSON.
    ///
    /// Deliberately separate from [`Self::execute_action`], which renders
    /// prose for a model and only surfaces a heuristic receipt id. The
    /// postcondition verifier (`crate::postcondition`) must match an exact
    /// identifier inside a structured response — matching against rendered
    /// prose would risk a false `Confirmed`, the one outcome that must never
    /// be produced on weak evidence.
    ///
    /// No `approval_ref` and therefore no write attestation: this path mints
    /// only an `integration:read` token, so it cannot be used to perform a
    /// write even if handed a write operation — integration-corev2 rejects
    /// the scope.
    pub async fn read_action_json(
        &self,
        org_id: &str,
        connection_id: &str,
        operation: &str,
        params: Value,
        session_bearer: Option<&str>,
    ) -> Result<Value, String> {
        if !is_connection_id(connection_id) {
            return Err("read_action_json requires a valid connection_id".to_owned());
        }
        if operation.trim().is_empty() {
            return Err("read_action_json requires operation".to_owned());
        }
        let request = serde_json::json!({
            "operation": operation,
            "params": params,
            "body": serde_json::json!({}),
        });
        let (status, value) = self
            .send_read(org_id, connection_id, request, session_bearer)
            .await?;
        if !status.is_success() {
            return Err(format!(
                "integration-corev2 read returned {status}: {}",
                truncate(&value.to_string())
            ));
        }
        Ok(value)
    }

    /// POST one read request with an `integration:read` token and return the gateway's status
    /// and body. Shared by [`Self::read_action_json`] and [`Self::read_action`], so both reads
    /// name the same credential and endpoint.
    async fn send_read(
        &self,
        org_id: &str,
        connection_id: &str,
        request: Value,
        session_bearer: Option<&str>,
    ) -> Result<(reqwest::StatusCode, Value), String> {
        let token = self
            .resolve_ingestion_token(org_id, "integration:read", session_bearer)
            .await?;
        let resp = self
            .http
            .post(format!(
                "{}/api/v1/connections/{}/actions",
                self.base_url, connection_id
            ))
            .bearer_auth(token)
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("integration-corev2 read request failed: {e}"))?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .map_err(|e| format!("integration-corev2 read decode failed: {e}"))?;
        Ok((status, value))
    }

    /// One connected-app read for the model (WS23-S3). The operation must be one the contract
    /// lists as a read, so a write cannot reach the executor from here. The parameters are cut
    /// to that operation's own list (a body is never sent), `maxResults` is clamped, and the
    /// answer is shaped to the fields the model needs. The token is `integration:read` only.
    ///
    /// # Errors
    ///
    /// [`ReadError::UnknownOperation`] for any operation the contract does not list as a read,
    /// [`ReadError::Refused`] for a missing connection, and [`ReadError::Upstream`] for a
    /// transport or provider failure. A connection that needs reconnecting or a missing
    /// capability is an answer, not an error: it comes back as a typed status.
    #[allow(clippy::too_many_arguments)]
    pub async fn read_action(
        &self,
        org_id: &str,
        connection_id: &str,
        operation: &str,
        params: &Value,
        max_results: Option<u64>,
        page_token: Option<&str>,
        session_bearer: Option<&str>,
    ) -> Result<ProviderRead, ReadError> {
        let Some(allowed) = read_operation_params(operation) else {
            return Err(ReadError::UnknownOperation(operation.trim().to_owned()));
        };
        if !is_connection_id(connection_id) {
            return Err(ReadError::Refused(
                "read_provider_action requires a valid connection_id".to_owned(),
            ));
        }
        let mut request_params = serde_json::Map::new();
        if let Value::Object(given) = params {
            for (key, value) in given {
                if allowed.contains(&key.as_str()) {
                    request_params.insert(key.clone(), value.clone());
                }
            }
        }
        if allowed.contains(&"maxResults") {
            let limit = max_results
                .unwrap_or(READ_MAX_RESULTS)
                .clamp(1, READ_MAX_RESULTS);
            request_params.insert("maxResults".to_owned(), Value::from(limit));
        }
        if let Some(token) = page_token.map(str::trim).filter(|token| !token.is_empty()) {
            if allowed.contains(&"pageToken") {
                request_params.insert("pageToken".to_owned(), Value::from(token));
            }
        }
        let request = serde_json::json!({
            "operation": operation,
            "params": request_params,
            "body": serde_json::json!({}),
        });
        let (status, value) = self
            .send_read(org_id, connection_id, request, session_bearer)
            .await
            .map_err(ReadError::Upstream)?;
        if !status.is_success() {
            return read_failure(&value);
        }
        let result = value.pointer("/data/action/result").unwrap_or(&Value::Null);
        Ok(shape_read(operation, &request_params, result))
    }

    async fn mint_ingestion_token(&self, org_id: &str, scope: &str) -> Result<String, String> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            return Err("integration request blocked: run organization is required".to_owned());
        }
        let response = self
            .http
            .post(format!(
                "{}/api/ingestion/internal-token",
                self.auth_core_url
            ))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", &self.service_credential)
            .json(&serde_json::json!({
                "orgId": org_id,
                "scopes": [scope],
                "reason": format!("execution-core {scope}"),
            }))
            .send()
            .await
            .map_err(|error| format!("Auth Core token request failed: {error}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!(
                "Auth Core refused the scoped integration credential ({status})"
            ));
        }
        let token = response
            .json::<PlaneTokenResponse>()
            .await
            .map_err(|error| format!("Auth Core token response was invalid: {error}"))?
            .token;
        if token.trim().is_empty() {
            return Err("Auth Core returned an empty integration credential".to_owned());
        }
        Ok(token)
    }

    /// Delegated-identity design (Gap A, 03-design-2-token-exchange.md §1.2).
    /// RFC 8693 token exchange: presents this run's own, already-verified
    /// `aud=session-core` bearer (`session_bearer`) as proof of the acting
    /// user, and receives back a strictly narrower `integration`-audience
    /// token that additionally carries `acting_user_id`. Auth-core re-verifies
    /// a signature it itself minted minutes earlier for this exact run — this
    /// is never execution-core vouching for the user on its own say-so.
    ///
    /// `zdr` on the returned token is the SUBJECT bearer's own claim, never
    /// this service's registry default — a ZDR-scoped user's run can never
    /// mint a persistent-posture credential through this path (design §1.6).
    async fn mint_delegated_ingestion_token(
        &self,
        org_id: &str,
        scope: &str,
        session_bearer: &str,
    ) -> Result<String, String> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            return Err("integration request blocked: run organization is required".to_owned());
        }
        if session_bearer.trim().is_empty() {
            return Err(
                "delegated ingestion credential unavailable: no session bearer for this run"
                    .to_owned(),
            );
        }
        let response = self
            .http
            .post(format!(
                "{}/api/ingestion/token-exchange",
                self.auth_core_url
            ))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", &self.service_credential)
            .json(&serde_json::json!({
                "orgId": org_id,
                "scopes": [scope],
                "reason": format!("execution-core {scope}"),
                "subjectToken": session_bearer,
                "subjectTokenType": "urn:ietf:params:oauth:token-type:jwt",
            }))
            .send()
            .await
            .map_err(|error| format!("Auth Core token-exchange request failed: {error}"))?;
        let status = response.status();
        if status == reqwest::StatusCode::FORBIDDEN {
            // Distinguished from every other failure on purpose: 403 here
            // means the `service:model-execution` principal's registry row
            // has no `tokenExchange` entry for this audience yet (deploy
            // step 4, not shipped with this code change) -- NOT that the
            // subject bearer itself is invalid. The caller falls back to the
            // org-only credential in exactly that one case, so shipping this
            // client change is genuinely zero-behavior-change until the
            // registry is updated (design §1.7), and never silently
            // downgrades a real subject-token failure (401/400 below).
            return Err(DELEGATION_NOT_CONFIGURED.to_owned());
        }
        if !status.is_success() {
            return Err(format!(
                "Auth Core refused the delegated integration credential ({status})"
            ));
        }
        let token = response
            .json::<PlaneTokenResponse>()
            .await
            .map_err(|error| format!("Auth Core token-exchange response was invalid: {error}"))?
            .token;
        if token.trim().is_empty() {
            return Err("Auth Core returned an empty delegated integration credential".to_owned());
        }
        Ok(token)
    }

    /// Single choke point the four read call sites mint through
    /// (`list_provider_actions`, `provider_key_for_connection`,
    /// `read_action_json`, `execute_action`'s read branch). Prefers the
    /// delegated, per-user credential whenever a `session_bearer` is
    /// available for this run; falls back to the plain org-scoped credential
    /// only when delegation is not yet configured for this deployment (see
    /// `mint_delegated_ingestion_token`'s doc) or no bearer was forwarded at
    /// all (a run that lost its delegated session credential — the same
    /// condition that already produces "delegation memory skipped" elsewhere
    /// in this codebase). A real subject-token failure is never swallowed
    /// into this fallback.
    async fn resolve_ingestion_token(
        &self,
        org_id: &str,
        scope: &str,
        session_bearer: Option<&str>,
    ) -> Result<String, String> {
        let Some(session_bearer) = session_bearer.map(str::trim).filter(|b| !b.is_empty()) else {
            return self.mint_ingestion_token(org_id, scope).await;
        };
        match self
            .mint_delegated_ingestion_token(org_id, scope, session_bearer)
            .await
        {
            Ok(token) => Ok(token),
            Err(message) if message == DELEGATION_NOT_CONFIGURED => {
                self.mint_ingestion_token(org_id, scope).await
            }
            Err(message) => Err(message),
        }
    }
}

/// Sentinel returned internally by `mint_delegated_ingestion_token` for the
/// one failure mode `resolve_ingestion_token` treats as "fall back," never
/// surfaced to a tool caller directly.
const DELEGATION_NOT_CONFIGURED: &str = "__delegation_not_configured__";

/// Largest text the model reads back from one connected-app read, in characters. The render
/// drops items that do not fit and says how many, so a read is never silently cut.
pub const READ_RESULT_CHAR_CAP: usize = 12_000;
/// Most items one read asks the provider for; a larger request is clamped to it.
pub const READ_MAX_RESULTS: u64 = 25;
/// Largest mail body or file text kept on one item.
const BODY_CHAR_CAP: usize = 8_000;
/// Keys a provider list response carries its rows under (Graph, Gmail, Drive and Sites).
const LIST_KEYS: &[&str] = &["value", "messages", "events", "files", "sites", "drives", "items"];

/// The environment switch for the read tool. Only the literal `true` enables it, and it is
/// read on every offer, so a restart is not needed to turn it off.
pub const PROVIDER_READ_TOOL_ENV: &str = "PROVIDER_READ_TOOL_ENABLED";

/// Whether the deployment has switched the connected-app read tool on.
#[must_use]
pub fn provider_read_tool_enabled() -> bool {
    parse_provider_read_tool_enabled(std::env::var(PROVIDER_READ_TOOL_ENV).ok().as_deref())
}

#[must_use]
pub fn parse_provider_read_tool_enabled(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether a run is offered `read_provider_action` (WS23-S3, K3-15). It needs the switch, a run
/// that is neither zero-retention nor Space-scoped, and at least one connection the user selected
/// for the run. Nothing else selects a connection, so a run with no selection never gets the tool.
#[must_use]
pub fn read_provider_action_offered(
    switched_on: bool,
    zdr: bool,
    space_scoped: bool,
    connector_scope: &[String],
) -> bool {
    switched_on && !zdr && !space_scoped && !connector_scope.is_empty()
}

/// A connection id is spliced into the gateway path, so only the characters an id is made of may
/// reach it: anything else (a `/`, `?` or `#`) could retarget the authenticated read.
fn is_connection_id(connection_id: &str) -> bool {
    !connection_id.is_empty()
        && connection_id.len() <= 128
        && connection_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// The read operations the tool may name, once each and sorted, for its closed `operation` enum.
#[must_use]
pub fn read_operation_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = READ_OPERATIONS.iter().map(|read| read.operation).collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// The parameter names an operation's reads take, across the providers that serve it, or `None`
/// when the contract lists no read by that name (a write, or an operation not yet catalogued).
fn read_operation_params(operation: &str) -> Option<Vec<&'static str>> {
    let mut names: Vec<&'static str> = Vec::new();
    let mut found = false;
    for read in READ_OPERATIONS
        .iter()
        .filter(|read| read.operation == operation)
    {
        found = true;
        for name in read.params {
            if !names.contains(name) {
                names.push(name);
            }
        }
    }
    found.then_some(names)
}

/// The typed status of one read, as the model is told it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadStatus {
    Ok,
    NoResults,
    ReconnectRequired,
    CapabilityMissing,
    TooLarge,
    UnsupportedContent,
}

impl ReadStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::NoResults => "no_results",
            Self::ReconnectRequired => "reconnect_required",
            Self::CapabilityMissing => "capability_missing",
            Self::TooLarge => "too_large",
            Self::UnsupportedContent => "unsupported_content",
        }
    }
}

/// One connected-app read, shaped for the model. Built by [`IntegrationActionsClient::read_action`]
/// and rendered once by [`render_provider_read`].
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderRead {
    pub status: ReadStatus,
    pub items: Vec<Value>,
    pub next_page_token: Option<String>,
    pub more_available: bool,
}

impl ProviderRead {
    fn bare(status: ReadStatus) -> Self {
        Self {
            status,
            items: Vec::new(),
            next_page_token: None,
            more_available: false,
        }
    }
}

/// Why a read did not run. A refusal the model should see as a tool error, not a status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    UnknownOperation(String),
    Refused(String),
    Upstream(String),
}

impl ReadError {
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::UnknownOperation(operation) => format!(
                "unknown_operation: '{operation}' is not a read operation; read_provider_action only reads"
            ),
            Self::Refused(message) | Self::Upstream(message) => message.clone(),
        }
    }
}

/// The gateway's refusal of a read. A connection that needs reconnecting and a missing capability
/// are answers the model reports; anything else is a provider error.
fn read_failure(value: &Value) -> Result<ProviderRead, ReadError> {
    let status = match value.pointer("/error/code").and_then(Value::as_str) {
        Some("reconnect_required") => ReadStatus::ReconnectRequired,
        Some("capability_not_granted" | "capability_required") => ReadStatus::CapabilityMissing,
        _ => {
            let message = value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .map_or_else(|| truncate(&value.to_string()), str::to_owned);
            return Err(ReadError::Upstream(message));
        }
    };
    Ok(ProviderRead::bare(status))
}

/// Shape a successful read's provider payload to the fields its operation lists. Fields outside
/// the list are dropped, a mail body is capped, and file content is returned only as text.
fn shape_read(
    operation: &str,
    request: &serde_json::Map<String, Value>,
    result: &Value,
) -> ProviderRead {
    if operation == "drive.file.content" {
        return shape_file_content(request, result);
    }
    let fields = read_fields(operation);
    let rows: Vec<&Value> = if operation.ends_with(".get") || operation == "profile" {
        vec![result]
    } else {
        LIST_KEYS
            .iter()
            .find_map(|key| result.get(*key).and_then(Value::as_array))
            .map(|rows| rows.iter().collect())
            .unwrap_or_default()
    };
    let items: Vec<Value> = rows
        .into_iter()
        .filter(|row| row.is_object())
        .map(|row| shape_item(row, fields))
        .collect();
    let next_page_token = result
        .get("nextPageToken")
        .or_else(|| result.get("next_page_token"))
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_owned);
    let status = if items.is_empty() {
        ReadStatus::NoResults
    } else {
        ReadStatus::Ok
    };
    ProviderRead {
        status,
        items,
        more_available: next_page_token.is_some(),
        next_page_token,
    }
}

/// File content is text or nothing: a binary file is `unsupported_content`, not a blob the model
/// is handed. The text is capped and says so when it was cut.
fn shape_file_content(request: &serde_json::Map<String, Value>, result: &Value) -> ProviderRead {
    let mime = result
        .get("mimeType")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let exported_as_text = request.get("exportAs").and_then(Value::as_str) == Some("text");
    let Some(content) = result
        .get("content")
        .and_then(Value::as_str)
        .filter(|_| exported_as_text || mime.starts_with("text/"))
    else {
        return ProviderRead::bare(ReadStatus::UnsupportedContent);
    };
    let (text, cut) = cap_chars(content, BODY_CHAR_CAP);
    ProviderRead {
        status: if cut {
            ReadStatus::TooLarge
        } else {
            ReadStatus::Ok
        },
        items: vec![serde_json::json!({ "mimeType": mime, "text": text })],
        next_page_token: None,
        more_available: false,
    }
}

fn shape_item(row: &Value, fields: &[&str]) -> Value {
    let mut out = serde_json::Map::new();
    for field in fields.iter().filter(|field| **field != "bodyText") {
        if let Some(value) = row.get(*field) {
            out.insert((*field).to_owned(), value.clone());
        }
    }
    // Graph nests a sender's address; the model needs the address and name themselves.
    if let Some(address) = out
        .get("from")
        .and_then(|from| from.pointer("/emailAddress"))
        .cloned()
    {
        out.insert("from".to_owned(), address);
    }
    if fields.contains(&"bodyText") {
        let body = row
            .get("bodyText")
            .or_else(|| row.pointer("/body/content"))
            .and_then(Value::as_str);
        if let Some(body) = body {
            let (text, _) = cap_chars(body, BODY_CHAR_CAP);
            out.insert("bodyText".to_owned(), Value::from(text));
        }
    }
    Value::Object(out)
}

/// The fields an operation's rows keep. A field not named here never reaches the model.
fn read_fields(operation: &str) -> &'static [&'static str] {
    match operation {
        "mail.messages" | "gmail.messages" => &[
            "id",
            "subject",
            "from",
            "receivedDateTime",
            "snippet",
            "webLink",
        ],
        "mail.messages.get" | "gmail.messages.get" => &[
            "id",
            "subject",
            "from",
            "receivedDateTime",
            "snippet",
            "webLink",
            "bodyText",
        ],
        "calendar.events" => &[
            "subject",
            "start",
            "end",
            "location",
            "organizer",
            "webLink",
        ],
        "drive.files" | "sharepoint.drives" => &[
            "id",
            "name",
            "mimeType",
            "size",
            "modifiedTime",
            "lastModifiedDateTime",
            "webUrl",
            "webViewLink",
        ],
        "sharepoint.sites" => &["id", "name", "webUrl"],
        "profile" => &["displayName", "mail", "userPrincipalName", "email"],
        _ => &[],
    }
}

/// The first `max` characters of `text`, and whether anything was cut.
fn cap_chars(text: &str, max: usize) -> (String, bool) {
    match text.char_indices().nth(max) {
        Some((cut, _)) => (text[..cut].to_owned(), true),
        None => (text.to_owned(), false),
    }
}

/// The model's copy of one connected-app read: a JSON object with the typed status, the items,
/// paging and how many items did not fit. It is capped at [`READ_RESULT_CHAR_CAP`] characters.
/// Items that do not fit are dropped from the end and counted in `omitted`, and a read that
/// dropped any reports `too_large`, so a partial answer is reported as partial.
#[must_use]
pub fn render_provider_read(read: &ProviderRead) -> String {
    let mut items = read.items.clone();
    let mut omitted = 0usize;
    loop {
        let status = if omitted > 0 {
            ReadStatus::TooLarge
        } else {
            read.status
        };
        let rendered = serde_json::json!({
            "status": status.as_str(),
            "items": items,
            "next_page_token": read.next_page_token,
            "more_available": read.more_available,
            "omitted": omitted,
        })
        .to_string();
        if rendered.chars().count() <= READ_RESULT_CHAR_CAP || items.is_empty() {
            return rendered;
        }
        items.pop();
        omitted += 1;
    }
}

fn parse_connections(value: &Value) -> Vec<ConnectionSummary> {
    // Envelope tolerance: {data:{connections:[…]}}, {connections:[…]}, or a
    // bare array — the Go handler's exact shape has moved before.
    let arr = value
        .pointer("/data/connections")
        .or_else(|| value.pointer("/connections"))
        .or_else(|| value.pointer("/data"))
        .and_then(Value::as_array)
        .or_else(|| value.as_array())
        .cloned()
        .unwrap_or_default();
    arr.iter()
        .map(|c| ConnectionSummary {
            id: c.get("id").and_then(Value::as_str).unwrap_or("").to_owned(),
            provider_key: c
                .get("providerKey")
                .or_else(|| c.get("provider_key"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            display_name: c
                .get("displayName")
                .or_else(|| c.get("display_name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            status: c
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        })
        .filter(|c| !c.id.is_empty())
        .collect()
}

fn catalog_for(provider_key: &str) -> Option<&'static [CatalogOp]> {
    CATALOG
        .iter()
        .find(|(p, _)| *p == provider_key)
        .map(|(_, ops)| *ops)
}

/// Classify a provider-action operation as write (`true`) or read (`false`)
/// using the frozen actions-surface catalog above (mirrors
/// docs/actions-surface-operations.md's approval column). Matches both the bare
/// operation (`pages.post`) and the provider-prefixed alias (`meta.pages.post`),
/// case-insensitively — the exhaustive gateway is stricter about case, so a
/// looser match here only ever ADDS caution.
///
/// Returns `None` when the operation is not catalogued. Callers MUST treat an
/// unknown operation as a write for gating purposes: integration-corev2 leaves
/// unmapped operations UNGATED (docs/actions-surface-operations.md §3), so
/// exec-core must never assume an unknown operation is a harmless read.
#[must_use]
pub fn operation_is_write(operation: &str) -> Option<bool> {
    let op = operation.trim();
    if op.is_empty() {
        return None;
    }
    for (provider, ops) in CATALOG {
        for c in *ops {
            let aliased = format!("{provider}.{}", c.operation);
            if op.eq_ignore_ascii_case(c.operation) || op.eq_ignore_ascii_case(&aliased) {
                return Some(c.is_write);
            }
        }
    }
    None
}

fn render_provider_actions(connections: &[ConnectionSummary]) -> String {
    if connections.is_empty() {
        return "No provider connections exist for this organization yet. Connect a provider \
(Meta, LinkedIn, Google, Microsoft, Slack, GitHub, Notion, Shopify, …) from the integrations \
settings before calling execute_provider_action."
            .to_owned();
    }
    let mut s = format!(
        "Connected providers ({}). Call execute_provider_action with the connection_id and one of \
the listed operations. Operations marked [write] pause for human approval under the deployed-agent \
posture.\n",
        connections.len()
    );
    for c in connections {
        let active =
            c.status.eq_ignore_ascii_case("active") || c.status.eq_ignore_ascii_case("connected");
        let label = if c.display_name.is_empty() {
            c.provider_key.as_str()
        } else {
            c.display_name.as_str()
        };
        let _ = write!(
            s,
            "\n• {} — {} (connection_id: {}",
            c.provider_key, label, c.id
        );
        if !active {
            let _ = write!(s, ", status: {}", c.status);
        }
        s.push_str(")\n");
        match catalog_for(&c.provider_key) {
            Some(ops) => {
                for op in ops {
                    let _ = writeln!(
                        s,
                        "    - {}{}: {}",
                        op.operation,
                        if op.is_write { " [write]" } else { "" },
                        op.summary
                    );
                }
            }
            None => {
                let _ = writeln!(
                    s,
                    "    (connected, but no agent-callable operations are catalogued for this \
provider yet)"
                );
            }
        }
    }
    s
}

fn render_action_result(operation: &str, value: &Value) -> String {
    let result = value
        .pointer("/data/action/result")
        .or_else(|| value.pointer("/data/result"))
        .cloned()
        .unwrap_or_else(|| value.clone());
    let pretty = serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());
    format!("Executed {operation}. Result:\n{}", truncate(&pretty))
}

/// Rust port of integration-corev2's own `actionProviderMessageIDAtDepth`
/// (`internal/api/server.go`) — kept byte-for-byte equivalent so a receipt
/// this client extracts client-side always agrees with what integration-corev2
/// itself would have stored server-side (`store.ActionReceipt.ProviderMessageID`)
/// for the same response. Depth-bounded, tries a fixed key set at each level,
/// then recurses into a fixed set of envelope/container keys.
fn extract_provider_receipt_id(value: &Value) -> Option<String> {
    provider_message_id_at_depth(value, 0)
}

fn provider_message_id_at_depth(value: &Value, depth: u8) -> Option<String> {
    if depth > 4 {
        return None;
    }
    match value {
        Value::Object(map) => {
            for key in [
                "provider_message_id",
                "providerMessageId",
                "message_id",
                "messageId",
                "id",
                "ts",
            ] {
                if let Some(candidate) = map.get(key).and_then(Value::as_str) {
                    let trimmed = candidate.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_owned());
                    }
                }
            }
            for key in ["message", "messages", "data", "result"] {
                if let Some(candidate) = map
                    .get(key)
                    .and_then(|nested| provider_message_id_at_depth(nested, depth + 1))
                {
                    return Some(candidate);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| provider_message_id_at_depth(item, depth + 1)),
        _ => None,
    }
}

fn truncate(s: &str) -> String {
    if s.len() <= MAX_RESULT_CHARS {
        return s.to_owned();
    }
    // Truncate on a char boundary so we never split a UTF-8 sequence.
    let mut end = MAX_RESULT_CHARS;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [truncated {} more chars]", &s[..end], s.len() - end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_connections_with_catalog_operations() {
        let conns = vec![
            ConnectionSummary {
                id: "conn_meta_1".to_owned(),
                provider_key: "meta".to_owned(),
                display_name: "Aquatiq AS".to_owned(),
                status: "active".to_owned(),
            },
            ConnectionSummary {
                id: "conn_li_1".to_owned(),
                provider_key: "linkedin".to_owned(),
                display_name: String::new(),
                status: "active".to_owned(),
            },
        ];
        let s = render_provider_actions(&conns);
        assert!(s.contains("meta — Aquatiq AS (connection_id: conn_meta_1"));
        assert!(s.contains("whatsapp.messages.send [write]"));
        assert!(s.contains("pages.list")); // read op, no [write] marker
        assert!(!s.contains("pages.list [write]"));
        assert!(s.contains("linkedin — linkedin (connection_id: conn_li_1"));
        assert!(s.contains("lead.responses"));
    }

    #[test]
    fn renders_empty_connections_honestly() {
        let s = render_provider_actions(&[]);
        assert!(s.contains("No provider connections exist"));
    }

    #[test]
    fn catalogues_uncatalogued_provider_honestly() {
        let conns = vec![ConnectionSummary {
            id: "conn_x".to_owned(),
            provider_key: "snapchat".to_owned(),
            display_name: "Snap".to_owned(),
            status: "active".to_owned(),
        }];
        let s = render_provider_actions(&conns);
        assert!(s.contains("no agent-callable operations are catalogued"));
    }

    #[test]
    fn parse_connections_tolerates_envelope_shapes() {
        let enveloped: Value = serde_json::from_str(
            r#"{"data":{"connections":[{"id":"c1","providerKey":"meta","displayName":"A","status":"active"}]}}"#,
        )
        .unwrap();
        let bare: Value = serde_json::from_str(
            r#"[{"id":"c2","provider_key":"slack","display_name":"B","status":"active"}]"#,
        )
        .unwrap();
        assert_eq!(parse_connections(&enveloped).len(), 1);
        assert_eq!(parse_connections(&enveloped)[0].provider_key, "meta");
        assert_eq!(parse_connections(&bare).len(), 1);
        assert_eq!(parse_connections(&bare)[0].provider_key, "slack");
    }

    #[test]
    fn render_action_result_unwraps_envelope_and_truncates() {
        let v: Value = serde_json::from_str(
            r#"{"success":true,"data":{"action":{"providerKey":"meta","operation":"pages.list","result":{"data":[{"id":"page_1","name":"Test Page"}]}}}}"#,
        )
        .unwrap();
        let s = render_action_result("pages.list", &v);
        assert!(s.contains("Executed pages.list"));
        assert!(s.contains("page_1"));

        let long = "x".repeat(MAX_RESULT_CHARS + 500);
        assert!(truncate(&long).contains("[truncated 500 more chars]"));
    }

    #[test]
    fn operation_is_write_classifies_reads_and_writes() {
        // Writes (outbound side effects) → true.
        assert_eq!(operation_is_write("pages.post"), Some(true));
        assert_eq!(operation_is_write("whatsapp.messages.send"), Some(true));
        assert_eq!(operation_is_write("message.send"), Some(true));
        assert_eq!(operation_is_write("issues.create"), Some(true));
        // Reads → false.
        assert_eq!(operation_is_write("pages.list"), Some(false));
        assert_eq!(operation_is_write("ads.insights"), Some(false));
        assert_eq!(operation_is_write("issues.list"), Some(false));
        assert_eq!(operation_is_write("calendar.events"), Some(false));
    }

    #[test]
    fn operation_is_write_accepts_provider_prefixed_aliases_case_insensitively() {
        assert_eq!(operation_is_write("meta.pages.post"), Some(true));
        assert_eq!(operation_is_write("slack.message.send"), Some(true));
        assert_eq!(operation_is_write("Github.Issues.List"), Some(false));
        assert_eq!(operation_is_write("meta.pages.list"), Some(false));
    }

    #[test]
    fn phase6_fixed_operation_strings_are_catalogued() {
        // Slack: the real op `channels.list` resolves (read); the old bare
        // `channels` string (which matched no gateway case arm) is gone.
        assert_eq!(operation_is_write("channels.list"), Some(false));
        assert_eq!(operation_is_write("slack.channels.list"), Some(false));
        assert_eq!(operation_is_write("channels"), None);
        // Notion: the real read ops resolve; the old non-functional `search` op
        // (no such executor case) is gone.
        assert_eq!(operation_is_write("databases"), Some(false));
        assert_eq!(operation_is_write("pages"), Some(false));
        assert_eq!(operation_is_write("user"), Some(false));
        assert_eq!(operation_is_write("notion.databases"), Some(false));
        assert_eq!(operation_is_write("search"), None);
    }

    #[test]
    fn mailread_operations_are_catalogued_as_reads_never_writes() {
        // Outlook mail, Gmail, and OneDrive listing are pure reads in
        // integration-corev2's executor (service.go executeMicrosoft /
        // executeGoogle) — they must classify as reads here too, or the
        // agent runtime would pause every mail-read call for human approval
        // the same way it gates mail.send.
        assert_eq!(operation_is_write("mail.messages"), Some(false));
        assert_eq!(operation_is_write("microsoft.mail.messages"), Some(false));
        assert_eq!(operation_is_write("gmail.messages"), Some(false));
        assert_eq!(operation_is_write("google.gmail.messages"), Some(false));
        assert_eq!(operation_is_write("drive.files"), Some(false));
        assert_eq!(operation_is_write("microsoft.drive.files"), Some(false));
        assert_eq!(operation_is_write("google.drive.files"), Some(false));
        // The write siblings are unaffected.
        assert_eq!(operation_is_write("mail.send"), Some(true));
        assert_eq!(operation_is_write("gmail.send"), Some(true));
    }

    #[test]
    fn renders_microsoft_and_google_read_ops_without_write_marker() {
        let conns = vec![
            ConnectionSummary {
                id: "conn_ms_1".to_owned(),
                provider_key: "microsoft".to_owned(),
                display_name: "Contoso".to_owned(),
                status: "active".to_owned(),
            },
            ConnectionSummary {
                id: "conn_g_1".to_owned(),
                provider_key: "google".to_owned(),
                display_name: "Aquatiq Google".to_owned(),
                status: "active".to_owned(),
            },
        ];
        let s = render_provider_actions(&conns);
        // New reads render without a [write] marker …
        assert!(s.contains("mail.messages: Read-only"));
        assert!(!s.contains("mail.messages [write]"));
        assert!(s.contains("gmail.messages: Read-only"));
        assert!(!s.contains("gmail.messages [write]"));
        assert!(s.contains("drive.files: Read-only"));
        assert!(!s.contains("drive.files [write]"));
        // … while the existing write ops are still marked.
        assert!(s.contains("mail.send [write]"));
        assert!(s.contains("gmail.send [write]"));
    }

    #[test]
    fn operation_is_write_returns_none_for_unknown_and_empty() {
        // Unknown / uncatalogued operations are None — callers gate them as writes.
        assert_eq!(operation_is_write("okta.user.suspend"), None); // not catalogued here
        assert_eq!(operation_is_write("totally.made.up"), None);
        assert_eq!(operation_is_write(""), None);
        assert_eq!(operation_is_write("   "), None);
    }

    /// Pins today's gating table end to end (classification here, the ask-mode
    /// decision in `permission::is_risky_call`), so a catalogue edit is a
    /// deliberate change and not a silent widening. Catalogued reads run
    /// ungated; uncatalogued operations are `None` and therefore gated, because
    /// integration-corev2 leaves them ungated and exec-core must not guess they
    /// are reads.
    #[test]
    fn current_gating_table_is_pinned() {
        use crate::permission::is_risky_call;

        let input = |operation: &str| serde_json::json!({ "operation": operation }).to_string();

        for read in [
            "pages.list",
            "ads.insights",
            "mail.messages",
            "microsoft.mail.messages",
            "gmail.messages",
            "google.gmail.messages",
            "drive.files",
            "calendar.events",
            "mail.messages.get",
            "microsoft.mail.messages.get",
            "gmail.messages.get",
            "google.gmail.messages.get",
            "drive.file.content",
            "microsoft.drive.file.content",
            "sharepoint.sites",
            "sharepoint.drives",
            "profile",
            "microsoft.profile",
            "google.profile",
        ] {
            assert_eq!(operation_is_write(read), Some(false), "{read} must stay a read");
            assert!(
                !is_risky_call("execute_provider_action", &input(read)),
                "{read} must stay ungated under ask"
            );
        }

        for uncatalogued in [
            "drive.file.delete",
            "sharepoint.sites.create",
            "profile.update",
        ] {
            assert_eq!(
                operation_is_write(uncatalogued),
                None,
                "{uncatalogued} is not catalogued yet"
            );
            assert!(
                is_risky_call("execute_provider_action", &input(uncatalogued)),
                "{uncatalogued} must stay gated under ask until it is catalogued"
            );
        }

        assert_eq!(operation_is_write("mail.send"), Some(true));
        assert!(is_risky_call("execute_provider_action", &input("mail.send")));
    }

    /// The golden in integration-corev2 (copied into mp-contracts) names the Microsoft and
    /// Google reads the executor serves. The catalogue must offer exactly those for those
    /// providers: a golden read left out would be gated as a write, and a catalogued read the
    /// golden does not name would be ungated without the executor having been checked for it.
    #[test]
    fn catalog_reads_match_contract_table() {
        use mp_contracts::provider_reads::READ_OPERATIONS;
        use std::collections::BTreeSet;

        let covered: BTreeSet<&str> = READ_OPERATIONS.iter().map(|read| read.provider).collect();
        let contract: BTreeSet<(&str, &str)> = READ_OPERATIONS
            .iter()
            .map(|read| (read.provider, read.operation))
            .collect();
        let catalogued: BTreeSet<(&str, &str)> = CATALOG
            .iter()
            .filter(|(provider, _)| covered.contains(provider))
            .flat_map(|(provider, ops)| {
                ops.iter()
                    .filter(|op| !op.is_write)
                    .map(move |op| (*provider, op.operation))
            })
            .collect();

        assert_eq!(catalogued, contract);
        assert_eq!(contract.len(), READ_OPERATIONS.len(), "golden rows are unique");
    }

    // -------------------------------------------------------------------
    // Connected-app reads (WS23-S3).
    // -------------------------------------------------------------------

    #[test]
    fn a_write_operation_is_not_a_read_and_reads_list_their_own_params() {
        assert!(read_operation_params("mail.send").is_none());
        assert!(read_operation_params("drive.file.delete").is_none());
        let gmail = read_operation_params("gmail.messages").expect("a read");
        assert!(gmail.contains(&"query") && gmail.contains(&"maxResults"));
        assert!(!gmail.contains(&"body"));
    }

    #[test]
    fn the_operation_enum_names_only_reads() {
        let names = read_operation_names();
        assert!(names.contains(&"calendar.events") && names.contains(&"gmail.messages.get"));
        for name in names {
            for verb in ["send", "create", "delete", "update", "post", "publish", "upload"] {
                assert!(!name.contains(verb), "{name} reads nothing");
            }
        }
    }

    #[test]
    fn the_switch_is_on_only_for_the_literal_true() {
        for raw in [None, Some(""), Some("1"), Some("TRUE"), Some("yes"), Some("false")] {
            assert!(!parse_provider_read_tool_enabled(raw), "{raw:?}");
        }
        assert!(parse_provider_read_tool_enabled(Some("true")));
    }

    #[test]
    fn the_tool_is_offered_only_to_a_selected_connection_outside_zdr_and_space() {
        let scope = vec!["conn_1".to_owned()];
        assert!(read_provider_action_offered(true, false, false, &scope));
        assert!(!read_provider_action_offered(false, false, false, &scope));
        assert!(!read_provider_action_offered(true, true, false, &scope));
        assert!(!read_provider_action_offered(true, false, true, &scope));
        assert!(!read_provider_action_offered(true, false, false, &[]));
    }

    #[test]
    fn a_rendered_read_is_capped_and_says_what_it_dropped() {
        let row = serde_json::json!({ "id": "m", "subject": "x".repeat(1000) });
        let read = ProviderRead {
            status: ReadStatus::Ok,
            items: vec![row; 40],
            next_page_token: Some("next".to_owned()),
            more_available: true,
        };
        let rendered = render_provider_read(&read);
        let parsed: Value = serde_json::from_str(&rendered).expect("valid json");
        assert!(rendered.chars().count() <= READ_RESULT_CHAR_CAP);
        assert_eq!(parsed["status"], "too_large");
        let kept = parsed["items"].as_array().expect("items").len() as u64;
        assert!(kept < 40);
        assert_eq!(parsed["omitted"].as_u64(), Some(40 - kept));
        assert_eq!(parsed["next_page_token"], "next");
    }

    #[test]
    fn a_mail_body_is_capped_and_no_other_field_passes_through() {
        let row = serde_json::json!({
            "id": "m1",
            "bodyText": "b".repeat(9_000),
            "internetMessageHeaders": [{"name": "Authorization"}],
        });
        let shaped = shape_item(&row, read_fields("mail.messages.get"));
        assert_eq!(shaped["bodyText"].as_str().map(str::len), Some(8_000));
        assert!(shaped.get("internetMessageHeaders").is_none());
        assert_eq!(shaped["id"], "m1");
    }

    #[test]
    fn binary_file_content_is_unsupported_and_text_is_returned() {
        let request = serde_json::Map::new();
        let pdf = serde_json::json!({ "mimeType": "application/pdf", "content": "%PDF" });
        assert_eq!(
            shape_read("drive.file.content", &request, &pdf).status,
            ReadStatus::UnsupportedContent
        );
        let mut exported = serde_json::Map::new();
        exported.insert("exportAs".to_owned(), Value::from("text"));
        let doc = serde_json::json!({ "mimeType": "application/pdf", "content": "hello" });
        assert_eq!(
            shape_read("drive.file.content", &exported, &doc).status,
            ReadStatus::Ok
        );
    }

    async fn mount_org_token(auth_core: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/api/ingestion/internal-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"token": "org-scoped-token"})),
            )
            .mount(auth_core)
            .await;
    }

    #[tokio::test]
    async fn read_action_sends_only_the_listed_params_and_clamps_max_results() {
        let auth_core = MockServer::start().await;
        let integration = MockServer::start().await;
        mount_org_token(&auth_core).await;
        Mock::given(method("POST"))
            .and(path("/api/v1/connections/conn_1/actions"))
            .and(header("authorization", "Bearer org-scoped-token"))
            .and(body_json(serde_json::json!({
                "operation": "gmail.messages",
                "params": {"query": "invoice", "maxResults": 25},
                "body": {},
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"success": true, "data": {"action": {"result": {"messages": []}}}}),
            ))
            .expect(1)
            .mount(&integration)
            .await;
        let client = test_client(&auth_core.uri(), &integration.uri());
        let params = serde_json::json!({"query": "invoice", "body": {"raw": "x"}, "evil": 1});
        let read = client
            .read_action("org-1", "conn_1", "gmail.messages", &params, Some(100), None, None)
            .await
            .expect("the read runs");
        assert_eq!(read.status, ReadStatus::NoResults);
    }

    #[tokio::test]
    async fn read_action_shapes_mail_and_returns_the_page_token() {
        let auth_core = MockServer::start().await;
        let integration = MockServer::start().await;
        mount_org_token(&auth_core).await;
        Mock::given(method("POST"))
            .and(path("/api/v1/connections/conn_1/actions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "data": {"action": {"result": {
                    "value": [{
                        "id": "m1",
                        "subject": "Invoice",
                        "from": {"emailAddress": {"address": "a@example.no", "name": "A"}},
                        "receivedDateTime": "2026-10-20T08:00:00Z",
                        "snippet": "hi",
                        "webLink": "https://outlook.example/m1",
                        "secretHeader": "drop me",
                    }],
                    "nextPageToken": "page-2",
                }}},
            })))
            .mount(&integration)
            .await;
        let client = test_client(&auth_core.uri(), &integration.uri());
        let read = client
            .read_action(
                "org-1",
                "conn_1",
                "mail.messages",
                &serde_json::json!({}),
                None,
                Some("page-1"),
                None,
            )
            .await
            .expect("the read runs");
        assert_eq!(read.status, ReadStatus::Ok);
        assert_eq!(read.items[0]["from"]["address"], "a@example.no");
        assert!(read.items[0].get("secretHeader").is_none());
        assert_eq!(read.next_page_token.as_deref(), Some("page-2"));
        assert!(read.more_available);
    }

    #[tokio::test]
    async fn read_action_reports_a_reconnect_and_a_missing_capability_as_statuses() {
        let auth_core = MockServer::start().await;
        let integration = MockServer::start().await;
        mount_org_token(&auth_core).await;
        Mock::given(method("POST"))
            .and(path("/api/v1/connections/conn_stale/actions"))
            .respond_with(ResponseTemplate::new(409).set_body_json(serde_json::json!({
                "error": {"code": "reconnect_required", "message": "reconnect"}
            })))
            .mount(&integration)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/connections/conn_nocap/actions"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": {"code": "capability_not_granted", "message": "no calendar"}
            })))
            .mount(&integration)
            .await;
        let client = test_client(&auth_core.uri(), &integration.uri());
        let empty = serde_json::json!({});
        let stale = client
            .read_action("org-1", "conn_stale", "calendar.events", &empty, None, None, None)
            .await
            .expect("a status, not an error");
        assert_eq!(stale.status, ReadStatus::ReconnectRequired);
        let nocap = client
            .read_action("org-1", "conn_nocap", "calendar.events", &empty, None, None, None)
            .await
            .expect("a status, not an error");
        assert_eq!(nocap.status, ReadStatus::CapabilityMissing);
    }

    #[tokio::test]
    async fn read_action_refuses_a_write_before_any_call_is_made() {
        let client = test_client("http://unused.invalid", "http://unused.invalid");
        let refused = client
            .read_action("org-1", "conn_1", "mail.send", &serde_json::json!({}), None, None, None)
            .await;
        assert!(matches!(refused, Err(ReadError::UnknownOperation(_))));
        assert!(refused
            .expect_err("a write is refused")
            .message()
            .starts_with("unknown_operation:"));
    }

    #[tokio::test]
    async fn a_connection_id_that_could_leave_the_connection_path_makes_no_call() {
        let integration = MockServer::start().await;
        let client = test_client("http://unused.invalid", &integration.uri());
        for hostile in ["conn/../x#", "conn_1?x=1", "..", " ", ""] {
            let refused = client
                .read_action(
                    "org-1",
                    hostile,
                    "mail.messages",
                    &serde_json::json!({}),
                    None,
                    None,
                    None,
                )
                .await;
            assert!(
                matches!(refused, Err(ReadError::Refused(_))),
                "{hostile:?} is refused"
            );
            assert!(client
                .read_action_json("org-1", hostile, "mail.messages", serde_json::json!({}), None)
                .await
                .is_err());
        }
        assert!(integration
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty());
    }

    // -------------------------------------------------------------------
    // Delegated-identity design, Gap A (03-design-2-token-exchange.md §1).
    // -------------------------------------------------------------------

    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_client(auth_core_url: &str, integration_url: &str) -> IntegrationActionsClient {
        IntegrationActionsClient {
            base_url: integration_url.trim_end_matches('/').to_owned(),
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id: "model-execution".to_owned(),
            service_credential: "test-only-execution-service-key".to_owned(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("build test http client"),
            write_attestor: None,
        }
    }

    #[tokio::test]
    async fn mint_delegated_ingestion_token_requires_a_session_bearer() {
        let client = test_client("http://unused.invalid", "http://unused.invalid");
        let error = client
            .mint_delegated_ingestion_token("org-1", "integration:read", "   ")
            .await
            .expect_err("an empty/whitespace-only session bearer must be refused");
        assert!(error.contains("no session bearer"), "unexpected error: {error}");
    }

    #[tokio::test]
    async fn mint_delegated_ingestion_token_sends_the_rfc8693_exchange_request_and_returns_the_token(
    ) {
        let auth_core = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/ingestion/token-exchange"))
            .and(header("x-service-id", "model-execution"))
            .and(header("x-service-api-key", "test-only-execution-service-key"))
            .and(body_json(serde_json::json!({
                "orgId": "org-1",
                "scopes": ["integration:read"],
                "reason": "execution-core integration:read",
                "subjectToken": "run-owns-session-core-bearer",
                "subjectTokenType": "urn:ietf:params:oauth:token-type:jwt",
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"token": "delegated-token-1"})),
            )
            .expect(1)
            .mount(&auth_core)
            .await;

        let client = test_client(&auth_core.uri(), "http://unused.invalid");
        let token = client
            .mint_delegated_ingestion_token(
                "org-1",
                "integration:read",
                "run-owns-session-core-bearer",
            )
            .await
            .expect("exchange succeeds");
        assert_eq!(token, "delegated-token-1");
    }

    #[tokio::test]
    async fn resolve_ingestion_token_falls_back_to_the_org_scoped_credential_when_delegation_is_not_yet_configured(
    ) {
        // 403 from the exchange endpoint == "this deployment's registry has
        // no tokenExchange entry yet" (T-4/§1.7) -- must fall back, not fail.
        let auth_core = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/ingestion/token-exchange"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&auth_core)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/ingestion/internal-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"token": "org-scoped-fallback-token"})),
            )
            .expect(1)
            .mount(&auth_core)
            .await;

        let client = test_client(&auth_core.uri(), "http://unused.invalid");
        let token = client
            .resolve_ingestion_token("org-1", "integration:read", Some("some-session-bearer"))
            .await
            .expect("falls back to the org-scoped credential");
        assert_eq!(token, "org-scoped-fallback-token");
    }

    #[tokio::test]
    async fn resolve_ingestion_token_never_falls_back_on_a_real_subject_token_failure() {
        // 401 (an actually invalid/expired/wrong-org subject bearer) must
        // NEVER be silently downgraded to the org-only credential (T-2/§1.5)
        // -- unlike the 403 "not configured yet" case above.
        let auth_core = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/ingestion/token-exchange"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&auth_core)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/ingestion/internal-token"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&auth_core)
            .await;

        let client = test_client(&auth_core.uri(), "http://unused.invalid");
        let error = client
            .resolve_ingestion_token("org-1", "integration:read", Some("expired-bearer"))
            .await
            .expect_err("a real subject-token failure must never fall back");
        assert!(
            error.contains("refused the delegated"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn resolve_ingestion_token_uses_the_org_scoped_credential_when_no_bearer_is_forwarded() {
        // A run with no delegated session credential forwarded (or one that
        // lost it) uses the plain org-scoped path directly -- the exchange
        // endpoint is never even called.
        let auth_core = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/ingestion/token-exchange"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&auth_core)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/ingestion/internal-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"token": "org-scoped-token"})),
            )
            .expect(1)
            .mount(&auth_core)
            .await;

        let client = test_client(&auth_core.uri(), "http://unused.invalid");
        let token = client
            .resolve_ingestion_token("org-1", "integration:read", None)
            .await
            .expect("org-scoped path succeeds");
        assert_eq!(token, "org-scoped-token");
    }

    fn connections_body(connection_id: &str, display_name: &str) -> serde_json::Value {
        serde_json::json!({"data": {"connections": [
            {"id": connection_id, "providerKey": "microsoft", "displayName": display_name, "status": "active"},
        ]}})
    }

    /// list_provider_actions_and_read_action_json_succeed_for_the_runs_own_user_and_fail_for_a_different_users_personal_connection
    /// (Gap A "Exact tests"). Two independent fake services: auth-core's
    /// exchange mints a distinguishable token PER subject bearer (mirroring
    /// its real `acting_user_id` binding), and integration-corev2 only
    /// answers a personal connection's data to the caller whose token names
    /// its owner -- exactly `assertPersonalConnectionOwner`'s real shape,
    /// modeled at the HTTP boundary since this is execution-core's own test.
    #[tokio::test]
    async fn list_provider_actions_succeeds_for_the_runs_own_user_and_fails_for_a_different_users_personal_connection(
    ) {
        let auth_core = MockServer::start().await;
        let integration = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/ingestion/token-exchange"))
            .and(body_json(serde_json::json!({
                "orgId": "org-1", "scopes": ["integration:read"],
                "reason": "execution-core integration:read",
                "subjectToken": "bearer-for-user-a",
                "subjectTokenType": "urn:ietf:params:oauth:token-type:jwt",
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"token": "delegated-for-user-a"})),
            )
            .mount(&auth_core)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/connections"))
            .and(header("authorization", "Bearer delegated-for-user-a"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(connections_body("conn_user_a", "User A's personal Outlook")),
            )
            .mount(&integration)
            .await;
        // Any OTHER (or missing) bearer is a different/no user -- the fake
        // integration-corev2 refuses exactly like the real
        // personal_connection_no_delegated_user fail-closed default.
        Mock::given(method("GET"))
            .and(path("/api/v1/connections"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "error": {"code": "personal_connection_no_delegated_user"}
            })))
            .mount(&integration)
            .await;
        // A run with no delegated bearer at all falls back to the plain
        // org-scoped credential (resolve_ingestion_token's None branch) --
        // still a real token, but not user A's, so it hits the catch-all
        // refusal above, never the personal connection.
        Mock::given(method("POST"))
            .and(path("/api/ingestion/internal-token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"token": "org-scoped-token"})),
            )
            .mount(&auth_core)
            .await;

        let client = test_client(&auth_core.uri(), &integration.uri());

        let own_user = client
            .list_provider_actions("org-1", Some("bearer-for-user-a"))
            .await
            .expect("the run's own user reads their own personal connection");
        assert!(own_user.contains("conn_user_a"));

        // No delegated bearer at all -> resolve_ingestion_token's fallback
        // mints the plain org credential, which the fake integration-corev2
        // (correctly) does not recognize as any particular user either.
        let no_bearer = client.list_provider_actions("org-1", None).await;
        assert!(no_bearer.is_err(), "an unrecognized credential must be refused");
    }

    /// The other half of
    /// list_provider_actions_and_read_action_json_succeed_for_the_runs_own_user_and_fail_for_a_different_users_personal_connection:
    /// `read_action_json` goes through the exact same `resolve_ingestion_token`
    /// choke point, proven here against a personal-connection-shaped POST
    /// action endpoint the same way the GET /connections case above proves it.
    #[tokio::test]
    async fn read_action_json_succeeds_for_the_runs_own_user_and_fails_for_a_different_users_personal_connection(
    ) {
        let auth_core = MockServer::start().await;
        let integration = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/ingestion/token-exchange"))
            .and(body_json(serde_json::json!({
                "orgId": "org-1", "scopes": ["integration:read"],
                "reason": "execution-core integration:read",
                "subjectToken": "bearer-for-user-a",
                "subjectTokenType": "urn:ietf:params:oauth:token-type:jwt",
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"token": "delegated-for-user-a"})),
            )
            .mount(&auth_core)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/ingestion/token-exchange"))
            .and(body_json(serde_json::json!({
                "orgId": "org-1", "scopes": ["integration:read"],
                "reason": "execution-core integration:read",
                "subjectToken": "bearer-for-user-b",
                "subjectTokenType": "urn:ietf:params:oauth:token-type:jwt",
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"token": "delegated-for-user-b"})),
            )
            .mount(&auth_core)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/connections/conn_user_a/actions"))
            .and(header("authorization", "Bearer delegated-for-user-a"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {"messages": [{"id": "msg_1"}]},
            })))
            .mount(&integration)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/connections/conn_user_a/actions"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "error": {"code": "personal_connection_no_delegated_user"}
            })))
            .mount(&integration)
            .await;

        let client = test_client(&auth_core.uri(), &integration.uri());

        let own_user = client
            .read_action_json(
                "org-1",
                "conn_user_a",
                "mail.messages",
                serde_json::json!({}),
                Some("bearer-for-user-a"),
            )
            .await
            .expect("the run's own user reads their own personal connection");
        assert_eq!(own_user["data"]["messages"][0]["id"], "msg_1");

        let different_user = client
            .read_action_json(
                "org-1",
                "conn_user_a",
                "mail.messages",
                serde_json::json!({}),
                Some("bearer-for-user-b"),
            )
            .await;
        assert!(
            different_user.is_err(),
            "a different user's delegated bearer must never read this personal connection"
        );
    }

    /// two_concurrent_runs_for_two_different_users_never_cross_contaminate_the_delegated_read_credential
    /// (Gap A "Exact tests", closes A-2 — the single concurrency test neither
    /// original design's test list included). `IntegrationActionsClient` is
    /// `Clone` and carries no shared mutable per-call state; every method
    /// takes `org_id`/`session_bearer` as plain arguments. This spins up many
    /// interleaved concurrent calls for TWO different users against fakes
    /// that only answer the correct per-user token, and asserts every single
    /// call — across the whole run — resolved its OWN user's connection,
    /// never the other user's.
    #[tokio::test]
    async fn two_concurrent_runs_for_two_different_users_never_cross_contaminate_the_delegated_read_credential(
    ) {
        let auth_core = MockServer::start().await;
        let integration = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/ingestion/token-exchange"))
            .and(body_json(serde_json::json!({
                "orgId": "org-1", "scopes": ["integration:read"],
                "reason": "execution-core integration:read",
                "subjectToken": "bearer-user-a",
                "subjectTokenType": "urn:ietf:params:oauth:token-type:jwt",
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"token": "token-user-a"})),
            )
            .mount(&auth_core)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/ingestion/token-exchange"))
            .and(body_json(serde_json::json!({
                "orgId": "org-1", "scopes": ["integration:read"],
                "reason": "execution-core integration:read",
                "subjectToken": "bearer-user-b",
                "subjectTokenType": "urn:ietf:params:oauth:token-type:jwt",
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"token": "token-user-b"})),
            )
            .mount(&auth_core)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/connections"))
            .and(header("authorization", "Bearer token-user-a"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(connections_body("conn_user_a", "User A")),
            )
            .mount(&integration)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/connections"))
            .and(header("authorization", "Bearer token-user-b"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(connections_body("conn_user_b", "User B")),
            )
            .mount(&integration)
            .await;

        let client = test_client(&auth_core.uri(), &integration.uri());
        let mut handles = Vec::new();
        for i in 0..40 {
            let client = client.clone();
            let (bearer, expected_connection) = if i % 2 == 0 {
                ("bearer-user-a", "conn_user_a")
            } else {
                ("bearer-user-b", "conn_user_b")
            };
            handles.push(tokio::spawn(async move {
                let result = client
                    .list_provider_actions("org-1", Some(bearer))
                    .await
                    .expect("concurrent call succeeds");
                (result, expected_connection)
            }));
        }
        for handle in handles {
            let (result, expected_connection) = handle.await.expect("task did not panic");
            assert!(
                result.contains(expected_connection),
                "expected {expected_connection} in result, got: {result}"
            );
            // The single most important assertion: never the OTHER user's
            // connection id, under any interleaving.
            let other = if expected_connection == "conn_user_a" {
                "conn_user_b"
            } else {
                "conn_user_a"
            };
            assert!(
                !result.contains(other),
                "cross-contamination: {other} leaked into a call scoped to {expected_connection}"
            );
        }
    }
}
