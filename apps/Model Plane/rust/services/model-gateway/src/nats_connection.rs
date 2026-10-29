//! Model-gateway NATS connector with scoped user/password authentication.
//!
//! `async_nats::connect` does not translate URL user-info into the NATS
//! `auth_token` field. Keep the server URL and credential separate so Docker
//! network aliases remain unambiguous. Token authentication is migration-only
//! and must be opted into explicitly.

const NATS_INBOX_PREFIX: &str = "_INBOX.MODEL_GATEWAY_RUNTIME";

fn normalize_token(raw: Option<String>) -> Option<String> {
    raw.map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
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

/// Connect to NATS using scoped `NATS_USER`/`NATS_PASSWORD` credentials.
/// `NATS_AUTH_TOKEN` is considered only when `NATS_ALLOW_TOKEN_FALLBACK=1`
/// and no user/password pair is configured.
///
/// # Errors
/// Returns the NATS client connection error when the broker is unavailable or
/// rejects the configured credential.
pub async fn connect(url: &str) -> Result<async_nats::Client, async_nats::ConnectError> {
    let user = std::env::var("NATS_USER").ok();
    let password = std::env::var("NATS_PASSWORD").ok();
    let token = normalize_token(std::env::var("NATS_AUTH_TOKEN").ok());
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

#[cfg(test)]
mod tests {
    use super::{normalize_token, select_auth, NatsAuth};

    #[test]
    fn empty_tokens_are_treated_as_unconfigured() {
        assert_eq!(normalize_token(None), None);
        assert_eq!(normalize_token(Some("  ".to_owned())), None);
    }

    #[test]
    fn configured_tokens_are_trimmed_without_logging_them() {
        assert_eq!(
            normalize_token(Some("  bounded-token  ".to_owned())),
            Some("bounded-token".to_owned())
        );
    }

    #[test]
    fn scoped_credentials_are_preferred_and_tokens_are_migration_only() {
        assert_eq!(
            select_auth(
                Some("model-gateway-runtime"),
                Some("0123456789abcdef0123456789abcdef"),
                Some("token"),
                true
            ),
            NatsAuth::UserPassword(
                "model-gateway-runtime".into(),
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
