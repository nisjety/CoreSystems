//! The offline routing diff (EP-14 S10): what a candidate routing policy would change, computed before a candidate
//! is recorded, without touching the network or any store.
//!
//! The diff replays a checked-in corpus of synthetic requests through the same pure functions the intent layer runs
//! on every request: [`classify`] for complexity and [`choose`] for the model. It walks the three Verevon modes and
//! every budget posture, so a reviewer sees each cell a candidate moves. It answers which cells move, not whether the
//! new routing is better. The corpus models traffic and is not traffic, and the diff's own output says so.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::provider::intent::{choose, classify, BudgetPosture, VerevonMode};
use crate::provider::routing_policy::RoutingPolicy;
use crate::provider::{ChatMessage, ToolDefinition};

/// The corpus the diff replays. It is checked in beside the integration fixtures and compiled into the binary, so an
/// admin path can diff without reading a file at runtime.
const ROUTING_CORPUS_V1: &str = include_str!("../../tests/fixtures/routing-corpus-v1.json");

/// The label every corpus file carries, and every diff echoes.
pub const CORPUS_LABEL: &str = "routing-corpus-v1";

/// What every diff says about the traffic it replayed.
const CORPUS_NOTE: &str = "replayed over synthetic requests: a model of traffic, not traffic";

const MODES: [VerevonMode; 3] = [VerevonMode::Budget, VerevonMode::Balance, VerevonMode::Genius];

const POSTURES: [BudgetPosture; 4] = [
    BudgetPosture::Healthy,
    BudgetPosture::Constrained,
    BudgetPosture::Exhausted,
    BudgetPosture::Unknown,
];

/// One synthetic request as the intent layer sees it: its turns, the tools offered and the tool choice.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CorpusCase {
    pub id: String,
    pub turns: Vec<CorpusTurn>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub tool_choice: String,
}

/// One turn. `repeat` repeats the content, so a case can reach a length without a long literal in the file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CorpusTurn {
    pub role: String,
    pub content: String,
    #[serde(default = "one")]
    pub repeat: usize,
}

fn one() -> usize {
    1
}

#[derive(Deserialize)]
struct CorpusFile {
    corpus: String,
    synthetic: bool,
    cases: Vec<CorpusCase>,
}

/// Parse a corpus file. A file that does not declare itself synthetic, or names another corpus, is refused: the diff
/// must never replay anything that could be real traffic.
///
/// # Errors
///
/// Returns the reason the file is not a usable corpus.
pub fn load_corpus(raw: &str) -> Result<Vec<CorpusCase>, String> {
    let file: CorpusFile =
        serde_json::from_str(raw).map_err(|error| format!("corpus does not parse: {error}"))?;
    if file.corpus != CORPUS_LABEL {
        return Err(format!("corpus `{}` is not {CORPUS_LABEL}", file.corpus));
    }
    if !file.synthetic {
        return Err("corpus must declare itself synthetic".to_owned());
    }
    Ok(file.cases)
}

/// The checked-in corpus, parsed.
///
/// # Errors
///
/// Returns the reason the checked-in file is not a usable corpus.
pub fn embedded_corpus() -> Result<Vec<CorpusCase>, String> {
    load_corpus(ROUTING_CORPUS_V1)
}

/// What a diff needs to know about a model beyond its id: whether the price source has a row for it, and whether the
/// attestation source has a ZDR route for it. The caller holds those sources, so this module stays pure.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteFacts {
    priced: BTreeSet<String>,
    zdr_routed: BTreeSet<String>,
}

impl RouteFacts {
    #[must_use]
    pub fn new(
        priced: impl IntoIterator<Item = impl AsRef<str>>,
        zdr_routed: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        Self {
            priced: priced.into_iter().map(|model| key(model.as_ref())).collect(),
            zdr_routed: zdr_routed
                .into_iter()
                .map(|model| key(model.as_ref()))
                .collect(),
        }
    }

    fn is_priced(&self, model: &str) -> bool {
        self.priced.contains(&key(model))
    }

    fn is_zdr_routed(&self, model: &str) -> bool {
        self.zdr_routed.contains(&key(model))
    }
}

/// Model ids compare the way the roster does: without case or surrounding space.
fn key(model: &str) -> String {
    model.trim().to_ascii_lowercase()
}

/// One replayed cell: a case under one mode and posture, with the model each policy resolves it to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffRow {
    pub case_id: String,
    pub mode: &'static str,
    pub posture: &'static str,
    pub live_model: String,
    pub candidate_model: String,
    pub changed: bool,
}

/// What a candidate would change. Stored as the candidate's `diff` document and shown to the reviewer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoutingDiff {
    pub corpus: &'static str,
    pub note: &'static str,
    /// Every replayed cell, changed or not.
    pub rows: Vec<DiffRow>,
    /// How many cells the candidate moves.
    pub changed: usize,
    /// Every model the candidate can route to that has no price row, sorted.
    pub unknown_models: Vec<String>,
    /// The changed cells that lose a ZDR route: the live model has one, and the candidate's model does not.
    pub zdr_route_changes: Vec<DiffRow>,
}

