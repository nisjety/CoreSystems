//! The signed-in person's tokens as a [`TokenSource`], so the Visma HTTP clients can act as that person.
//!
//! A token is looked up per company. A fresh one is returned as it is; an expired one is refreshed under a
//! per-company lock, and the new tokens are written to disk BEFORE the access token is handed out, because
//! Visma may rotate the refresh token and a rotated token that was never saved would end the sign-in.
//! Another process refreshing the same company at the same moment is tolerated: when Visma rejects the
//! refresh token, the store is read again and whatever the other process saved is used.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;
use vsv2_auth::{Scope, TokenSource};
use vsv2_common::TenantId;

use crate::config::SignInConfig;
use crate::error::{Result, SignInError};
use crate::exchange::{Exchanger, Grant};
use crate::store::{Connection, Session, Store};

/// A token must have this many seconds left to be used as it is.
const FRESH_MARGIN_SECS: i64 = 60;
/// How long to wait before looking at the store a second time after Visma rejected a refresh token: another
/// process that rotated it saves the new tokens right after Visma answers it.
const REREAD_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

/// The scope a token must carry for each kind of call. Only the Finance (interactive) and SalesOrders APIs
/// have a per-person sign-in; DataMart and the expense API do not.
fn default_required() -> HashMap<Scope, String> {
    [
        (Scope::Erp, "vismanet_erp_interactive_api:read"),
        (Scope::ErpWrite, "vismanet_erp_interactive_api:create"),
        (Scope::SalesOrder, "visma.net.erp.salesorder:read"),
        (Scope::SalesOrderWrite, "visma.net.erp.salesorder:write"),
    ]
    .into_iter()
    .map(|(scope, wire)| (scope, wire.to_string()))
    .collect()
}

/// One connected company, without any token.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Company {
    pub tenant_id: String,
    /// The company's own name as Visma gave it when it was connected, if it could be read.
    pub name: Option<String>,
    pub scopes: Vec<String>,
    pub connected_at: DateTime<Utc>,
    pub access_expires_at: DateTime<Utc>,
}

impl From<Connection> for Company {
    fn from(c: Connection) -> Self {
        Self {
            tenant_id: c.tenant_id,
            name: c.name,
            scopes: c.scopes,
            connected_at: c.connected_at,
            access_expires_at: c.access_expires_at,
        }
    }
}

/// Who is signed in and for which companies, read from disk with no network and no client registration:
/// what `whoami` shows.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub session: Session,
    pub companies: Vec<Company>,
}

pub fn snapshot(store: &Store) -> Result<Option<Snapshot>> {
    let Some(session) = store.session()? else {
        return Ok(None);
    };
    let companies = store
        .connections()?
        .into_iter()
        .map(Company::from)
        .collect();
    Ok(Some(Snapshot { session, companies }))
}

#[derive(Clone)]
pub struct UserTokens {
    inner: Arc<Inner>,
}

struct Inner {
    session: Session,
    store: Store,
    exchanger: Exchanger,
    required: HashMap<Scope, String>,
    locks: StdMutex<HashMap<String, Arc<Mutex<()>>>>,
    /// Refreshed tokens that could not be written to disk. Visma may already have retired the old refresh token,
    /// so these are kept for as long as this server runs, and the disk is tried again on the next use.
    unsaved: StdMutex<HashMap<String, Connection>>,
}

impl std::fmt::Debug for UserTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserTokens")
            .field("user", &self.inner.session.display())
            .finish_non_exhaustive()
    }
}

impl UserTokens {
    /// The tokens of whoever is signed in on this computer. Fails when nobody is, or when the sign-in
    /// belongs to another application than the one configured (its refresh tokens would be refused).
    pub fn open(cfg: &SignInConfig) -> Result<Self> {
        let store = Store::new(cfg.store_dir.clone());
        let session = store.session()?.ok_or(SignInError::NotSignedIn)?;
        if session.client_id != cfg.client_id {
            return Err(SignInError::Config(
                "the sign-in on this computer was made with another application (client id): run `vsv2-mcp logout`, then `vsv2-mcp login`".into(),
            ));
        }
        Ok(Self {
            inner: Arc::new(Inner {
                session,
                store,
                exchanger: Exchanger::new(cfg),
                required: default_required(),
                locks: StdMutex::new(HashMap::new()),
                unsaved: StdMutex::new(HashMap::new()),
            }),
        })
    }

