//! The canonical reasoning-effort ladder.
//!
//! One ordered set of levels, one table of what each model family can express,
//! and one `resolve` that turns a requested level into the wire shape a provider
//! actually accepts. Before this module the same decision was made three times
//! (a gateway profile mapped to a token budget, then each adapter guessed what
//! the budget meant), which is how `deep` came to equal `quick` on the default
//! deploy.
//!
//! Inert by design: nothing in the request path calls it yet. The adapters
//! (`openai`, `anthropic`, `codex_subscription`) adopt it one at a time behind
//! their own switches, so with every switch off the wire is byte-identical to
//! before.
//!
//! # `verified_on`
//!
//! Provider parameter support drifts per model, so every row of the support
//! table carries the date it was last confirmed against a live provider. `None`
//! means "from documentation or the plan, not yet probed" and is the honest
//! value for every row until the live provider probe has run. A row whose
//! `verified_on` is `None` is a statement of intent, not of fact.

use std::collections::HashMap;
use std::sync::OnceLock;

use tracing::warn;

/// The environment variable that maps a deployment name to the base model whose
/// support row applies, as `deployment=base-model,deployment2=base-model2`.
/// Azure deployments carry operator-chosen names that say nothing about the
/// model behind them, and guessing from the name would send a parameter the
/// real model rejects.
pub const MODEL_ALIASES_ENV: &str = "INFERENCE_CORE_EFFORT_MODEL_ALIASES";

/// The most completion tokens a request may ask for once a reasoning allowance
/// has been added to the caller's own ceiling.
pub const TOTAL_TOKEN_CAP: i32 = 32_768;

/// A reasoning effort level, weakest first. The declaration order IS the
/// ladder, so the derived `Ord` is what clamping relies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl Level {
    /// Every level, weakest first.
    pub const ALL: [Level; 6] = [
        Level::Minimal,
        Level::Low,
        Level::Medium,
        Level::High,
        Level::XHigh,
        Level::Max,
    ];

    /// The canonical lowercase name, which is also what the catalogue lists in
    /// `ModelInfo.effort_levels` and what the `OpenAI` and Codex wires carry.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Minimal => "minimal",
            Level::Low => "low",
            Level::Medium => "medium",
            Level::High => "high",
            Level::XHigh => "xhigh",
            Level::Max => "max",
        }
    }

    /// Parse a canonical name, ignoring case and surrounding whitespace.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Level> {
        let name = name.trim().to_ascii_lowercase();
        Level::ALL
            .into_iter()
            .find(|level| level.as_str() == name)
    }
}

/// How a level is rendered for a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    /// The model has no effort control; the parameter is omitted.
    None,
    /// `thinking: {type: "enabled", budget_tokens}`, where the level picks the
    /// budget through [`allowance`].
    AnthropicBudget,
    /// `thinking: {type: "adaptive"}` plus `output_config: {effort}`.
    AnthropicAdaptive,
    /// `reasoning_effort` on a chat-completions body.
    OpenAiReasoningEffort,
    /// `reasoningEffort` on the subscription broker request.
    CodexEffort,
}

/// The provider family a model is served by. Wire support is a property of the
/// family and the model together: the same model name can be served by two
/// families that render effort differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    OpenAi,
    Anthropic,
    Codex,
}

/// What one model can express.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Support {
    pub wire: Wire,
    /// The levels the model accepts, weakest first. Empty exactly when `wire`
    /// is [`Wire::None`].
    pub levels: &'static [Level],
    /// The date this row was last confirmed against a live provider, or `None`
    /// when it has not been probed. See the module docs.
    pub verified_on: Option<&'static str>,
}

impl Support {
    /// A model with no effort control.
    pub const NONE: Support = Support {
        wire: Wire::None,
        levels: &[],
        verified_on: None,
    };
}

/// A level resolved against one model's support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    pub wire: Wire,
    pub requested: Level,
    pub applied: Level,
}

impl Resolved {
    /// True when the model could not honour the requested level as asked.
    #[must_use]
    pub fn clamped(&self) -> bool {
        self.requested != self.applied
    }
}

const GPT5_LEVELS: &[Level] = &[Level::Minimal, Level::Low, Level::Medium, Level::High];
const THREE_LEVELS: &[Level] = &[Level::Low, Level::Medium, Level::High];
const ADAPTIVE_LEVELS: &[Level] = &[
    Level::Low,
    Level::Medium,
    Level::High,
    Level::XHigh,
    Level::Max,
];
// The subscription broker accepts only these two values today.
const CODEX_LEVELS: &[Level] = &[Level::Low, Level::High];

