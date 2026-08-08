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
//!   * `execute_provider_action` — runs one operation. Listed in
//!     `permission::is_risky_tool`, so under `ask` posture the run pauses for
//!     explicit human approval first (same spine as `book_shipment`); the
//!     approval reference is then forwarded as `approvalId` in the body to
//!     satisfy integration-corev2's own write-approval check.
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
            CatalogOp { operation: "calendar.events", is_write: false, summary: "List upcoming Google Calendar events." },
            CatalogOp { operation: "drive.files", is_write: false, summary: "List Google Drive files." },
        ],
    ),
    (
        "microsoft",
        &[
            CatalogOp { operation: "mail.send", is_write: true, summary: "Send an Outlook email. body.{to,subject,bodyText}." },
            CatalogOp { operation: "calendar.events", is_write: false, summary: "List Outlook calendar events." },
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
    /// host-published compose default; `INTEGRATION_COREV2_INTERNAL_KEY`
    /// supplies the `x-internal-api-key` header (integration-corev2 gates
    /// /api/v1/* on it). Returns `None` only when the reqwest client cannot be
    /// built — a missing key still yields a client (the gateway will answer
    /// 401, which surfaces as an honest tool error rather than a silent skip).
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

    /// GET /api/v1/connections?organizationId=<org> — the org's live provider
    /// connections. Rendered together with the static catalog so the model can
    /// see what is connected and what it can call on each.
    pub async fn list_provider_actions(&self, org_id: &str) -> Result<String, String> {
        if org_id.trim().is_empty() {
            return Err("list_provider_actions requires a run org_id (tenant scope)".to_owned());
        }
        let token = self
            .mint_ingestion_token(org_id, "integration:read")
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
    ) -> Result<String, String> {
        let token = self
            .mint_ingestion_token(org_id, "integration:read")
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
                .provider_key_for_connection(org_id, connection_id)
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
        let scope = if approval_ref.is_some() {
            "integration:write"
        } else {
            "integration:read"
        };
        let token = self.mint_ingestion_token(org_id, scope).await?;
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
    ) -> Result<Value, String> {
        if connection_id.trim().is_empty() {
            return Err("read_action_json requires connection_id".to_owned());
        }
        if operation.trim().is_empty() {
            return Err("read_action_json requires operation".to_owned());
        }
        let request = serde_json::json!({
            "operation": operation,
            "params": params,
            "body": serde_json::json!({}),
        });
        let token = self
            .mint_ingestion_token(org_id, "integration:read")
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
        if !status.is_success() {
            return Err(format!(
                "integration-corev2 read returned {status}: {}",
                truncate(&value.to_string())
            ));
        }
        Ok(value)
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
    fn operation_is_write_returns_none_for_unknown_and_empty() {
        // Unknown / uncatalogued operations are None — callers gate them as writes.
        assert_eq!(operation_is_write("okta.user.suspend"), None); // not catalogued here
        assert_eq!(operation_is_write("totally.made.up"), None);
        assert_eq!(operation_is_write(""), None);
        assert_eq!(operation_is_write("   "), None);
    }
}