    pub fn session(&self) -> &Session {
        &self.inner.session
    }

    /// Every connected company with its scopes and when its access token runs out. No tokens.
    pub fn companies(&self) -> Result<Vec<Company>> {
        Ok(self
            .inner
            .store
            .connections()?
            .into_iter()
            .map(Company::from)
            .collect())
    }

    /// Whether any connected company has this scope.
    pub fn any_company_has_scope(&self, scope: Scope) -> bool {
        let Some(wire) = self.inner.required.get(&scope) else {
            return false;
        };
        self.inner
            .store
            .connections()
            .is_ok_and(|all| all.iter().any(|c| c.has_scope(wire)))
    }

    /// Force a refresh for `tenant` and report whether Visma accepts the sign-in: for `whoami --check`.
    pub async fn verify(&self, tenant: &str) -> Result<()> {
        let lock = self.lock_for(tenant);
        let _guard = lock.lock().await;
        let connection = self.load(tenant)?;
        self.refresh(connection).await.map(|_| ())
    }

    fn lock_for(&self, tenant: &str) -> Arc<Mutex<()>> {
        let mut locks = self.inner.locks.lock().expect("lock map is never poisoned");
        locks.entry(tenant.to_string()).or_default().clone()
    }

    /// The stored connection for `tenant`, if it belongs to the person this server was started for. After a
    /// logout and a login as somebody else, the files on disk are the new person's: they are refused rather than
    /// used under the old person's name.
    fn load(&self, tenant: &str) -> Result<Connection> {
        let held = self
            .inner
            .unsaved
            .lock()
            .expect("unsaved lock")
            .get(tenant)
            .cloned();
        if let Some(held) = held {
            if self.inner.store.save_connection(&held).is_ok() {
                self.inner
                    .unsaved
                    .lock()
                    .expect("unsaved lock")
                    .remove(tenant);
            }
            return Ok(held);
        }
        let connection = self
            .inner
            .store
            .connection(tenant)?
            .ok_or_else(|| SignInError::NotConnected(tenant.to_string()))?;
        if connection.sub != self.inner.session.sub {
            return Err(SignInError::SignInChanged);
        }
        Ok(connection)
    }

    async fn token(&self, tenant: &str, scope: Scope) -> Result<String> {
        let wire = self.inner.required.get(&scope).ok_or_else(|| {
            SignInError::Unsupported(format!(
                "{scope:?} has no per-person sign-in: use the service identity for it"
            ))
        })?;
        let connection = self.load(tenant)?;
        if !connection.has_scope(wire) {
            return Err(SignInError::MissingScope {
                tenant: tenant.to_string(),
                scope: wire.clone(),
                hint: "ask for it in VISMA_USER_SCOPES (or VISMA_USER_SCOPES_WRITE and `login --write`), make sure the application may use it, and sign in again".into(),
            });
        }
        if connection.access_is_fresh(FRESH_MARGIN_SECS) {
            return Ok(connection.access_token);
        }
        let lock = self.lock_for(tenant);
        let _guard = lock.lock().await;
        // Another task, or another process, may have refreshed while this one waited.
        let connection = self.load(tenant)?;
        if connection.access_is_fresh(FRESH_MARGIN_SECS) {
            return Ok(connection.access_token);
        }
        self.refresh(connection).await
    }

    async fn refresh(&self, connection: Connection) -> Result<String> {
        let tenant = connection.tenant_id.clone();
        let used = connection.refresh_token.clone();
        match self.inner.exchanger.refresh(&used).await {
            Ok(grant) => self.keep(connection, grant),
            Err(SignInError::InvalidGrant) => self.after_rejection(&tenant, &used).await,
            Err(other) => Err(other),
        }
    }

