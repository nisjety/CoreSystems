//! Server configuration from the environment. Every capability is optional: a missing setting
//! disables that group of tools (and says so on stderr) instead of failing the whole server.
//! A setting that is present but unsafe or invalid fails startup instead.

use std::path::PathBuf;

use anyhow::bail;

use crate::warehouse::{DatasetPolicy, Limits, DEFAULT_DATASETS};

/// How the person behind the client approves a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalMode {
    /// The server asks the user through MCP elicitation; the model cannot answer for them, and a
    /// client that cannot ask means no writes. Default.
    Elicitation,
    /// The server trusts the client's own tool-permission prompt plus an explicit `user_approved`
    /// argument from the model. Weaker: for clients without elicitation, chosen deliberately.
    PermissionPrompt,
}

/// Whose Visma authority the live reads and the writes use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityMode {
    /// The signed-in person's own tokens when someone has run `vsv2-mcp login`, otherwise the service
    /// credentials. Default.
    Auto,
    /// The signed-in person only. Without a sign-in the live tools are off; there is no quiet fallback to
    /// the broader service identity.
    User,
    /// The service credentials only; a sign-in on this computer is ignored.
    Service,
}

#[derive(Debug, Clone)]
pub struct McpConfig {
    pub endpoint_dir: PathBuf,
    pub tenant_dir: PathBuf,
    /// `VSV2_MCP_BQ_PROJECT`. Unset leaves the warehouse tools out (and `visma_init` says why); the v2
    /// warehouse is [`crate::warehouse::DEFAULT_PROJECT`].
    pub bq_project: Option<String>,
    /// `VSV2_MCP_BQ_DATASETS`, default [`DEFAULT_DATASETS`]: the datasets of that project that are readable.
    pub bq_datasets: DatasetPolicy,
    pub bq_limits: Limits,
    /// Register the write tools. Off unless `VSV2_MCP_ENABLE_WRITES` is 1/true/yes.
    pub enable_writes: bool,
    pub write_approval: ApprovalMode,
    /// Host for SalesOrder V3 reads AND writes (the same host, so what the user reviews is what is hit).
    pub salesorder_base_url: String,
    /// The Finance API host (`VISMA_ERP_REST_BASE_URL`), for the service credentials and a sign-in alike.
    pub erp_base_url: String,
    /// Live-call rate limits of this process (`VSV2_RATELIMIT_ERP_REST_RPM` / `_RPD`), capped per minute at 30.
    pub rest_per_min: u32,
    pub rest_per_day: u32,
    /// The default company (`VSV2_PILOT_TENANT_ID`), Aquatiq AS unless set.
    pub pilot_tenant: String,
    /// `VSV2_DATABASE_URL`: the sync database, for `visma_sync_status` and the write audit log. Optional in
    /// user mode; the service credentials need it anyway.
    pub database_url: Option<String>,
    /// A label recorded in the audit log for writes made through this server. It is self-declared,
    /// not an authenticated identity.
    pub caller: String,
    /// Where `visma_submit_feedback` appends its reports (`VSV2_MCP_FEEDBACK_FILE`).
    pub feedback_file: PathBuf,
    /// `VSV2_MCP_IDENTITY`: `auto` (default), `user` or `service`.
    pub identity: IdentityMode,
    /// True when `VSV2_MCP_CALLER` was set, so a sign-in does not replace the label in the audit log.
    pub caller_is_explicit: bool,
}

