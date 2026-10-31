use thiserror::Error;

pub type Result<T> = std::result::Result<T, SignInError>;

/// Everything that can go wrong signing in or using a sign-in. The messages are written for the person
/// at the terminal: each says what happened and what to do next. None of them contains a token, a code
/// or the client secret.
#[derive(Debug, Error)]
pub enum SignInError {
    #[error("sign-in is not configured: {0}")]
    Config(String),

    #[error("sign-in store problem: {0}")]
    Store(String),

    #[error("could not reach Visma Connect: {0}")]
    Http(String),

    #[error("Visma Connect refused the request ({status}, {error}){}", description_suffix(.description))]
    Token {
        status: u16,
        error: String,
        description: String,
    },

    /// The refresh token was rejected: it expired, was revoked, or was already used.
    #[error("the sign-in was rejected by Visma Connect (expired or revoked): run `vsv2-mcp login` again")]
    InvalidGrant,

    #[error("Visma Connect did not complete the sign-in ({error}){}", description_suffix(.description))]
    Denied { error: String, description: String },

    #[error(
        "the answer to the sign-in did not match the request that was sent (state): start again"
    )]
    StateMismatch,

    #[error("the answer to the sign-in came from {got:?}, not from Visma Connect ({expected:?}): start again")]
    IssuerMismatch { expected: String, got: String },

    #[error("the redirect carried no authorization code: {0}")]
    NoCode(String),

    #[error("nobody completed the sign-in in the browser within {0} seconds")]
    Timeout(u64),

    #[error("the sign-in was stopped before any company was connected")]
    Cancelled,

    #[error("this computer is signed in as {signed_in_as}, not as the person who just approved: run `vsv2-mcp logout` first")]
    DifferentUser { signed_in_as: String },

    #[error("Visma Connect returned no refresh token, so the sign-in would last about an hour: ask for the `offline_access` scope (VISMA_USER_SCOPES) and make sure the app is allowed to use it")]
    NoRefreshToken,

    #[error("the access token from Visma Connect could not be read: {0}")]
    BadToken(String),

    #[error("not signed in: run `vsv2-mcp login`")]
    NotSignedIn,

    #[error("the sign-in on this computer changed while this server was running (a different person signed in): restart the MCP server")]
    SignInChanged,

    #[error("the sign-in does not cover company {0}: run `vsv2-mcp login`")]
    NotConnected(String),

    #[error("the sign-in for company {tenant} does not include the scope {scope}: {hint}")]
    MissingScope {
        tenant: String,
        scope: String,
        hint: String,
    },

    #[error("{0}")]
    Unsupported(String),
}

fn description_suffix(description: &str) -> String {
    if description.is_empty() {
        String::new()
    } else {
        format!(": {description}")
    }
}

impl From<SignInError> for vsv2_common::Error {
    fn from(e: SignInError) -> Self {
        match e {
            SignInError::Http(m) => vsv2_common::Error::Http(m),
            other => vsv2_common::Error::Auth(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_names_the_error_and_its_description_only_when_there_is_one() {
        let with = SignInError::Token {
            status: 400,
            error: "invalid_scope".into(),
            description: "scope not allowed".into(),
        }
        .to_string();
        assert!(
            with.contains("invalid_scope") && with.ends_with("scope not allowed"),
            "{with}"
        );
        let without = SignInError::Denied {
            error: "access_denied".into(),
            description: String::new(),
        }
        .to_string();
        assert!(without.ends_with("(access_denied)"), "{without}");
    }

    #[test]
    fn a_transport_failure_stays_an_http_error_and_everything_else_an_auth_error() {
        assert!(matches!(
            vsv2_common::Error::from(SignInError::Http("x".into())),
            vsv2_common::Error::Http(_)
        ));
        assert!(matches!(
            vsv2_common::Error::from(SignInError::InvalidGrant),
            vsv2_common::Error::Auth(_)
        ));
    }
}
