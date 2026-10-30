use crate::backend::Failure;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, net::SocketAddr, path::PathBuf};
use subtle::ConstantTimeEq;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessScope {
    pub client_id: String,
    pub tenant_ids: Vec<String>,
    #[serde(default)]
    pub allow_extract: bool,
    #[serde(default)]
    pub allow_sensitive: bool,
    #[serde(default)]
    pub allow_warehouse: bool,
    #[serde(default)]
    pub zero_data_retention: bool,
    #[serde(default)]
    pub write_actions: Vec<String>,
}

impl AccessScope {
    pub fn new(
        client_id: &str,
        tenant_ids: Vec<String>,
        allow_extract: bool,
        allow_sensitive: bool,
        zero_data_retention: bool,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !client_id.is_empty()
                && client_id.len() <= 100
                && client_id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')),
            "invalid MCP client identity"
        );
        anyhow::ensure!(!tenant_ids.is_empty(), "MCP tenant allowlist is required");
        let mut normalized = tenant_ids
            .iter()
            .map(|id| uuid::Uuid::parse_str(id).map(|id| id.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        normalized.sort();
        normalized.dedup();
        Ok(Self {
            client_id: client_id.into(),
            tenant_ids: normalized,
            allow_extract,
            allow_sensitive,
            allow_warehouse: false,
            zero_data_retention,
            write_actions: Vec::new(),
        })
    }

    pub fn with_write_actions(mut self, actions: Vec<String>) -> anyhow::Result<Self> {
        anyhow::ensure!(
            actions
                .iter()
                .all(|action| crate::writes::ACTIONS.contains(&action.as_str()))
                && actions
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    == actions.len(),
            "unknown or duplicate MCP write action"
        );
        self.write_actions = actions;
        Ok(self)
    }

    pub fn authorize_write(&self, tenant: &str, action: &str) -> Result<(), Failure> {
        self.authorize(tenant, false, false)?;
        if self.zero_data_retention {
            return Err(Failure::new("retention_policy", "Writes require a private approval and audit record, which this client's retention policy forbids.", false));
        }
        if !self.write_actions.iter().any(|allowed| allowed == action) {
            return Err(Failure::new(
                "write_forbidden",
                "This MCP client cannot perform the selected Xero write action.",
                false,
            ));
        }
        Ok(())
    }

    pub fn authorize(&self, tenant: &str, sensitive: bool, extract: bool) -> Result<(), Failure> {
        let tenant = uuid::Uuid::parse_str(tenant)
            .map_err(|_| Failure::invalid("tenant_id must be a UUID"))?
            .to_string();
        if !self.tenant_ids.contains(&tenant) {
            return Err(Failure::new(
                "tenant_forbidden",
                "This client cannot access the selected Xero tenant.",
                false,
            ));
        }
        if sensitive && !self.allow_sensitive {
            return Err(Failure::new(
                "sensitive_access_required",
                "This operation requires the client's sensitive-data permission.",
                false,
            ));
        }
        if extract && (!self.allow_extract || self.zero_data_retention) {
            return Err(Failure::new(
                "extraction_forbidden",
                "Extraction is disabled by this client's access or retention policy.",
                false,
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct ClientCredential {
    digest: [u8; 32],
    pub scope: AccessScope,
}

impl ClientCredential {
    pub fn new(token: &str, scope: AccessScope) -> anyhow::Result<Self> {
        anyhow::ensure!(
            token.len() >= 32 && token.len() <= 1024 && !token.chars().any(char::is_control),
            "MCP service tokens must contain 32..1024 characters"
        );
        Ok(Self {
            digest: Sha256::digest(token.as_bytes()).into(),
            scope,
        })
    }
    pub fn matches(&self, token: &str) -> bool {
        let candidate: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        bool::from(self.digest.ct_eq(&candidate))
    }
}

#[derive(Clone)]
pub struct Settings {
    pub bind: SocketAddr,
    pub local_scope: AccessScope,
    pub clients: Vec<ClientCredential>,
    pub storage_root: PathBuf,
    pub allowed_hosts: Vec<String>,
    pub allowed_origins: Vec<String>,
    pub compact: bool,
    pub writes_enabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfiguredClient {
    token: String,
    #[serde(flatten)]
    scope: AccessScope,
}

impl Settings {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_map(&std::env::vars().collect())
    }

    pub fn from_map(vars: &HashMap<String, String>) -> anyhow::Result<Self> {
        let flag = |name: &str| -> anyhow::Result<bool> {
            vars.get(name)
                .map(|s| {
                    s.parse::<bool>()
                        .map_err(|_| anyhow::anyhow!("{name} must be true or false"))
                })
                .unwrap_or(Ok(false))
        };
        let list = |name: &str| {
            vars.get(name)
                .map(|s| {
                    s.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        let local_tenants = list("XERO_MCP_TENANTS");
        let local_scope = if local_tenants.is_empty()
            && (vars.contains_key("XERO_MCP_OAUTH_ISSUER")
                || vars.contains_key("XERO_MCP_CLIENTS_JSON"))
        {
            AccessScope {
                client_id: "local".into(),
                tenant_ids: Vec::new(),
                allow_extract: false,
                allow_sensitive: false,
                allow_warehouse: false,
                zero_data_retention: true,
                write_actions: Vec::new(),
            }
        } else {
            AccessScope::new(
                "local",
                local_tenants,
                flag("XERO_MCP_ALLOW_EXTRACT")?,
                flag("XERO_MCP_ALLOW_SENSITIVE")?,
                flag("XERO_MCP_ZERO_DATA_RETENTION")?,
            )?
        };
        let local_scope = AccessScope {
            allow_warehouse: !local_scope.tenant_ids.is_empty()
                && flag("XERO_MCP_ALLOW_WAREHOUSE")?,
            ..local_scope
        }
        .with_write_actions(list("XERO_MCP_WRITE_ACTIONS"))?;
        let clients = if let Some(json) = vars.get("XERO_MCP_CLIENTS_JSON") {
            let configured: Vec<ConfiguredClient> = serde_json::from_str(json)
                .map_err(|_| anyhow::anyhow!("invalid XERO_MCP_CLIENTS_JSON"))?;
            configured
                .into_iter()
                .map(|c| {
                    let s = c.scope;
                    let scope = AccessScope {
                        allow_warehouse: s.allow_warehouse,
                        ..AccessScope::new(
                            &s.client_id,
                            s.tenant_ids,
                            s.allow_extract,
                            s.allow_sensitive,
                            s.zero_data_retention,
                        )?
                    }
                    .with_write_actions(s.write_actions)?;
                    ClientCredential::new(&c.token, scope)
                })
                .collect::<anyhow::Result<Vec<_>>>()?
        } else if let Some(token) = vars.get("XERO_MCP_TOKEN").filter(|token| !token.is_empty()) {
            anyhow::ensure!(
                !local_scope.tenant_ids.is_empty(),
                "XERO_MCP_TOKEN requires XERO_MCP_TENANTS"
            );
            vec![ClientCredential::new(
                token,
                AccessScope {
                    client_id: "model-plane".into(),
                    write_actions: Vec::new(),
                    ..local_scope.clone()
                },
            )?]
        } else {
            Vec::new()
        };
        for (i, client) in clients.iter().enumerate() {
            anyhow::ensure!(
                !clients[..i]
                    .iter()
                    .any(|other| other.digest == client.digest
                        || other.scope.client_id == client.scope.client_id),
                "MCP clients require distinct tokens and identities"
            );
        }
        let bind: SocketAddr = vars
            .get("XERO_MCP_BIND")
            .map(String::as_str)
            .unwrap_or("127.0.0.1:5003")
            .parse()?;
        let tls_proxy = flag("XERO_MCP_TLS_PROXY")?;
        anyhow::ensure!(bind.ip().is_loopback() || tls_proxy, "Non-loopback MCP HTTP requires XERO_MCP_TLS_PROXY=true and an enforced TLS proxy/network boundary");
        Ok(Self {
            bind,
            local_scope,
            clients,
            storage_root: vars
                .get("XERO_MCP_STORAGE_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("./out/mcp")),
            allowed_hosts: {
                let hosts = list("XERO_MCP_ALLOWED_HOSTS");
                if hosts.is_empty() {
                    vec!["localhost".into(), "127.0.0.1".into(), "::1".into()]
                } else {
                    hosts
                }
            },
            allowed_origins: list("XERO_MCP_ALLOWED_ORIGINS"),
            compact: match vars
                .get("XERO_MCP_TOOL_MODE")
                .map(String::as_str)
                .unwrap_or("full")
            {
                "full" => false,
                "compact" => true,
                _ => anyhow::bail!("XERO_MCP_TOOL_MODE must be full or compact"),
            },
            writes_enabled: flag("XERO_MCP_ENABLE_WRITES")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fails_closed_on_missing_tenants_bad_policy_and_weak_tokens() {
        assert!(Settings::from_map(&HashMap::new()).is_err());
        let tenant = "5f3c9a2e-0000-0000-0000-000000000000";
        let mut vars = HashMap::from([("XERO_MCP_TENANTS".into(), tenant.into())]);
        assert!(Settings::from_map(&vars).is_ok());
        vars.insert("XERO_MCP_ALLOW_EXTRACT".into(), "yes".into());
        assert!(Settings::from_map(&vars).is_err());
        let scope = AccessScope::new("test", vec![tenant.into()], false, false, false).unwrap();
        assert!(ClientCredential::new("short", scope.clone()).is_err());
        let credential = ClientCredential::new(&"a".repeat(32), scope).unwrap();
        assert!(credential.matches(&"a".repeat(32)));
        assert!(!credential.matches(&"b".repeat(32)));
    }
    #[test]
    fn remote_http_binds_require_explicit_trusted_tls_boundary() {
        let mut vars = HashMap::from([
            (
                "XERO_MCP_TENANTS".into(),
                "5f3c9a2e-0000-0000-0000-000000000000".into(),
            ),
            ("XERO_MCP_BIND".into(), "0.0.0.0:5003".into()),
        ]);
        assert!(Settings::from_map(&vars).is_err());
        vars.insert("XERO_MCP_TLS_PROXY".into(), "true".into());
        assert!(Settings::from_map(&vars).is_ok());
    }
    #[test]
    fn oauth_only_http_needs_no_bootstrap_tenant() {
        let vars = HashMap::from([(
            "XERO_MCP_OAUTH_ISSUER".into(),
            "https://tools.example.com".into(),
        )]);
        let settings = Settings::from_map(&vars).unwrap();
        assert!(settings.local_scope.tenant_ids.is_empty());
        assert!(settings.clients.is_empty());
    }
    #[test]
    fn writes_are_disabled_by_default_and_never_granted_to_static_fallback() {
        let tenant = "5f3c9a2e-0000-0000-0000-000000000000";
        let mut vars = HashMap::from([
            ("XERO_MCP_TENANTS".into(), tenant.into()),
            (
                "XERO_MCP_TOKEN".into(),
                "test-only-token-with-at-least-32-characters".into(),
            ),
            ("XERO_MCP_WRITE_ACTIONS".into(), "create_contact".into()),
        ]);
        let settings = Settings::from_map(&vars).unwrap();
        assert!(!settings.writes_enabled);
        assert_eq!(settings.local_scope.write_actions, ["create_contact"]);
        assert!(settings.clients[0].scope.write_actions.is_empty());
        vars.insert("XERO_MCP_ENABLE_WRITES".into(), "true".into());
        let settings = Settings::from_map(&vars).unwrap();
        assert!(settings.writes_enabled);
        assert!(settings.clients[0].scope.write_actions.is_empty());
        vars.insert(
            "XERO_MCP_WRITE_ACTIONS".into(),
            "create_payment,delete_everything".into(),
        );
        assert!(Settings::from_map(&vars).is_err());
    }
    #[test]
    fn configured_client_write_grants_are_action_scoped_and_retention_checked() {
        let tenant = "5f3c9a2e-0000-0000-0000-000000000000";
        let vars = HashMap::from([
            ("XERO_MCP_TENANTS".into(), tenant.into()),
            ("XERO_MCP_ENABLE_WRITES".into(), "true".into()),
            (
                "XERO_MCP_CLIENTS_JSON".into(),
                format!(
                    r#"[{{"token":"test-only-token-with-at-least-32-characters","client_id":"writer","tenant_ids":["{tenant}"],"write_actions":["create_contact"]}}]"#
                ),
            ),
        ]);
        let settings = Settings::from_map(&vars).unwrap();
        let scope = &settings.clients[0].scope;
        assert!(scope.authorize_write(tenant, "create_contact").is_ok());
        assert_eq!(
            scope
                .authorize_write(tenant, "create_payment")
                .unwrap_err()
                .code,
            "write_forbidden"
        );
        let zdr = AccessScope {
            zero_data_retention: true,
            ..scope.clone()
        };
        assert_eq!(
            zdr.authorize_write(tenant, "create_contact")
                .unwrap_err()
                .code,
            "retention_policy"
        );
    }
    #[test]
    fn independent_http_client_needs_no_local_tenant_allowlist() {
        let tenant = "5f3c9a2e-0000-0000-0000-000000000000";
        let vars = HashMap::from([(
            "XERO_MCP_CLIENTS_JSON".into(),
            format!(
                r#"[{{"token":"test-only-token-with-at-least-32-characters","client_id":"writer","tenant_ids":["{tenant}"],"write_actions":["create_contact"]}}]"#
            ),
        )]);
        let settings = Settings::from_map(&vars).unwrap();
        assert!(settings.local_scope.tenant_ids.is_empty());
        assert_eq!(settings.clients[0].scope.tenant_ids, [tenant]);
    }
}