fn opt(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Bearer tokens are sent to this host, so it must be a Visma host over TLS. Plain-HTTP loopback
/// is accepted only so tests can point at a local mock. The host is the one a URL parser finds, not the
/// one a suffix check hopes for: `https://evil.example?.visma.net` and `https://evil.example#.visma.net` are not
/// Visma hosts.
pub fn validate_base_url(raw: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    let url =
        url::Url::parse(trimmed).map_err(|_| anyhow::anyhow!("base URL {raw:?} is not a URL"))?;
    if !url.username().is_empty() || url.password().is_some() {
        bail!("base URL {raw:?} must not contain credentials");
    }
    if url.query().is_some() || url.fragment().is_some() || trimmed.contains('\\') {
        bail!("base URL {raw:?} must not have a query, a fragment or a backslash");
    }
    let host = url.host_str().unwrap_or_default();
    let loopback = matches!(host, "127.0.0.1" | "localhost" | "[::1]");
    let visma = host == "visma.net" || host.ends_with(".visma.net");
    match (url.scheme(), visma, loopback) {
        ("https", true, _) | ("http", _, true) | ("https", _, true) => Ok(trimmed.to_string()),
        _ => bail!("base URL {raw:?} must be https on a *.visma.net host (or loopback for tests)"),
    }
}

impl McpConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let _ = dotenvy::dotenv(); // local development convenience; real env always wins
        let mut limits = Limits::default();
        if let Some(b) = opt("VSV2_MCP_BQ_MAX_BYTES").and_then(|s| s.parse().ok()) {
            limits.max_bytes = b;
        }
        if let Some(b) = opt("VSV2_MCP_BQ_MAX_BYTES_PER_HOUR").and_then(|s| s.parse().ok()) {
            limits.max_bytes_per_hour = b;
        }
        let write_approval = match opt("VSV2_MCP_WRITE_APPROVAL").as_deref() {
            None | Some("elicitation") => ApprovalMode::Elicitation,
            Some("permission_prompt") => ApprovalMode::PermissionPrompt,
            Some(other) => bail!("VSV2_MCP_WRITE_APPROVAL must be `elicitation` or `permission_prompt`, got {other:?}"),
        };
        let identity = match opt("VSV2_MCP_IDENTITY").as_deref() {
            None | Some("auto") => IdentityMode::Auto,
            Some("user") => IdentityMode::User,
            Some("service") => IdentityMode::Service,
            Some(other) => {
                bail!("VSV2_MCP_IDENTITY must be `auto`, `user` or `service`, got {other:?}")
            }
        };
        let salesorder_base_url = validate_base_url(
            &opt("VSV2_MCP_SALESORDER_BASE_URL")
                .unwrap_or_else(|| "https://salesorder.visma.net".into()),
        )?;
        let rate =
            |key: &str, default: u32| opt(key).and_then(|v| v.parse().ok()).unwrap_or(default);
        let erp_base_url = validate_base_url(
            &opt("VISMA_ERP_REST_BASE_URL")
                .unwrap_or_else(|| "https://api.finance.visma.net".into()),
        )?;
        Ok(Self {
            endpoint_dir: opt("VSV2_ENDPOINT_CONFIG_DIR")
                .unwrap_or_else(|| "config/endpoints".into())
                .into(),
            tenant_dir: opt("VSV2_TENANT_CONFIG_DIR")
                .unwrap_or_else(|| "config/tenants".into())
                .into(),
            bq_project: opt("VSV2_MCP_BQ_PROJECT"),
            bq_datasets: DatasetPolicy::parse(
                &opt("VSV2_MCP_BQ_DATASETS").unwrap_or_else(|| DEFAULT_DATASETS.into()),
            ),
            bq_limits: limits,
            enable_writes: matches!(
                opt("VSV2_MCP_ENABLE_WRITES").as_deref(),
                Some("1" | "true" | "yes")
            ),
            write_approval,
            salesorder_base_url,
            erp_base_url,
            rest_per_min: rate("VSV2_RATELIMIT_ERP_REST_RPM", 60).min(30),
            rest_per_day: rate("VSV2_RATELIMIT_ERP_REST_RPD", 5_000),
            pilot_tenant: opt("VSV2_PILOT_TENANT_ID")
                .unwrap_or_else(|| vsv2_common::tenant::AQUATIQ_AS_TENANT.into()),
            database_url: opt("VSV2_DATABASE_URL"),
            feedback_file: opt("VSV2_MCP_FEEDBACK_FILE")
                .map(PathBuf::from)
                .unwrap_or_else(|| match std::env::var_os("HOME") {
                    Some(home) if !home.is_empty() => {
                        PathBuf::from(home).join(".vsv2").join("mcp-feedback.jsonl")
                    }
                    _ => PathBuf::from("mcp-feedback.jsonl"),
                }),
            identity,
            caller_is_explicit: opt("VSV2_MCP_CALLER").is_some(),
            caller: opt("VSV2_MCP_CALLER").unwrap_or_else(|| {
                format!(
                    "mcp:{}",
                    std::env::var("USER").unwrap_or_else(|_| "unknown".into())
                )
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_visma_https_hosts_and_loopback_only() {
        for ok in [
            "https://salesorder.visma.net",
            "https://salesorder.visma.net/",
            "https://api.finance.visma.net:443/x",
            "http://127.0.0.1:8080",
            "http://localhost:1234",
            "http://[::1]:9",
        ] {
            assert!(validate_base_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://salesorder.visma.net",       // no TLS
            "https://evil.example",              // not Visma
            "https://visma.net.evil.example",    // suffix trick
            "https://evil.example?.visma.net",   // the real host is before the query
            "https://evil.example#.visma.net",   // ... or before the fragment
            "https://evil.example\\.visma.net",  // ... or before a backslash
            "https://evil.example%2f.visma.net", // an escaped slash is not a host
            "https://salesorder.visma.net?x=1",  // a query has no place in a base URL
            "https://salesorder.visma.net/#frag",
            "https://notvisma.net",                      // not a subdomain
            "https://user:pw@salesorder.visma.net",      // credentials
            "https://salesorder.visma.net@evil.example", // userinfo trick
            "ftp://salesorder.visma.net",
            "salesorder.visma.net",
            "",
        ] {
            assert!(validate_base_url(bad).is_err(), "{bad}");
        }
    }
}
