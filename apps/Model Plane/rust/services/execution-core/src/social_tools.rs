//! Social publishing tools for the agentic loop — Velion's social workspace.
//!
//! Backed by `social-core` (Application Plane), the same publish pipeline the
//! human Social workspace uses — posts created here are REAL workspace posts
//! (visible in the UI, audited, metric-tracked), not side-channel provider
//! calls. This closes the eval-harness finding (case 07, 2026-07-08): the
//! deployed-agent path had no social-publish capability at all, so the HITL
//! approval gate was never exercised for social posting.
//!
//! `list_social_accounts` is read-only and runs under `ask` posture without
//! an approval gate, like the info tools. `publish_social_post` is a REAL
//! outbound write and is DOUBLY gated by design (the audits' decorative-HITL
//! history is why):
//!   1. Model Plane HITL — the tool is in `permission::is_risky_tool`, so an
//!      `ask`-posture run pauses for explicit human approval of the call.
//!   2. social-core's own org-visible ApprovalState — the post is always
//!      created `approval_required=true`, so it publishes only after a
//!      workspace approval too (`ensurePublishApproved`, re-checked at
//!      execution). The tool result says exactly this, so the model sets
//!      honest expectations instead of claiming it published.
//!
//! Transport (internal key + org scoping, same style as the other planes):
//!   GET  {SOCIAL_CORE_URL}/api/v1/social/accounts            → connected accounts
//!   POST {SOCIAL_CORE_URL}/api/v1/social/posts               → create post (draft)
//!   POST {SOCIAL_CORE_URL}/api/v1/social/posts/{id}/publish-jobs → request publish

#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]

use std::fmt::Write as _;
use std::time::Duration;

use serde_json::Value;

/// Default social-core address. execution-core sits on model-plane-network
/// only, so cross-plane services are dialled via the host-published port
/// (same pattern as `SHIPPING_CORE_URL`/`INFORMATION_CORE_URL`).
const DEFAULT_SOCIAL_CORE_URL: &str = "http://host.docker.internal:3162";

/// Input for `publish_social_post`.
#[derive(serde::Deserialize)]
pub struct PublishPostInput {
    #[serde(default)]
    pub title: String,
    pub body: String,
    /// Platform keys as listed by `list_social_accounts` (e.g. "linkedin",
    /// "meta", "instagram", "tiktok", "x", "snapchat").
    pub platforms: Vec<String>,
    /// RFC 3339 timestamp; when set the publish is scheduled instead of
    /// requested immediately.
    #[serde(default)]
    pub scheduled_at: Option<String>,
    /// Optional media references, e.g. [{"type":"image","url":"https://…"}].
    #[serde(default)]
    pub media: Vec<Value>,
}

/// HTTP client for social-core. Cheap to clone.
#[derive(Clone)]
pub struct SocialToolsClient {
    base_url: String,
    internal_api_key: String,
    http: reqwest::Client,
}