struct Row {
    family: Family,
    matches: fn(&str) -> bool,
    support: Support,
}

/// First match wins, so a narrower row must precede the broader one it carves a
/// hole in (`gpt-5-chat` before `gpt-5`, `o1-mini` before `o1`).
const ROWS: &[Row] = &[
    // OpenAI and Azure OpenAI.
    Row {
        family: Family::OpenAi,
        // The chat-tuned gpt-5 alias is not a reasoning model.
        matches: |m| m.starts_with("gpt-5-chat"),
        support: Support::NONE,
    },
    Row {
        family: Family::OpenAi,
        matches: |m| in_series(m, "gpt-5"),
        support: Support {
            wire: Wire::OpenAiReasoningEffort,
            levels: GPT5_LEVELS,
            verified_on: None,
        },
    },
    Row {
        family: Family::OpenAi,
        // Point releases drop `minimal`; whether they accept `xhigh` is for the
        // live probe to say, so it is not claimed here.
        matches: |m| m.starts_with("gpt-5."),
        support: Support {
            wire: Wire::OpenAiReasoningEffort,
            levels: THREE_LEVELS,
            verified_on: None,
        },
    },
    Row {
        family: Family::OpenAi,
        // The first o1 models reject the parameter outright.
        matches: |m| m.starts_with("o1-mini") || m.starts_with("o1-preview"),
        support: Support::NONE,
    },
    Row {
        family: Family::OpenAi,
        matches: |m| in_series(m, "o1") || in_series(m, "o3") || in_series(m, "o4"),
        support: Support {
            wire: Wire::OpenAiReasoningEffort,
            levels: THREE_LEVELS,
            verified_on: None,
        },
    },
    Row {
        family: Family::OpenAi,
        // Declared, not merely unlisted: a configured model with no row is a
        // test failure, and these are the non-reasoning models the roster ships.
        matches: |m| m.starts_with("gpt-4") || m.starts_with("gpt-3") || m.starts_with("chatgpt"),
        support: Support::NONE,
    },
    // Anthropic direct and Foundry. Pre-3.7 models reject extended thinking.
    Row {
        family: Family::Anthropic,
        matches: |m| claude_version(m).is_some_and(|version| version < (3, 7)),
        support: Support::NONE,
    },
    // Through 4.6 the budget form is the contract.
    Row {
        family: Family::Anthropic,
        matches: |m| claude_version(m).is_some_and(|version| version < (4, 7)),
        support: Support {
            wire: Wire::AnthropicBudget,
            levels: THREE_LEVELS,
            verified_on: None,
        },
    },
    // From 4.7 `budget_tokens` is rejected, and a model whose version cannot be
    // read is treated like a future one: asking a new model to think fails
    // loudly and is fixed with a row, where an allowlist would drop thinking
    // silently for every model nobody had added yet.
    Row {
        family: Family::Anthropic,
        matches: |m| m.starts_with("claude"),
        support: Support {
            wire: Wire::AnthropicAdaptive,
            levels: ADAPTIVE_LEVELS,
            verified_on: None,
        },
    },
    // Subscription-backed Codex: the broker, not the model, bounds the levels.
    Row {
        family: Family::Codex,
        matches: |_| true,
        support: Support {
            wire: Wire::CodexEffort,
            levels: CODEX_LEVELS,
            verified_on: None,
        },
    },
];

/// True for the bare series name or any `<series>-...` variant, so `o3` and
/// `o3-mini` match but `o30` does not.
fn in_series(model: &str, series: &str) -> bool {
    model == series
        || model
            .strip_prefix(series)
            .is_some_and(|rest| rest.starts_with('-'))
}

/// The `(major, minor)` of a Claude model id, read from its numeric tokens and
/// skipping date stamps: `claude-sonnet-4-5` is `(4, 5)`,
/// `claude-opus-4-1-20250805` is `(4, 1)`, `claude-sonnet-4-20250514` is
/// `(4, 0)`, `claude-3-7-sonnet` is `(3, 7)`. `None` when no version can be read.
fn claude_version(model: &str) -> Option<(u32, u32)> {
    let mut numbers = model
        .split(['-', '.', '@', '_'])
        .filter(|token| !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit()))
        // A date stamp or build id, not a version component.
        .filter(|token| token.len() < 6)
        .filter_map(|token| token.parse::<u32>().ok());
    let major = numbers.next()?;
    Some((major, numbers.next().unwrap_or(0)))
}

