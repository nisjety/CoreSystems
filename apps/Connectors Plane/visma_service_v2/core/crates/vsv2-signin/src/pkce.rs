//! PKCE (RFC 7636, method S256) and the random strings the authorization request needs.

use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// A code verifier and its S256 challenge. The verifier stays here; only the challenge goes to the browser.
#[derive(Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn generate() -> Self {
        Self::from_verifier(random_string())
    }

    pub fn from_verifier(verifier: String) -> Self {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
        }
    }
}

impl fmt::Debug for Pkce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pkce")
            .field("verifier", &"<hidden>")
            .finish()
    }
}

/// 64 hex characters from two random v4 UUIDs: 244 bits from the operating system's generator, inside the
/// 43 to 128 characters of the unreserved set that RFC 7636 allows for a verifier, and fine as a `state`.
pub fn random_string() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_matches_the_rfc_7636_example() {
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into());
        assert_eq!(
            pkce.challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn generated_values_are_long_unreserved_and_different_every_time() {
        let a = Pkce::generate();
        let b = Pkce::generate();
        assert_ne!(a.verifier, b.verifier);
        assert!((43..=128).contains(&a.verifier.len()));
        assert!(a.verifier.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a.challenge.len(), 43, "S256 gives 43 base64url characters");
        assert_ne!(random_string(), random_string());
    }

    #[test]
    fn the_verifier_never_shows_in_debug_output() {
        let pkce = Pkce::generate();
        assert!(!format!("{pkce:?}").contains(&pkce.verifier));
    }
}