/// Replay `corpus` through both policies and report every cell that moves. Pure: the same inputs always give the same
/// diff.
#[must_use]
pub fn diff(
    live: &RoutingPolicy,
    candidate: &RoutingPolicy,
    corpus: &[CorpusCase],
    facts: &RouteFacts,
) -> RoutingDiff {
    let mut rows = Vec::with_capacity(corpus.len() * MODES.len() * POSTURES.len());
    for case in corpus {
        let (messages, tools) = case_inputs(case);
        let live_complexity = classify(live, &messages, &tools, &case.tool_choice);
        let candidate_complexity = classify(candidate, &messages, &tools, &case.tool_choice);
        for mode in MODES {
            for posture in POSTURES {
                let live_model = choose(live, mode, live_complexity, posture).to_owned();
                let candidate_model =
                    choose(candidate, mode, candidate_complexity, posture).to_owned();
                rows.push(DiffRow {
                    case_id: case.id.clone(),
                    mode: mode.as_str(),
                    posture: posture_name(posture),
                    changed: live_model != candidate_model,
                    live_model,
                    candidate_model,
                });
            }
        }
    }
    let changed = rows.iter().filter(|row| row.changed).count();
    let unknown: BTreeSet<String> = routed_models(candidate)
        .into_iter()
        .filter(|model| !facts.is_priced(model))
        .collect();
    let zdr_route_changes = rows
        .iter()
        .filter(|row| {
            row.changed
                && facts.is_zdr_routed(&row.live_model)
                && !facts.is_zdr_routed(&row.candidate_model)
        })
        .cloned()
        .collect();
    RoutingDiff {
        corpus: CORPUS_LABEL,
        note: CORPUS_NOTE,
        rows,
        changed,
        unknown_models: unknown.into_iter().collect(),
        zdr_route_changes,
    }
}

/// The messages and tools the intent layer would see for a case.
fn case_inputs(case: &CorpusCase) -> (Vec<ChatMessage>, Vec<ToolDefinition>) {
    let messages = case
        .turns
        .iter()
        .map(|turn| ChatMessage {
            compaction_summary: String::new(),
            role: turn.role.clone(),
            content: turn.content.repeat(turn.repeat),
            name: String::new(),
        })
        .collect();
    let tools = case
        .tools
        .iter()
        .map(|name| ToolDefinition {
            name: name.clone(),
            description: String::new(),
            parameters_json: "{}".to_owned(),
        })
        .collect();
    (messages, tools)
}

/// Every model a policy can route a request to: the mode table, the cheap fallback, the tool ladder and the side-task
/// role candidates.
fn routed_models(policy: &RoutingPolicy) -> Vec<String> {
    let rows = [
        &policy.table.budget,
        &policy.table.balance,
        &policy.table.genius,
    ];
    let mut models: Vec<String> = rows
        .into_iter()
        .flat_map(|row| [row.simple.clone(), row.moderate.clone(), row.complex.clone()])
        .collect();
    models.push(policy.cheap_fallback.clone());
    models.extend(policy.tool_fallback_ladder.iter().cloned());
    models.extend(
        policy
            .roles
            .values()
            .flat_map(|entry| entry.candidates.iter().cloned()),
    );
    models
}