/// The support row declared for this model, or `None` when no row names it.
/// An undeclared model is not the same as one declared without effort control:
/// the former is a gap in the table.
#[must_use]
pub fn declared_support(family: Family, model: &str) -> Option<Support> {
    let model = model.trim().to_ascii_lowercase();
    ROWS.iter()
        .find(|row| row.family == family && (row.matches)(&model))
        .map(|row| row.support)
}

/// What this model can express. A model with no declared row is treated as
/// having no effort control, because sending a parameter a model rejects is a
/// hard 400 where omitting it is a quiet no-op.
#[must_use]
pub fn support(family: Family, model: &str) -> Support {
    declared_support(family, model).unwrap_or(Support::NONE)
}

/// Resolve a requested level against a model's support.
///
/// The nearest weaker supported level is applied: asking for more effort than a
/// model offers gets its strongest, never a silent jump upward. When the model's
/// weakest level is still stronger than the request, that weakest level is
/// applied. `None` means "send nothing": no level was requested, or the model
/// has no effort control.
#[must_use]
pub fn resolve(requested: Option<Level>, support: Support) -> Option<Resolved> {
    let requested = requested?;
    if support.wire == Wire::None {
        return None;
    }
    let applied = support
        .levels
        .iter()
        .copied()
        .filter(|level| *level <= requested)
        .max()
        .or_else(|| support.levels.iter().copied().min())?;
    Some(Resolved {
        wire: support.wire,
        requested,
        applied,
    })
}

/// Map a legacy thinking budget onto the ladder. The gateway's profiles used to
/// be expressed only as a token budget, and 0 means "no thinking requested".
#[must_use]
pub fn from_legacy_budget(thinking_budget_tokens: i32) -> Option<Level> {
    match thinking_budget_tokens {
        i32::MIN..=0 => None,
        1..=1024 => Some(Level::Low),
        1025..=2048 => Some(Level::Medium),
        _ => Some(Level::High),
    }
}

/// Extra completion tokens a level may spend on reasoning, on top of the
/// caller's own answer ceiling. Without it a reasoning model spends the whole
/// ceiling thinking and returns an empty answer.
///
/// For [`Wire::AnthropicBudget`] the same figure is the `budget_tokens`, which
/// is why Low, Medium and High equal the legacy budgets 1024, 2048 and 4096.
#[must_use]
pub fn allowance(level: Level) -> i32 {
    match level {
        Level::Minimal => 512,
        Level::Low => 1_024,
        Level::Medium => 2_048,
        Level::High => 4_096,
        Level::XHigh => 8_192,
        Level::Max => 16_384,
    }
}

/// The completion-token ceiling to send: the caller's ceiling plus the level's
/// allowance, bounded by [`TOTAL_TOKEN_CAP`]. A ceiling the caller already set
/// above the cap is left as it is, never lowered.
#[must_use]
pub fn raised_ceiling(ceiling: i32, level: Level) -> i32 {
    ceiling.max(ceiling.saturating_add(allowance(level)).min(TOTAL_TOKEN_CAP))
}

/// Switch for clamping effort to Low when the org's budget is exhausted. Off
/// unless the literal `true`, read once at startup.
pub const BUDGET_CLAMP_ENV: &str = "INFERENCE_CORE_EFFORT_BUDGET_CLAMP";

/// Whether an explicit env value turns the switch on. Pure so the policy is
/// unit-testable without touching the process environment; a stray `1` or `on`
/// must not start overriding what the user asked for.
fn parse_enabled(raw: Option<&str>) -> bool {
    raw.map(str::trim) == Some("true")
}

/// Whether the budget clamp is on for this process.
#[must_use]
pub fn budget_clamp_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| parse_enabled(std::env::var(BUDGET_CLAMP_ENV).ok().as_deref()))
}

/// Bound a level by a cap: clamps down, never up. `None` is no cap, which is
/// every caller's answer until the per-role cap (owned by Control, delivered by
/// WS10) exists; the seam is here so that source plugs in without touching the
/// callers.
#[must_use]
pub fn clamp_to_cap(level: Level, cap: Option<Level>) -> Level {
    cap.map_or(level, |cap| level.min(cap))
}

