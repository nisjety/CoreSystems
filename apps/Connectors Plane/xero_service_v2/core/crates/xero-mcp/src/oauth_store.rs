//! Durable, single-worker OAuth state for the Xero-to-MCP authorization bridge.

use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

const PENDING_SECONDS: i64 = 600;
const CODE_SECONDS: i64 = 300;
const ACCESS_SECONDS: i64 = 1800;
const LOGIN_SECONDS: i64 = 86400;

#[derive(Clone, Serialize, Deserialize)]
pub struct AuthorizationRequest {
    pub client_id: String,
    #[serde(default)]
    pub resource: String,
    pub redirect_uri: String,
    pub client_state: String,
    pub code_challenge: String,
    pub upstream_verifier: String,
    pub nonce: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PendingAuthorization(pub AuthorizationRequest);

#[derive(Clone, Serialize, Deserialize)]
struct CodeGrant {
    request: AuthorizationRequest,
    user_sub: String,
    tenant_ids: Vec<String>,
    #[serde(default)]
    tenant_bindings: Vec<ConnectionBinding>,
    not_after: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionBinding {
    pub tenant_id: String,
    pub connected_at: DateTime<Utc>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AccessGrant {
    pub client_id: String,
    #[serde(default)]
    pub resource: String,
    pub user_sub: String,
    pub tenant_ids: Vec<String>,
    #[serde(default)]
    pub tenant_bindings: Vec<ConnectionBinding>,
    not_after: i64,
}

pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
}

#[derive(Clone)]
pub struct OAuthStore(Arc<Mutex<Connection>>);

fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn opaque() -> String {
    xero_auth::random_state()
}

fn now() -> i64 {
    Utc::now().timestamp()
}

fn valid_verifier(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

fn bindings_match(tenant_ids: &[String], bindings: &[ConnectionBinding]) -> bool {
    !bindings.is_empty()
        && tenant_ids.len() == bindings.len()
        && tenant_ids
            .iter()
            .zip(bindings)
            .all(|(tenant_id, binding)| tenant_id == &binding.tenant_id)
}

impl OAuthStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            if !path.exists() {
                let _ = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(path)?;
            }
            let metadata = std::fs::symlink_metadata(path)?;
            anyhow::ensure!(
                metadata.file_type().is_file() && metadata.permissions().mode() & 0o077 == 0,
                "MCP OAuth database must be a private regular file"
            );
        }
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(3))?;
        connection.pragma_update(None, "secure_delete", "ON")?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch("\
            CREATE TABLE IF NOT EXISTS pending (key BLOB PRIMARY KEY, payload TEXT NOT NULL, expires_at INTEGER NOT NULL, approved INTEGER NOT NULL DEFAULT 0);\
            CREATE TABLE IF NOT EXISTS codes (key BLOB PRIMARY KEY, payload TEXT NOT NULL, expires_at INTEGER NOT NULL);\
            CREATE TABLE IF NOT EXISTS tokens (key BLOB PRIMARY KEY, kind TEXT NOT NULL, payload TEXT NOT NULL, expires_at INTEGER NOT NULL);\
            CREATE INDEX IF NOT EXISTS tokens_expiry ON tokens(expires_at);\
            CREATE TABLE IF NOT EXISTS used_refresh (key BLOB PRIMARY KEY, family_id BLOB NOT NULL, client_id TEXT NOT NULL, expires_at INTEGER NOT NULL);\
            CREATE INDEX IF NOT EXISTS used_refresh_expiry ON used_refresh(expires_at);\
        ")?;
        let has_family_id: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('tokens') WHERE name = 'family_id')",
            [],
            |row| row.get(0),
        )?;
        if !has_family_id {
            tx.execute("ALTER TABLE tokens ADD COLUMN family_id BLOB", [])?;
        }
        tx.execute(
            "CREATE INDEX IF NOT EXISTS tokens_family ON tokens(family_id)",
            [],
        )?;
        // Legacy access tokens cannot be assigned to a refresh family reliably.
        tx.execute(
            "DELETE FROM tokens WHERE kind = 'access' AND family_id IS NULL",
            [],
        )?;
        tx.execute("DELETE FROM used_refresh WHERE expires_at <= ?1", [now()])?;
        tx.commit()?;
        Ok(Self(Arc::new(Mutex::new(connection))))
    }

    fn connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.0
            .lock()
            .map_err(|_| anyhow::anyhow!("OAuth store lock poisoned"))
    }

    pub fn begin(&self, request: AuthorizationRequest) -> Result<String> {
        anyhow::ensure!(
            !request.client_id.is_empty()
                && !request.resource.is_empty()
                && !request.redirect_uri.is_empty()
                && !request.client_state.is_empty()
                && request.client_state.len() <= 512
                && request.code_challenge.len() == 43
                && request
                    .code_challenge
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
                && !request.upstream_verifier.is_empty()
                && !request.nonce.is_empty(),
            "invalid authorization request"
        );
        let state = opaque();
        let connection = self.connection()?;
        connection.execute("DELETE FROM pending WHERE expires_at <= ?1", [now()])?;
        connection.execute(
            "INSERT INTO pending (key, payload, expires_at) VALUES (?1, ?2, ?3)",
            params![
                digest(&state).as_slice(),
                serde_json::to_string(&request)?,
                now() + PENDING_SECONDS
            ],
        )?;
        Ok(state)
    }

    pub fn finish(&self, state: &str) -> Result<Option<PendingAuthorization>> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(String, i64)> = tx
            .query_row(
                "SELECT payload, expires_at FROM pending WHERE key = ?1 AND approved = 1",
                [digest(state).as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if row.is_some() {
            tx.execute(
                "DELETE FROM pending WHERE key = ?1",
                [digest(state).as_slice()],
            )?;
        }
        tx.commit()?;
        row.filter(|(_, expires_at)| *expires_at > now())
            .map(|(payload, _)| serde_json::from_str(&payload).map(PendingAuthorization))
            .transpose()
            .context("decode pending authorization")
    }

    pub fn approve(&self, state: &str) -> Result<Option<PendingAuthorization>> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(String, i64)> = tx
            .query_row(
                "SELECT payload, expires_at FROM pending WHERE key = ?1",
                [digest(state).as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((payload, _)) = row.filter(|(_, expiry)| *expiry > now()) else {
            return Ok(None);
        };
        tx.execute(
            "UPDATE pending SET approved = 1 WHERE key = ?1",
            [digest(state).as_slice()],
        )?;
        tx.commit()?;
        Ok(Some(PendingAuthorization(serde_json::from_str(&payload)?)))
    }

    pub fn cancel(&self, state: &str) -> Result<Option<PendingAuthorization>> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(String, i64)> = tx
            .query_row(
                "SELECT payload, expires_at FROM pending WHERE key = ?1",
                [digest(state).as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        tx.execute(
            "DELETE FROM pending WHERE key = ?1",
            [digest(state).as_slice()],
        )?;
        tx.commit()?;
        row.filter(|(_, expiry)| *expiry > now())
            .map(|(payload, _)| serde_json::from_str(&payload).map(PendingAuthorization))
            .transpose()
            .context("decode pending authorization")
    }

    pub fn issue_code(
        &self,
        pending: PendingAuthorization,
        user_sub: &str,
        tenant_bindings: &[ConnectionBinding],
    ) -> Result<String> {
        anyhow::ensure!(
            !user_sub.is_empty() && user_sub.len() <= 256 && !tenant_bindings.is_empty(),
            "verified user and connected tenant required"
        );
        let mut normalized = tenant_bindings
            .iter()
            .map(|binding| {
                Uuid::parse_str(&binding.tenant_id).map(|id| ConnectionBinding {
                    tenant_id: id.to_string(),
                    connected_at: binding.connected_at,
                })
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        normalized.sort_by(|a, b| a.tenant_id.cmp(&b.tenant_id));
        anyhow::ensure!(
            normalized
                .windows(2)
                .all(|pair| pair[0].tenant_id != pair[1].tenant_id),
            "duplicate connected tenant"
        );
        let tenant_ids = normalized
            .iter()
            .map(|binding| binding.tenant_id.clone())
            .collect();
        let code = opaque();
        let grant = CodeGrant {
            request: pending.0,
            user_sub: user_sub.into(),
            tenant_ids,
            tenant_bindings: normalized,
            not_after: now() + LOGIN_SECONDS,
        };
        self.connection()?.execute(
            "INSERT INTO codes (key, payload, expires_at) VALUES (?1, ?2, ?3)",
            params![
                digest(&code).as_slice(),
                serde_json::to_string(&grant)?,
                now() + CODE_SECONDS
            ],
        )?;
        Ok(code)
    }

    pub fn exchange_code(
        &self,
        code: &str,
        client_id: &str,
        redirect_uri: &str,
        verifier: &str,
    ) -> Result<Option<TokenPair>> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(String, i64)> = tx
            .query_row(
                "SELECT payload, expires_at FROM codes WHERE key = ?1",
                [digest(code).as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        tx.execute(
            "DELETE FROM codes WHERE key = ?1",
            [digest(code).as_slice()],
        )?;
        let Some((payload, _)) = row.filter(|(_, expires_at)| *expires_at > now()) else {
            tx.commit()?;
            return Ok(None);
        };
        let grant: CodeGrant = serde_json::from_str(&payload)?;
        let expected = xero_auth::PkceChallenge::challenge_for(verifier);
        if grant.request.client_id != client_id
            || grant.request.resource.is_empty()
            || grant.request.redirect_uri != redirect_uri
            || !valid_verifier(verifier)
            || !bindings_match(&grant.tenant_ids, &grant.tenant_bindings)
            || !bool::from(
                expected
                    .as_bytes()
                    .ct_eq(grant.request.code_challenge.as_bytes()),
            )
        {
            tx.commit()?;
            return Ok(None);
        }
        let tokens = Self::mint(
            &tx,
            AccessGrant {
                client_id: grant.request.client_id,
                resource: grant.request.resource,
                user_sub: grant.user_sub,
                tenant_ids: grant.tenant_ids,
                tenant_bindings: grant.tenant_bindings,
                not_after: grant.not_after,
            },
            &digest(&opaque()),
        )?;
        tx.commit()?;
        Ok(Some(tokens))
    }

    pub fn refresh(&self, refresh_token: &str, client_id: &str) -> Result<Option<TokenPair>> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = now();
        tx.execute("DELETE FROM used_refresh WHERE expires_at <= ?1", [current])?;
        let key = digest(refresh_token);
        let row: Option<(String, i64, Option<Vec<u8>>)> = tx
            .query_row(
                "SELECT payload, expires_at, family_id FROM tokens WHERE key = ?1 AND kind = 'refresh'",
                [key.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((payload, _, family_id)) = row.filter(|(_, expires_at, _)| *expires_at > current)
        else {
            let used: Option<(Vec<u8>, String)> = tx
                .query_row(
                    "SELECT family_id, client_id FROM used_refresh WHERE key = ?1 AND expires_at > ?2",
                    params![key.as_slice(), current],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            if let Some((family_id, original_client_id)) = used {
                if original_client_id == client_id {
                    tx.execute("DELETE FROM tokens WHERE family_id = ?1", [family_id])?;
                }
            }
            tx.commit()?;
            return Ok(None);
        };
        let grant: AccessGrant = serde_json::from_str(&payload)?;
        if grant.client_id != client_id
            || grant.resource.is_empty()
            || grant.not_after <= current
            || !bindings_match(&grant.tenant_ids, &grant.tenant_bindings)
        {
            tx.commit()?;
            return Ok(None);
        }
        let family_id = family_id.unwrap_or_else(|| digest(&opaque()).to_vec());
        tx.execute(
            "DELETE FROM tokens WHERE key = ?1 AND kind = 'refresh'",
            [key.as_slice()],
        )?;
        tx.execute(
            "INSERT INTO used_refresh (key, family_id, client_id, expires_at) VALUES (?1, ?2, ?3, ?4)",
            params![key.as_slice(), &family_id, client_id, grant.not_after],
        )?;
        let tokens = Self::mint(&tx, grant, &family_id)?;
        tx.commit()?;
        Ok(Some(tokens))
    }

    pub fn access(&self, access_token: &str) -> Result<Option<AccessGrant>> {
        let row: Option<(String, i64)> = self
            .connection()?
            .query_row(
                "SELECT payload, expires_at FROM tokens WHERE key = ?1 AND kind = 'access'",
                [digest(access_token).as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.filter(|(_, expires_at)| *expires_at > now())
            .map(|(payload, _)| serde_json::from_str::<AccessGrant>(&payload))
            .transpose()
            .map(|grant| {
                grant.filter(|grant| {
                    !grant.resource.is_empty()
                        && bindings_match(&grant.tenant_ids, &grant.tenant_bindings)
                })
            })
            .context("decode MCP access grant")
    }

    fn mint(
        tx: &rusqlite::Transaction<'_>,
        grant: AccessGrant,
        family_id: &[u8],
    ) -> Result<TokenPair> {
        let current = now();
        let access_expires_at = (current + ACCESS_SECONDS).min(grant.not_after);
        anyhow::ensure!(access_expires_at > current, "authorization expired");
        let access_token = opaque();
        let refresh_token = opaque();
        let payload = serde_json::to_string(&grant)?;
        tx.execute(
            "INSERT INTO tokens (key, kind, payload, expires_at, family_id) VALUES (?1, 'access', ?2, ?3, ?4)",
            params![digest(&access_token).as_slice(), payload, access_expires_at, family_id],
        )?;
        tx.execute(
            "INSERT INTO tokens (key, kind, payload, expires_at, family_id) VALUES (?1, 'refresh', ?2, ?3, ?4)",
            params![digest(&refresh_token).as_slice(), payload, grant.not_after, family_id],
        )?;
        tx.execute("DELETE FROM tokens WHERE expires_at <= ?1", [current])?;
        tx.execute("DELETE FROM used_refresh WHERE expires_at <= ?1", [current])?;
        Ok(TokenPair {
            access_token,
            refresh_token,
            expires_in: access_expires_at - current,
        })
    }
}
