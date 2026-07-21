/// Connect to NATS using the Data Plane's token from a dedicated environment
/// variable. Credentials are deliberately kept out of the URL so they cannot
/// leak through URL logging and are handled correctly by async-nats 0.49.
pub async fn connect(url: &str) -> Result<async_nats::Client, async_nats::ConnectError> {
    match token_from_env() {
        Some(token) => {
            async_nats::ConnectOptions::with_token(token)
                .connect(url)
                .await
        }
        None => async_nats::connect(url).await,
    }
}

fn token_from_env() -> Option<String> {
    normalize_token(std::env::var("DATAPLANE_NATS_TOKEN").ok().as_deref())
}

/// Connect to a shared cross-plane broker (e.g. `control-shared-nats`) under a
/// dedicated username/password identity, with a caller-supplied custom inbox
/// prefix so this connection's inbox namespace never collides with the
/// plane-local connection [`connect`] establishes on the same process.
///
/// Used by cross-plane consumers — e.g. a service's GDPR
/// organization-erasure fan-out consumer — that must authenticate as a
/// narrowly-scoped identity distinct from this service's own Data-Plane-local
/// broker connection (which uses [`connect`] and `DATAPLANE_NATS_TOKEN`
/// above). Deliberately username/password only, with no token fallback: a
/// missing or misconfigured identity fails the connection attempt outright
/// rather than silently degrading to an unauthenticated session on a broker
/// this service does not own.
pub async fn connect_shared(
    url: &str,
    user: &str,
    password: &str,
    inbox_prefix: &str,
) -> Result<async_nats::Client, async_nats::ConnectError> {
    async_nats::ConnectOptions::with_user_and_password(user.to_owned(), password.to_owned())
        .custom_inbox_prefix(inbox_prefix.to_owned())
        .connect(url)
        .await
}

fn normalize_token(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::normalize_token;

    #[test]
    fn token_normalization_rejects_missing_or_blank_values() {
        assert_eq!(normalize_token(None), None);
        assert_eq!(normalize_token(Some("")), None);
        assert_eq!(normalize_token(Some(" \t\n")), None);
    }

    #[test]
    fn token_normalization_trims_environment_whitespace() {
        assert_eq!(
            normalize_token(Some("  opaque-service-token\n")),
            Some("opaque-service-token".to_owned())
        );
    }
}
