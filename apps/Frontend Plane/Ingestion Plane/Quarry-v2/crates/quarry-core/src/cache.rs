//! Cache policy. Mirrors CONTRACTS §6.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachePolicy {
    /// INNH-7: defaults to `read_write` (what the contract documents), so
    /// `{"cache":{"max_age_s":60}}` is valid. It used to be a required field.
    #[serde(default)]
    pub mode: CacheMode,
    /// The TTL, in seconds, of a result this request writes to the cache. Not
    /// the freshness a read requires: that is the request's `max_age` (ms).
    #[serde(default)]
    pub max_age_s: u32,
    #[serde(default)]
    pub vary_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_while_revalidate_s: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheMode {
    Bypass,
    ReadOnly,
    #[default]
    ReadWrite,
    WriteOnly,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_policy_without_a_mode_is_read_write() {
        let policy: CachePolicy = serde_json::from_str(r#"{"max_age_s":60}"#).unwrap();
        assert_eq!(policy.mode, CacheMode::ReadWrite);
        assert_eq!(policy.max_age_s, 60);
        let empty: CachePolicy = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.mode, CacheMode::ReadWrite);
        assert_eq!(empty.max_age_s, 0);
    }

    #[test]
    fn a_named_mode_is_kept() {
        let policy: CachePolicy = serde_json::from_str(r#"{"mode":"bypass"}"#).unwrap();
        assert_eq!(policy.mode, CacheMode::Bypass);
    }
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            mode: CacheMode::ReadWrite,
            max_age_s: 3600,
            vary_on: vec!["url".into()],
            stale_while_revalidate_s: Some(30),
        }
    }
}
