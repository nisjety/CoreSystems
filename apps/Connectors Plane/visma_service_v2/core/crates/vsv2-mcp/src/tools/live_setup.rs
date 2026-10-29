//! Assembling the live Visma tools (reads, DataMart, writes) for the identity the server runs as.
//!
//! With the service credentials the server reads every configured company and can reach DataMart. As a
//! signed-in person it reads and writes only through that person's own tokens, one per connected company,
//! so Visma applies their roles; DataMart has no per-person sign-in and is left out. Either way each
//! group that cannot be set up is switched off with its reason on record, never half-enabled.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use vsv2_auth::{OAuthClient, Scope, Tokens};
use vsv2_common::config::AppConfig;
use vsv2_common::{load_tenants, Source, TenantConfig};
use vsv2_ratelimit::Limiter;

use super::{register_live, register_writes, sync_status, LiveDeps};
use crate::catalog::Catalog;
use crate::config::{ApprovalMode, McpConfig};
use crate::identity::{self, Resolved, SignedIn};
use crate::status::ServerStatus;
use crate::tool::ToolRegistry;
use crate::visma::{ErpGet, GraphQl, LiveErp, LiveGraphQl, Tenants};

/// How long startup waits for the audit database before leaving the write tools out.
const AUDIT_DB_TIMEOUT: Duration = Duration::from_secs(5);

/// Whose authority the live tools run with, and what that identity brings along.
enum Acting {
    Service(Box<AppConfig>),
    User(Box<SignedIn>),
}

impl Acting {
    fn database_url(&self, cfg: &McpConfig) -> Option<String> {
        match self {
            Self::Service(app) => Some(app.pg_dsn.clone()),
            Self::User(_) => cfg.database_url.clone(),
        }
    }
}

/// Live Visma tools need an identity (the OAuth credentials of the sync service, or a sign-in) and, for the
/// service, the configured tenant list. Anything missing disables that group, with the reason on record.
pub(super) async fn add_visma_tools(
    mut reg: ToolRegistry,
    cfg: &McpConfig,
    catalog: &Catalog,
    status: &mut ServerStatus,
) -> ToolRegistry {
    let Some(acting) = acting_as(cfg, status) else {
        return reg;
    };
    let configured = match configured_tenants(cfg, &acting) {
        Ok(list) => list,
        Err(reason) => {
            status.disabled("live_visma", reason);
            status.disabled("writes", "live Visma access is not configured");
            return reg;
        }
    };
    let tenants = match &acting {
        Acting::Service(_) => Tenants::new(configured, cfg.pilot_tenant.clone()),
        Acting::User(signed_in) => match user_tenants(configured, signed_in, &cfg.pilot_tenant) {
            Ok(tenants) => tenants,
            Err(reason) => {
                status.disabled("live_visma", reason);
                status.disabled("writes", "live Visma access is not configured");
                return reg;
            }
        },
    };

    let limiter = Limiter::new(Source::ErpRest, cfg.rest_per_min, cfg.rest_per_day);
    // The service identity gets read scopes only here, so the read tools can never mint a write token. A signed-in
    // person's token carries whatever they granted; the write tools still need the allow-lists and an approval.
    let (tokens, oauth) = match &acting {
        Acting::Service(app) => {
            let oauth = OAuthClient::new(
                &app.oauth_token_url,
                &app.oauth_client_id,
                &app.oauth_client_secret,
            )
            .with_scope(Scope::Erp, app.oauth_scope_erp.clone())
            .with_scope(Scope::Datamart, app.oauth_scope_datamart.clone())
            .with_scope(Scope::SalesOrder, app.oauth_scope_salesorder.clone());
            (Tokens::from(oauth.clone()), Some(oauth))
        }
        Acting::User(signed_in) => (Tokens::new(signed_in.tokens.clone()), None),
    };
    let erp: Arc<dyn ErpGet> = Arc::new(LiveErp(vsv2_erp_rest::ErpRestClient::new(
        &cfg.erp_base_url,
        tokens.clone(),
        limiter.clone(),
    )));
    // Same SalesOrder host as the write path: what a user reviews is what a write would hit.
    let reader: Arc<dyn vsv2_interactive::Reader> = Arc::new(vsv2_interactive::HttpTransport::new(
        tokens,
        limiter,
        &cfg.erp_base_url,
        &cfg.salesorder_base_url,
    ));
    let gql: Option<Arc<dyn GraphQl>> = match (&acting, oauth) {
        (Acting::Service(app), Some(oauth)) => {
            // Be a polite neighbour of the scheduler: it shares Visma's quota but not this process's bucket.
            let dm_limiter = Limiter::new(
                Source::DatamartGraphql,
                app.rate_dm_per_min.min(30),
                app.rate_dm_per_day,
            );
            Some(Arc::new(LiveGraphQl(vsv2_datamart::DatamartClient::new(
                &app.datamart_graphql_url,
                oauth,
                dm_limiter,
            ))))
        }
        _ => {
            status.disabled(
                "datamart",
                "DataMart has no per-person sign-in: it is available with the service credentials only (VSV2_MCP_IDENTITY=service)",
            );
            None
        }
    };

    let sync = match acting.database_url(cfg) {
        Some(dsn) => match sync_status::PgSyncStatus::connect_lazy(&dsn) {
            Ok(pg) => {
                status.enabled(
                    "sync_status",
                    json!({"source": "sync state database (read-only session)"}),
                );
                Some(Arc::new(pg) as Arc<dyn sync_status::SyncStatusSource>)
            }
            Err(e) => {
                status.disabled("sync_status", e.to_string());
                None
            }
        },
        None => {
            status.disabled("sync_status", "VSV2_DATABASE_URL is not set");
            None
        }
    };
    let datamart = gql.is_some();
    reg = register_live(
        reg,
        LiveDeps {
            catalog: catalog.clone(),
            tenants: tenants.clone(),
            erp,
            reader,
            gql,
            sync,
        },
    );
    announce(status, cfg, &acting, &tenants, datamart);

    if cfg.enable_writes {
        reg = add_write_tools(reg, cfg, &acting, &tenants, status).await;
    } else {
        status.disabled(
            "writes",
            "VSV2_MCP_ENABLE_WRITES is not set (read-only server)",
        );
    }
    reg
}

