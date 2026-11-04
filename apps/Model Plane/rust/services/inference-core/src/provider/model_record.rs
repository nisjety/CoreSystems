//! The record a provider advertises for each model on `/v1/models`: its context
//! window, its synchronous output ceiling and its vendor lifecycle stage.
//!
//! One static table, keyed by provider family and model id the way
//! [`super::effort::declared_support`] is keyed. A model with no row is advertised
//! with no limits: a provider's own ceiling bounds everything that provider serves,
//! but it is no fact about any one model, so it never fills a gap.

use super::effort::{Family, ModelAliases};

/// Where a model stands in its vendor's lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// Active at the vendor. A model the vendor also calls legacy stays `Ga` until a
    /// retirement is announced, so the picker does not fold it away.
    Ga,
    /// Offered ahead of general availability.
    Preview,
    /// The vendor has announced its retirement. The model still answers.
    Deprecated,
}

impl Lifecycle {
    /// The lowercase name carried on the wire.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ga => "ga",
            Self::Preview => "preview",
            Self::Deprecated => "deprecated",
        }
    }
}

/// The limits and lifecycle one model is advertised with. The default is the
/// unknown record: zero tokens and no lifecycle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecordLimits {
    pub context_window_tokens: u32,
    pub max_output_tokens: u32,
    pub lifecycle: Option<Lifecycle>,
}

struct Row {
    family: Family,
    model: &'static str,
    limits: RecordLimits,
}

const fn row(
    family: Family,
    model: &'static str,
    context_window_tokens: u32,
    max_output_tokens: u32,
    lifecycle: Lifecycle,
) -> Row {
    Row {
        family,
        model,
        limits: RecordLimits {
            context_window_tokens,
            max_output_tokens,
            lifecycle: Some(lifecycle),
        },
    }
}

/// Sourced rows only, read on 2026-11-03. A row is added when its numbers have been
/// read from the vendor page it cites, never estimated.
const ROWS: &[Row] = &[
    // Claude Opus 5.5, Active: 1M context and 128K synchronous output.
    // https://platform.claude.com/docs/en/models/overview
    row(
        Family::Anthropic,
        "claude-opus-5-5",
        1_000_000,
        128_000,
        Lifecycle::Ga,
    ),
    // Claude Sonnet 5.5, Active: 1M context and 128K synchronous output.
    // https://platform.claude.com/docs/en/models/overview
    row(
        Family::Anthropic,
        "claude-sonnet-5-5",
        1_000_000,
        128_000,
        Lifecycle::Ga,
    ),
    // Claude Haiku 5.5, Active: 1M context and 128K synchronous output.
    // https://platform.claude.com/docs/en/models/overview
    row(
        Family::Anthropic,
        "claude-haiku-5-5",
        1_000_000,
        128_000,
        Lifecycle::Ga,
    ),
    // Claude Haiku 4.5, the Foundry default: 200K context and 64K output. The model page
    // says "Legacy" while the deprecations table says "Active" with no retirement announced.
    // https://platform.claude.com/docs/en/models/haiku-4-5/overview
    // https://platform.claude.com/docs/en/about-claude/model-deprecations
    row(
        Family::Anthropic,
        "claude-haiku-4-5",
        200_000,
        64_000,
        Lifecycle::Ga,
    ),
    // Claude Sonnet 4.5: 200K context and 64K output. Deprecated on 2026-09-30, retiring
    // on 2026-11-30.
    // https://platform.claude.com/docs/en/models/sonnet-4-5/overview
    // https://platform.claude.com/docs/en/about-claude/model-deprecations
    row(
        Family::Anthropic,
        "claude-sonnet-4-5",
        200_000,
        64_000,
        Lifecycle::Deprecated,
    ),
    // Gemini 2.5 Pro on Vertex: 1,048,576 input and 65,536 output tokens. The page
    // announces its retirement on 2026-10-20 and says the model may still answer on the
    // Gemini API afterwards.
    // https://docs.cloud.google.com/vertex-ai/generative-ai/docs/models/gemini/2-5-pro
    row(
        Family::Gemini,
        "gemini-2.5-pro",
        1_048_576,
        65_536,
        Lifecycle::Deprecated,
    ),
    // Gemini 2.5 Flash: the same numbers and the same announced retirement.
    // https://docs.cloud.google.com/vertex-ai/generative-ai/docs/models/gemini/2-5-flash
    row(
        Family::Gemini,
        "gemini-2.5-flash",
        1_048_576,
        65_536,
        Lifecycle::Deprecated,
    ),
];

