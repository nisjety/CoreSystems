//! Output profiles — named bundles of format + extraction + cache
//! policy that callers reference by short string instead of passing
//! 20 flags per request.
//!
//! Cycle 25 / cluster #8.
//!
//! A caller scraping a docs site says `output_profile: "docs-site"`
//! and the runtime resolves that to:
//!   - format chain (markdown + html + extract)
//!   - LLM-section emitter config (sectionize for downstream RAG)
//!   - cache policy (long TTL for stable docs)
//!   - retention policy (90d artifact retention)
//!
//! Without profiles, every caller has to encode that bundle inline →
//! drift, copy-paste errors, no central place to bump policy.

use serde::{Deserialize, Serialize};

use crate::cache::CachePolicy;

/// Format chain pin — which output formats the runtime computes.
///
/// `markdown` + `html` are cheap; `extract` invokes Model Plane and
/// has a cost; `screenshot` requires a browser driver. Profiles let
/// us state "docs sites want markdown + html, no screenshot, no MP
/// extract" without per-request bookkeeping.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FormatChain {
    #[serde(default)]
    pub markdown: bool,
    #[serde(default)]
    pub html: bool,
    /// LLM-structured extract via Model Plane.
    #[serde(default)]
    pub extract: bool,
    #[serde(default)]
    pub screenshot: bool,
    #[serde(default)]
    pub pdf: bool,
    #[serde(default)]
    pub source_trace: bool,
}

/// LLM-friendly section emission. When `enabled`, the runtime breaks
/// the markdown into semantic sections (h2/h3 boundaries) and emits
/// each with stable IDs so downstream RAG chunkers don't have to
/// re-split.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSections {
    pub enabled: bool,
    /// Max section length in chars. Sections longer than this split
    /// at the next paragraph boundary.
    #[serde(default = "default_max_section_chars")]
    pub max_section_chars: u32,
    /// Whether to emit a table-of-contents preface.
    #[serde(default)]
    pub emit_toc: bool,
}

fn default_max_section_chars() -> u32 {
    8_000
}

impl Default for LlmSections {
    fn default() -> Self {
        Self {
            enabled: false,
            max_section_chars: default_max_section_chars(),
            emit_toc: false,
        }
    }
}

/// Retention policy — how long the durable plane keeps the artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Days the artifact stays in primary storage. `None` = forever.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_days: Option<u32>,
    /// Days the artifact stays in archive (cheaper storage) after
    /// primary expires. Total lifetime = primary_days + archive_days.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_days: Option<u32>,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            primary_days: Some(90),
            archive_days: Some(180),
        }
    }
}

/// One named output profile. Resolves a short string ("docs-site")
/// to a complete configuration bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputProfile {
    pub name: String,
    pub format_chain: FormatChain,
    #[serde(default)]
    pub llm_sections: LlmSections,
    pub cache: CachePolicy,
    #[serde(default)]
    pub retention: RetentionPolicy,
    /// Free-form per-profile config the runtime passes through to
    /// downstream tools (extractor schema_id, custom selectors, etc.).
    #[serde(default)]
    pub extras: serde_json::Value,
}

impl OutputProfile {
    /// Merge a caller's override on top of the resolved profile. The
    /// override has the same shape (partial JSON); only fields the
    /// caller specifies are replaced. Fields the caller omits keep
    /// the profile's value.
    pub fn merge_with_overrides(self, overrides: &serde_json::Value) -> Self {
        // We do the merge JSON-side to keep it transport-friendly
        // (callers send a JSON patch; we don't want to invent a
        // separate Rust merge protocol).
        let base = match serde_json::to_value(&self) {
            Ok(v) => v,
            Err(_) => return self,
        };
        let merged = deep_merge(base, overrides.clone());
        serde_json::from_value(merged).unwrap_or(self)
    }
}

/// Recursive JSON merge. Objects merge field-by-field; arrays and
/// primitives are replaced wholesale. Public so consumers writing
/// their own profile resolvers can reuse the semantics.
pub fn deep_merge(a: serde_json::Value, b: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match (a, b) {
        (Value::Object(mut a_map), Value::Object(b_map)) => {
            for (k, v) in b_map {
                let merged = match a_map.remove(&k) {
                    Some(existing) => deep_merge(existing, v),
                    None => v,
                };
                a_map.insert(k, merged);
            }
            Value::Object(a_map)
        }
        // Any non-object on either side: b wins.
        (_, b) => b,
    }
}