fn posture_name(posture: BudgetPosture) -> &'static str {
    match posture {
        BudgetPosture::Healthy => "healthy",
        BudgetPosture::Constrained => "constrained",
        BudgetPosture::Exhausted => "exhausted",
        BudgetPosture::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::routing_policy::{RoleEntry, RolePurpose};
    use crate::provider::ModelInfo;

    /// The models the default policy routes to. The Claude deployments are the ZDR-routed ones.
    fn roster() -> RouteFacts {
        RouteFacts::new(
            [
                "gpt-5-nano",
                "gpt-4o-mini",
                "gpt-5-mini",
                "claude-sonnet-4-6",
                "claude-sonnet-4-5",
                "claude-opus-4-8",
                "claude-haiku-4-5",
            ],
            [
                "claude-sonnet-4-6",
                "claude-sonnet-4-5",
                "claude-opus-4-8",
                "claude-haiku-4-5",
            ],
        )
    }

    #[test]
    fn routing_diff_identical_policy_has_no_changes() {
        let corpus = embedded_corpus().expect("the checked-in corpus loads");
        let policy = RoutingPolicy::default();
        let report = diff(&policy, &policy, &corpus, &roster());
        assert_eq!(report.changed, 0);
        assert!(report.rows.iter().all(|row| !row.changed));
        assert!(report.unknown_models.is_empty(), "{:?}", report.unknown_models);
        assert!(report.zdr_route_changes.is_empty());
        assert_eq!(report.rows.len(), corpus.len() * MODES.len() * POSTURES.len());
    }

    #[test]
    fn routing_diff_flags_unpriced_model() {
        let corpus = embedded_corpus().expect("the checked-in corpus loads");
        let live = RoutingPolicy::default();
        let mut candidate = RoutingPolicy::default();
        candidate.table.budget.simple = "unpriced-model".to_owned();
        let report = diff(&live, &candidate, &corpus, &roster());
        assert_eq!(report.unknown_models, ["unpriced-model"]);
        assert!(
            report
                .rows
                .iter()
                .any(|row| row.changed && row.candidate_model == "unpriced-model"),
            "a simple budget turn moves to the unpriced model"
        );
    }

    #[test]
    fn routing_diff_flags_zdr_route_change() {
        let corpus = embedded_corpus().expect("the checked-in corpus loads");
        let live = RoutingPolicy::default();
        let mut candidate = RoutingPolicy::default();
        // Genius on a complex turn is Opus today, a ZDR-routed deployment. The candidate sends it to a model with no
        // ZDR route.
        candidate.table.genius.complex = "gpt-5-mini".to_owned();
        let report = diff(&live, &candidate, &corpus, &roster());
        assert!(!report.zdr_route_changes.is_empty());
        for row in &report.zdr_route_changes {
            assert!(row.changed);
            assert_eq!(row.mode, "genius");
            assert_eq!(row.live_model, "claude-opus-4-8");
            assert_eq!(row.candidate_model, "gpt-5-mini");
        }
        assert!(report.unknown_models.is_empty());
    }

    #[test]
    fn a_candidate_with_roles_validates_and_round_trips() {
        // EP-13 S4 gave the policy a role table. The candidate store keeps the policy as its own JSON, so the roles must
        // validate against the chat catalogue, survive the round trip, and diff like any other field.
        let catalogue = vec![
            ModelInfo {
                id: "gpt-5-nano".to_owned(),
                modality: "chat".to_owned(),
                ..Default::default()
            },
            ModelInfo {
                id: "claude-opus-4-8".to_owned(),
                modality: "chat".to_owned(),
                ..Default::default()
            },
        ];
        let mut candidate = RoutingPolicy::default();
        candidate.roles.insert(
            RolePurpose::Title,
            RoleEntry {
                candidates: vec!["gpt-5-nano".to_owned()],
                effort: Some("minimal".to_owned()),
            },
        );
        candidate.roles.insert(
            RolePurpose::ResearchPlan,
            RoleEntry {
                candidates: vec!["claude-opus-4-8".to_owned()],
                effort: None,
            },
        );
        assert_eq!(candidate.validate_roles(&catalogue), Ok(()));
        let stored = serde_json::to_string(&candidate).expect("the candidate serialises");
        let reread: RoutingPolicy = serde_json::from_str(&stored).expect("the candidate parses");
        assert_eq!(reread, candidate);
        let corpus = embedded_corpus().expect("the checked-in corpus loads");
        let report = diff(&RoutingPolicy::default(), &reread, &corpus, &roster());
        assert!(report.unknown_models.is_empty(), "{:?}", report.unknown_models);
    }

    #[test]
    fn the_embedded_corpus_reaches_every_tier_and_lever() {
        let corpus = embedded_corpus().expect("the checked-in corpus loads");
        assert!(corpus.len() >= 12, "{} cases", corpus.len());
        assert!(corpus.iter().any(|case| !case.tools.is_empty()), "tool use");
        assert!(
            corpus.iter().any(|case| case.tool_choice == "required"),
            "a required tool choice"
        );
        assert!(
            corpus
                .iter()
                .flat_map(|case| case.turns.iter())
                .any(|turn| turn.content.contains("```")),
            "a code fence"
        );
        assert!(
            corpus.iter().any(|case| case.turns.len() > 12),
            "a deep conversation"
        );
        assert!(
            corpus.iter().any(|case| {
                let (messages, _) = case_inputs(case);
                messages.iter().map(|m| m.content.chars().count()).sum::<usize>() > 4000
            }),
            "a large context"
        );
        let policy = RoutingPolicy::default();
        let tiers: BTreeSet<&str> = corpus
            .iter()
            .map(|case| {
                let (messages, tools) = case_inputs(case);
                classify(&policy, &messages, &tools, &case.tool_choice).as_str()
            })
            .collect();
        assert_eq!(tiers.len(), 3, "{tiers:?}");
    }

    #[test]
    fn a_corpus_that_is_not_synthetic_is_refused() {
        let real = r#"{"corpus": "routing-corpus-v1", "synthetic": false, "cases": []}"#;
        assert!(load_corpus(real).unwrap_err().contains("synthetic"));
        let other = r#"{"corpus": "other", "synthetic": true, "cases": []}"#;
        assert!(load_corpus(other).unwrap_err().contains(CORPUS_LABEL));
    }

    #[test]
    fn the_diff_is_deterministic_for_the_same_inputs() {
        let corpus = embedded_corpus().expect("the checked-in corpus loads");
        let live = RoutingPolicy::default();
        let mut candidate = RoutingPolicy::default();
        candidate.table.balance.moderate = "gpt-4o-mini".to_owned();
        let first = diff(&live, &candidate, &corpus, &roster());
        assert_eq!(first, diff(&live, &candidate, &corpus, &roster()));
        assert_eq!(first.corpus, CORPUS_LABEL);
    }
}
