//! Concrete preset bundles for common ingestion patterns.
//!
//! Cycle 25 / cluster #10.
//!
//! Six shipped presets:
//!
//! | Preset                       | Use case                                            |
//! | ---------------------------- | --------------------------------------------------- |
//! | `docs-site`                  | Technical docs (markdown-heavy, slow refresh)        |
//! | `help-center`                | Customer-facing help articles (similar to docs)      |
//! | `pricing-monitor`            | Pricing pages — change tracking, fast refresh        |
//! | `knowledge-base-sync`        | Internal KB → durable archive                        |
//! | `ecommerce-catalog`          | Product catalog crawls — high pagination, schema-aware |
//! | `policy-and-legal-tracker`   | T&Cs / privacy policies — high-fidelity diff         |
//!
//! Each pins crawl + extraction + chunking + retry + retention so a
//! caller saying `preset: "docs-site"` gets a tested bundle instead
//! of hand-tuning 20 flags.

use serde::{Deserialize, Serialize};

use crate::cache::{CacheMode, CachePolicy};
use crate::output_profile::{FormatChain, LlmSections, OutputProfile, RetentionPolicy};

/// Complete preset definition. Mirrors the OutputProfile shape plus
/// crawl + retry knobs that don't belong on a per-page format
/// profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub display_name: String,
    /// Concrete crawl-level knobs.
    pub crawl: CrawlKnobs,
    /// Re-uses the OutputProfile shape from cluster #8 so callers
    /// can mix-and-match preset + output profile without redundancy.
    pub output: OutputProfile,
    /// Retry behaviour the preset locks in.
    pub retry: RetryKnobs,
    /// Whether to track changes across runs. When `true`, the
    /// runtime saves baselines + emits diffs on each refresh.
    #[serde(default)]
    pub change_tracking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlKnobs {
    /// Max depth from seed. 0 = single page only.
    pub max_depth: u32,
    /// Cap on pages visited per run.
    pub max_pages: u32,
    /// Whether subdomain expansion is in-scope.
    pub include_subdomains: bool,
    /// Honour `<meta name="robots" content="noindex">` headers.
    pub respect_meta_robots: bool,
    /// Path patterns to include (regex strings; empty = all).
    #[serde(default)]
    pub include_patterns: Vec<String>,
    /// Path patterns to exclude.
    #[serde(default)]
    pub exclude_patterns: Vec<String>,
    /// Cron schedule for recurring refresh; `None` = one-shot only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_cron: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryKnobs {
    pub max_attempts: u32,
    /// Initial backoff in milliseconds.
    pub base_delay_ms: u64,
    /// HTTP status codes that trigger a retry.
    pub transient_codes: Vec<u16>,
}

// =============================================================================
// Builtin preset constructors
// =============================================================================

fn docs_site() -> Preset {
    Preset {
        name: "docs-site".into(),
        display_name: "Technical Documentation Site".into(),
        crawl: CrawlKnobs {
            max_depth: 6,
            max_pages: 5_000,
            include_subdomains: false,
            respect_meta_robots: true,
            include_patterns: vec![".*/docs/.*".into()],
            exclude_patterns: vec![
                "/blog/".into(),
                "/api/".into(), // API references usually need a different preset
            ],
            refresh_cron: Some("0 3 * * *".into()), // nightly 03:00
        },
        output: OutputProfile {
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
            cache: CachePolicy {
                mode: CacheMode::ReadWrite,
                max_age_s: 24 * 60 * 60,
                vary_on: vec!["url".into()],
                stale_while_revalidate_s: Some(60 * 60),
            },
            retention: RetentionPolicy {
                primary_days: Some(180),
                archive_days: Some(365),
            },
            extras: serde_json::Value::Null,
        },
        retry: RetryKnobs {
            max_attempts: 3,
            base_delay_ms: 1_000,
            transient_codes: vec![408, 429, 500, 502, 503, 504],
        },
        change_tracking: false,
    }
}

fn help_center() -> Preset {
    let mut p = docs_site();
    p.name = "help-center".into();
    p.display_name = "Customer Help Center".into();
    p.crawl.include_patterns = vec![".*/help/.*".into(), ".*/support/.*".into()];
    p.crawl.exclude_patterns = vec!["/forum/".into(), "/community/".into()];
    p
}

