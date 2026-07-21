//! Authenticated NATS connection helper.
//!
//! `async-nats` does not translate URL user-info into token authentication, so
//! credentials are supplied explicitly through `ConnectOptions`.

const NATS_INBOX_PREFIX: &str = "_INBOX.SESSION_CORE_RUNTIME";

/// Inbox prefix for the dedicated shared-broker connection used by the GDPR
/// erasure consumer (see [`connect_shared`]). Kept distinct from
/// [`NATS_INBOX_PREFIX`] because the two connections are separate NATS
/// sessions, under separate credentials, against separate brokers.
const NATS_SHARED_INBOX_PREFIX: &str = "_INBOX.SESSION_CORE_GDPR";

fn configured_token() -> Option<String> {
    normalize_token(std::env::var("NATS_AUTH_TOKEN").ok())
}

fn normalize_token(value: Option<String>) -> Option<String> {
    value
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
}

#[derive(Debug, PartialEq)]
enum NatsAuth {
    UserPassword(String, String),
    Token(String),
    None,
}

fn select_auth(
    user: Option<&str>,
    password: Option<&str>,
    token: Option<&str>,
    allow_token: bool,
) -> NatsAuth {
    let user = user.map(str::trim).filter(|value| !value.is_empty());
    let password = password.map(str::trim).filter(|value| value.len() >= 32);
    match (user, password) {
        (Some(user), Some(password)) => NatsAuth::UserPassword(user.into(), password.into()),
        (None, None) if allow_token => token
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map_or(NatsAuth::None, |value| NatsAuth::Token(value.into())),
        (Some(_) | None, None) | (None, Some(_)) => NatsAuth::None,
    }
}

pub async fn connect(url: &str) -> Result<async_nats::Client, async_nats::ConnectError> {
    let user = std::env::var("NATS_USER").ok();
    let password = std::env::var("NATS_PASSWORD").ok();
    let token = configured_token();
    match select_auth(
        user.as_deref(),
        password.as_deref(),
        token.as_deref(),
        std::env::var("NATS_ALLOW_TOKEN_FALLBACK").as_deref() == Ok("1"),
    ) {
        NatsAuth::UserPassword(user, password) => {
            async_nats::ConnectOptions::with_user_and_password(user, password)
                .custom_inbox_prefix(NATS_INBOX_PREFIX)
                .connect(url)
                .await
        }
        NatsAuth::Token(token) => {
            async_nats::ConnectOptions::with_token(token)
                .custom_inbox_prefix(NATS_INBOX_PREFIX)
                .connect(url)
                .await
        }
        NatsAuth::None => {
            async_nats::ConnectOptions::new()
                .custom_inbox_prefix(NATS_INBOX_PREFIX)
                .connect(url)
                .await
        }
    }
}

/// Connect to the shared cross-plane broker (`control-shared-nats`) under the
/// narrowly-scoped `session-core-gdpr` identity used only by the GDPR
/// erasure consumer.
///
/// Unlike [`connect`], which serves session-core's own Model-Plane-local
/// broker and tolerates a migration-only bearer-token fallback, this path is
/// username/password only: `NATS_SHARED_USER` and `NATS_SHARED_PASSWORD` are
/// read directly with no token fallback, so a missing/misconfigured shared
/// identity fails the connection attempt (and is retried by the caller's
/// supervised background loop) rather than silently degrading to an
/// unauthenticated session on a broker this crate does not own.
pub async fn connect_shared(url: &str) -> Result<async_nats::Client, async_nats::ConnectError> {
    let user = std::env::var("NATS_SHARED_USER").unwrap_or_default();
    let password = std::env::var("NATS_SHARED_PASSWORD").unwrap_or_default();
    async_nats::ConnectOptions::with_user_and_password(user, password)
        .custom_inbox_prefix(NATS_SHARED_INBOX_PREFIX)
        .connect(url)
        .await
}

#[cfg(test)]
mod tests {
    use super::{normalize_token, select_auth, NatsAuth};

    #[test]
    fn empty_tokens_are_treated_as_unconfigured() {
        assert_eq!(normalize_token(None), None);
        assert_eq!(normalize_token(Some("   ".to_owned())), None);
    }

    #[test]
    fn configured_tokens_are_trimmed_without_exposing_them() {
        assert_eq!(
            normalize_token(Some("  configured  ".to_owned())),
            Some("configured".to_owned())
        );
    }

    #[test]
    fn scoped_credentials_are_preferred_and_tokens_are_migration_only() {
        assert_eq!(
            select_auth(
                Some("session-core-runtime"),
                Some("0123456789abcdef0123456789abcdef"),
                Some("token"),
                true
            ),
            NatsAuth::UserPassword(
                "session-core-runtime".into(),
                "0123456789abcdef0123456789abcdef".into()
            )
        );
        assert_eq!(
            select_auth(None, None, Some("token"), false),
            NatsAuth::None
        );
        assert_eq!(
            select_auth(None, None, Some("token"), true),
            NatsAuth::Token("token".into())
        );
    }
}
