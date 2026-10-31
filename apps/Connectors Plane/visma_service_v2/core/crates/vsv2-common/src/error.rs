use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),

    #[error("auth error: {0}")]
    Auth(String),

    #[error("rate limit exceeded for source {kind}: cooldown until {until}")]
    RateLimited {
        kind: &'static str,
        until: chrono::DateTime<chrono::Utc>,
    },

    #[error("HTTP error: {0}")]
    Http(String),

    #[error(
        "upstream returned non-success status {status}: {}",
        crate::text::printable(.body, crate::text::MAX_ERROR_BODY_CHARS)
    )]
    UpstreamStatus { status: u16, body: String },

    #[error("graphql error: {0}")]
    GraphQl(String),

    #[error("state store error: {0}")]
    StateStore(String),

    #[error("endpoint {name} not found in registry")]
    UnknownEndpoint { name: String },

    #[error("endpoint {name} not enabled for tenant {tenant} on source {kind}")]
    SourceDisabled {
        name: String,
        tenant: String,
        kind: &'static str,
    },

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde_json: {0}")]
    SerdeJson(#[from] serde_json::Error),

    #[error("other: {0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn upstream_status_display_is_printable_and_bounded() {
        let body = format!("\0\u{1f}{}", "x".repeat(10_000));
        let shown = Error::UpstreamStatus { status: 502, body }.to_string();

        assert!(shown.starts_with("upstream returned non-success status 502: .."));
        assert!(
            !shown.contains('\0'),
            "NUL would poison sync_operation jsonb"
        );
        assert!(
            shown.chars().count() < 2_200,
            "body must be cut, got {}",
            shown.len()
        );
    }

    #[test]
    fn upstream_status_display_keeps_a_normal_json_body() {
        let shown = Error::UpstreamStatus {
            status: 400,
            body: r#"{"message":"bad"}"#.into(),
        }
        .to_string();
        assert_eq!(
            shown,
            r#"upstream returned non-success status 400: {"message":"bad"}"#
        );
    }
}
