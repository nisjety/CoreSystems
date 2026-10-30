use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{InteractiveError, Result};

/// Which Visma API family a write goes to. Fixes the host, so a caller can never aim a write at
/// an arbitrary URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteTarget {
    Finance,
    SalesOrder,
}

impl WriteTarget {
    pub const fn as_str(&self) -> &'static str {
        match self {
            WriteTarget::Finance => "finance",
            WriteTarget::SalesOrder => "salesorder",
        }
    }
}

/// Deliberately has no `Delete`: the OAuth scope forbids it and Layer 3 never offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteMethod {
    Post,
    Put,
    Patch,
}

impl WriteMethod {
    pub const fn as_str(&self) -> &'static str {
        match self {
            WriteMethod::Post => "POST",
            WriteMethod::Put => "PUT",
            WriteMethod::Patch => "PATCH",
        }
    }
}

/// A clean relative path (+ optional query). Shared by writes and precondition reads so neither can
/// be aimed at another host or climb out of the API root.
pub fn validate_path(p: &str) -> Result<()> {
    let bad = !p.starts_with('/')
        || p.starts_with("//")
        || p.contains("..")
        || p.contains("://")
        || p.contains('\\')
        || p.contains('#')
        || p.chars().any(|c| c.is_whitespace() || c.is_control());
    if bad {
        return Err(InteractiveError::InvalidInput(format!(
            "write path must be a clean relative path, got {p:?}"
        )));
    }
    Ok(())
}

/// One fully specified write. This is what the dry-run shows and what execute sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteRequest {
    pub target: WriteTarget,
    pub method: WriteMethod,
    /// Path (and optional query) relative to the target host, e.g. `/v3/salesorder/SO001`.
    pub path: String,
    pub body: Value,
    /// ETag for optimistic concurrency (`If-Match`), where the endpoint requires it.
    pub if_match: Option<String>,
}

impl WriteRequest {
    pub fn validate(&self) -> Result<()> {
        validate_path(&self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req(path: &str) -> WriteRequest {
        WriteRequest {
            target: WriteTarget::SalesOrder,
            method: WriteMethod::Put,
            path: path.into(),
            body: json!({}),
            if_match: None,
        }
    }

    #[test]
    fn accepts_a_plain_relative_path_with_query() {
        assert!(req("/v3/salesorder/SO001?expand=lines").validate().is_ok());
    }

    #[test]
    fn rejects_paths_that_could_escape_the_target_host() {
        for bad in [
            "v3/x",
            "//evil.example/x",
            "https://evil.example/x",
            "/a/../b",
            "/a b",
            "/a\\b",
            "/a#frag",
            "/a\nb",
            "",
        ] {
            assert!(req(bad).validate().is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn method_enum_has_no_delete() {
        let all = [WriteMethod::Post, WriteMethod::Put, WriteMethod::Patch];
        assert!(all.iter().all(|m| m.as_str() != "DELETE"));
        assert!(serde_json::from_str::<WriteMethod>("\"delete\"").is_err());
    }
}
