//! Shared by the unit tests of this crate.

use crate::config::SignInConfig;

/// A config pointing at nothing real, with the secret `s3cret` so tests can prove it never leaks.
pub(crate) fn config() -> SignInConfig {
    SignInConfig::from_lookup(|key| match key {
        "VISMA_USER_CLIENT_ID" => Some("web-app".into()),
        "VISMA_USER_CLIENT_SECRET" => Some("s3cret".into()),
        "HOME" => Some("/home/test".into()),
        _ => None,
    })
    .expect("test config is valid")
}