fn pricing_monitor() -> Preset {
    Preset {
        name: "pricing-monitor".into(),
        display_name: "Pricing Page Change Monitor".into(),
        crawl: CrawlKnobs {
            max_depth: 1,
            max_pages: 50,
            include_subdomains: false,
            respect_meta_robots: false, // pricing pages frequently use noindex
            include_patterns: vec![".*/pricing.*".into(), ".*/plans.*".into()],
            exclude_patterns: vec![],
            refresh_cron: Some("*/30 * * * *".into()), // every 30 min
        },
        output: OutputProfile {
            name: "pricing-monitor".into(),
            format_chain: FormatChain {
                markdown: true,
                html: true,
                screenshot: true, // diff includes visual comparison
                source_trace: true,
                ..Default::default()
            },
            llm_sections: LlmSections {
                enabled: false, // pricing pages don't benefit from sectioning
                ..Default::default()
            },
            cache: CachePolicy {
                mode: CacheMode::WriteOnly, // always refetch to detect change
                max_age_s: 60,
                vary_on: vec!["url".into()],
                stale_while_revalidate_s: None,
            },
            retention: RetentionPolicy {
                primary_days: Some(365),
                archive_days: None, // pricing history is invaluable
            },
            extras: serde_json::Value::Null,
        },
        retry: RetryKnobs {
            max_attempts: 5,
            base_delay_ms: 2_000,
            transient_codes: vec![408, 429, 500, 502, 503, 504],
        },
        change_tracking: true,
    }
}

fn knowledge_base_sync() -> Preset {
    Preset {
        name: "knowledge-base-sync".into(),
        display_name: "Internal Knowledge Base Sync".into(),
        crawl: CrawlKnobs {
            max_depth: 10,
            max_pages: 50_000,
            include_subdomains: true,
            respect_meta_robots: true,
            include_patterns: vec![],
            exclude_patterns: vec!["/login".into(), "/admin/".into()],
            refresh_cron: Some("0 2 * * *".into()),
        },
        output: OutputProfile {
            name: "knowledge-base-sync".into(),
            format_chain: FormatChain {
                markdown: true,
                html: true,
                extract: true, // KB → structured chunks
                source_trace: true,
                ..Default::default()
            },
            llm_sections: LlmSections {
                enabled: true,
                max_section_chars: 8_000,
                emit_toc: false,
            },
            cache: CachePolicy::default(),
            retention: RetentionPolicy {
                primary_days: None, // forever
                archive_days: None,
            },
            extras: serde_json::Value::Null,
        },
        retry: RetryKnobs {
            max_attempts: 3,
            base_delay_ms: 1_000,
            transient_codes: vec![408, 429, 500, 502, 503, 504],
        },
        change_tracking: true,
    }
}

fn ecommerce_catalog() -> Preset {
    Preset {
        name: "ecommerce-catalog".into(),
        display_name: "E-commerce Product Catalog".into(),
        crawl: CrawlKnobs {
            max_depth: 4,
            max_pages: 100_000,
            include_subdomains: false,
            respect_meta_robots: true,
            include_patterns: vec![".*/product/.*".into(), ".*/category/.*".into()],
            exclude_patterns: vec!["/cart".into(), "/checkout".into(), "/account/".into()],
            refresh_cron: Some("0 */6 * * *".into()), // every 6h
        },
        output: OutputProfile {
            name: "ecommerce-catalog".into(),
            format_chain: FormatChain {
                markdown: false,
                html: true,
                extract: true, // schema.org / JSON-LD
                source_trace: true,
                ..Default::default()
            },
            llm_sections: LlmSections::default(),
            cache: CachePolicy::default(),
            retention: RetentionPolicy {
                primary_days: Some(30),
                archive_days: Some(90),
            },
            extras: serde_json::json!({"extract_schema": "Product"}),
        },
        retry: RetryKnobs {
            max_attempts: 5,
            base_delay_ms: 1_500,
            transient_codes: vec![408, 429, 500, 502, 503, 504],
        },
        change_tracking: true,
    }
}

fn policy_and_legal_tracker() -> Preset {
    Preset {
        name: "policy-and-legal-tracker".into(),
        display_name: "Policy & Legal Document Tracker".into(),
        crawl: CrawlKnobs {
            max_depth: 1,
            max_pages: 20,
            include_subdomains: false,
            respect_meta_robots: false,
            include_patterns: vec![
                ".*/terms.*".into(),
                ".*/privacy.*".into(),
                ".*/legal.*".into(),
                ".*/policies.*".into(),
            ],
            exclude_patterns: vec![],
            refresh_cron: Some("0 0 * * *".into()), // daily 00:00
        },
        output: OutputProfile {
            name: "policy-and-legal-tracker".into(),
            format_chain: FormatChain {
                markdown: true,
                html: true,
                source_trace: true,
                ..Default::default()
            },
            llm_sections: LlmSections {
                enabled: true,
                max_section_chars: 10_000,
                emit_toc: true,
            },
            cache: CachePolicy {
                mode: CacheMode::WriteOnly,
                max_age_s: 0,
                vary_on: vec!["url".into()],
                stale_while_revalidate_s: None,
            },
            retention: RetentionPolicy {
                primary_days: None, // legal history forever
                archive_days: None,
            },
            extras: serde_json::Value::Null,
        },
        retry: RetryKnobs {
            max_attempts: 4,
            base_delay_ms: 2_000,
            transient_codes: vec![408, 429, 500, 502, 503, 504],
        },
        change_tracking: true,
    }
}

