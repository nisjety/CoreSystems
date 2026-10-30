//! Signing out: tell Visma the refresh tokens are no longer wanted, then forget them.

use crate::error::Result;
use crate::exchange::Exchanger;
use crate::store::Store;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogoutReport {
    /// Companies forgotten on this computer.
    pub removed: usize,
    /// Refresh tokens Visma confirmed as revoked.
    pub revoked: usize,
    /// Refresh tokens Visma could not be asked to revoke (offline, or refused). They are forgotten here anyway.
    pub revoke_failed: usize,
    /// Nobody is signed in any more.
    pub signed_out: bool,
}

/// Forget one company (`only`) or all of them. Revoking is best effort and needs an [`Exchanger`]; without
/// one the tokens are only forgotten locally. When no company is left the person is forgotten too.
pub async fn logout(
    store: &Store,
    exchanger: Option<&Exchanger>,
    only: Option<&str>,
) -> Result<LogoutReport> {
    let mut report = LogoutReport::default();
    for connection in store.connections()? {
        if only.is_some_and(|wanted| wanted != connection.tenant_id) {
            continue;
        }
        if let Some(exchanger) = exchanger {
            match exchanger.revoke(&connection.refresh_token).await {
                Ok(()) => report.revoked += 1,
                Err(e) => {
                    tracing::warn!(tenant = %connection.tenant_id, "could not revoke the refresh token: {e}");
                    report.revoke_failed += 1;
                }
            }
        }
        report.removed += usize::from(store.remove_connection(&connection.tenant_id)?);
    }
    if store.connections()?.is_empty() {
        store.clear()?;
        report.signed_out = true;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Connection, Session};
    use chrono::Utc;

    fn store_with(ids: &[&str]) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("user"));
        store
            .save_session(&Session::new("u".into(), None, "web-app".into()))
            .unwrap();
        for id in ids {
            store
                .save_connection(&Connection::new(
                    (*id).into(),
                    "u".into(),
                    vec![],
                    format!("refresh-{id}"),
                    "a".into(),
                    Utc::now() + chrono::Duration::hours(1),
                ))
                .unwrap();
        }
        (dir, store)
    }

    fn exchanger(url: &str) -> Exchanger {
        let mut cfg = crate::testutil::config();
        cfg.revocation_url = format!("{url}/revoke");
        Exchanger::new(&cfg)
    }

    #[tokio::test]
    async fn everything_is_revoked_at_visma_and_forgotten_here() {
        let mut s = mockito::Server::new_async().await;
        let revoke = s
            .mock("POST", "/revoke")
            .with_status(200)
            .expect(2)
            .create_async()
            .await;
        let (_dir, store) = store_with(&["t-a", "t-b"]);
        let report = logout(&store, Some(&exchanger(&s.url())), None)
            .await
            .unwrap();
        revoke.assert_async().await;
        assert_eq!(
            report,
            LogoutReport {
                removed: 2,
                revoked: 2,
                revoke_failed: 0,
                signed_out: true
            }
        );
        assert!(store.session().unwrap().is_none() && store.connections().unwrap().is_empty());
    }

    #[tokio::test]
    async fn one_company_can_be_forgotten_and_the_person_stays_signed_in_for_the_rest() {
        let (_dir, store) = store_with(&["t-a", "t-b"]);
        let report = logout(&store, None, Some("t-a")).await.unwrap();
        assert_eq!(
            report,
            LogoutReport {
                removed: 1,
                revoked: 0,
                revoke_failed: 0,
                signed_out: false
            }
        );
        assert!(store.session().unwrap().is_some());
        assert_eq!(store.connections().unwrap().len(), 1);
        let last = logout(&store, None, Some("t-b")).await.unwrap();
        assert!(last.signed_out && store.session().unwrap().is_none());
    }

    #[tokio::test]
    async fn a_revocation_that_fails_does_not_keep_the_tokens_on_disk() {
        let mut s = mockito::Server::new_async().await;
        let _revoke = s
            .mock("POST", "/revoke")
            .with_status(500)
            .create_async()
            .await;
        let (_dir, store) = store_with(&["t-a"]);
        let report = logout(&store, Some(&exchanger(&s.url())), None)
            .await
            .unwrap();
        assert_eq!(
            (report.revoked, report.revoke_failed, report.removed),
            (0, 1, 1)
        );
        assert!(store.connections().unwrap().is_empty());
    }

    #[tokio::test]
    async fn signing_out_of_nothing_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let report = logout(&Store::new(dir.path().join("none")), None, None)
            .await
            .unwrap();
        assert_eq!(report.removed, 0);
        assert!(report.signed_out);
    }
}
