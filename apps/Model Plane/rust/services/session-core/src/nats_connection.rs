//! Authenticated NATS connection helper.
//!
//! `async-nats` does not translate URL user-info into token authentication, so
//! credentials are supplied explicitly through `ConnectOptions`.

fn configured_token() -> Option<String> {
    normalize_token(std::env::var("NATS_AUTH_TOKEN").ok())
}

fn normalize_token(value: Option<String>) -> Option<String> {
    value
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
}

pub async fn connect(url: &str) -> Result<async_nats::Client, async_nats::ConnectError> {
    match configured_token() {
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
        assert_eq!(normalize_token(Some("   ".to_owned())), None);
    }

    #[test]
    fn configured_tokens_are_trimmed_without_exposing_them() {
        assert_eq!(
            normalize_token(Some("  configured  ".to_owned())),
            Some("configured".to_owned())
        );
    }
}