/// Deployment-name to base-model aliases, from [`MODEL_ALIASES_ENV`].
#[derive(Debug, Clone, Default)]
pub struct ModelAliases(HashMap<String, String>);

impl ModelAliases {
    /// Parse `deployment=base-model` pairs separated by commas. Names are
    /// matched case-insensitively; a malformed pair is skipped with a warning
    /// rather than silently dropped.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        let mut aliases = HashMap::new();
        for pair in raw.split(',').map(str::trim).filter(|pair| !pair.is_empty()) {
            match pair.split_once('=') {
                Some((deployment, base))
                    if !deployment.trim().is_empty() && !base.trim().is_empty() =>
                {
                    aliases.insert(
                        deployment.trim().to_ascii_lowercase(),
                        base.trim().to_ascii_lowercase(),
                    );
                }
                _ => warn!(
                    env = MODEL_ALIASES_ENV,
                    "ignoring a malformed model alias; expected deployment=base-model"
                ),
            }
        }
        Self(aliases)
    }

    /// Read the aliases from the environment; unset means none.
    #[must_use]
    pub fn from_env() -> Self {
        std::env::var(MODEL_ALIASES_ENV)
            .map(|raw| Self::parse(&raw))
            .unwrap_or_default()
    }

    /// The model whose row applies to `model`: its alias target, or `model`
    /// itself when it has none.
    #[must_use]
    pub fn base_model(&self, model: &str) -> String {
        let key = model.trim().to_ascii_lowercase();
        self.0.get(&key).cloned().unwrap_or(key)
    }

    /// [`support`] after resolving the deployment alias.
    #[must_use]
    pub fn support(&self, family: Family, model: &str) -> Support {
        support(family, &self.base_model(model))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::InferenceConfig;
    use crate::provider::routing_policy::RoutingPolicy;

    #[test]
    fn ladder_is_ordered() {
        assert!(Level::ALL.windows(2).all(|pair| pair[0] < pair[1]));
        for row in ROWS {
            assert!(
                row.support.levels.windows(2).all(|pair| pair[0] < pair[1]),
                "a row's levels must be strictly ascending"
            );
            assert_eq!(
                row.support.wire == Wire::None,
                row.support.levels.is_empty(),
                "a row has levels exactly when it has a wire"
            );
        }
    }

    #[test]
    fn names_round_trip() {
        for level in Level::ALL {
            assert_eq!(Level::from_name(level.as_str()), Some(level));
        }
        assert_eq!(Level::from_name("  HIGH "), Some(Level::High));
        assert_eq!(Level::from_name("ultra"), None);
    }

    #[test]
    fn clamp_picks_nearest_weaker() {
        let codex = support(Family::Codex, "gpt-5.5");
        let applied = |requested| resolve(Some(requested), codex).map(|r| r.applied);
        assert_eq!(applied(Level::Medium), Some(Level::Low));
        assert_eq!(applied(Level::XHigh), Some(Level::High));
        assert_eq!(applied(Level::Max), Some(Level::High));
        assert_eq!(applied(Level::High), Some(Level::High));
        assert!(resolve(Some(Level::Medium), codex).unwrap().clamped());
        assert!(!resolve(Some(Level::High), codex).unwrap().clamped());
    }

    #[test]
    fn no_weaker_level_picks_weakest_supported() {
        let o3 = support(Family::OpenAi, "o3");
        assert_eq!(o3.levels.first(), Some(&Level::Low));
        let resolved = resolve(Some(Level::Minimal), o3).expect("o3 has effort control");
        assert_eq!(resolved.applied, Level::Low);
        assert!(resolved.clamped());
    }

    #[test]
    fn unsupported_model_resolves_to_none() {
        let gpt4o = support(Family::OpenAi, "gpt-4o-mini");
        assert_eq!(gpt4o.wire, Wire::None);
        assert_eq!(resolve(Some(Level::High), gpt4o), None);
        // An undeclared model is also unsupported rather than guessed at.
        assert_eq!(
            support(Family::OpenAi, "some-private-deployment"),
            Support::NONE
        );
        // No request means no parameter, whatever the model supports.
        assert_eq!(resolve(None, support(Family::OpenAi, "gpt-5-mini")), None);
    }

    #[test]
    fn legacy_budget_maps_1024_to_low_and_4096_to_high() {
        assert_eq!(from_legacy_budget(0), None);
        assert_eq!(from_legacy_budget(-1), None);
        assert_eq!(from_legacy_budget(1), Some(Level::Low));
        assert_eq!(from_legacy_budget(1024), Some(Level::Low));
        assert_eq!(from_legacy_budget(1025), Some(Level::Medium));
        assert_eq!(from_legacy_budget(2048), Some(Level::Medium));
        assert_eq!(from_legacy_budget(2049), Some(Level::High));
        assert_eq!(from_legacy_budget(4096), Some(Level::High));
        assert_eq!(from_legacy_budget(i32::MAX), Some(Level::High));
    }

    #[test]
    fn cap_clamps_down_never_up() {
        assert_eq!(clamp_to_cap(Level::Max, Some(Level::Low)), Level::Low);
        assert_eq!(clamp_to_cap(Level::High, Some(Level::High)), Level::High);
        // A cap above the request must not raise it.
        assert_eq!(clamp_to_cap(Level::Minimal, Some(Level::Max)), Level::Minimal);
        assert_eq!(clamp_to_cap(Level::Low, Some(Level::Medium)), Level::Low);
        // No cap leaves every level alone.
        for level in Level::ALL {
            assert_eq!(clamp_to_cap(level, None), level);
        }
    }

    #[test]
    fn only_the_literal_true_enables_the_budget_clamp() {
        assert!(parse_enabled(Some("true")));
        assert!(parse_enabled(Some(" true ")));
        for off in ["1", "on", "TRUE", "yes", "false", ""] {
            assert!(!parse_enabled(Some(off)), "{off:?} must leave it off");
        }
        assert!(!parse_enabled(None));
    }

    #[test]
    fn allowance_is_monotonic() {
        let figures: Vec<i32> = Level::ALL.into_iter().map(allowance).collect();
        assert!(figures.windows(2).all(|pair| pair[0] < pair[1]));
        // Low, Medium and High are the legacy budgets, so a budget-class wire
        // and the legacy mapping agree.
        for budget in [1024, 2048, 4096] {
            let level = from_legacy_budget(budget).expect("a positive budget maps to a level");
            assert_eq!(allowance(level), budget);
        }
        assert_eq!(allowance(Level::Max), 16_384);
    }

    #[test]
    fn raised_ceiling_adds_the_allowance_up_to_the_total_cap() {
        assert_eq!(raised_ceiling(4096, Level::Low), 5120);
        assert_eq!(raised_ceiling(4096, Level::Max), 20_480);
        assert_eq!(raised_ceiling(30_000, Level::Max), TOTAL_TOKEN_CAP);
        // A ceiling the caller already set above the cap is never lowered.
        assert_eq!(raised_ceiling(40_000, Level::High), 40_000);
        assert_eq!(raised_ceiling(i32::MAX, Level::Max), i32::MAX);
    }

    #[test]
    fn openai_rows_cover_the_reasoning_and_non_reasoning_families() {
        let wire = |model| support(Family::OpenAi, model).wire;
        assert_eq!(wire("gpt-5"), Wire::OpenAiReasoningEffort);
        assert_eq!(wire("gpt-5-mini"), Wire::OpenAiReasoningEffort);
        assert_eq!(wire("GPT-5-Nano"), Wire::OpenAiReasoningEffort);
        assert_eq!(wire("gpt-5.1"), Wire::OpenAiReasoningEffort);
        assert_eq!(wire("o3"), Wire::OpenAiReasoningEffort);
        assert_eq!(wire("o3-mini"), Wire::OpenAiReasoningEffort);
        assert_eq!(wire("o4-mini"), Wire::OpenAiReasoningEffort);
        assert_eq!(wire("gpt-5-chat-latest"), Wire::None);
        assert_eq!(wire("o1-mini"), Wire::None);
        assert_eq!(wire("o30"), Wire::None);
        assert_eq!(wire("gpt-4o-mini"), Wire::None);
        // `minimal` is a gpt-5 level that the o-series does not have.
        assert!(support(Family::OpenAi, "gpt-5-mini")
            .levels
            .contains(&Level::Minimal));
        assert!(!support(Family::OpenAi, "o3").levels.contains(&Level::Minimal));
    }

    #[test]
    fn anthropic_rows_split_budget_adaptive_and_none_by_version() {
        let wire = |model| support(Family::Anthropic, model).wire;
        for budget in [
            "claude-3-7-sonnet-20250219",
            "claude-sonnet-4-20250514",
            "claude-opus-4-1-20250805",
            "claude-sonnet-4-5",
            "claude-haiku-4-5",
            "claude-sonnet-4-6",
        ] {
            assert_eq!(wire(budget), Wire::AnthropicBudget, "{budget}");
        }
        for adaptive in [
            "claude-opus-4-7",
            "claude-opus-4-8",
            "claude-opus-5",
            "claude-opus-6",
            "claude-brand-new-name",
        ] {
            assert_eq!(wire(adaptive), Wire::AnthropicAdaptive, "{adaptive}");
        }
        for none in [
            "claude-2.1",
            "claude-instant-1.2",
            "claude-3-opus-20240229",
            "claude-3-5-sonnet-20241022",
            "claude-3.5-haiku",
        ] {
            assert_eq!(wire(none), Wire::None, "{none}");
        }
        assert_eq!(wire("gpt-5-mini"), Wire::None, "not a Claude model");
    }

    #[test]
    fn claude_versions_skip_date_stamps() {
        assert_eq!(claude_version("claude-sonnet-4-5"), Some((4, 5)));
        assert_eq!(claude_version("claude-sonnet-4-20250514"), Some((4, 0)));
        assert_eq!(claude_version("claude-opus-4-1-20250805"), Some((4, 1)));
        assert_eq!(claude_version("claude-3-7-sonnet-20250219"), Some((3, 7)));
        assert_eq!(claude_version("claude-opus-5"), Some((5, 0)));
        assert_eq!(claude_version("claude-sonnet-4-5@20250929"), Some((4, 5)));
        assert_eq!(claude_version("claude-latest"), None);
    }

    #[test]
    fn every_codex_model_is_the_broker_pair() {
        let support = support(Family::Codex, "anything-the-operator-enables");
        assert_eq!(support.wire, Wire::CodexEffort);
        assert_eq!(support.levels, &[Level::Low, Level::High]);
    }

    #[test]
    fn aliases_resolve_a_deployment_to_its_base_model() {
        let aliases = ModelAliases::parse("prod-chat=gpt-5-mini, Reason-Deep = O3 ,bad,=x,y=");
        assert_eq!(aliases.base_model("prod-chat"), "gpt-5-mini");
        assert_eq!(aliases.base_model("PROD-CHAT"), "gpt-5-mini");
        assert_eq!(aliases.base_model("reason-deep"), "o3");
        assert_eq!(aliases.base_model("gpt-4o-mini"), "gpt-4o-mini");
        assert_eq!(
            aliases.support(Family::OpenAi, "prod-chat").wire,
            Wire::OpenAiReasoningEffort
        );
        assert_eq!(
            ModelAliases::default()
                .support(Family::OpenAi, "prod-chat")
                .wire,
            Wire::None,
            "an unaliased deployment name is not guessed at"
        );
    }

    #[test]
    fn every_configured_default_model_has_a_declared_support() {
        let config = InferenceConfig::from_env().expect("default configuration builds");
        let mut roster: Vec<(Family, String)> = Vec::new();
        roster.extend(
            config
                .openai_chat_models
                .iter()
                .map(|model| (Family::OpenAi, model.clone())),
        );
        roster.extend(
            config
                .azure_anthropic_deployments
                .iter()
                .map(|model| (Family::Anthropic, model.clone())),
        );
        let policy = RoutingPolicy::default();
        let table = &policy.table;
        let routed = [&table.budget, &table.balance, &table.genius]
            .into_iter()
            .flat_map(|row| [&row.simple, &row.moderate, &row.complex])
            .chain(policy.tool_fallback_ladder.iter())
            .chain(std::iter::once(&policy.cheap_fallback));
        for model in routed {
            let family = if model.starts_with("claude") {
                Family::Anthropic
            } else {
                Family::OpenAi
            };
            roster.push((family, model.clone()));
        }
        assert!(!roster.is_empty());
        for (family, model) in roster {
            assert!(
                declared_support(family, &model).is_some(),
                "{model} ({family:?}) is shipped as a default but has no effort support row"
            );
        }
    }
}
