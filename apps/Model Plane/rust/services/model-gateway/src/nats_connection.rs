//! Shared NATS connector with explicit token authentication.
//!
//! `async_nats::connect` does not translate URL user-info into the NATS
//! `auth_token` field. Keep the server URL and credential separate so Docker
//! network aliases remain unambiguous and the token is sent with the protocol
//! field the server actually validates.

fn normalize_token(raw: Option<String>) -> Option<String> {
    raw.map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Connect to NATS, using `NATS_AUTH_TOKEN` when configured.
///
/// # Errors
/// Returns the NATS client connection error when the broker is unavailable or
/// rejects the configured credential.
pub async fn connect(url: &str) -> Result<async_nats::Client, async_nats::ConnectError> {
    let token = normalize_token(std::env::var("NATS_AUTH_TOKEN").ok());
    match token {
        Some(token) => {
            async_nats::ConnectOptions::with_token(token)
                .connect(url)
                .await
        }
        None => async_nats::connect(url).await,
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_token;

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
}