/// The record the table declares for `model` under `family`, read the way
/// [`super::effort::declared_support`] reads its id. `None` when no row names it.
#[must_use]
pub fn declared_record(family: Family, model: &str) -> Option<RecordLimits> {
    let model = model.trim().to_ascii_lowercase();
    ROWS.iter()
        .find(|entry| entry.family == family && entry.model == model)
        .map(|entry| entry.limits)
}

/// What a provider advertises for `model`: the declared record, with each token limit
/// lowered to the provider's own ceiling when that ceiling is lower. A zero ceiling
/// declares none. A deployment name resolves to its base model through `aliases`
/// first, as it does for the effort table.
#[must_use]
pub fn advertised(
    family: Family,
    aliases: &ModelAliases,
    model: &str,
    ceiling_context_tokens: u32,
    ceiling_output_tokens: u32,
) -> RecordLimits {
    let Some(declared) = declared_record(family, &aliases.base_model(model)) else {
        return RecordLimits::default();
    };
    RecordLimits {
        context_window_tokens: bounded(declared.context_window_tokens, ceiling_context_tokens),
        max_output_tokens: bounded(declared.max_output_tokens, ceiling_output_tokens),
        lifecycle: declared.lifecycle,
    }
}

/// `declared`, lowered to `ceiling` when the ceiling is set and lower than it.
fn bounded(declared: u32, ceiling: u32) -> u32 {
    if ceiling == 0 {
        declared
    } else {
        declared.min(ceiling)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_model_has_zero_limits() {
        // A provider ceiling bounds a row; it never invents one for a model with no row.
        assert_eq!(
            advertised(
                Family::OpenAi,
                &ModelAliases::default(),
                "gpt-5",
                128_000,
                16_384
            ),
            RecordLimits::default()
        );
        assert_eq!(declared_record(Family::Anthropic, "claude-unknown-9"), None);
    }

    #[test]
    fn alias_maps_to_base_row() {
        let aliases = ModelAliases::parse("prod-sonnet=claude-sonnet-5-5");
        let base = declared_record(Family::Anthropic, "claude-sonnet-5-5").expect("a sourced row");
        assert_eq!(
            advertised(Family::Anthropic, &aliases, "Prod-Sonnet", 0, 0),
            base
        );
    }

    #[test]
    fn provider_ceiling_clamps_the_table() {
        let none = ModelAliases::default();
        // Foundry serves Claude at 200k context and 8,192 output, and that ceiling holds there.
        let foundry = advertised(
            Family::Anthropic,
            &none,
            "claude-sonnet-5-5",
            200_000,
            8_192,
        );
        assert_eq!(foundry.context_window_tokens, 200_000);
        assert_eq!(foundry.max_output_tokens, 8_192);
        assert_eq!(foundry.lifecycle, Some(Lifecycle::Ga));
        // A zero ceiling declares nothing, so the row stands as the vendor states it.
        let direct = advertised(Family::Anthropic, &none, "claude-sonnet-5-5", 0, 0);
        assert_eq!(
            (direct.context_window_tokens, direct.max_output_tokens),
            (1_000_000, 128_000)
        );
    }

    #[test]
    fn family_keys_the_table() {
        assert_eq!(declared_record(Family::OpenAi, "claude-sonnet-5-5"), None);
        assert_eq!(declared_record(Family::Codex, "claude-sonnet-5-5"), None);
    }

    #[test]
    fn a_retiring_model_is_declared_deprecated() {
        let retiring =
            declared_record(Family::Anthropic, "claude-sonnet-4-5").expect("a sourced row");
        assert_eq!(retiring.lifecycle, Some(Lifecycle::Deprecated));
    }

    #[test]
    fn lifecycle_wire_names_are_the_contract() {
        assert_eq!(Lifecycle::Ga.as_str(), "ga");
        assert_eq!(Lifecycle::Preview.as_str(), "preview");
        assert_eq!(Lifecycle::Deprecated.as_str(), "deprecated");
    }

    #[test]
    fn every_row_has_a_source_comment() {
        // Read the source of record: each row sits under a comment citing a vendor page.
        let source = include_str!("model_record.rs");
        let lines: Vec<&str> = source.lines().collect();
        let mut rows = 0;
        for (index, line) in lines.iter().enumerate() {
            if !line.trim_start().starts_with("row(") {
                continue;
            }
            rows += 1;
            let cited = lines[..index]
                .iter()
                .rev()
                .take_while(|above| above.trim_start().starts_with("//"))
                .any(|above| above.contains("https://"));
            assert!(cited, "a row without a vendor citation above it: {line}");
        }
        assert_eq!(rows, ROWS.len(), "every row is written as a `row(` call");
    }
}
