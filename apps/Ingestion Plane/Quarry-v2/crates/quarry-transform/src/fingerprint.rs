//! Content fingerprint. blake3 over normalized text.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint(pub String);

impl Fingerprint {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn content_fingerprint(body: &[u8]) -> Fingerprint {
    let mut h = blake3::Hasher::new();
    h.update(body);
    Fingerprint(format!("blake3:{}", h.finalize().to_hex()))
}

/// Normalized text fingerprint: strips whitespace + case for semantic-ish diff.
pub fn text_fingerprint(text: &str) -> Fingerprint {
    let norm: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| c.to_lowercase())
        .collect();
    content_fingerprint(norm.as_bytes())
}