/// Return every shipped preset. Used by `/v1/presets` to enumerate.
pub fn builtin_presets() -> Vec<Preset> {
    vec![
        docs_site(),
        help_center(),
        pricing_monitor(),
        knowledge_base_sync(),
        ecommerce_catalog(),
        policy_and_legal_tracker(),
    ]
}

/// Resolve a preset by name. Returns `None` for unknown names —
/// callers MUST treat that as a 404, not a default-fallback (so
/// typos surface).
pub fn resolve_preset(name: &str) -> Option<Preset> {
    builtin_presets().into_iter().find(|p| p.name == name)
}

/// Layer JSON overrides on top of a resolved preset. See
/// `OutputProfile::merge_with_overrides` for merge semantics — both
/// reuse the same `deep_merge`.
pub fn merge_preset_with_overrides(preset: Preset, overrides: &serde_json::Value) -> Preset {
    let base = match serde_json::to_value(&preset) {
        Ok(v) => v,
        Err(_) => return preset,
    };
    let merged = crate::output_profile::deep_merge(base, overrides.clone());
    serde_json::from_value(merged).unwrap_or(preset)
}

/// Compatibility check for a preset + override pair. Today verifies
/// the inner `OutputProfile`; future cycles extend with crawl-level
/// rules.
pub fn validate_preset_compatibility(
    preset: &Preset,
    overrides: &serde_json::Value,
) -> Result<(), String> {
    let merged = merge_preset_with_overrides(preset.clone(), overrides);
    if merged.crawl.max_pages == 0 {
        return Err("crawl.max_pages must be > 0".into());
    }
    if merged.crawl.max_depth > 50 {
        return Err("crawl.max_depth capped at 50 to prevent runaway crawls".into());
    }
    crate::output_profile::validate_preset_compatibility(&merged.output, &serde_json::Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_builtin_preset_resolves() {
        for p in builtin_presets() {
            let resolved = resolve_preset(&p.name).unwrap();
            assert_eq!(resolved.name, p.name);
        }
    }

    #[test]
    fn unknown_preset_returns_none() {
        assert!(resolve_preset("not-a-preset").is_none());
    }

    #[test]
    fn presets_have_unique_names() {
        let names: Vec<_> = builtin_presets().iter().map(|p| p.name.clone()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            names.len(),
            sorted.len(),
            "duplicate preset names: {names:?}"
        );
    }

    #[test]
    fn pricing_monitor_enables_change_tracking_and_screenshot() {
        let p = resolve_preset("pricing-monitor").unwrap();
        assert!(p.change_tracking);
        assert!(p.output.format_chain.screenshot);
        // Frequent refresh expected.
        assert_eq!(p.crawl.refresh_cron.as_deref(), Some("*/30 * * * *"));
    }

    #[test]
    fn policy_tracker_keeps_history_forever() {
        let p = resolve_preset("policy-and-legal-tracker").unwrap();
        assert_eq!(p.output.retention.primary_days, None);
        assert_eq!(p.output.retention.archive_days, None);
        assert!(p.change_tracking);
    }

    #[test]
    fn merge_overrides_replaces_specified_fields() {
        let p = resolve_preset("docs-site").unwrap();
        let merged = merge_preset_with_overrides(p, &json!({ "crawl": { "max_depth": 10 } }));
        assert_eq!(merged.crawl.max_depth, 10);
        // Other crawl fields preserved.
        assert!(merged.crawl.refresh_cron.is_some());
    }

    #[test]
    fn validate_rejects_zero_max_pages() {
        let p = resolve_preset("docs-site").unwrap();
        let err =
            validate_preset_compatibility(&p, &json!({"crawl": {"max_pages": 0}})).unwrap_err();
        assert!(err.contains("max_pages"));
    }

    #[test]
    fn validate_rejects_runaway_depth() {
        let p = resolve_preset("docs-site").unwrap();
        let err =
            validate_preset_compatibility(&p, &json!({"crawl": {"max_depth": 100}})).unwrap_err();
        assert!(err.contains("max_depth"));
    }

    #[test]
    fn validate_accepts_default_preset() {
        for p in builtin_presets() {
            validate_preset_compatibility(&p, &json!({}))
                .unwrap_or_else(|e| panic!("preset {} invalid: {e}", p.name));
        }
    }

    #[test]
    fn ecommerce_preset_carries_extract_schema_extras() {
        let p = resolve_preset("ecommerce-catalog").unwrap();
        assert_eq!(p.output.extras["extract_schema"], "Product");
    }
}