    /// Visma rejected the refresh token `used`. Either it really is gone, or another process rotated it a
    /// moment ago and saved the new tokens: look again (once more after a short wait) and use what it saved.
    /// A refresh token that is still the one that was used is rejected, however fresh the access token is,
    /// because the sign-in is over as soon as that access token runs out.
    async fn after_rejection(&self, tenant: &str, used: &str) -> Result<String> {
        for attempt in 0..2 {
            if attempt > 0 {
                tokio::time::sleep(REREAD_DELAY).await;
            }
            let latest = self.load(tenant)?;
            if latest.refresh_token == used {
                continue;
            }
            if latest.access_is_fresh(FRESH_MARGIN_SECS) {
                return Ok(latest.access_token);
            }
            let grant = self.inner.exchanger.refresh(&latest.refresh_token).await?;
            return self.keep(latest, grant);
        }
        Err(SignInError::InvalidGrant)
    }

    /// Save the new tokens, then return the access token.
    fn keep(&self, mut connection: Connection, grant: Grant) -> Result<String> {
        connection.access_token = grant.access_token.clone();
        connection.access_expires_at = Utc::now() + chrono::Duration::seconds(grant.expires_in);
        if let Some(rotated) = grant.refresh_token {
            connection.refresh_token = rotated;
        }
        if let Some(scope) = grant
            .scope
            .as_deref()
            .map(|s| s.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        {
            if !scope.is_empty() {
                connection.scopes = scope;
            }
        }
        match self.inner.store.save_connection(&connection) {
            Ok(()) => {
                self.inner
                    .unsaved
                    .lock()
                    .expect("unsaved lock")
                    .remove(&connection.tenant_id);
            }
            Err(e) => {
                tracing::error!(tenant = %connection.tenant_id, "could not save refreshed tokens, keeping them in memory: {e}");
                self.inner
                    .unsaved
                    .lock()
                    .expect("unsaved lock")
                    .insert(connection.tenant_id.clone(), connection);
            }
        }
        Ok(grant.access_token)
    }

    /// Mark the stored access token as used up, so the next request refreshes.
    async fn expire(&self, tenant: &str) -> Result<()> {
        let lock = self.lock_for(tenant);
        let _guard = lock.lock().await;
        let mut connection = self.load(tenant)?;
        connection.access_expires_at = Utc::now() - chrono::Duration::seconds(1);
        self.inner.store.save_connection(&connection)
    }
}

#[async_trait]
impl TokenSource for UserTokens {
    async fn token_for(&self, tenant: &TenantId, scope: Scope) -> vsv2_common::Result<String> {
        self.token(tenant.as_str(), scope).await.map_err(Into::into)
    }

    async fn forget_token(&self, tenant: &TenantId, _scope: Scope) {
        if let Err(e) = self.expire(tenant.as_str()).await {
            tracing::warn!(tenant = %tenant, "could not mark the token as expired: {e}");
        }
    }

    fn token_depends_on_scope(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;

    const TENANT: &str = "t-1";
    const READ: &str = "vismanet_erp_interactive_api:read";

    struct Rig {
        _dir: tempfile::TempDir,
        server: mockito::ServerGuard,
        cfg: SignInConfig,
    }

    async fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let server = mockito::Server::new_async().await;
        let mut cfg = crate::testutil::config();
        cfg.store_dir = dir.path().join("user");
        cfg.token_url = format!("{}/token", server.url());
        Rig {
            _dir: dir,
            server,
            cfg,
        }
    }

    fn connection(
        access: &str,
        refresh: &str,
        expires_in_secs: i64,
        scopes: &[&str],
    ) -> Connection {
        Connection::new(
            TENANT.into(),
            "user-1".into(),
            scopes.iter().map(|s| s.to_string()).collect(),
            refresh.into(),
            access.into(),
            Utc::now() + chrono::Duration::seconds(expires_in_secs),
        )
    }

