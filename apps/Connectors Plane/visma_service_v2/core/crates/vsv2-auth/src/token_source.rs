//! Where a bearer token comes from. The sync service asks Visma for a token with its own client
//! credentials ([`OAuthClient`]); a person who signed in has tokens of their own (`vsv2-signin`). The HTTP
//! clients do not care which: they hold a [`Tokens`].

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use vsv2_common::{Result, TenantId};

use crate::{OAuthClient, Scope};

#[async_trait]
pub trait TokenSource: Send + Sync + fmt::Debug {
    /// A bearer token that is valid now for `tenant`, good for the API behind `scope`.
    async fn token_for(&self, tenant: &TenantId, scope: Scope) -> Result<String>;

    /// Drop what is cached for `tenant` and `scope` after the API refused the token, so the next
    /// [`token_for`](Self::token_for) asks for a new one.
    async fn forget_token(&self, tenant: &TenantId, scope: Scope);

    /// Whether the scope decides which token is issued. True for client credentials, where each scope is
    /// its own token. False for a signed-in person, whose one token per company carries every scope they
    /// granted, so asking for "the other scope" after a refusal would return the same token.
    fn token_depends_on_scope(&self) -> bool {
        true
    }
}

#[async_trait]
impl TokenSource for OAuthClient {
    async fn token_for(&self, tenant: &TenantId, scope: Scope) -> Result<String> {
        OAuthClient::token_for(self, tenant, scope).await
    }

    async fn forget_token(&self, tenant: &TenantId, scope: Scope) {
        OAuthClient::forget_token(self, tenant, scope).await;
    }
}

/// A shared handle to a [`TokenSource`]. Clients are built from either an [`OAuthClient`] or a
/// signed-in person's tokens with `.into()`.
#[derive(Clone)]
pub struct Tokens(Arc<dyn TokenSource>);

impl Tokens {
    pub fn new(source: impl TokenSource + 'static) -> Self {
        Self(Arc::new(source))
    }

    pub async fn token_for(&self, tenant: &TenantId, scope: Scope) -> Result<String> {
        self.0.token_for(tenant, scope).await
    }

    pub async fn forget_token(&self, tenant: &TenantId, scope: Scope) {
        self.0.forget_token(tenant, scope).await;
    }

    pub fn token_depends_on_scope(&self) -> bool {
        self.0.token_depends_on_scope()
    }
}

impl fmt::Debug for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<OAuthClient> for Tokens {
    fn from(client: OAuthClient) -> Self {
        Self::new(client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Fixed;
    #[async_trait]
    impl TokenSource for Fixed {
        async fn token_for(&self, tenant: &TenantId, _: Scope) -> Result<String> {
            Ok(format!("token-for-{tenant}"))
        }
        async fn forget_token(&self, _: &TenantId, _: Scope) {}
        fn token_depends_on_scope(&self) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn a_handle_forwards_to_its_source() {
        let tokens = Tokens::new(Fixed);
        assert_eq!(
            tokens
                .token_for(&TenantId::new("t1"), Scope::Erp)
                .await
                .unwrap(),
            "token-for-t1"
        );
        assert!(!tokens.token_depends_on_scope());
        assert!(format!("{tokens:?}").contains("Fixed"));
    }

    #[test]
    fn client_credentials_pick_the_token_by_scope() {
        let tokens: Tokens = OAuthClient::new("https://x/token", "id", "secret").into();
        assert!(tokens.token_depends_on_scope());
    }
}