/// Trait the runtime consumes to resolve named profiles. Backed by
/// an in-memory registry at boot in dev; cycle 27+ adds a
/// Postgres-backed registry so operators can edit profiles via REST.
pub trait OutputProfileRegistry: Send + Sync {
    fn resolve(&self, name: &str) -> Option<OutputProfile>;
    fn names(&self) -> Vec<String>;
}

/// Composability check — does an override JSON make the resolved
/// profile internally consistent? Caller passes the profile name +
/// override; returns Err with a short reason on conflict.
///
/// Today this only catches the obvious incompatibilities (e.g.,
/// `llm_sections.enabled=true` but `format_chain.markdown=false`).
/// Future cycles can grow the checks.
pub fn validate_preset_compatibility(
    profile: &OutputProfile,
    overrides: &serde_json::Value,
) -> Result<(), String> {
    let merged = profile.clone().merge_with_overrides(overrides);
    if merged.llm_sections.enabled && !merged.format_chain.markdown {
        return Err("llm_sections requires format_chain.markdown".into());
    }
    if merged.format_chain.extract && !merged.format_chain.html {
        return Err("extract format requires html to be enabled".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn docs_profile() -> OutputProfile {
        OutputProfile {
            name: "docs-site".into(),
            format_chain: FormatChain {
                markdown: true,
                html: true,
                source_trace: true,
                ..Default::default()
            },
            llm_sections: LlmSections {
                enabled: true,
                max_section_chars: 6_000,
                emit_toc: true,
            },
            cache: CachePolicy::default(),
            retention: RetentionPolicy::default(),
            extras: json!({}),
        }
    }

    #[test]
    fn merge_overrides_replaces_specified_field_only() {
        let base = docs_profile();
        let merged = base.merge_with_overrides(&json!({
            "llm_sections": { "max_section_chars": 12_000 }
        }));
        // Override field replaced, other llm_sections fields preserved.
        assert_eq!(merged.llm_sections.max_section_chars, 12_000);
        assert!(merged.llm_sections.enabled, "kept from base");
        assert!(merged.llm_sections.emit_toc, "kept from base");
        // Other top-level fields untouched.
        assert!(merged.format_chain.markdown);
    }

    #[test]
    fn merge_overrides_handles_nested_objects() {
        let base = docs_profile();
        let merged = base.merge_with_overrides(&json!({
            "format_chain": { "screenshot": true, "pdf": true }
        }));
        assert!(merged.format_chain.screenshot);
        assert!(merged.format_chain.pdf);
        // Existing format_chain fields preserved.
        assert!(merged.format_chain.markdown);
        assert!(merged.format_chain.html);
    }

    #[test]
    fn validate_rejects_llm_sections_without_markdown() {
        let base = docs_profile();
        let bad = json!({ "format_chain": { "markdown": false } });
        let err = validate_preset_compatibility(&base, &bad).unwrap_err();
        assert!(err.contains("markdown"));
    }

    #[test]
    fn validate_rejects_extract_without_html() {
        let base = OutputProfile {
            name: "test".into(),
            format_chain: FormatChain {
                markdown: true,
                html: false,
                extract: true,
                ..Default::default()
            },
            ..docs_profile()
        };
        let err = validate_preset_compatibility(&base, &json!({})).unwrap_err();
        assert!(err.contains("extract"));
    }

    #[test]
    fn validate_accepts_consistent_profile() {
        assert!(validate_preset_compatibility(&docs_profile(), &json!({})).is_ok());
    }

    #[test]
    fn retention_defaults_to_90_180() {
        let r = RetentionPolicy::default();
        assert_eq!(r.primary_days, Some(90));
        assert_eq!(r.archive_days, Some(180));
    }

    #[test]
    fn output_profile_serde_roundtrips() {
        let p = docs_profile();
        let s = serde_json::to_string(&p).unwrap();
        let back: OutputProfile = serde_json::from_str(&s).unwrap();
        assert_eq!(back.name, "docs-site");
        assert!(back.format_chain.markdown);
        assert_eq!(back.llm_sections.max_section_chars, 6_000);
    }
}
