//! Cache policy. Mirrors CONTRACTS §6.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachePolicy {
    pub mode: CacheMode,
    #[serde(default)]
    pub max_age_s: u32,
    #[serde(default)]
    pub vary_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_while_revalidate_s: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheMode {
    Bypass,
    ReadOnly,
    ReadWrite,
    WriteOnly,
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