impl SocialToolsClient {
    /// Build from the environment: `SOCIAL_CORE_URL` overrides the
    /// host-published compose default; `INTERNAL_API_KEY` authenticates the
    /// cross-plane call. Returns `None` when the key is absent (social-core
    /// rejects keyless callers, so a client without one is useless) or the
    /// reqwest client cannot be built.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("SOCIAL_CORE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_SOCIAL_CORE_URL.to_owned());
        // The fleet uses one consolidated internal key; exec-core's compose
        // historically only sets the integration-corev2 caller variable, so
        // accept it as a fallback (same value) rather than failing closed on
        // a naming difference.
        let internal_api_key = std::env::var("INTERNAL_API_KEY")
            .ok()
            .or_else(|| std::env::var("INTEGRATION_COREV2_INTERNAL_KEY").ok())
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .ok()?;
        Some(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            internal_api_key,
            http,
        })
    }

    /// `list_social_accounts`: the org's connected social accounts with
    /// provider, status, and capabilities — the model's discovery step
    /// before drafting or publishing anything.
    pub async fn list_accounts(&self, org_id: &str) -> Result<String, String> {
        let response = self
            .http
            .get(format!("{}/api/v1/social/accounts", self.base_url))
            .header("x-internal-api-key", &self.internal_api_key)
            .header("x-org-id", org_id)
            .send()
            .await
            .map_err(|e| format!("social-core unreachable: {e}"))?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|e| format!("social-core returned unparseable accounts: {e}"))?;
        if !status.is_success() {
            return Err(format!("social-core accounts failed ({status}): {body}"));
        }
        let accounts = body
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if accounts.is_empty() {
            return Ok(
                "No social accounts are connected for this organization yet. The user can \
                 connect providers (Meta/Facebook/Instagram, LinkedIn, TikTok, X, Snapchat) \
                 under Settings → Integrations; publishing needs at least one connected \
                 account."
                    .to_owned(),
            );
        }
        let mut out = format!("{} connected social account(s):\n", accounts.len());
        for account in accounts {
            let provider = str_field(&account, "provider_key");
            let name = str_field(&account, "display_name");
            let status = str_field(&account, "status");
            let capabilities = account
                .get("capabilities")
                .and_then(Value::as_array)
                .map(|caps| {
                    caps.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "- {provider} ({name}) — status: {status}; capabilities: {capabilities}"
            );
        }
        Ok(out)
    }

    /// `publish_social_post`: create the post in the social workspace
    /// (always approval-required) and request its publish. The result is an
    /// honest state report — created + awaiting workspace approval — never a
    /// claim that content is live.
    pub async fn publish_post(
        &self,
        org_id: &str,
        user_id: &str,
        run_id: &str,
        input: &PublishPostInput,
    ) -> Result<String, String> {
        if input.body.trim().is_empty() {
            return Err("post body must not be empty".to_owned());
        }
        if input.platforms.is_empty() {
            return Err("platforms must name at least one connected provider (call \
                 list_social_accounts first)"
                .to_owned());
        }

        let create_body = serde_json::json!({
            "title": input.title,
            "body": input.body,
            "platforms": input.platforms,
            "media": input.media,
            // Defense in depth (see module docs): agent-created posts are
            // ALWAYS approval-required in the workspace, regardless of the
            // run-level HITL approval that already gated this call.
            "approval_required": true,
            "source": {
                "kind": "agent",
                "label": "Velion agent run",
                "metadata": {"run_id": run_id, "requested_by": user_id},
            },
            "ai_context": {"run_id": run_id, "tool": "publish_social_post"},
            "scheduled_at": input.scheduled_at,
        });
        let response = self
            .http
            .post(format!("{}/api/v1/social/posts", self.base_url))
            .header("x-internal-api-key", &self.internal_api_key)
            .header("x-org-id", org_id)
            .header("x-user-id", user_id)
            .json(&create_body)
            .send()
            .await
            .map_err(|e| format!("social-core unreachable: {e}"))?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|e| format!("social-core returned unparseable post: {e}"))?;
        if !status.is_success() {
            return Err(format!("social-core create post failed ({status}): {body}"));
        }
        let post = body.get("data").cloned().unwrap_or(body);
        let post_id = str_field(&post, "id");
        if post_id.is_empty() {
            return Err(format!("social-core returned a post without an id: {post}"));
        }

        // Request the publish. With approval_required=true this is EXPECTED
        // to be refused until a human approves in the Social workspace —
        // that refusal is the correct, honest outcome, not an error.
        let publish_response = self
            .http
            .post(format!(
                "{}/api/v1/social/posts/{}/publish-jobs",
                self.base_url, post_id
            ))
            .header("x-internal-api-key", &self.internal_api_key)
            .header("x-org-id", org_id)
            .header("x-user-id", user_id)
            .json(&serde_json::json!({
                "idempotency_key": format!("agent:{run_id}:{post_id}"),
            }))
            .send()
            .await
            .map_err(|e| format!("social-core unreachable for publish request: {e}"))?;
        let publish_status = publish_response.status();
        let publish_body: Value = publish_response.json().await.unwrap_or(Value::Null);

        if publish_status.is_success() {
            let job = publish_body.get("data").cloned().unwrap_or(publish_body);
            let job_id = str_field(&job, "id");
            let job_state = str_field(&job, "status");
            return Ok(format!(
                "Social post created (id {post_id}) on platforms {:?} and its publish job \
                 {job_id} is {job_state}. Publishing proceeds through the Social workspace \
                 pipeline; tell the user where to follow it.",
                input.platforms
            ));
        }

        let error_code = publish_body
            .pointer("/error/code")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if error_code == "approval_required" || publish_status.as_u16() == 409 {
            return Ok(format!(
                "Social post created as id {post_id} for platforms {:?} and is AWAITING \
                 WORKSPACE APPROVAL — it will NOT publish until a human approves it under \
                 Social → Approvals. Tell the user the post is drafted and where to approve \
                 it; do not claim it is live.",
                input.platforms
            ));
        }
        Err(format!(
            "post {post_id} was created but the publish request failed \
             ({publish_status}): {publish_body}"
        ))
    }
}

fn str_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_input_parses_minimal_and_full() {
        let minimal: PublishPostInput =
            serde_json::from_str(r#"{"body":"Hei!","platforms":["linkedin"]}"#).expect("minimal");
        assert_eq!(minimal.platforms, vec!["linkedin"]);
        assert!(minimal.scheduled_at.is_none());

        let full: PublishPostInput = serde_json::from_str(
            r#"{"title":"T","body":"B","platforms":["meta","x"],
                "scheduled_at":"2026-08-01T09:00:00Z",
                "media":[{"type":"image","url":"https://example.com/a.png"}]}"#,
        )
        .expect("full");
        assert_eq!(full.platforms.len(), 2);
        assert_eq!(full.media.len(), 1);
        assert!(full.scheduled_at.is_some());
    }

    #[test]
    fn from_env_requires_internal_key() {
        // INTERNAL_API_KEY absent → no client: a keyless caller can never
        // pass social-core's gate, so building one would only defer failure.
        std::env::remove_var("INTERNAL_API_KEY");
        std::env::remove_var("INTEGRATION_COREV2_INTERNAL_KEY");
        assert!(SocialToolsClient::from_env().is_none());
    }
}