    fn sign_in(cfg: &SignInConfig, connection: &Connection) -> UserTokens {
        let store = Store::new(cfg.store_dir.clone());
        store
            .save_session(&Session::new(
                "user-1".into(),
                Some("Ima".into()),
                cfg.client_id.clone(),
            ))
            .unwrap();
        store.save_connection(connection).unwrap();
        UserTokens::open(cfg).unwrap()
    }

    fn stored(cfg: &SignInConfig) -> Connection {
        Store::new(cfg.store_dir.clone())
            .connection(TENANT)
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn a_fresh_token_is_returned_without_asking_visma() {
        let mut r = rig().await;
        let never = r
            .server
            .mock("POST", "/token")
            .expect(0)
            .create_async()
            .await;
        let tokens = sign_in(&r.cfg, &connection("fresh", "r1", 3600, &[READ]));
        assert_eq!(
            tokens
                .token_for(&TenantId::new(TENANT), Scope::Erp)
                .await
                .unwrap(),
            "fresh"
        );
        never.assert_async().await;
    }

    #[tokio::test]
    async fn an_expired_token_is_refreshed_and_the_rotated_refresh_token_is_saved_first() {
        let mut r = rig().await;
        let refresh = r
            .server
            .mock("POST", "/token")
            .match_body(Matcher::AllOf(vec![
                Matcher::UrlEncoded("grant_type".into(), "refresh_token".into()),
                Matcher::UrlEncoded("refresh_token".into(), "r1".into()),
            ]))
            .with_body(r#"{"access_token":"new-access","refresh_token":"r2","expires_in":3600}"#)
            .expect(1)
            .create_async()
            .await;
        let tokens = sign_in(&r.cfg, &connection("old", "r1", -10, &[READ]));
        assert_eq!(
            tokens
                .token_for(&TenantId::new(TENANT), Scope::Erp)
                .await
                .unwrap(),
            "new-access"
        );
        let saved = stored(&r.cfg);
        assert_eq!(
            (saved.access_token.as_str(), saved.refresh_token.as_str()),
            ("new-access", "r2")
        );
        // The second call uses what was saved.
        assert_eq!(
            tokens
                .token_for(&TenantId::new(TENANT), Scope::Erp)
                .await
                .unwrap(),
            "new-access"
        );
        refresh.assert_async().await;
    }

    #[tokio::test]
    async fn callers_that_arrive_together_cause_one_refresh() {
        let mut r = rig().await;
        let refresh = r
            .server
            .mock("POST", "/token")
            .with_body(r#"{"access_token":"new-access","refresh_token":"r2","expires_in":3600}"#)
            .expect(1)
            .create_async()
            .await;
        let tokens = sign_in(&r.cfg, &connection("old", "r1", -10, &[READ]));
        let calls: Vec<_> = (0..6)
            .map(|_| {
                let tokens = tokens.clone();
                tokio::spawn(
                    async move { tokens.token_for(&TenantId::new(TENANT), Scope::Erp).await },
                )
            })
            .collect();
        for call in calls {
            assert_eq!(call.await.unwrap().unwrap(), "new-access");
        }
        refresh.assert_async().await;
    }

    #[tokio::test]
    async fn a_refresh_token_another_process_just_rotated_is_not_a_failure() {
        let mut r = rig().await;
        let store = Store::new(r.cfg.store_dir.clone());
        // While Visma answers "invalid_grant", the other process has saved fresh tokens.
        let _refresh = r
            .server
            .mock("POST", "/token")
            .with_status(400)
            .with_body_from_request(move |_| {
                store
                    .save_connection(&connection(
                        "saved-by-the-other-process",
                        "r2",
                        3600,
                        &[READ],
                    ))
                    .unwrap();
                br#"{"error":"invalid_grant"}"#.to_vec()
            })
            .create_async()
            .await;
        let tokens = sign_in(&r.cfg, &connection("old", "r1", -10, &[READ]));
        assert_eq!(
            tokens
                .token_for(&TenantId::new(TENANT), Scope::Erp)
                .await
                .unwrap(),
            "saved-by-the-other-process"
        );
    }

    #[tokio::test]
    async fn a_refresh_token_that_is_really_gone_says_to_sign_in_again() {
        let mut r = rig().await;
        let _refresh = r
            .server
            .mock("POST", "/token")
            .with_status(400)
            .with_body(r#"{"error":"invalid_grant"}"#)
            .create_async()
            .await;
        let tokens = sign_in(&r.cfg, &connection("old", "r1", -10, &[READ]));
        let e = tokens
            .token_for(&TenantId::new(TENANT), Scope::Erp)
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("vsv2-mcp login"), "{e}");
    }

    #[tokio::test]
    async fn verifying_reports_a_revoked_refresh_token_even_while_the_access_token_is_still_fresh()
    {
        let mut r = rig().await;
        let _revoked = r
            .server
            .mock("POST", "/token")
            .with_status(400)
            .with_body(r#"{"error":"invalid_grant"}"#)
            .create_async()
            .await;
        let tokens = sign_in(&r.cfg, &connection("still-fresh", "r1", 3600, &[READ]));
        assert!(matches!(
            tokens.verify(TENANT).await,
            Err(SignInError::InvalidGrant)
        ));
        // Ordinary use is unaffected until the access token runs out.
        assert_eq!(
            tokens
                .token_for(&TenantId::new(TENANT), Scope::Erp)
                .await
                .unwrap(),
            "still-fresh"
        );
    }

    #[tokio::test]
    async fn tokens_that_cannot_be_saved_are_kept_in_memory_until_the_disk_works_again() {
        let mut r = rig().await;
        let store = Store::new(r.cfg.store_dir.clone());
        let blocked = r
            .cfg
            .store_dir
            .join("connections")
            .join(format!("{TENANT}.json"));
        let in_the_way = blocked.clone();
        // While Visma answers, the file the new tokens go to is replaced by a directory, so the save fails.
        let refresh = r
            .server
            .mock("POST", "/token")
            .with_body_from_request(move |_| {
                std::fs::remove_file(&in_the_way).unwrap();
                std::fs::create_dir(&in_the_way).unwrap();
                br#"{"access_token":"new-access","refresh_token":"r2","expires_in":3600}"#.to_vec()
            })
            .expect(1)
            .create_async()
            .await;
        let tokens = sign_in(&r.cfg, &connection("old", "r1", -10, &[READ]));
        let tenant = TenantId::new(TENANT);
        assert_eq!(
            tokens.token_for(&tenant, Scope::Erp).await.unwrap(),
            "new-access"
        );
        // The next call uses what is held; Visma is not asked again, and the rotated refresh token is not lost.
        assert_eq!(
            tokens.token_for(&tenant, Scope::Erp).await.unwrap(),
            "new-access"
        );
        refresh.assert_async().await;
        // The disk comes back: the held tokens are written out on the next use.
        std::fs::remove_dir(&blocked).unwrap();
        assert_eq!(
            tokens.token_for(&tenant, Scope::Erp).await.unwrap(),
            "new-access"
        );
        let saved = store.connection(TENANT).unwrap().unwrap();
        assert_eq!(
            (saved.access_token.as_str(), saved.refresh_token.as_str()),
            ("new-access", "r2")
        );
    }

    #[tokio::test]
    async fn forgetting_a_token_makes_the_next_call_refresh() {
        let mut r = rig().await;
        let refresh = r
            .server
            .mock("POST", "/token")
            .with_body(r#"{"access_token":"after-401","expires_in":3600}"#)
            .expect(1)
            .create_async()
            .await;
        let tokens = sign_in(&r.cfg, &connection("rejected", "r1", 3600, &[READ]));
        let tenant = TenantId::new(TENANT);
        tokens.forget_token(&tenant, Scope::Erp).await;
        assert_eq!(
            tokens.token_for(&tenant, Scope::Erp).await.unwrap(),
            "after-401"
        );
        assert_eq!(
            stored(&r.cfg).refresh_token,
            "r1",
            "an answer without a new refresh token keeps the old one"
        );
        refresh.assert_async().await;
    }

    #[tokio::test]
    async fn a_scope_the_sign_in_lacks_is_named_before_visma_is_asked() {
        let r = rig().await;
        let tokens = sign_in(&r.cfg, &connection("a", "r1", 3600, &[READ]));
        let tenant = TenantId::new(TENANT);
        let e = tokens
            .token_for(&tenant, Scope::SalesOrderWrite)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("visma.net.erp.salesorder:write") && e.contains("VISMA_USER_SCOPES"),
            "{e}"
        );
        assert!(tokens.token_for(&tenant, Scope::Erp).await.is_ok());
    }

    #[tokio::test]
    async fn apis_without_a_person_sign_in_and_unknown_companies_are_refused() {
        let r = rig().await;
        let tokens = sign_in(&r.cfg, &connection("a", "r1", 3600, &[READ]));
        let e = tokens
            .token_for(&TenantId::new(TENANT), Scope::Datamart)
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("no per-person sign-in"), "{e}");
        let e = tokens
            .token_for(&TenantId::new("t-other"), Scope::Erp)
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("t-other") && e.contains("vsv2-mcp login"), "{e}");
        assert!(tokens
            .token_for(&TenantId::new("../x"), Scope::Erp)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn opening_needs_a_sign_in_made_with_the_same_application() {
        let r = rig().await;
        assert!(matches!(
            UserTokens::open(&r.cfg),
            Err(SignInError::NotSignedIn)
        ));
        let store = Store::new(r.cfg.store_dir.clone());
        store
            .save_session(&Session::new("u".into(), None, "another-app".into()))
            .unwrap();
        let e = UserTokens::open(&r.cfg).unwrap_err().to_string();
        assert!(e.contains("another application"), "{e}");
    }

    #[tokio::test]
    async fn tokens_of_somebody_else_are_refused_when_the_sign_in_changed_under_a_running_server() {
        let r = rig().await;
        let tokens = sign_in(&r.cfg, &connection("alice-access", "r1", 3600, &[READ]));
        assert_eq!(
            tokens
                .token_for(&TenantId::new(TENANT), Scope::Erp)
                .await
                .unwrap(),
            "alice-access"
        );
        // While the server runs, Alice logs out and Bob signs in: the files on disk are now Bob's.
        let store = Store::new(r.cfg.store_dir.clone());
        let mut bobs = connection("bobs-access", "r-bob", 3600, &[READ]);
        bobs.sub = "user-2".into();
        store.save_connection(&bobs).unwrap();
        let e = tokens
            .token_for(&TenantId::new(TENANT), Scope::Erp)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("changed while this server was running") && e.contains("restart"),
            "{e}"
        );
        assert!(tokens.verify(TENANT).await.is_err());
    }

    #[tokio::test]
    async fn the_person_and_the_companies_are_listed_without_tokens() {
        let r = rig().await;
        let tokens = sign_in(
            &r.cfg,
            &connection("secret-access", "secret-refresh", 3600, &[READ]),
        );
        assert_eq!(tokens.session().display(), "Ima");
        let companies = tokens.companies().unwrap();
        assert_eq!(companies.len(), 1);
        let shown = serde_json::to_string(&companies).unwrap();
        assert!(
            shown.contains(TENANT) && !shown.contains("secret"),
            "{shown}"
        );
        assert!(tokens.any_company_has_scope(Scope::Erp));
        assert!(!tokens.any_company_has_scope(Scope::SalesOrderWrite));
        assert!(!tokens.token_depends_on_scope());
    }

    #[tokio::test]
    async fn verifying_refreshes_even_a_fresh_token_and_reports_a_dead_one() {
        let mut r = rig().await;
        let ok = r
            .server
            .mock("POST", "/token")
            .with_body(r#"{"access_token":"checked","expires_in":3600}"#)
            .expect(1)
            .create_async()
            .await;
        let tokens = sign_in(&r.cfg, &connection("fresh", "r1", 3600, &[READ]));
        tokens.verify(TENANT).await.unwrap();
        ok.assert_async().await;
        assert!(matches!(
            tokens.verify("t-other").await,
            Err(SignInError::NotConnected(_))
        ));
    }
}