/// Decide whose authority to use and record it. `None` leaves the live tools out, with the reason.
fn acting_as(cfg: &McpConfig, status: &mut ServerStatus) -> Option<Acting> {
    match identity::resolve(cfg.identity) {
        Resolved::Unusable(reason) => {
            status.disabled("identity", reason.clone());
            status.disabled("live_visma", reason);
            status.disabled("writes", "live Visma access is not configured");
            None
        }
        Resolved::User(signed_in) => Some(Acting::User(signed_in)),
        Resolved::Service => match AppConfig::from_env() {
            Ok(app) => Some(Acting::Service(Box::new(app))),
            Err(e) => {
                status.disabled("live_visma", format!("credentials/configuration: {e}"));
                status.disabled("writes", "live Visma access is not configured");
                None
            }
        },
    }
}

/// The tenant files. The service needs them; a signed-in person only borrows their names.
fn configured_tenants(cfg: &McpConfig, acting: &Acting) -> Result<Vec<TenantConfig>, String> {
    match (load_tenants(&cfg.tenant_dir), acting) {
        (Ok(list), Acting::User(_)) => Ok(list),
        (Ok(list), Acting::Service(_)) if !list.is_empty() => Ok(list),
        (Ok(_), Acting::Service(_)) => Err(format!("no tenants in {}", cfg.tenant_dir.display())),
        (Err(_), Acting::User(_)) => Ok(Vec::new()),
        (Err(e), Acting::Service(_)) => {
            Err(format!("tenants from {}: {e}", cfg.tenant_dir.display()))
        }
    }
}

fn user_tenants(
    configured: Vec<TenantConfig>,
    signed_in: &SignedIn,
    pilot: &str,
) -> Result<Tenants, String> {
    let companies = signed_in
        .tokens
        .companies()
        .map_err(|e| format!("the sign-in cannot be read: {e}"))?;
    if companies.is_empty() {
        return Err("signed in, but no company is connected: run `vsv2-mcp login`".into());
    }
    let connected: Vec<(String, Option<String>)> = companies
        .into_iter()
        .map(|c| (c.tenant_id, c.name))
        .collect();
    Ok(Tenants::for_sign_in(configured, &connected, pilot))
}

/// The `identity` and `live_visma` capabilities, and the sentence `visma_init` gives about whose authority applies.
fn announce(
    status: &mut ServerStatus,
    cfg: &McpConfig,
    acting: &Acting,
    tenants: &Tenants,
    datamart: bool,
) {
    let default_tenant = tenants
        .describe()
        .into_iter()
        .find(|t| t["is_default"] == true)
        .map(|t| json!({"tenant_id": t["tenant_id"], "name": t["name"]}));
    let name_of = |id: &str| {
        tenants
            .describe()
            .into_iter()
            .find(|t| t["tenant_id"] == id)
            .and_then(|t| t["name"].as_str().map(str::to_string))
            .filter(|name| name != id)
    };
    match acting {
        Acting::Service(_) => status.enabled("identity", json!({"mode": "service"})),
        Acting::User(signed_in) => {
            status.enabled("identity", signed_in.capability(&name_of));
            status.set_acts_as(signed_in.acts_as());
        }
    }
    // The sales-order tools exist either way; a sign-in without the read scope answers them with that scope named.
    let salesorder_v3 = match acting {
        Acting::Service(_) => true,
        Acting::User(signed_in) => signed_in.tokens.any_company_has_scope(Scope::SalesOrder),
    };
    if !salesorder_v3 {
        status.note(
            "sales-order reads (visma_salesorder_get, sales_orders_v3) will be refused: the sign-in lacks visma.net.erp.salesorder:read. Add it to VISMA_USER_SCOPES and sign in again",
        );
    }
    status.enabled(
        "live_visma",
        json!({
            "identity": match acting { Acting::Service(_) => "service", Acting::User(_) => "user" },
            "tenants": tenants.describe().len(),
            "default_tenant": default_tenant,
            "erp_rest": true,
            "salesorder_v3": salesorder_v3,
            "datamart_graphql": datamart,
            "rate_limit_per_minute": cfg.rest_per_min,
            "read_only": true,
        }),
    );
}

/// The audit database must be reachable and migrated *now*: a write tool that fails on its first
/// call because the audit table is missing is worse than no write tool.
async fn audit_db_ready(dsn: &str) -> Result<sqlx::PgPool, String> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(AUDIT_DB_TIMEOUT)
        .connect_lazy(dsn)
        .map_err(|e| e.to_string())?;
    let found: Option<String> = tokio::time::timeout(
        AUDIT_DB_TIMEOUT,
        sqlx::query_scalar("SELECT to_regclass('vsv2.write_audit')::text").fetch_one(&pool),
    )
    .await
    .map_err(|_| format!("timed out after {}s connecting", AUDIT_DB_TIMEOUT.as_secs()))?
    .map_err(|e| e.to_string())?;
    found.map(|_| pool).ok_or_else(|| {
        "table vsv2.write_audit not found (apply migrations/0012_write_audit.sql)".into()
    })
}

/// The write tokens: the service's own client with the write scope (a separate client, so a read cache and a
/// write cache never mix), or the signed-in person's tokens, which Visma limits to what they may do.
fn write_tokens(acting: &Acting, status: &mut ServerStatus) -> Option<(Tokens, String)> {
    match acting {
        Acting::Service(app) => {
            let Some(write_scope) = app.oauth_scope_salesorder_write.clone() else {
                status.disabled("writes", "VISMA_OAUTH_SCOPE_SALESORDER_WRITE is not set");
                return None;
            };
            let oauth = OAuthClient::new(
                &app.oauth_token_url,
                &app.oauth_client_id,
                &app.oauth_client_secret,
            )
            .with_scope(Scope::SalesOrderWrite, write_scope)
            .with_scope(Scope::SalesOrder, app.oauth_scope_salesorder.clone());
            Some((Tokens::from(oauth), "service credentials".into()))
        }
        Acting::User(signed_in) => {
            if !signed_in
                .tokens
                .any_company_has_scope(Scope::SalesOrderWrite)
            {
                status.disabled(
                    "writes",
                    "your sign-in has no SalesOrders write scope: run `vsv2-mcp login --write` (the scopes are VISMA_USER_SCOPES_WRITE), and the application must be allowed to use them",
                );
                return None;
            }
            Some((
                Tokens::new(signed_in.tokens.clone()),
                format!("{} (own sign-in)", signed_in.person()),
            ))
        }
    }
}

/// Write tools are opt-in and need everything a write depends on. Each missing piece is named in
/// the status, and nothing is half-enabled.
async fn add_write_tools(
    mut reg: ToolRegistry,
    cfg: &McpConfig,
    acting: &Acting,
    tenants: &Tenants,
    status: &mut ServerStatus,
) -> ToolRegistry {
    use vsv2_interactive as l3;

    let Some((tokens, authority)) = write_tokens(acting, status) else {
        return reg;
    };
    let signer = match l3::ConfirmSigner::from_env() {
        Ok(s) => s,
        Err(e) => {
            status.disabled("writes", e.to_string());
            return reg;
        }
    };
    let Some(dsn) = acting.database_url(cfg) else {
        status.disabled(
            "writes",
            "VSV2_DATABASE_URL is not set (the write audit log lives in the sync database)",
        );
        return reg;
    };
    let pool = match audit_db_ready(&dsn).await {
        Ok(p) => p,
        Err(e) => {
            status.disabled("writes", format!("audit database not ready: {e}"));
            return reg;
        }
    };
    let policy = l3::WritePolicy::from_env();
    let (allowed_tenants, allowed_operations) = (policy.tenants(), policy.operations());

    let transport = Arc::new(l3::HttpTransport::new(
        tokens,
        Limiter::new(Source::ErpRest, 20, 2_000),
        &cfg.erp_base_url,
        &cfg.salesorder_base_url,
    ));
    let executor = Arc::new(l3::Executor::new(
        l3::default_registry(),
        policy,
        signer,
        Arc::new(l3::PgAuditSink::new(pool)),
        transport.clone(),
        transport,
    ));
    // In the audit log a signed-in person's writes carry who they are, unless the operator chose a label.
    let caller = match acting {
        Acting::User(signed_in) if !cfg.caller_is_explicit => {
            format!("visma-user:{}", signed_in.audit_label())
        }
        _ => cfg.caller.clone(),
    };
    reg = register_writes(reg, executor, tenants.clone(), caller, cfg.write_approval);
    let tenant_names: Vec<String> = tenants
        .describe()
        .into_iter()
        .filter(|t| allowed_tenants.iter().any(|a| t["tenant_id"] == a.as_str()))
        .filter_map(|t| t["name"].as_str().map(String::from))
        .collect();
    status.enabled(
        "writes",
        json!({
            "approval_mode": format!("{:?}", cfg.write_approval),
            "authority": authority,
            "operations_allowed": allowed_operations,
            "tenants_allowed": tenant_names,
            "note": "default deny: a write needs its tenant AND its operation allow-listed, and the user approves each change",
        }),
    );
    if allowed_tenants.is_empty() || allowed_operations.is_empty() {
        status.note("writes are registered but every write will be denied: the tenant/operation allow-list is empty");
    }
    if cfg.write_approval == ApprovalMode::PermissionPrompt {
        status.note(
            "WARNING: VSV2_MCP_WRITE_APPROVAL=permission_prompt: writes rely on the client's tool-permission \
             prompt, not on a server-side approval of the exact change",
        );
    }
    reg
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use vsv2_signin::{Connection, Session, SignInConfig, Store, UserTokens};

    const READ: &str = "vismanet_erp_interactive_api:read";
    const SO_READ: &str = "visma.net.erp.salesorder:read";
    const SO_WRITE: &str = "visma.net.erp.salesorder:write";

    /// Somebody signed in with `scopes` for the companies `ids`.
    fn signed_in(dir: &tempfile::TempDir, ids: &[&str], scopes: &[&str]) -> SignedIn {
        let config = SignInConfig::from_lookup(|key| match key {
            "VISMA_USER_CLIENT_ID" => Some("web-app".into()),
            "VISMA_USER_CLIENT_SECRET" => Some("s3cret".into()),
            "VSV2_USER_STORE_DIR" => Some(dir.path().join("user").display().to_string()),
            _ => None,
        })
        .unwrap();
        let store = Store::new(config.store_dir.clone());
        store
            .save_session(&Session::new(
                "user-1".into(),
                Some("ima@example.com".into()),
                "web-app".into(),
            ))
            .unwrap();
        for id in ids {
            let mut connection = Connection::new(
                (*id).into(),
                "user-1".into(),
                scopes.iter().map(|s| s.to_string()).collect(),
                "refresh".into(),
                "access".into(),
                Utc::now() + chrono::Duration::hours(1),
            );
            connection.name = Some(format!("Name of {id}"));
            store.save_connection(&connection).unwrap();
        }
        SignedIn {
            tokens: UserTokens::open(&config).unwrap(),
            config,
        }
    }

    fn configured(id: &str, name: &str) -> TenantConfig {
        serde_json::from_value(json!({"tenant_id": id, "tenant_name": name})).unwrap()
    }

    fn disabled_reason(status: &ServerStatus, capability: &str) -> String {
        status.capabilities_json()[capability]["reason"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn a_person_without_a_write_scope_gets_no_write_tools_and_is_told_how_to_get_them() {
        let dir = tempfile::tempdir().unwrap();
        let acting = Acting::User(Box::new(signed_in(&dir, &["t-1"], &[READ, SO_READ])));
        let mut status = ServerStatus::default();
        assert!(write_tokens(&acting, &mut status).is_none());
        let why = disabled_reason(&status, "writes");
        assert!(
            why.contains("login --write") && why.contains("VISMA_USER_SCOPES_WRITE"),
            "{why}"
        );
    }

    #[test]
    fn a_person_with_the_write_scope_writes_with_their_own_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let acting = Acting::User(Box::new(signed_in(
            &dir,
            &["t-1"],
            &[READ, SO_READ, SO_WRITE],
        )));
        let mut status = ServerStatus::default();
        let (_, authority) = write_tokens(&acting, &mut status).expect("write tokens");
        assert!(
            authority.contains("ima@example.com") && authority.contains("own sign-in"),
            "{authority}"
        );
        assert!(status.notes.is_empty());
    }

    #[test]
    fn a_sign_in_with_no_company_cannot_serve_live_calls() {
        let dir = tempfile::tempdir().unwrap();
        let none = signed_in(&dir, &[], &[READ]);
        let why = user_tenants(Vec::new(), &none, "t-aq").expect_err("no company");
        assert!(why.contains("no company is connected"), "{why}");
    }

    #[test]
    fn the_companies_of_a_sign_in_come_with_the_names_visma_gave_them() {
        let dir = tempfile::tempdir().unwrap();
        let person = signed_in(&dir, &["t-1", "t-2"], &[READ]);
        let tenants = user_tenants(vec![configured("t-1", "Aquatiq AS")], &person, "t-1").unwrap();
        let names: Vec<String> = tenants
            .describe()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            ["Aquatiq AS", "Name of t-2"],
            "a configured name wins, an unconfigured company keeps Visma's"
        );
        assert_eq!(tenants.resolve(None).unwrap().as_str(), "t-1");
    }

    #[test]
    fn a_missing_tenant_directory_stops_the_service_but_not_a_person() {
        let dir = tempfile::tempdir().unwrap();
        let person = Acting::User(Box::new(signed_in(&dir, &["t-1"], &[READ])));
        let mut cfg = test_config();
        cfg.tenant_dir = dir.path().join("no-such-tenants");
        assert_eq!(configured_tenants(&cfg, &person).unwrap().len(), 0);
    }

    /// An `McpConfig` for these tests, built from an empty environment.
    fn test_config() -> McpConfig {
        McpConfig {
            endpoint_dir: "config/endpoints".into(),
            tenant_dir: "config/tenants".into(),
            bq_project: None,
            bq_datasets: crate::warehouse::DatasetPolicy::parse(crate::warehouse::DEFAULT_DATASETS),
            bq_limits: crate::warehouse::Limits::default(),
            enable_writes: false,
            write_approval: ApprovalMode::Elicitation,
            salesorder_base_url: "https://salesorder.visma.net".into(),
            erp_base_url: "https://api.finance.visma.net".into(),
            rest_per_min: 30,
            rest_per_day: 5_000,
            pilot_tenant: "t-aq".into(),
            database_url: None,
            caller: "mcp:test".into(),
            caller_is_explicit: false,
            feedback_file: "feedback.jsonl".into(),
            identity: crate::config::IdentityMode::Auto,
        }
    }
}
