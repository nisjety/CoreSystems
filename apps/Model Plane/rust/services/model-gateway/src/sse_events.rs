//! Chat-parity rich SSE event taxonomy (docs/chat-parity-audit.md §2).
//!
//! ONE forward-compatible event set for `/v1/invoke/stream`. Each [`ChatEvent`]
//! maps to an SSE `event:` name + JSON `data:` payload the verevonv2 client
//! consumes. Rich events are **gated** by the request's opt-in `features[]`, so
//! `profile:"chat"` (empty features) keeps emitting only
//! connected/chunk/done/error — the plain path is untouched and the BFF
//! re-streams unknown events verbatim.
//!
//! This is the one seam every later capability emits into: reasoning, tools,
//! citations, artifacts, steps, usage. Build it once; never duplicate per-feature
//! SSE plumbing.

use std::convert::Infallible;

use axum::response::sse::Event;
use serde_json::{json, Value};
use tokio::sync::mpsc::Sender;

use crate::knowledge_receipt::KnowledgeReceipt;

// Declared here, not in lib.rs, so this slice stays inside its owned files (WS27-S6).
#[path = "run_ceiling.rs"]
pub mod run_ceiling;

/// Who processed one answer, as the server stamped it. It rides on the `usage`
/// frame so the answer meta can say where an answer was processed, from
/// server-stamped values only: the client never infers residency from a model
/// name, and an older gateway simply sends nothing.
///
/// `provider_used` and `residency` are read off the final chunk (or unary
/// response) of the call that served the answer. inference-core's fallback chain
/// writes both from the serving provider's own declared capabilities, so the
/// ChatGPT-subscription route reads `global` even though that provider itself
/// returns both empty.
///
/// `privacy_tier` is the strongest tier the gateway can substantiate for this
/// answer, never an overstatement: the tier its stamped residency implies on
/// its own (`global` is `global`; `eu` and `norway` are `eu_resident`), raised
/// to the floor the request enforced (inference-core skips every provider below
/// it, so an answer that was served met it). Retention is not carried on the
/// chunk, so `zdr_contractual` and `sovereign` appear only when the request
/// demanded them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServedBy {
    /// Registry id of the provider deployment that served the answer.
    pub provider_used: String,
    /// Residency that deployment declares: `global`, `eu` or `norway`.
    pub residency: String,
    /// `global`, `eu_resident`, `zdr_contractual` or `sovereign`; `None` when
    /// nothing is asserted.
    pub privacy_tier: Option<&'static str>,
}

impl ServedBy {
    /// The provenance of an answer from what the call that served it reported.
    ///
    /// `None` when no provider served it (a synthetic answer that made no
    /// provider call, or an older inference-core that stamped nothing), so the
    /// frame then carries no provenance at all rather than an empty claim.
    /// `enforced_floor_wire` is the `PrivacyTier` ordinal the request sent
    /// with the call; anything outside 1..=4 expresses no floor.
    #[must_use]
    pub fn stamped(provider_used: &str, residency: &str, enforced_floor_wire: i32) -> Option<Self> {
        let provider_used = provider_used.trim();
        if provider_used.is_empty() {
            return None;
        }
        let residency = residency.trim();
        let implied = match residency.to_ascii_lowercase().as_str() {
            "global" => 1,
            "eu" | "norway" => 2,
            _ => 0,
        };
        let floor = if (1..=4).contains(&enforced_floor_wire) {
            enforced_floor_wire
        } else {
            0
        };
        let privacy_tier = match implied.max(floor) {
            1 => Some("global"),
            2 => Some("eu_resident"),
            3 => Some("zdr_contractual"),
            4 => Some("sovereign"),
            _ => None,
        };
        Some(Self {
            provider_used: provider_used.to_owned(),
            residency: residency.to_owned(),
            privacy_tier,
        })
    }

    /// Writes each non-empty field onto a `usage` payload, leaving the keys of
    /// every payload that predates them untouched.
    fn write_to(&self, usage: &mut Value) {
        if !self.provider_used.is_empty() {
            usage["provider_used"] = json!(self.provider_used);
        }
        if !self.residency.is_empty() {
            usage["residency"] = json!(self.residency);
        }
        if let Some(tier) = self.privacy_tier {
            usage["privacy_tier"] = json!(tier);
        }
    }
}

/// How the answer's content was retained, as the posture the turn ran under
/// decided it. It rides on the `usage` frame so the answer line can say what kind
/// of turn produced the answer. A frame that asserts no retention omits the key,
/// which is what every earlier payload does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionClass {
    /// A persistence-free turn under a zero-data-retention request: the gateway
    /// stores nothing of the turn, and inference-core served it only from a
    /// provider with a verified ZDR contract.
    Zdr,
    /// A turn of a ZDR org on one user connection that cannot give zero retention,
    /// under a waiver the user acknowledged. The turn is persistent for that
    /// connection, and the frame says so and why.
    ZdrWaived {
        /// The resolver's reason (`connection_not_zdr_capable`).
        reason: &'static str,
    },
}

impl RetentionClass {
    /// The wire value of the `retention_class` key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Zdr => "zdr",
            Self::ZdrWaived { .. } => "zdr_waived",
        }
    }

    /// The wire value of the `retention_reason` key, for a class that carries a reason.
    #[must_use]
    pub const fn reason(self) -> Option<&'static str> {
        match self {
            Self::Zdr => None,
            Self::ZdrWaived { reason } => Some(reason),
        }
    }
}

/// The render of an Office file for the artifact panel: its pages as a PDF and the first pages as thumbnails.
/// It is a view of the file the sandbox wrote, never the download, and it is transient: it rides on the
/// attachment frame and is not kept with the thread.
#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentPreview {
    /// Always `application/pdf`.
    pub mime: String,
    /// A `data:` URI of the rendered PDF.
    pub url: String,
    /// The pages the render layer produced.
    pub pages: u64,
    /// Whether a font the file names was drawn with a stand-in, so the viewer can say so.
    pub fonts_substituted: bool,
    /// `data:` URIs of the first pages, in page order.
    pub thumbs: Vec<String>,
}

/// Environment switch for cited passages and the research provenance stamp (EP-2-S4). Off, no
/// citation or artifact frame carries `quote`, `locator` or `provenance`, and nothing new is
/// persisted with the message. The one exception is a deep-research report from a run with
/// `deep_research::SECOND_PASS_ENV` on (EP-2-S6): its stamp carries the claim ledger, so it is
/// stamped whichever way this switch is set.
pub const CITATION_PASSAGES_ENV: &str = "MODEL_GATEWAY_CITATION_PASSAGES";

/// Whether cited passages are switched on in this process.
#[must_use]
pub fn citation_passages_enabled() -> bool {
    parse_citation_passages(std::env::var(CITATION_PASSAGES_ENV).ok().as_deref())
}

/// Only the literal `true` enables; `1`, `on` and anything else stay off.
#[must_use]
pub fn parse_citation_passages(raw: Option<&str>) -> bool {
    raw.map(str::trim) == Some("true")
}

/// Longest quote a citation carries. A quote is the passage the source contributed, not the
/// source: the viewer reads more through Data Plane's own ACL'd chunk read.
pub const MAX_QUOTE_CHARS: usize = 400;

/// Where a knowledge source's passage sits in Data Plane, so the viewer can ask Data Plane for it
/// again under the viewer's own grants. Ids only: no text, no score.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CitationLocator {
    pub document_id: String,
    pub chunk_index: i32,
}

/// The passage a citation points at (EP-2-S4): the quoted text and, for a knowledge source, its
/// locator. Each half is written only when set, so a frame without one is unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct CitationPassage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locator: Option<CitationLocator>,
}

impl CitationPassage {
    /// A passage from a quote and an optional locator, the quote whitespace-collapsed and bounded
    /// to [`MAX_QUOTE_CHARS`]. `None` when there is neither, so an empty object is never sent.
    #[must_use]
    pub fn new(quote: &str, locator: Option<CitationLocator>) -> Option<Self> {
        let collapsed = quote.split_whitespace().collect::<Vec<_>>().join(" ");
        let quote: String = collapsed.chars().take(MAX_QUOTE_CHARS).collect();
        let quote = Some(quote.trim_end().to_owned()).filter(|text| !text.is_empty());
        (quote.is_some() || locator.is_some()).then_some(Self { quote, locator })
    }

    /// The passage of a knowledge citation: the first grounding fact of its document, which is the
    /// chunk the context block numbered it from. `None` when no fact names the document.
    #[must_use]
    pub fn knowledge(facts: &[crate::retrieval::GroundingFact], document_id: &str) -> Option<Self> {
        let fact = facts.iter().find(|fact| fact.document_id == document_id)?;
        Self::new(
            &fact.text,
            Some(CitationLocator {
                document_id: fact.document_id.clone(),
                chunk_index: fact.chunk_index,
            }),
        )
    }
}

/// A rich chat-stream event beyond the plain text delta.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatEvent {
    /// Version-bound local checks, distinct from factual/source confidence.
    /// Emitted only after the matching result has been persisted and completed.
    ResultReceipt { receipt: Value },
    /// Collapsible "thinking…" trace tokens.
    ReasoningDelta { delta: String },
    /// Agent-activity timeline / Steps tab.
    StepUpdate {
        id: String,
        title: String,
        detail: String,
        status: String,
    },
    /// A tool/function call the model initiated.
    ToolCall {
        id: String,
        name: String,
        args: Value,
    },
    /// The result of a tool call.
    ToolResult {
        id: String,
        status: String,
        output: String,
        error: Option<String>,
    },
    /// A web/RAG source citation (Sources tab).
    Citation {
        id: String,
        title: String,
        url: String,
        snippet: String,
        /// Machine-stable grouping key (chat-wave2 citations spec §2.2/§2.6):
        /// `kb:{provider_slug}` for a knowledge-base source, or
        /// `web_domain(url)` for a web/deep-research one. Every producer can
        /// always compute one, so this is not optional — an ungrouped
        /// citation would otherwise fall back to one-chip-per-source in the
        /// popover with no way to tell that was the fallback, not a real
        /// per-source grouping decision.
        source_group_id: String,
        /// The human, already-localized label for `source_group_id`
        /// (currently only set by KB grounding, from `source_label()`'s
        /// output) — so the frontend never has to re-derive a
        /// Norwegian/English provider name from a slug. `None` for a web/
        /// deep-research citation, which groups by hostname instead and
        /// renders the hostname itself as the label.
        source_group_label: Option<String>,
        /// The bracketed `[n]` this citation was assigned when it was
        /// numbered (KB grounding's `context_block`, or a web/deep-research
        /// producer's own kept-order numbering). The anchor pass
        /// (`citation_anchors.rs`) resolves a model-cited marker back to a
        /// citation by this stable number, never by array position — see
        /// the off-by-gap bug that field replaced. `None` only for a
        /// deep-research source that was never numbered (an unread lead,
        /// deliberately excluded from the numbered range).
        number: Option<u32>,
        /// The claim boundary this citation was anchored to, filled in by
        /// the post-stream anchor pass. `None` on the citation's first
        /// (pre-answer) emission; a second `Citation` frame with the same
        /// `id` carries it once the answer is complete. First-occurrence
        /// only: if the same source is cited at two disjoint claims, only
        /// the first gets a `claim_id` (mirrors `upsertCitation`'s
        /// first-write-wins dedup on the client).
        claim_id: Option<String>,
        /// The marker's own byte offset range (UTF-8, per `regex::Match`; NOT
        /// a Unicode codepoint or UTF-16 count — matters for a Norwegian
        /// character like æ/ø/å, which is 2 UTF-8 bytes) within the complete
        /// answer
        /// text, set by the same anchor-pass frame as `claim_id`.
        /// INFORMATIONAL ONLY — never used to slice or highlight the answer
        /// (the client's rendering still locates `[n]` itself via its own
        /// regex match over already-split markdown blocks, whose own start
        /// offset within the full answer is not tracked). Do not wire these
        /// into rendering without re-deriving a block-relative offset first.
        start: Option<u32>,
        /// See `start`.
        end: Option<u32>,
        /// Source-scope provenance (chat-wave2 scope spec §2.3): `"web"` for
        /// a `web_search`/deep-research/`fetch_url` producer, `"knowledge"`
        /// for KB/RAG grounding or an org-connected search
        /// (`knowledge_search` and siblings). Always set by every producer —
        /// never inferred client-side — so a `knowledge_only`/`web_only`
        /// turn's receipt and Sources panel can show, and the server can
        /// defensively reject, a citation whose family disagrees with the
        /// turn's active retrieval scope. `"connector"` marks a connected account's read (WS23-S5): listed
        /// as a source, never numbered, and carrying metadata only.
        origin: String,
        /// The passage this source contributed and, for knowledge, where it sits (EP-2-S4). Set
        /// only behind [`CITATION_PASSAGES_ENV`]; `None` writes neither `quote` nor `locator`, so
        /// a frame is byte-identical to before.
        passage: Option<CitationPassage>,
    },
    /// Per-turn retrieval-source filter transparency frame (chat-wave2 scope
    /// spec §2.2/§2.3's "shown in the receipt" requirement). Emitted at most
    /// once per turn, only when the turn's effective `RetrievalScope`
    /// (`crate::normalize::RetrievalScope`) is not `Auto`, or when a Space scopes the turn and it
    /// carries a `knowledge` receipt (WS18-S3). Consumed by
    /// `ChatPanels.tsx`'s assembled-context inspector and `ChatMessages.tsx`'s
    /// scope badge — the concrete UI surface the spec's "shown in the
    /// receipt" requirement asks for.
    SourceScopeApplied {
        /// `"knowledge_only"` or `"web_only"`, or `"auto"` for a turn whose only receipt is
        /// `knowledge`. An ordinary turn emits no frame at all.
        scope: String,
        /// Builtin tool names this turn's offered set excluded because of
        /// `scope`, for a concrete "Nett-kilder ekskludert" / "Organisasjons-
        /// kilder ekskludert" style disclosure rather than a vague claim.
        excluded_tools: Vec<String>,
        /// What the Space's knowledge did for this turn (WS18-S3). Absent for a turn no Space
        /// scopes, so those frames stay byte-identical to before.
        knowledge: Option<KnowledgeReceipt>,
        /// Why tools were withheld for a reason the person did not ask for (Wave 3 EP-9 W2): `"org_web_off"` when
        /// the organisation turned web access off. Absent otherwise, so those frames stay byte-identical to before.
        reason: Option<String>,
    },
    /// The search providers that served this turn's web searches, and the route its entitlement allowed (EP-9 W1).
    /// Sent once, after the tool phase, and only while `MODEL_GATEWAY_SEARCH_PROVENANCE` is on and a search ran.
    /// Provider ids, labels and counts only: no query, URL or hit text rides it. It is a `citations`-family frame,
    /// so it arrives beside the source-scope frame it explains.
    SearchProvenance {
        provenance: crate::search_provenance::SearchProvenance,
    },
    /// Structured internal grounding payload rendered by the chat UI.
    Grounding {
        grounding: crate::retrieval::Grounding,
    },
    /// A canvas/doc/image artifact (side panel).
    Artifact {
        id: String,
        kind: String,
        title: String,
        content: String,
        version: u32,
        /// What a research report was built from (EP-2-S4), set only on a deep-research report
        /// behind [`CITATION_PASSAGES_ENV`]. `None` writes no `provenance` key.
        provenance: Option<Box<crate::deep_research::ResearchProvenance>>,
        /// Who made this revision, `model`, `user` or `restore` (EP-3 S3). Set only on a stored
        /// revision's frame while `MODEL_GATEWAY_ARTIFACT_REVISIONS` is `on`; `None` writes no
        /// `editor` key, so older clients see the frame unchanged.
        editor: Option<String>,
    },
    /// A generated file/image attachment.
    Attachment {
        id: String,
        name: String,
        mime: String,
        url: String,
        size: i64,
        /// The structural check of a generated Office or PDF file, when the
        /// verifier made one. Carried on this frame, not in a new event name.
        check: Option<mp_contracts::office_verification::OfficeVerification>,
        /// The render of a generated Office file, when the render layer made one.
        /// Transient: it rides on this frame only, and the download is still the file.
        preview: Option<AttachmentPreview>,
    },
    /// Usage/latency/cost/confidence for the insight chip + reasoning popover.
    Usage {
        input_tokens: u32,
        output_tokens: u32,
        cost_usd: Option<f64>,
        /// How `cost_usd` was established. On a subscription turn the cost
        /// is 0.0 and the tokens are usage, not spend. `None` where this
        /// layer establishes no cost at all; the key is then omitted.
        cost_basis: Option<crate::pricing::CostBasis>,
        latency_ms: u64,
        confidence: Option<f64>,
        /// Which provider served the answer, under what declared residency and
        /// privacy tier, as stamped by the server (see [`ServedBy`]). `None`
        /// where this layer saw no serving provider; the three keys are then
        /// omitted, so older clients and every earlier payload stay valid.
        served_by: Option<ServedBy>,
        /// How the answer's content was retained, when the turn's posture is
        /// known (see [`RetentionClass`]). `None` omits the key.
        retention_class: Option<RetentionClass>,
        /// Prompt tokens served from the provider's cache, and written to it, on
        /// this turn (zero where the provider reports none or the turn is
        /// zero-retention). Counts only. Omitted from the payload when both are
        /// zero so every earlier payload stays valid.
        cache_read_tokens: u32,
        cache_write_tokens: u32,
        /// Model calls this total covers: the tool-decision rounds, the artifact
        /// checks, the overhead calls and the answer. A turn whose answer was its
        /// only call says 1.
        calls: u32,
        /// Of `calls`, the metered ones whose price was unknown. A total that has
        /// any is missing that call's cost, so `cost_usd` is then null.
        unpriced_calls: u32,
        /// The cost split by what was billed, present exactly when `cost_usd` is a
        /// metered total.
        cost_by_category: Option<crate::pricing::CategoryCost>,
        /// The answer's routing decision (W5-073-b, `tool_loop::route_decision`): the Verevon mode, complexity
        /// and reason its route note states. Closed tokens only. `None` keeps the frame's shape, and the key is
        /// set only while `MODEL_GATEWAY_ROUTING_DECISION_ENABLED` is on.
        routing_decision: Option<serde_json::Value>,
    },
    /// Per-turn phase timing (Q14): ids, milliseconds and counts, never
    /// content. Emitted once, just before the terminal `done`, `error` or
    /// `stopped` frame, and gated exactly like `usage`.
    TurnTiming {
        report: crate::turn_timing::TurnTimingReport,
    },
    /// AI-generated thread title, produced once after the thread's FIRST
    /// exchange completes (sidebar summary, ChatGPT-style). Control event —
    /// never feature-gated: when a title was generated it must reach the
    /// client, and a client that predates it ignores the unknown event.
    Title { title: String },
    /// AI-generated follow-up question suggestions, produced after (almost)
    /// every non-ZDR exchange completes (composer chips, ChatGPT-style).
    /// Control event — never feature-gated, same reasoning as `Title`: when
    /// suggestions were generated they must reach the client, and an older
    /// client ignores the unknown event name.
    FollowUps { suggestions: Vec<String> },
    /// The model's structured plan-mode plan (chat-wave2-specs plan-gate
    /// §2.1), emitted once a plan-mode run finishes when its `plan.propose`
    /// tool call created one. Control event — never feature-gated: an older
    /// client that has never heard of `plan_proposed` ignores the unknown
    /// event name and falls back to today's rung-only `PlanApprovalControl`,
    /// exactly the degrade path the plan-gate rollout note calls for.
    /// `steps` is a JSON array of `{id, title, operation}`, matching
    /// `PlanStep`'s wire shape — a bare `Value` here, the same pattern
    /// `ResultReceipt.receipt` already uses, rather than a new struct only
    /// this one event needs.
    PlanProposed {
        plan_id: String,
        summary: String,
        steps: Value,
    },
    /// The question a Do run's `ask_user` call put to the run's owner, sent once the run completed with
    /// it still waiting (WS14 S4). Control event, ungated: a client that never opted in still has to see
    /// that its turn waits on an answer. `items` is the question as the card reads it, the shape of
    /// `ListRunQuestions`' items. An older client ignores the unknown event name and reads the prose
    /// the turn ends with, which is the degrade path.
    UserQuestion {
        question_id: String,
        delivery: String,
        expires_at: Option<String>,
        items: Value,
        /// `mcp_elicitation` for a form an MCP server sent through the run (EP-4 S6). `None` is the run's
        /// model, and the key is then left off the frame, so a model question reads as it always did.
        origin: Option<String>,
        /// The asking server's display name, set exactly when `origin` is (EP-4 S6).
        origin_label: Option<String>,
    },
    /// The one connected-app suggestion a turn's model made (Wave 3 EP-10 S8), sent only while
    /// `CONNECTOR_SUGGESTIONS_ENABLED` is on and never on a zero-retention or Space turn. Control event,
    /// ungated: the card is how the person learns the answer lacked their own data. An older client
    /// ignores the unknown event name, and the answer's prose still says what it could not see.
    ConnectorSuggestion {
        /// Server-made id the card is keyed by.
        suggestion_id: String,
        /// One entry of `mp_contracts::connector_suggest::SUGGESTABLE_PROVIDERS`.
        provider: String,
        /// One scrubbed line, at most 200 characters, with no address or link.
        reason: String,
    },
    /// The editable deep-research plan a plan turn ends with (Wave 3 EP-2 S5), sent only while
    /// `DEEP_RESEARCH_PLAN_TURN` is on. Control event, ungated: the plan card is the turn's whole
    /// point, so a client that opted into nothing still has to see it. The run turn sends the edited
    /// plan back and the gateway re-clamps it, so nothing here is trusted on the way back.
    ResearchPlan {
        /// The planned search phrases, already normalized and capped.
        sub_queries: Vec<String>,
        /// Up to three clarifying questions the plan inference asked; answers travel in the run turn.
        questions: Vec<String>,
        /// The most sub-queries a run turn keeps, so the card can stop the user adding past it.
        max_sub_queries: usize,
    },
    /// The research run a deep-research turn began, and where it stands (E4.6 062b). Sent once
    /// when the run is begun (`running`), and once when each end of it is recorded, so a client learns the id its
    /// read and cancel routes address. Control event, ungated: the id is the only way to reach those routes, so a
    /// client that opted into no family still needs it. Sent only while `MODEL_GATEWAY_RESEARCH_RUNS` is on, and
    /// never for a run that is not recorded. Ids and one closed word only, so zero retention keeps it too.
    ResearchRun {
        /// The run's id, `rsr_` followed by a ULID, assigned by Session Core.
        id: String,
        /// `running`, or the terminal status the end records: `completed`, `stopped`, `cancelled` or `failed`. A run
        /// that ended elsewhere first carries the status its record holds, `expired` for a run past its deadline, since
        /// this stream never writes `expired` itself.
        status: &'static str,
    },
    /// Deterministic "memory was used" indicator (harness-adoption §7.9,
    /// hermes-agent's `RecallStatus`, MIT): emitted once per turn, before the
    /// answer streams, when long-term memory was actually injected into the
    /// prompt. `count` is the number of discrete memories injected — never 0;
    /// a turn that recalled nothing emits no event at all, so the UI can
    /// render the indicator without an "0 memories" special case. Opt-in via
    /// the `memory` feature family: the injection itself is unconditional
    /// (prompt quality is not a client choice), only this VISIBILITY signal
    /// is gated, so older clients and plain chat are byte-identical.
    /// What the low-confidence verification pass did and what it found.
    ///
    /// The score moving without explanation is worse than no score: a reader
    /// cannot tell "we checked and found nothing" from "we never looked",
    /// and those justify very different trust. Emitted only for turns that
    /// actually ran the pass.
    Verification {
        /// `supports` | `contradicts` | `unrelated` — see
        /// `verification::SourceVerdict`.
        verdict: &'static str,
        /// Knowledge-base sources the pass confirmed against.
        kb_citations: u32,
        /// Web sources it confirmed against, when the turn allowed the web.
        web_citations: u32,
        /// Whether the pass was permitted to escalate past the knowledge base.
        web_allowed: bool,
    },
    MemoryRecall {
        count: u32,
        latency_ms: u64,
        /// What was recalled, so a reader can see and correct it — not just how
        /// much. Bounded and previewed by `memory_provenance`.
        memories: Vec<crate::memory_provenance::RecalledMemoryView>,
    },
    /// A message the user sent MID-RUN has been delivered to the agent at a
    /// tool-round boundary (`queued_input`). Emitted at delivery, not at
    /// enqueue: the POST already told the client its message was accepted, and
    /// what the client cannot otherwise know is when the agent actually saw it.
    ///
    /// A control event, deliberately ungated: the user typed this and is
    /// watching for it. Hiding it behind a feature family would reproduce the
    /// exact bug it exists to fix — a message that seems to have vanished.
    QueuedInput { messages: Vec<String> },
    /// [`ChatEvent::QueuedInput`] with the `client_message_id` of each delivered message (WS04 S6,
    /// behind `MODEL_GATEWAY_SEND_RECEIPTS`): the same `queued_input` frame plus an additive `ids`
    /// key, parallel to `messages`, an empty string for a message sent without one. The SPA's tray
    /// matches a delivery to the message it sent by id instead of by text. A separate variant so
    /// the frame with the switch off stays exactly what it was.
    QueuedInputReceipted {
        messages: Vec<String>,
        ids: Vec<String>,
    },
    /// The turn's answer was produced in a reduced form, and the user must be told (Z-41b). Emitted once
    /// per such answer, before the answer streams, and only on a turn whose retrieval was restrictive: a
    /// ZDR answer whose query embedding had no ZDR-approved route answered from text search alone.
    ///
    /// A control event, deliberately ungated: a client that never opted into a feature family still has
    /// to hear that its answer was reduced. The payload is the typed code and the key the client files its
    /// wording under, never text, so no document content or Data Plane sentence can ride it.
    RetrievalNotice {
        notice: crate::retrieval_metadata::RetrievalNotice,
    },
    // ---- usage progress (WS27) ----
    /// The turn's running total after one of its provider calls, streamed while the turn runs
    /// (`MODEL_GATEWAY_USAGE_PROGRESS`). It belongs to the `usage` family, so a client that did not ask
    /// for usage never receives it. Counts and closed tokens only: no content, tool name or argument.
    /// The terminal `usage` frame stays the authority for the turn's total.
    UsageProgress {
        /// The provider call this frame reports, as its closed phase token.
        phase: crate::usage_meter::CallPhase,
        /// The turn so far, the call included.
        snapshot: crate::usage_meter::Snapshot,
        /// The turn's run ceiling, on the frames that carry one (WS27-S6). `None` leaves the frame
        /// as it was before the ceiling existed.
        ceiling: Option<crate::sse_events::run_ceiling::CeilingProgress>,
        /// How the budget moved this call's route (WS27-S7): `tier_shifted` or `cheap_fallback`, only when
        /// inference-core's route note says the posture moved it. `None` keeps the frame's earlier shape.
        routing: Option<&'static str>,
        /// The answer's routing decision (W5-073-b), on the answer's own progress frame only. Closed tokens
        /// only. `None` keeps the frame's shape, as `routing` does.
        routing_decision: Option<serde_json::Value>,
    },
    // ---- end usage progress (WS27) ----
    // ---- lifecycle (WS15) ----
    /// A tool call is being retried after a transient failure, before the next attempt. Counts and
    /// closed tokens only (see `mp_contracts::lifecycle_notice`): the error text never reaches here.
    /// `steps` family, so only a client that asked for step detail sees it.
    RetryScheduled {
        scope: &'static str,
        tool: String,
        attempt: u32,
        max_attempts: u32,
        wait_ms: u64,
        reason: &'static str,
    },
    /// The history or the tool results were compacted to fit the model. A control event, ungated:
    /// a client that never opted in still has to be told that its answer was built from less.
    Compaction {
        scope: &'static str,
        outcome: &'static str,
        messages_compacted: Option<u32>,
        messages_kept: Option<u32>,
        pinned_kept: Option<u32>,
    },
    /// The context the model was given changed: history shed, skill guidance cut short, or a run
    /// context part skipped. A control event, ungated, for the same reason as `Compaction`.
    ContextChange {
        change: &'static str,
        messages_dropped: Option<u32>,
        skills_truncated: Option<u32>,
        skills_dropped: Option<u32>,
        layer: Option<&'static str>,
        state: Option<&'static str>,
    },
    /// A parked run (WS03 S8-A): the executor stopped the run on a closed reason and it resumes
    /// later. A control event, ungated: a client that never opted in still has to hear that its
    /// answer is waiting. Ids, a closed token and a time only. The keys are the ones WS03 shipped
    /// and the SPA reads, so this variant adds and renames none.
    RunParked {
        run_id: String,
        reason: mp_contracts::loop_fold::ParkReason,
        /// When the run can be resumed (RFC 3339, UTC), or `None` when that time is unreadable.
        resume_after: Option<String>,
    },
    /// The run paused for the owner's answer (WS14 S5): its `ask_user` question is waiting, the run
    /// stays open and is resumed through the question form of `/continue`. A control event, ungated: a
    /// client that never opted in still has to hear that its turn waits on an answer. Ids only; the
    /// question itself rides the `user_question` frame sent just before it. `question_id` is empty when
    /// the question could not be read.
    AwaitingInput {
        run_id: String,
        question_id: String,
    },
    /// A goal turn was settled (EP-7 S2): the goal's spend against its caps and its status after
    /// the turn. A control event, ungated: the goal bar needs it whatever the client opted into.
    /// Ids, counts and closed tokens only; the objective never rides it.
    GoalProgress {
        goal_id: String,
        turn_no: u32,
        tokens_used: u64,
        /// The metered-token budget; `None` when the goal sets none.
        token_budget: Option<u64>,
        cost_used_usd_micros: u64,
        /// The cost budget in micro-USD; `None` when the goal sets none.
        cost_budget_usd_micros: Option<u64>,
        turns_used: u32,
        max_turns: u32,
        /// A goal status token (`active`, `budget_reached`, `turns_reached`, ...).
        status: &'static str,
        /// The closed reason beside the status; empty when none.
        status_reason: &'static str,
        /// The completion judge's verdict on the goal's latest judged turn (`met`, `not_met`, `blocked`,
        /// `unsure`); empty when none is known (EP-7 S3).
        last_verdict: &'static str,
    },
    /// The workspace of the sandboxed computer a tool call used: where its files are kept, and whether they
    /// were (WS22-S6). `scope` is the workspace, `state` its outcome, and `cause` the closed reason when the
    /// lease recorded one. A `steps` family frame, so only a client that asked for step detail sees it.
    /// Closed tokens only: no path, file name, content or error text.
    SandboxState {
        scope: &'static str,
        state: &'static str,
        cause: Option<&'static str>,
    },
    // ---- end lifecycle (WS15) ----
    /// Terminal control: generation stopped/cancelled by the user.
    Stopped { reason: String },
    /// Terminal control: a structured error (chat-parity §20). `code` is a
    /// stable machine token; `retryable` tells the client whether to offer retry.
    Error {
        code: String,
        message: String,
        retryable: bool,
    },
    /// Terminal control: a typed zero-data-retention refusal. The same `error`
    /// frame as [`ChatEvent::Error`], carrying the catalogue's `action` and, when
    /// the code takes one, the `feature` (`zdr_feature_not_supported`), so the
    /// client can say what to change rather than repeat a sentence.
    ZdrError {
        refusal: mp_contracts::zdr_refusal::Refusal,
    },
    /// Terminal control: a ChatGPT-subscription refusal. The same `error`
    /// frame as [`ChatEvent::Error`], plus the content-free timing the SPA
    /// needs to say when a retry can work: `retry_after_ms`, `resets_at`
    /// (RFC 3339 UTC), and `retry_at`, each omitted when unknown.
    ///
    /// `retry_at` is the absolute instant a retry could succeed (the reset,
    /// or the wait counted from when this frame was built). The frame is
    /// buffered and replayed on resume, and `retry_after_ms` is relative, so
    /// a client should prefer `retry_at` over adding the wait to its own clock.
    SubscriptionError {
        code: String,
        message: String,
        retryable: bool,
        retry_after_ms: Option<u64>,
        resets_at: Option<String>,
        retry_at: Option<String>,
    },
}

impl ChatEvent {
    /// An `artifact` frame naming who made the revision (EP-3 S3); `None`, and every other
    /// variant, is returned unchanged.
    #[must_use]
    pub fn with_editor(self, editor: Option<&str>) -> Self {
        match (self, editor) {
            (ChatEvent::Artifact { id, kind, title, content, version, provenance, .. }, Some(editor)) => {
                ChatEvent::Artifact { id, kind, title, content, version, provenance, editor: Some(editor.to_owned()) }
            }
            (event, _) => event,
        }
    }

    /// The `sandbox_state` frame for a workspace result a tool call produced (WS22-S6). Each token must be a
    /// member of its closed list, so a result that carries free text or a path yields `None` rather than a
    /// frame that could show it.
    #[must_use]
    pub fn sandbox_state(scope: &str, state: &str, cause: Option<&str>) -> Option<Self> {
        use mp_contracts::lifecycle_notice::{closed_token, SANDBOX_CAUSES, SANDBOX_SCOPES, SANDBOX_STATES};
        let cause = match cause {
            Some(token) => Some(closed_token(&SANDBOX_CAUSES, token)?),
            None => None,
        };
        Some(Self::SandboxState {
            scope: closed_token(&SANDBOX_SCOPES, scope)?,
            state: closed_token(&SANDBOX_STATES, state)?,
            cause,
        })
    }

    /// The opt-in feature family this event belongs to, or `None` if it is a
    /// control event that is ALWAYS emitted (never gated).
    #[must_use]
    pub fn family(&self) -> Option<&'static str> {
        match self {
            ChatEvent::ReasoningDelta { .. } => Some("reasoning"),
            ChatEvent::StepUpdate { .. } => Some("steps"),
            ChatEvent::ToolCall { .. } | ChatEvent::ToolResult { .. } => Some("tools"),
            ChatEvent::Citation { .. }
            | ChatEvent::Grounding { .. }
            | ChatEvent::Verification { .. }
            | ChatEvent::SourceScopeApplied { .. }
            | ChatEvent::SearchProvenance { .. } => Some("citations"),
            ChatEvent::Artifact { .. } | ChatEvent::Attachment { .. } => Some("artifacts"),
            ChatEvent::Usage { .. }
            | ChatEvent::ResultReceipt { .. }
            | ChatEvent::TurnTiming { .. } => Some("usage"),
            // ---- usage progress (WS27) ----
            ChatEvent::UsageProgress { .. } => Some("usage"),
            // ---- end usage progress (WS27) ----
            ChatEvent::MemoryRecall { .. } => Some("memory"),
            // ---- lifecycle (WS15) ----
            ChatEvent::RetryScheduled { .. } => Some("steps"),
            // ---- sandbox state (WS22-S6) ----
            ChatEvent::SandboxState { .. } => Some("steps"),
            // control events — always allowed (Title/FollowUps only exist when
            // the gateway actually generated them; gating on a feature family
            // would silently drop them for the plain `chat` profile, which is
            // exactly the surface the sidebar title / composer chips are for)
            ChatEvent::Title { .. }
            | ChatEvent::FollowUps { .. }
            // The structured plan-mode plan (plan-gate §2.1) — essential for
            // `PlanApprovalControl` to render the edit surface, not an
            // opt-in enhancement.
            | ChatEvent::PlanProposed { .. }
            // The user typed this and is watching for it — gating it would
            // reproduce the vanishing-message bug it exists to fix.
            | ChatEvent::QueuedInput { .. }
            | ChatEvent::QueuedInputReceipted { .. }
            // A reduced answer's notice (Z-41b) — a client that never opted in still has to hear it.
            | ChatEvent::RetrievalNotice { .. }
            | ChatEvent::Stopped { .. }
            | ChatEvent::Error { .. }
            | ChatEvent::ZdrError { .. }
            | ChatEvent::SubscriptionError { .. }
            // ---- lifecycle (WS15) ----
            | ChatEvent::Compaction { .. }
            | ChatEvent::ContextChange { .. }
            | ChatEvent::RunParked { .. }
            // The run waits on the owner's answer (WS14 S5): the card needs it whatever the client opted into.
            | ChatEvent::AwaitingInput { .. }
            // A goal's spend after a turn (EP-7 S2): the goal bar shows it on every client.
            | ChatEvent::GoalProgress { .. }
            // The question a run waits on (WS14 S4): the card needs it whatever the client opted into.
            | ChatEvent::UserQuestion { .. }
            // The connected-app suggestion card (EP-10 S8): it says the answer lacked the person's data.
            | ChatEvent::ConnectorSuggestion { .. }
            // The deep-research plan card (EP-2 S5): the plan turn shows nothing else.
            | ChatEvent::ResearchPlan { .. }
            // The research run's id and status (E4.6 062b): a client reaches the run's read and cancel routes by the id.
            | ChatEvent::ResearchRun { .. } => None,
        }
    }

    /// Whether to emit this event given the client's opt-in families. Control
    /// events (family `None`) always emit; rich events only when opted in. This
    /// is what protects the plain `chat` path: with no features, only control
    /// events pass.
    #[must_use]
    pub fn should_emit(&self, features: &[String]) -> bool {
        match self.family() {
            None => true,
            Some(fam) => features.iter().any(|f| f == fam),
        }
    }

    /// The SSE `event:` name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            ChatEvent::ResultReceipt { .. } => "result_receipt",
            ChatEvent::ReasoningDelta { .. } => "reasoning_delta",
            ChatEvent::StepUpdate { .. } => "step_update",
            ChatEvent::ToolCall { .. } => "tool_call",
            ChatEvent::ToolResult { .. } => "tool_result",
            ChatEvent::Citation { .. } => "citation",
            ChatEvent::SourceScopeApplied { .. } => "source_scope_applied",
            ChatEvent::SearchProvenance { .. } => "search_provenance",
            ChatEvent::Grounding { .. } => "grounding",
            ChatEvent::Artifact { .. } => "artifact",
            ChatEvent::Attachment { .. } => "attachment",
            ChatEvent::Usage { .. } => "usage",
            ChatEvent::TurnTiming { .. } => "turn_timing",
            ChatEvent::Verification { .. } => "verification",
            ChatEvent::MemoryRecall { .. } => "memory_recall",
            // ---- lifecycle (WS15) ----
            ChatEvent::RetryScheduled { .. } => "retry_scheduled",
            ChatEvent::SandboxState { .. } => "sandbox_state",
            ChatEvent::Compaction { .. } => "compaction",
            ChatEvent::ContextChange { .. } => "context_change",
            ChatEvent::RunParked { .. } => "run_parked",
            ChatEvent::AwaitingInput { .. } => "awaiting_input",
            ChatEvent::GoalProgress { .. } => "goal_progress",
            ChatEvent::Title { .. } => "title",
            ChatEvent::FollowUps { .. } => "follow_ups",
            ChatEvent::PlanProposed { .. } => "plan_proposed",
            ChatEvent::UserQuestion { .. } => "user_question",
            ChatEvent::ConnectorSuggestion { .. } => "connector_suggestion",
            ChatEvent::ResearchPlan { .. } => "research_plan",
            ChatEvent::ResearchRun { .. } => "research_run",
            ChatEvent::QueuedInput { .. } | ChatEvent::QueuedInputReceipted { .. } => "queued_input",
            ChatEvent::RetrievalNotice { .. } => "retrieval_notice",
            // ---- usage progress (WS27) ----
            ChatEvent::UsageProgress { .. } => "usage_progress",
            // ---- end usage progress (WS27) ----
            ChatEvent::Stopped { .. } => "stopped",
            ChatEvent::Error { .. }
            | ChatEvent::ZdrError { .. }
            | ChatEvent::SubscriptionError { .. } => "error",
        }
    }

    /// The SSE `data:` JSON payload.
    #[must_use]
    pub fn payload(&self, request_id: &str) -> Value {
        match self {
            ChatEvent::ResultReceipt { receipt } => json!({ "receipt": receipt, "request_id": request_id }),
            ChatEvent::ReasoningDelta { delta } => {
                json!({ "delta": delta, "request_id": request_id })
            }
            ChatEvent::StepUpdate {
                id,
                title,
                detail,
                status,
            } => {
                json!({ "id": id, "title": title, "detail": detail, "status": status })
            }
            ChatEvent::ToolCall { id, name, args } => {
                json!({ "id": id, "name": name, "args": args })
            }
            ChatEvent::ToolResult {
                id,
                status,
                output,
                error,
            } => {
                json!({ "id": id, "status": status, "output": output, "error": error })
            }
            ChatEvent::Citation {
                id,
                title,
                url,
                snippet,
                source_group_id,
                source_group_label,
                number,
                claim_id,
                start,
                end,
                origin,
                passage,
            } => {
                let mut payload = json!({
                    "id": id,
                    "title": title,
                    "url": url,
                    "snippet": snippet,
                    "source_group_id": source_group_id,
                    "origin": origin,
                });
                if let Some(passage) = passage {
                    if let Some(quote) = &passage.quote {
                        payload["quote"] = json!(quote);
                    }
                    if let Some(locator) = &passage.locator {
                        payload["locator"] = json!(locator);
                    }
                }
                // Kept out of the object entirely (not even `null`) when
                // absent, so the common "no anchor yet" frame stays small —
                // see the field docs on `ChatEvent::Citation`.
                if let Some(label) = source_group_label {
                    payload["source_group_label"] = json!(label);
                }
                if let Some(n) = number {
                    payload["number"] = json!(n);
                }
                if let Some(id) = claim_id {
                    payload["claim_id"] = json!(id);
                }
                if let Some(value) = start {
                    payload["start"] = json!(value);
                }
                if let Some(value) = end {
                    payload["end"] = json!(value);
                }
                payload
            }
            ChatEvent::SourceScopeApplied { scope, excluded_tools, knowledge, reason } => {
                let mut payload = json!({ "scope": scope, "excluded_tools": excluded_tools });
                if let Some(receipt) = knowledge {
                    payload["knowledge"] = serde_json::to_value(receipt).unwrap_or_else(|_| json!({}));
                }
                if let Some(reason) = reason {
                    payload["reason"] = json!(reason);
                }
                payload
            }
            // Closed ids, labels and counts, and the route: nothing a query, a URL or a hit carries.
            ChatEvent::SearchProvenance { provenance } => json!({
                "providers": provenance.providers,
                "route": provenance.route,
                "request_id": request_id,
            }),
            ChatEvent::Grounding { grounding } => {
                serde_json::to_value(grounding).unwrap_or_else(|_| json!({}))
            }
            ChatEvent::Artifact {
                id,
                kind,
                title,
                content,
                version,
                provenance,
                editor,
            } => {
                let mut payload = json!({ "id": id, "kind": kind, "title": title, "content": content, "version": version });
                if let Some(provenance) = provenance {
                    payload["provenance"] = serde_json::to_value(provenance).unwrap_or_else(|_| json!({}));
                }
                if let Some(editor) = editor {
                    payload["editor"] = json!(editor);
                }
                payload
            }
            ChatEvent::Attachment {
                id,
                name,
                mime,
                url,
                size,
                check,
                preview,
            } => {
                let mut attachment =
                    json!({ "id": id, "name": name, "type": mime, "url": url, "size": size });
                // Optional, and only where the verifier ran: an attachment with no check keeps its old keys.
                if let Some(report) = check {
                    attachment["check"] = serde_json::to_value(report).unwrap_or_else(|_| json!({}));
                }
                // The same rule for the render: no preview, no key.
                if let Some(preview) = preview {
                    attachment["preview"] = json!({
                        "mime": preview.mime,
                        "url": preview.url,
                        "pages": preview.pages,
                        "fonts_substituted": preview.fonts_substituted,
                        "thumbs": preview.thumbs,
                    });
                }
                attachment
            }
            ChatEvent::Usage {
                input_tokens,
                output_tokens,
                cost_usd,
                cost_basis,
                latency_ms,
                confidence,
                served_by,
                retention_class,
                cache_read_tokens,
                cache_write_tokens,
                calls,
                unpriced_calls,
                cost_by_category,
                routing_decision,
            } => {
                let mut usage = json!({
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens,
                    "cost_usd": cost_usd,
                    "latency_ms": latency_ms,
                    "confidence": confidence,
                });
                // Optional, and only when the provider reported cache activity.
                if *cache_read_tokens > 0 || *cache_write_tokens > 0 {
                    usage["cache_read_tokens"] = json!(cache_read_tokens);
                    usage["cache_write_tokens"] = json!(cache_write_tokens);
                }
                // Optional: a client that predates it sees the same payload.
                if let Some(basis) = cost_basis {
                    usage["cost_basis"] = json!(basis.as_str());
                }
                // Always stated, so a total is never read as covering calls it does not.
                usage["calls"] = json!(calls);
                usage["unpriced_calls"] = json!(unpriced_calls);
                // Only where the total was priced; a breakdown of nothing is not stated.
                if let Some(parts) = cost_by_category {
                    usage["cost_by_category"] = json!({
                        "input_usd": parts.input_usd,
                        "cache_read_usd": parts.cache_read_usd,
                        "cache_write_usd": parts.cache_write_usd,
                        "output_usd": parts.output_usd,
                    });
                }
                // Optional, and per field: an empty value is left out.
                if let Some(served_by) = served_by {
                    served_by.write_to(&mut usage);
                }
                // Optional, and only where the turn's retention is known.
                if let Some(class) = retention_class {
                    usage["retention_class"] = json!(class.as_str());
                    // Optional, and only where the class carries a reason (a waiver does).
                    if let Some(reason) = class.reason() {
                        usage["retention_reason"] = json!(reason);
                    }
                }
                // Stated only where the answer's route note states a decision, so a frame without one keeps its shape.
                if let Some(decision) = routing_decision {
                    usage["routing_decision"] = decision.clone();
                }
                usage
            }
            // The report carries its own request id.
            ChatEvent::TurnTiming { report } => {
                serde_json::to_value(report).unwrap_or_else(|_| json!({}))
            }
            ChatEvent::Verification {
                verdict,
                kb_citations,
                web_citations,
                web_allowed,
            } => json!({
                "verdict": verdict,
                "kb_citations": kb_citations,
                "web_citations": web_citations,
                "web_allowed": web_allowed,
                "request_id": request_id,
            }),
            ChatEvent::MemoryRecall {
                count,
                latency_ms,
                memories,
            } => json!({
                "count": count,
                "latency_ms": latency_ms,
                // `role`/`origin` go out as their own strings rather than as
                // booleans: `origin` has THREE states, and the third
                // ("unrecorded") must never collapse into "stated". See
                // `memory_provenance`.
                "memories": memories
                    .iter()
                    .map(|memory| json!({
                        "memory_id": memory.memory_id,
                        "role": memory.role.as_str(),
                        "origin": memory.origin.as_str(),
                        "label": memory.label,
                        "preview": memory.preview,
                    }))
                    .collect::<Vec<_>>(),
                "request_id": request_id,
            }),
            ChatEvent::Title { title } => json!({ "title": title, "request_id": request_id }),
            ChatEvent::FollowUps { suggestions } => {
                json!({ "suggestions": suggestions, "request_id": request_id })
            }
            ChatEvent::PlanProposed {
                plan_id,
                summary,
                steps,
            } => json!({
                "plan_id": plan_id,
                "summary": summary,
                "steps": steps,
                "request_id": request_id,
            }),
            ChatEvent::UserQuestion {
                question_id,
                delivery,
                expires_at,
                items,
                origin,
                origin_label,
            } => {
                let mut payload = json!({
                    "question_id": question_id,
                    "delivery": delivery,
                    "expires_at": expires_at,
                    "items": items,
                    "request_id": request_id,
                });
                // The origin keys come only with a server's question, so a model question's frame is unchanged.
                if let Some(origin) = origin {
                    payload["origin"] = json!(origin);
                }
                if let Some(origin_label) = origin_label {
                    payload["origin_label"] = json!(origin_label);
                }
                payload
            }
            ChatEvent::ConnectorSuggestion {
                suggestion_id,
                provider,
                reason,
            } => json!({
                "suggestion_id": suggestion_id,
                "provider": provider,
                "reason": reason,
                "request_id": request_id,
            }),
            ChatEvent::ResearchPlan {
                sub_queries,
                questions,
                max_sub_queries,
            } => json!({
                "sub_queries": sub_queries,
                "questions": questions,
                "caps": { "max_sub_queries": max_sub_queries },
                "request_id": request_id,
            }),
            // The run's id and its closed status, and nothing else: the run record keeps no more (E4.6 062b).
            ChatEvent::ResearchRun { id, status } => {
                json!({ "research_run_id": id, "status": status })
            }
            ChatEvent::QueuedInput { messages } => {
                json!({ "messages": messages, "request_id": request_id })
            }
            ChatEvent::QueuedInputReceipted { messages, ids } => {
                json!({ "messages": messages, "ids": ids, "request_id": request_id })
            }
            // The typed code and the wording key, and nothing else: see the variant's docs.
            ChatEvent::RetrievalNotice { notice } => json!({
                "code": notice.code,
                "wording_key": notice.wording_key,
                "request_id": request_id,
            }),
            // ---- usage progress (WS27) ----
            ChatEvent::UsageProgress {
                phase,
                snapshot,
                ceiling,
                routing,
                routing_decision,
            } => {
                let mut payload = json!({
                    "calls": snapshot.calls,
                    "phase": phase.as_str(),
                    "input_tokens": snapshot.input,
                    "output_tokens": snapshot.output,
                    "cache_read_tokens": snapshot.cache_read,
                    "cache_write_tokens": snapshot.cache_write,
                    "cost_usd": snapshot.cost_usd,
                });
                // Omitted, not null, where the total has no single basis: a basis stated for a mixed
                // total would describe one of its calls as if it described all of them.
                if let Some(basis) = snapshot.basis {
                    payload["cost_basis"] = json!(basis.as_str());
                }
                // Stated only once a call went unpriced, so a frame whose calls are all priced keeps its shape.
                if snapshot.unpriced_calls > 0 {
                    payload["unpriced_calls"] = json!(snapshot.unpriced_calls);
                }
                // Each ceiling key rides only a frame that carries a value for it, so a frame without a
                // ceiling keeps the shape it had before.
                if let Some(ceiling) = ceiling {
                    if let Some(usd) = ceiling.ceiling_usd {
                        payload["ceiling_usd"] = json!(usd);
                    }
                    if let Some(tokens) = ceiling.ceiling_tokens {
                        payload["ceiling_tokens"] = json!(tokens);
                    }
                    if let Some(fraction) = ceiling.fraction {
                        payload["fraction"] = json!(fraction);
                    }
                    if let Some(warning) = ceiling.warning {
                        payload["warning"] = json!(warning.as_str());
                    }
                }
                // Stated only on a call the budget moved, so an unmoved frame keeps its shape.
                if let Some(routing) = routing {
                    payload["routing"] = json!(routing);
                }
                // Stated only where the answer's route note states a decision, so an unstated frame keeps its shape.
                if let Some(decision) = routing_decision {
                    payload["routing_decision"] = decision.clone();
                }
                payload
            }
            // ---- end usage progress (WS27) ----
            // ---- lifecycle (WS15) ----
            ChatEvent::RetryScheduled {
                scope,
                tool,
                attempt,
                max_attempts,
                wait_ms,
                reason,
            } => json!({
                "scope": scope,
                "tool": tool,
                "attempt": attempt,
                "max_attempts": max_attempts,
                "wait_ms": wait_ms,
                "reason": reason,
                "request_id": request_id,
            }),
            ChatEvent::SandboxState { scope, state, cause } => {
                let mut payload = json!({ "scope": scope, "state": state, "request_id": request_id });
                // Omitted, not null, when the workspace names no cause: a frame that says the files were kept carries none.
                if let Some(cause) = cause {
                    payload["cause"] = json!(cause);
                }
                payload
            }
            ChatEvent::Compaction {
                scope,
                outcome,
                messages_compacted,
                messages_kept,
                pinned_kept,
            } => {
                let mut payload = json!({ "scope": scope, "outcome": outcome, "request_id": request_id });
                // Each count is omitted, not null, when the gateway did not measure it.
                for (key, count) in [
                    ("messages_compacted", messages_compacted),
                    ("messages_kept", messages_kept),
                    ("pinned_kept", pinned_kept),
                ] {
                    if let Some(count) = count {
                        payload[key] = json!(count);
                    }
                }
                payload
            }
            ChatEvent::ContextChange {
                change,
                messages_dropped,
                skills_truncated,
                skills_dropped,
                layer,
                state,
            } => {
                let mut payload = json!({ "change": change, "request_id": request_id });
                for (key, count) in [
                    ("messages_dropped", messages_dropped),
                    ("skills_truncated", skills_truncated),
                    ("skills_dropped", skills_dropped),
                ] {
                    if let Some(count) = count {
                        payload[key] = json!(count);
                    }
                }
                for (key, token) in [("layer", layer), ("state", state)] {
                    if let Some(token) = token {
                        payload[key] = json!(token);
                    }
                }
                payload
            }
            // The keys and their values are WS03's `run_parked` frame exactly, so the wire is unchanged.
            ChatEvent::RunParked {
                run_id,
                reason,
                resume_after,
            } => json!({
                "run_id": run_id,
                "request_id": request_id,
                "status": "parked",
                "reason": reason.as_str(),
                "resume_after": resume_after,
            }),
            // The keys are the `awaiting_approval` frame's, plus the question's id (WS14 S5).
            ChatEvent::AwaitingInput {
                run_id,
                question_id,
            } => json!({
                "run_id": run_id,
                "request_id": request_id,
                "question_id": question_id,
                "status": "awaiting_input",
            }),
            // A budget the goal does not set is `null`, so a client reads "none" without a guess.
            ChatEvent::GoalProgress {
                goal_id,
                turn_no,
                tokens_used,
                token_budget,
                cost_used_usd_micros,
                cost_budget_usd_micros,
                turns_used,
                max_turns,
                status,
                status_reason,
                last_verdict,
            } => json!({
                "goal_id": goal_id,
                "turn_no": turn_no,
                "tokens_used": tokens_used,
                "token_budget": token_budget,
                "cost_used_usd_micros": cost_used_usd_micros,
                "cost_budget_usd_micros": cost_budget_usd_micros,
                "turns_used": turns_used,
                "max_turns": max_turns,
                "status": status,
                "status_reason": status_reason,
                "last_verdict": last_verdict,
                "request_id": request_id,
            }),
            ChatEvent::Stopped { reason } => json!({ "reason": reason, "request_id": request_id }),
            ChatEvent::Error {
                code,
                message,
                retryable,
            } => json!({
                "code": code,
                "message": message,
                "retryable": retryable,
                "request_id": request_id,
            }),
            ChatEvent::ZdrError { refusal } => {
                let mut payload = json!({
                    "code": refusal.code(),
                    "message": refusal.message(),
                    "retryable": refusal.retryable(),
                    "action": refusal.action().as_str(),
                    "request_id": request_id,
                });
                if let Some(feature) = refusal.feature() {
                    payload["feature"] = json!(feature);
                }
                payload
            }
            ChatEvent::SubscriptionError {
                code,
                message,
                retryable,
                retry_after_ms,
                resets_at,
                retry_at,
            } => {
                let mut error = json!({
                    "code": code,
                    "message": message,
                    "retryable": retryable,
                    "request_id": request_id,
                });
                // Omitted, not null, when unknown: a client that predates
                // them sees exactly the `Error` payload.
                if let Some(resets_at) = resets_at {
                    error["resets_at"] = json!(resets_at);
                }
                if let Some(retry_after_ms) = retry_after_ms {
                    error["retry_after_ms"] = json!(retry_after_ms);
                }
                if let Some(retry_at) = retry_at {
                    error["retry_at"] = json!(retry_at);
                }
                error
            }
        }
    }

    /// Encode as an axum SSE [`Event`] (name + JSON data).
    pub fn to_sse(&self, request_id: &str) -> Event {
        Event::default()
            .event(self.name())
            .data(self.payload(request_id).to_string())
    }

    /// The `error` frame for a subscription failure, worded by the caller.
    #[must_use]
    pub fn subscription_error(
        failure: &crate::subscription_failure::SubscriptionFailure,
        message: String,
    ) -> Self {
        Self::subscription_error_at(failure, message, chrono::Utc::now())
    }

    /// [`Self::subscription_error`] built at `now`, which dates `retry_at`.
    #[must_use]
    pub fn subscription_error_at(
        failure: &crate::subscription_failure::SubscriptionFailure,
        message: String,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        ChatEvent::SubscriptionError {
            code: failure.code.to_owned(),
            message,
            retryable: crate::subscription_failure::retryable(failure.code),
            retry_after_ms: failure.retry_after_ms,
            resets_at: failure.resets_at_label(),
            retry_at: failure.retry_at_label(now),
        }
    }
}

/// A typed refusal inference-core declared on a subscription call (V3-mg-1): a zero-retention refusal or a
/// model-entitlement one. On the subscription route it keeps its own catalogue code (and, for a zero-retention refusal,
/// its action and feature), and the run ends under that code. It is not the route's generic failure, which a client
/// may retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypedRefusal {
    Zdr(mp_contracts::zdr_refusal::Refusal),
    Entitlement(mp_contracts::model_entitlement::Refusal),
}

impl TypedRefusal {
    /// The refusal a failed call carries, zero-retention first, as the direct run and the side chat check them.
    /// `None` for any other status.
    #[must_use]
    pub fn from_status(status: &tonic::Status) -> Option<Self> {
        mp_contracts::zdr_refusal::Refusal::from_status(status)
            .map(Self::Zdr)
            .or_else(|| {
                mp_contracts::model_entitlement::Refusal::from_status(status).map(Self::Entitlement)
            })
    }

    /// The catalogue code the run ends under.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Zdr(refusal) => refusal.code(),
            Self::Entitlement(refusal) => refusal.code(),
        }
    }

    /// The catalogue's English fallback message.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Zdr(refusal) => refusal.message(),
            Self::Entitlement(refusal) => refusal.message(),
        }
    }

    /// Whether sending the same turn again could succeed, as the catalogue says.
    #[must_use]
    pub fn retryable(&self) -> bool {
        match self {
            Self::Zdr(refusal) => refusal.retryable(),
            Self::Entitlement(refusal) => refusal.retryable(),
        }
    }

    /// The stream's terminal frame. A zero-retention refusal is the `ZdrError` frame, which carries its action and
    /// feature; a model-entitlement refusal is an `error` frame with its code, message and retry flag, as the
    /// ephemeral answer sends it.
    #[must_use]
    pub fn into_event(self) -> ChatEvent {
        match self {
            Self::Zdr(refusal) => ChatEvent::ZdrError { refusal },
            Self::Entitlement(refusal) => ChatEvent::Error {
                code: refusal.code().to_owned(),
                message: refusal.message(),
                retryable: refusal.retryable(),
            },
        }
    }
}

/// Emits [`ChatEvent`]s onto an SSE channel, honoring the client's opt-in
/// `features[]`. Cheap to clone the inputs; holds the sender + gating context.
pub struct RichEventSink {
    stage_text_artifacts: bool,
    tx: Sender<Result<Event, Infallible>>,
    features: Vec<String>,
    request_id: String,
    /// Every citation that passed through this sink, kept so the turn's
    /// evidence can be persisted with the assistant message.
    ///
    /// The tool loop streams citations straight out through `emit` and returns
    /// an EMPTY buffer whenever a live sink is supplied (see
    /// `DeepResearchOutcome.events`), so this is the only place a web-search
    /// turn's citations can still be observed. Without it, only Data Plane
    /// `Grounding` was persistable and every tool-sourced source vanished from
    /// history the moment the browser cache was gone.
    citations: std::sync::Arc<std::sync::Mutex<Vec<RecordedCitation>>>,
    /// Every text artifact that passed through this sink, for the same reason
    /// as `citations`: the durable thread read returned zero artifacts for a
    /// turn that had produced three (RUN-LOG finding 3), because the only
    /// copy lived in the browser that watched the stream. Generated binaries
    /// (data: URIs) are NOT recorded — see [`RecordedArtifact::from_event`].
    artifacts: std::sync::Arc<std::sync::Mutex<Vec<RecordedArtifact>>>,
    /// The provider calls the turn made beside its answer and its tool rounds (a
    /// source judge, a self-consistency resample), kept for the final usage frame
    /// and their ledger rows. Each is recorded by the site that made it.
    overhead: std::sync::Arc<std::sync::Mutex<Vec<crate::usage_meter::CallUsage>>>,
    /// Whether this turn streams a `usage_progress` frame for each call it records (WS27 S3). The
    /// handler that builds the sink sets it once, from the switch, so every site reads one answer.
    usage_progress: bool,
    /// Whether a run ceiling is tracked on this turn (WS27-S6). A tracked ceiling needs every call
    /// priced and counted, streamed or not, so the sink says whether it records calls at all.
    ceiling_tracked: bool,
    /// The running total of the calls this sink has recorded, which each progress frame states, and
    /// the turn's run ceiling. One lock, so a frame never states a total its ceiling does not match.
    tally: std::sync::Mutex<RunTally>,
    /// The turn's clock, shared the same way, so the tool loop and the draft
    /// checks it runs can time their calls without new parameters.
    timing: Option<crate::turn_timing::TurnTiming>,
    /// The connected accounts the person selected for this turn (WS23-S5), read by the Ask loop's
    /// connected-app read. It rides here as the clock does, so the loop takes no new parameter.
    connector_turn: Option<crate::tool_loop::connector_reads::ConnectorTurn>,
    /// The turn's web-search provenance record (EP-9 W1), attached only while the line is switched on. Every
    /// answered search records on it, forced or looped, so the one frame after the tool phase names them all.
    search_provenance: Option<crate::search_provenance::SearchProvenanceTurn>,
    /// The stream's resume buffer and its shared frame cursor, when the caller
    /// attached them. Tool calls, tool results and steps used to go out on the
    /// live channel only, so a reload during the tool phase, source review or
    /// repair resumed into a stream that had none of that progress.
    resume: Option<SinkResume>,
    /// A typed subscription refusal the tool phase ended on, and what it was
    /// producing. The tool loop can only return a `&'static str` reason, so the
    /// timing that came with the refusal waits here for the stream handler.
    subscription_failure: std::sync::Mutex<
        Option<(
            crate::subscription_failure::SubscriptionFailure,
            crate::subscription_failure::FailureContext,
        )>,
    >,
    /// A typed zero-retention or model-entitlement refusal the tool phase ended on (V3-mg-1), kept whole so its frame
    /// keeps the catalogue's code, action and feature. The tool loop can only return a reason string.
    typed_refusal: std::sync::Mutex<Option<TypedRefusal>>,
}

/// What a sink keeps of a turn's calls: the running total every progress frame states, and the turn's
/// run ceiling when one is tracked.
#[derive(Default)]
struct RunTally {
    meter: crate::usage_meter::UsageMeter,
    ceiling: Option<crate::sse_events::run_ceiling::CeilingTracker>,
}

/// The tokens a call adds to a token cap: its input and output when it is metered. A subscription
/// call's tokens are usage, not spend, so it adds none, as cost-core counts them.
#[must_use]
pub fn metered_tokens(call: &crate::usage_meter::CallUsage) -> u64 {
    if call.basis != crate::pricing::CostBasis::Metered {
        return 0;
    }
    let count = |tokens: i64| u64::try_from(tokens.max(0)).unwrap_or(0);
    count(call.input).saturating_add(count(call.output))
}

/// The metered tokens of a set of calls, as a token cap counts them.
#[must_use]
pub fn metered_tokens_of<'a>(
    calls: impl IntoIterator<Item = &'a crate::usage_meter::CallUsage>,
) -> u64 {
    calls
        .into_iter()
        .map(metered_tokens)
        .fold(0, u64::saturating_add)
}

/// Where a sink buffers what it sends: the same store, key and `id:` cursor
/// the stream's own frames use, so one resume cursor orders them all.
struct SinkResume {
    buffers: crate::stream_buffer::StreamBufferStore,
    key: String,
    /// The next frame id. Held across append and send so concurrent emits
    /// (a round's parallel tool results) reach the buffer and the wire in the
    /// same order.
    next_seq: tokio::sync::Mutex<u64>,
}

/// A generated image or file, whose payload is an inline `data:` URI. A preview is binary as well, and it is the
/// larger of the two, so it bypasses the resume buffer with the file it was rendered from.
fn carries_binary(event: &ChatEvent) -> bool {
    match event {
        ChatEvent::Artifact { content, .. } => content.starts_with("data:"),
        ChatEvent::Attachment { url, preview, .. } => url.starts_with("data:") || preview.is_some(),
        _ => false,
    }
}

/// A citation observed on the wire, in the shape the SPA already reads back.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecordedCitation {
    pub id: String,
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source_group_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_group_label: Option<String>,
    /// The `[n]` this citation was assigned when numbered — see
    /// `ChatEvent::Citation::number`. Used by `citation_anchors::anchor_citations`
    /// to resolve a model-cited marker back to this citation by stable
    /// number rather than by position in this vec.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    /// See `ChatEvent::Citation::origin`.
    pub origin: String,
    /// See `ChatEvent::Citation::passage`. Flattened, so history carries `quote` and `locator`
    /// under the same keys the stream does, and nothing when it is `None`.
    #[serde(flatten)]
    pub passage: Option<CitationPassage>,
}

impl RecordedCitation {
    /// The citation without its passage, for a turn that may not persist one (D12: a ZDR or
    /// waived turn streams the passage but never stores it).
    #[must_use]
    pub fn without_passage(self) -> Self {
        Self { passage: None, ..self }
    }
}

/// An authored artifact observed on the wire, in the shape the SPA's
/// `isChatArtifact` accepts back from a thread read.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RecordedArtifact {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub version: u32,
    /// See `ChatEvent::Artifact::provenance`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Box<crate::deep_research::ResearchProvenance>>,
}

impl RecordedArtifact {
    /// Largest artifact body persisted with a message. Matches the authoring
    /// cap in `artifacts::MAX_TEXT_ARTIFACT_CHARS`, so nothing the model may
    /// write is ever too big to keep.
    pub const MAX_PERSISTED_CHARS: usize = 200_000;

    /// `Some` for a model-authored text artifact (document/code/html) and for a
    /// deck manifest (EP-5 S3); `None` for generated files and images, whose
    /// `content` is a base64 `data:` URI far too large for a metadata column and
    /// already downloadable from the message's attachment.
    ///
    /// A deck is kept only as a manifest that reads back under the deck's own
    /// cap and names the id it is kept under: its pptx is never stored, it is
    /// rebuilt from this manifest. A `deck` frame only the `deck` tool emits,
    /// and only while `MODEL_GATEWAY_DECK_TOOL` is on.
    #[must_use]
    pub fn from_event(event: &ChatEvent) -> Option<Self> {
        let ChatEvent::Artifact {
            id,
            kind,
            title,
            content,
            version,
            provenance,
            // The editor is a property of the stored revision, not of the message metadata.
            editor: _,
        } = event
        else {
            return None;
        };
        match kind.as_str() {
            "document" | "code" | "html" => {}
            "deck" if is_storable_deck(id, content) => {}
            _ => return None,
        }
        if content.starts_with("data:") || content.chars().count() > Self::MAX_PERSISTED_CHARS {
            return None;
        }
        Some(Self {
            id: id.clone(),
            kind: kind.clone(),
            title: title.clone(),
            content: content.clone(),
            version: *version,
            provenance: provenance.clone(),
        })
    }

    /// The artifact without its provenance stamp, for a turn that may not persist one (D12).
    #[must_use]
    pub fn without_provenance(self) -> Self {
        Self { provenance: None, ..self }
    }

    /// The live frame for this artifact, as the staged publish re-emits it after persisting.
    #[must_use]
    pub fn into_event(self) -> ChatEvent {
        ChatEvent::Artifact {
            id: self.id,
            kind: self.kind,
            title: self.title,
            content: self.content,
            version: self.version,
            provenance: self.provenance,
            editor: None,
        }
    }
}

/// Whether `content` is a deck manifest fit to keep under `id`: JSON that reads as a manifest
/// (which also holds it to `MAX_MANIFEST_CHARS`) and names the same deck. A frame that does not is
/// streamed but never stored, so a reload cannot hand the `deck` tool a manifest it would refuse.
fn is_storable_deck(id: &str, content: &str) -> bool {
    mp_contracts::deck_manifest::DeckManifest::from_untrusted_json(content)
        .is_ok_and(|manifest| manifest.deck_id == id)
}

impl RichEventSink {
    #[must_use]
    pub fn new(
        tx: Sender<Result<Event, Infallible>>,
        features: Vec<String>,
        request_id: String,
    ) -> Self {
        Self {
            stage_text_artifacts: false,
            tx,
            features,
            request_id,
            citations: std::sync::Arc::default(),
            artifacts: std::sync::Arc::default(),
            overhead: std::sync::Arc::default(),
            usage_progress: false,
            ceiling_tracked: false,
            tally: std::sync::Mutex::new(RunTally::default()),
            timing: None,
            connector_turn: None,
            search_provenance: None,
            resume: None,
            subscription_failure: std::sync::Mutex::new(None),
            typed_refusal: std::sync::Mutex::new(None),
        }
    }

    /// Keep the typed refusal a tool phase ended on. The first one is kept:
    /// it is what stopped the phase.
    pub fn set_subscription_failure(
        &self,
        failure: crate::subscription_failure::SubscriptionFailure,
        context: crate::subscription_failure::FailureContext,
    ) {
        if let Ok(mut slot) = self.subscription_failure.lock() {
            slot.get_or_insert((failure, context));
        }
    }

    /// Take the typed refusal the tool phase ended on, if it ended on one.
    pub fn take_subscription_failure(
        &self,
    ) -> Option<(
        crate::subscription_failure::SubscriptionFailure,
        crate::subscription_failure::FailureContext,
    )> {
        self.subscription_failure
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }

    /// Keep the typed zero-retention or model-entitlement refusal a tool phase ended on. The first one is kept, as
    /// the subscription failure is.
    pub fn set_typed_refusal(&self, refusal: TypedRefusal) {
        if let Ok(mut slot) = self.typed_refusal.lock() {
            slot.get_or_insert(refusal);
        }
    }

    /// Take the typed refusal the tool phase ended on, if it ended on one.
    pub fn take_typed_refusal(&self) -> Option<TypedRefusal> {
        self.typed_refusal
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }

    /// Text artifacts are private candidates until the caller persists the
    /// completed assistant message and terminalizes the run. Progress/citations
    /// continue to stream. Dropping this sink discards unpublished candidates.
    pub fn staging_artifacts(
        tx: Sender<Result<Event, Infallible>>, features: Vec<String>, request_id: String,
    ) -> Self {
        Self { stage_text_artifacts: true, ..Self::new(tx, features, request_id) }
    }

    /// Attach the turn's clock.
    #[must_use]
    pub fn with_timing(mut self, timing: crate::turn_timing::TurnTiming) -> Self {
        self.timing = Some(timing);
        self
    }

    /// The turn's clock, when the caller attached one.
    #[must_use]
    pub fn timing(&self) -> Option<&crate::turn_timing::TurnTiming> {
        self.timing.as_ref()
    }

    /// Attach the connected accounts this turn selected (WS23-S5).
    #[must_use]
    pub fn with_connector_turn(
        mut self,
        turn: crate::tool_loop::connector_reads::ConnectorTurn,
    ) -> Self {
        self.connector_turn = Some(turn);
        self
    }

    /// The turn's connected-account selection, when the route attached one.
    #[must_use]
    pub fn connector_turn(&self) -> Option<&crate::tool_loop::connector_reads::ConnectorTurn> {
        self.connector_turn.as_ref()
    }

    /// Attach the turn's web-search provenance record (EP-9 W1). The caller attaches one only while the line is
    /// switched on, so with the switch off the sink never records or sends anything for it.
    #[must_use]
    pub fn with_search_provenance(
        mut self,
        turn: Option<crate::search_provenance::SearchProvenanceTurn>,
    ) -> Self {
        self.search_provenance = turn;
        self
    }

    /// The turn's web-search provenance record, when the caller attached one.
    #[must_use]
    pub fn search_provenance(&self) -> Option<&crate::search_provenance::SearchProvenanceTurn> {
        self.search_provenance.as_ref()
    }

    /// Send the turn's search-provenance frame, once. The caller invokes it after the tool phase, so every
    /// search of the turn is on the record. Sends nothing while no record is attached or no search ran.
    pub async fn emit_search_provenance(&self) {
        if let Some(provenance) = self
            .search_provenance
            .as_ref()
            .and_then(crate::search_provenance::SearchProvenanceTurn::take_provenance)
        {
            self.emit(ChatEvent::SearchProvenance { provenance }).await;
        }
    }

    /// Buffer every frame this sink sends for resume, under the stream's own
    /// `id:` sequence starting at `next_seq`. The caller hands its cursor over
    /// and takes it back with [`sync_cursor`](Self::sync_cursor) around every
    /// stretch in which the sink emits.
    #[must_use]
    pub fn buffering(
        mut self,
        buffers: crate::stream_buffer::StreamBufferStore,
        key: String,
        next_seq: u64,
    ) -> Self {
        self.resume = Some(SinkResume {
            buffers,
            key,
            next_seq: tokio::sync::Mutex::new(next_seq),
        });
        self
    }

    /// Reconcile the caller's frame cursor with this sink's and return the
    /// shared value. Both only ever grow and never emit at the same time, so
    /// the larger one is always the next free id: calling this before handing
    /// the sink to the tool loop gives the sink the caller's cursor, and
    /// calling it after gives the caller the sink's. Without a buffer the
    /// cursor comes back unchanged.
    pub async fn sync_cursor(&self, seq: u64) -> u64 {
        let Some(resume) = &self.resume else {
            return seq;
        };
        let mut next = resume.next_seq.lock().await;
        *next = (*next).max(seq);
        *next
    }

    /// Record citations that reached the wire BEFORE this sink existed,
    /// without re-emitting them.
    ///
    /// chat-parity §8 emits KB/RAG grounding's citations "up front" (so the
    /// Sources panel can render before the answer streams), and it does so
    /// via the free-standing `emit_and_buffer` helper directly on the
    /// stream's `tx`/buffer — at that point in the handler `RichEventSink`
    /// has not been constructed yet. `emit()` is the ONLY place that
    /// normally appends to `self.citations`, so without this call the
    /// up-front citations would never appear in `recorded_citations()` —
    /// silently starving both the post-stream anchor pass
    /// (`citation_anchors::anchor_citations`, chat-wave2 citations spec
    /// §2.4) and persisted `turn_evidence_metadata`'s `citations` field of
    /// the single most common citation producer. Call this once, right
    /// after constructing the sink, with the same citations already sent.
    pub fn seed_recorded_citations(&self, citations: impl IntoIterator<Item = RecordedCitation>) {
        if let Ok(mut recorded) = self.citations.lock() {
            recorded.extend(citations);
        }
    }

    /// Emit an event if the client opted into its family (control events always
    /// emit). Best-effort: a closed channel (client gone) is ignored.
    ///
    /// With a resume buffer attached the frame is buffered first and sent
    /// with the next `id:`. Staged documents never get here, and generated
    /// binaries (a `data:` image or file) stay live-only as on the image
    /// route: one such frame can be megabytes, and the saved message and its
    /// attachment are how a reload gets it back.
    pub async fn emit(&self, event: ChatEvent) {
        // Recorded BEFORE the feature gate on purpose: which sources a turn
        // used is a fact about the turn, not about what this client opted into
        // receiving. Persisting only what one client subscribed to would make
        // durable history depend on the shape of the request that created it.
        if let ChatEvent::Citation {
            id,
            title,
            url,
            snippet,
            source_group_id,
            source_group_label,
            number,
            origin,
            passage,
            ..
        } = &event
        {
            if let Ok(mut recorded) = self.citations.lock() {
                recorded.push(RecordedCitation {
                    id: id.clone(),
                    title: title.clone(),
                    url: url.clone(),
                    snippet: snippet.clone(),
                    source_group_id: source_group_id.clone(),
                    source_group_label: source_group_label.clone(),
                    number: *number,
                    origin: origin.clone(),
                    passage: passage.clone(),
                });
            }
        }
        if let Some(artifact) = RecordedArtifact::from_event(&event) {
            if let Ok(mut recorded) = self.artifacts.lock() {
                recorded.push(artifact);
            }
            if self.stage_text_artifacts { return; }
        }
        if !event.should_emit(&self.features) {
            return;
        }
        if let Some(resume) = self.resume.as_ref().filter(|_| !carries_binary(&event)) {
            let name = event.name();
            let data = event.payload(&self.request_id).to_string();
            let mut next = resume.next_seq.lock().await;
            // Buffer before sending, as `emit_and_buffer` does, so a reconnect
            // can never observe a frame the buffer does not have.
            resume.buffers.append(&resume.key, *next, name, &data).await;
            let _ = self
                .tx
                .send(Ok(Event::default()
                    .id(next.to_string())
                    .event(name)
                    .data(data)))
                .await;
            *next += 1;
            return;
        }
        let _ = self.tx.send(Ok(event.to_sse(&self.request_id))).await;
    }

    /// Record a provider call the turn made beside its answer. It is stored as an
    /// overhead call whatever phase it was built with.
    pub fn record_overhead(&self, call: crate::usage_meter::CallUsage) {
        if let Ok(mut recorded) = self.overhead.lock() {
            recorded.push(crate::usage_meter::CallUsage {
                phase: crate::usage_meter::CallPhase::Overhead,
                ..call
            });
        }
    }

    /// The overhead calls recorded so far, in the order they ran.
    #[must_use]
    pub fn overhead_usage(&self) -> Vec<crate::usage_meter::CallUsage> {
        self.overhead
            .lock()
            .map(|recorded| recorded.clone())
            .unwrap_or_default()
    }

    /// Stream a `usage_progress` frame for each call this sink records, when `enabled`.
    #[must_use]
    pub fn with_usage_progress(mut self, enabled: bool) -> Self {
        self.usage_progress = enabled;
        self
    }

    /// Whether this sink streams usage progress.
    #[must_use]
    pub fn streams_usage_progress(&self) -> bool {
        self.usage_progress
    }

    /// Attach the turn's run ceiling (WS27-S6), when the handler resolved one. The tracker carries
    /// the switches, so the sink records calls exactly when it holds one.
    #[must_use]
    pub fn with_run_ceiling(mut self, ceiling: Option<crate::sse_events::run_ceiling::CeilingTracker>) -> Self {
        self.ceiling_tracked = ceiling.is_some();
        if let Ok(tally) = self.tally.get_mut() {
            tally.ceiling = ceiling;
        }
        self
    }

    /// Whether this sink records the calls it is given: it streams usage progress, or it tracks a run
    /// ceiling. A caller prices a call for [`Self::record_call`] only then, so a turn with neither reads
    /// the price catalogue no more often than it did before.
    #[must_use]
    pub fn records_calls(&self) -> bool {
        self.usage_progress || self.ceiling_tracked
    }

    /// Record a provider call the turn made and, when the sink streams usage progress, stream the turn's
    /// running total after it. Does nothing unless the sink records calls.
    pub async fn record_call(
        &self,
        call: &crate::usage_meter::CallUsage,
        priced: &crate::pricing::Priced,
        routing: Option<&'static str>,
    ) {
        if !self.records_calls() {
            return;
        }
        // The lock is released before the emit below: a frame is never sent while the tally is held.
        let Ok((snapshot, ceiling)) = self.tally.lock().map(|mut tally| {
            let snapshot = tally.meter.record(call, priced);
            let ceiling = tally
                .ceiling
                .as_mut()
                .and_then(|ceiling| ceiling.record(metered_tokens(call), snapshot.cost_usd));
            (snapshot, ceiling)
        }) else {
            return;
        };
        if self.usage_progress {
            self.emit(ChatEvent::UsageProgress {
                phase: call.phase,
                snapshot,
                ceiling,
                routing,
                // A tool round or overhead call carries no answer decision; only the answer's own frame does.
                routing_decision: None,
            })
            .await;
        }
    }

    /// The run ceiling as the closing answer frame states it: the turn's own total, and the metered
    /// tokens of every call the turn counts, the answer's included, since the answer is not in the tally
    /// yet. `None` unless the ceiling reports on frames.
    #[must_use]
    pub fn run_ceiling_at_turn_end(
        &self,
        turn: &crate::usage_meter::Snapshot,
        turn_metered_tokens: u64,
    ) -> Option<crate::sse_events::run_ceiling::CeilingProgress> {
        let mut tally = self.tally.lock().ok()?;
        tally
            .ceiling
            .as_mut()?
            .progress(turn.cost_usd, turn_metered_tokens)
    }

    /// Whether the tool phase ends before its next round: enforcement is on and the run has reached a
    /// cap. The tool loop asks at each round boundary.
    #[must_use]
    pub fn run_ceiling_stops_tool_phase(&self) -> bool {
        self.tally.lock().is_ok_and(|tally| {
            tally
                .ceiling
                .as_ref()
                .is_some_and(|ceiling| ceiling.stops_tool_phase(tally.meter.snapshot().cost_usd))
        })
    }

    /// The artifacts seen so far, one entry per id carrying its LATEST version,
    /// in first-seen order. A reopened thread needs the final document, not
    /// every draft the loop went through.
    #[must_use]
    pub fn recorded_artifacts(&self) -> Vec<RecordedArtifact> {
        let Ok(recorded) = self.artifacts.lock() else {
            return Vec::new();
        };
        let mut order: Vec<String> = Vec::new();
        let mut latest: std::collections::HashMap<String, RecordedArtifact> =
            std::collections::HashMap::new();
        for artifact in recorded.iter() {
            match latest.get(&artifact.id) {
                Some(existing) if existing.version >= artifact.version => {}
                Some(_) => {
                    latest.insert(artifact.id.clone(), artifact.clone());
                }
                None => {
                    order.push(artifact.id.clone());
                    latest.insert(artifact.id.clone(), artifact.clone());
                }
            }
        }
        order
            .into_iter()
            .filter_map(|id| latest.remove(&id))
            .collect()
    }

    /// The citations seen so far, deduplicated by url (falling back to id) in
    /// first-seen order — the tool loop can legitimately re-cite one source
    /// across rounds, and history should list it once.
    #[must_use]
    pub fn recorded_citations(&self) -> Vec<RecordedCitation> {
        let Ok(recorded) = self.citations.lock() else {
            return Vec::new();
        };
        let mut seen = std::collections::HashSet::new();
        recorded
            .iter()
            .filter(|citation| {
                let key = if citation.url.is_empty() {
                    &citation.id
                } else {
                    &citation.url
                };
                seen.insert(key.clone())
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn staged_artifacts_never_reach_the_wire_on_a_failed_turn() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let sink = RichEventSink::staging_artifacts(tx, families(&["artifacts", "steps"]), "r".into());
        for version in [3, 4] {
            sink.emit(ChatEvent::Artifact { id: "doc".into(), kind: "document".into(),
                title: "Draft".into(), content: format!("private {version}"), version, provenance: None, editor: None }).await;
        }
        assert!(rx.try_recv().is_err());
        assert_eq!(sink.recorded_artifacts()[0].version, 4);
        sink.emit(ChatEvent::Error { code: "response_validation_failed".into(),
            message: "Not completed".into(), retryable: true }).await;
        assert!(rx.try_recv().is_ok(), "failure still reaches the browser");
        drop(sink);
        assert!(rx.try_recv().is_err(), "dropping a failed run never releases drafts");
    }

    fn families(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    /// A subscription refusal is an ordinary, ungated `error` frame with two
    /// optional fields, each left out rather than sent as null.
    #[test]
    fn a_subscription_error_is_an_error_frame_that_omits_unknown_timing() {
        use crate::subscription_failure::{SubscriptionFailure, SUBSCRIPTION_USAGE_LIMIT};
        let failure = SubscriptionFailure {
            code: SUBSCRIPTION_USAGE_LIMIT,
            retry_after_ms: Some(52_260_000),
            resets_at: chrono::DateTime::parse_from_rfc3339("2026-09-26T01:21:00Z")
                .ok()
                .map(|at| at.with_timezone(&chrono::Utc)),
        };
        let event = ChatEvent::subscription_error(&failure, "Grensen er nådd.".into());
        assert_eq!(event.name(), "error");
        assert_eq!(event.family(), None);
        assert!(event.should_emit(&[]), "the plain chat profile receives it");
        assert_eq!(
            event.payload("req-1"),
            serde_json::json!({
                "code": "subscription_usage_limit",
                "message": "Grensen er nådd.",
                "retryable": false,
                "resets_at": "2026-09-26T01:21:00Z",
                "retry_after_ms": 52_260_000_u64,
                "retry_at": "2026-09-26T01:21:00Z",
                "request_id": "req-1",
            })
        );

        // A wait alone is dated when the frame is built, so a replay of the
        // buffered frame names the same instant rather than a later one.
        let built = chrono::DateTime::parse_from_rfc3339("2026-09-25T10:50:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let throttled = ChatEvent::subscription_error_at(
            &SubscriptionFailure {
                retry_after_ms: Some(1_500),
                ..SubscriptionFailure::bare(crate::subscription_failure::RATE_LIMITED)
            },
            "Vent litt.".into(),
            built,
        );
        let payload = throttled.payload("req-1");
        assert_eq!(payload["retry_after_ms"], 1_500);
        assert_eq!(payload["retry_at"], "2026-09-25T10:50:02Z");
        assert!(payload.get("resets_at").is_none(), "{payload}");

        let bare = ChatEvent::subscription_error(
            &SubscriptionFailure::bare(crate::subscription_failure::SUBSCRIPTION_UPSTREAM_UNAVAILABLE),
            "Prøv igjen.".into(),
        );
        let plain = ChatEvent::Error {
            code: "subscription_upstream_unavailable".into(),
            message: "Prøv igjen.".into(),
            retryable: true,
        };
        assert_eq!(
            bare.payload("req-1"),
            plain.payload("req-1"),
            "with no timing it is exactly the Error payload"
        );
    }

    #[test]
    fn the_sink_keeps_the_first_subscription_failure_until_it_is_taken() {
        use crate::subscription_failure::{FailureContext, SubscriptionFailure};
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let sink = RichEventSink::new(tx, Vec::new(), "r".into());
        assert!(sink.take_subscription_failure().is_none());
        sink.set_subscription_failure(
            SubscriptionFailure::bare("subscription_reauth_required"),
            FailureContext::Draft,
        );
        sink.set_subscription_failure(
            SubscriptionFailure::bare("rate_limited"),
            FailureContext::ToolRound,
        );
        assert_eq!(
            sink.take_subscription_failure(),
            Some((
                SubscriptionFailure::bare("subscription_reauth_required"),
                FailureContext::Draft
            ))
        );
        assert!(sink.take_subscription_failure().is_none());
    }

    /// V3-mg-1: a typed refusal keeps its catalogue code, action and feature on the frame that ends the turn, and its
    /// retry flag is the catalogue's. Only a catalogue kind is a typed refusal, and the sink keeps the first one until
    /// the stream takes it.
    #[test]
    fn a_typed_refusal_keeps_its_catalogue_frame_and_the_sink_keeps_the_first() {
        use mp_contracts::model_entitlement::{
            code as entitlement_code, Reason, Refusal as EntitlementRefusal,
        };
        use mp_contracts::zdr_refusal::{code as zdr_code, Refusal as ZdrRefusal};

        let zdr = TypedRefusal::from_status(
            &ZdrRefusal::CONNECTION_NOT_ACKNOWLEDGED.grpc_status(tonic::Code::FailedPrecondition),
        )
        .expect("a catalogue ZDR kind is typed");
        assert_eq!(zdr.code(), zdr_code::CONNECTION_NOT_ACKNOWLEDGED);
        assert!(!zdr.retryable());
        let payload = zdr.clone().into_event().payload("req-1");
        assert_eq!(payload["code"], zdr_code::CONNECTION_NOT_ACKNOWLEDGED);
        assert_eq!(payload["retryable"], false);
        assert_eq!(payload["message"].as_str(), Some(zdr.message().as_str()));
        assert!(payload["action"].is_string(), "{payload}");
        assert_eq!(payload["request_id"], "req-1");

        let entitled = TypedRefusal::from_status(
            &EntitlementRefusal::not_entitled("gpt-x", Reason::NotInAllowList).grpc_status(),
        )
        .expect("a model-entitlement kind is typed");
        assert_eq!(entitled.code(), entitlement_code::MODEL_NOT_ENTITLED);
        let payload = entitled.clone().into_event().payload("req-1");
        assert_eq!(payload["code"], entitlement_code::MODEL_NOT_ENTITLED);
        assert_eq!(payload["retryable"], false);
        assert_eq!(
            payload["message"].as_str(),
            Some(entitled.message().as_str())
        );

        let unavailable =
            TypedRefusal::from_status(&EntitlementRefusal::unavailable().grpc_status())
                .expect("the unavailable entitlement kind is typed");
        assert_eq!(
            unavailable.code(),
            entitlement_code::ENTITLEMENT_UNAVAILABLE
        );
        assert_eq!(
            unavailable.into_event().payload("req-1")["retryable"],
            true,
            "the catalogue says to try again shortly"
        );

        // A subscription reason and an untyped failure are not typed refusals.
        assert_eq!(
            TypedRefusal::from_status(&tonic::Status::unavailable("upstream said: nope")),
            None
        );
        let mut usage = tonic::Status::resource_exhausted("quota");
        usage.metadata_mut().insert(
            crate::compaction::PROVIDER_ERROR_KIND_TRAILER,
            "subscription_usage_limit".parse().expect("ascii"),
        );
        assert_eq!(TypedRefusal::from_status(&usage), None);

        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let sink = RichEventSink::new(tx, Vec::new(), "r".into());
        assert_eq!(sink.take_typed_refusal(), None);
        sink.set_typed_refusal(zdr.clone());
        sink.set_typed_refusal(entitled);
        assert_eq!(sink.take_typed_refusal(), Some(zdr));
        assert_eq!(sink.take_typed_refusal(), None);
    }

    /// Q15: tool calls, tool results and steps went out on the live channel
    /// only, so a reload during the tool phase or source review resumed with
    /// none of that progress. A buffering sink continues the stream's own
    /// cursor; staged drafts, gated events and generated binaries take no id.
    #[tokio::test]
    async fn a_buffering_sink_continues_the_stream_cursor() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let buffers = crate::stream_buffer::StreamBufferStore::new();
        let features = families(&["tools", "steps", "artifacts"]);
        let sink = RichEventSink::staging_artifacts(tx, features, "r".into()).buffering(
            buffers.clone(),
            "key".into(),
            0,
        );
        // The stream buffered frames 0..3 itself before the tool phase.
        assert_eq!(sink.sync_cursor(3).await, 3);
        sink.emit(ChatEvent::StepUpdate {
            id: "s".into(),
            title: "Kontrollerer".into(),
            detail: String::new(),
            status: "running".into(),
        })
        .await;
        sink.emit(ChatEvent::ToolCall {
            id: "c".into(),
            name: "web_search".into(),
            args: json!({}),
        })
        .await;
        let artifact = |id: &str, kind: &str, content: &str| ChatEvent::Artifact {
            id: id.into(),
            kind: kind.into(),
            title: "T".into(),
            content: content.into(),
            version: 1,
            provenance: None,
            editor: None,
        };
        sink.emit(artifact("doc", "document", "private draft"))
            .await;
        sink.emit(ChatEvent::ReasoningDelta {
            delta: "not opted in".into(),
        })
        .await;
        sink.emit(artifact("img", "image", "data:image/png;base64,AAAA"))
            .await;
        assert_eq!(
            sink.sync_cursor(0).await,
            5,
            "the stream continues after the sink's frames"
        );

        let replay = buffers.replay_after("key", None).await;
        let buffered: Vec<(u64, &str)> = replay
            .deltas
            .iter()
            .map(|frame| (frame.seq, frame.event.as_str()))
            .collect();
        assert_eq!(buffered, vec![(3, "step_update"), (4, "tool_call")]);
        // Live: the step, the tool call and the image; never the staged draft.
        let mut live = 0;
        while rx.try_recv().is_ok() {
            live += 1;
        }
        assert_eq!(live, 3);
    }

    /// The anchor pass (`citation_anchors.rs`) re-emits a `Citation` frame
    /// with the SAME `id` once the answer is complete, now carrying
    /// `claim_id`. `SinkResume`'s buffer is generic over any `ChatEvent` and
    /// keys purely on the stream's own sequence number, never on a citation's
    /// logical id, so a reconnect must replay BOTH frames in order — a
    /// client that only saw the buffered replay must still end up with the
    /// anchored citation, the same as one that watched the live stream (see
    /// `chat-normalizers.ts::upsertCitation`'s merge-on-duplicate-id change).
    #[tokio::test]
    async fn a_repeated_citation_id_is_buffered_and_replayed_both_times() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let buffers = crate::stream_buffer::StreamBufferStore::new();
        let sink = RichEventSink::new(tx, families(&["citations"]), "r".into()).buffering(
            buffers.clone(),
            "key".into(),
            0,
        );
        let first = ChatEvent::Citation {
            id: "c1".into(),
            title: "Doc".into(),
            url: "https://kb/doc".into(),
            snippet: "hello".into(),
            source_group_id: "kb:sharepoint".into(),
            source_group_label: Some("Microsoft 365".into()),
            number: Some(1),
            claim_id: None,
            start: None,
            end: None,
            origin: "knowledge".into(),
            passage: None,
        };
        let anchored = ChatEvent::Citation {
            id: "c1".into(),
            title: "Doc".into(),
            url: "https://kb/doc".into(),
            snippet: "hello".into(),
            source_group_id: "kb:sharepoint".into(),
            source_group_label: Some("Microsoft 365".into()),
            number: Some(1),
            claim_id: Some("r:claim-0".into()),
            start: Some(4),
            end: Some(7),
            origin: "knowledge".into(),
            passage: None,
        };
        sink.emit(first).await;
        sink.emit(anchored).await;

        let replay = buffers.replay_after("key", None).await;
        let buffered: Vec<(u64, &str)> = replay
            .deltas
            .iter()
            .map(|frame| (frame.seq, frame.event.as_str()))
            .collect();
        assert_eq!(
            buffered,
            vec![(0, "citation"), (1, "citation")],
            "both the original and the anchor-pass update must be buffered"
        );
        assert!(!replay.deltas[0].data.contains("claim_id"));
        assert!(replay.deltas[1].data.contains("r:claim-0"));

        let mut live = Vec::new();
        while let Ok(Ok(event)) = rx.try_recv() {
            live.push(event);
        }
        assert_eq!(live.len(), 2, "both frames also reach the live channel");
    }

    /// Regression: KB/RAG grounding's citations reach the wire via the
    /// free-standing `emit_and_buffer` helper before `RichEventSink` exists
    /// (chat-parity §8's up-front emission, `sse.rs` around its `sink`
    /// construction). Without `seed_recorded_citations`, those citations
    /// never entered `self.citations`, so `recorded_citations()` — and the
    /// post-stream anchor pass built on it (chat-wave2 citations spec §2.4)
    /// — silently never saw the single most common citation producer.
    #[tokio::test]
    async fn seeded_citations_appear_in_recorded_citations_without_reaching_the_wire() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let sink = RichEventSink::new(tx, families(&["citations"]), "r".into());
        sink.seed_recorded_citations([RecordedCitation {
            id: "kb-1".into(),
            title: "Policy".into(),
            url: "https://kb/policy".into(),
            snippet: "Refunds within 30 days.".into(),
            source_group_id: "kb:sharepoint".into(),
            source_group_label: Some("Microsoft 365".into()),
            number: Some(1),
            origin: "knowledge".into(),
            passage: None,
        }]);
        let recorded = sink.recorded_citations();
        assert_eq!(recorded.len(), 1, "the seeded citation must be visible to recorded_citations()");
        assert_eq!(recorded[0].id, "kb-1");
        assert_eq!(recorded[0].source_group_id, "kb:sharepoint");
        // Seeding records without emitting: it describes what already went
        // out via `emit_and_buffer` directly, so a live-channel send here
        // would duplicate the frame the client already received.
        assert!(rx.try_recv().is_err(), "seeding must never itself send a frame");
    }

    #[tokio::test]
    async fn a_sink_without_a_buffer_leaves_the_cursor_alone() {
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let sink = RichEventSink::new(tx, families(&["tools"]), "r".into());
        sink.emit(ChatEvent::ToolCall {
            id: "c".into(),
            name: "web_search".into(),
            args: json!({}),
        })
        .await;
        assert_eq!(sink.sync_cursor(7).await, 7);
    }

    /// The durable read returned zero artifacts for a turn that produced
    /// three (RUN-LOG finding 3). The sink must keep what went out — latest
    /// version per id, drafts collapsed, binaries excluded.
    #[tokio::test]
    async fn sink_records_the_latest_version_of_each_text_artifact() {
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let sink = RichEventSink::new(tx, families(&["artifacts"]), "req".into());
        let artifact = |id: &str, kind: &str, content: &str, version: u32| ChatEvent::Artifact {
            id: id.into(),
            kind: kind.into(),
            title: format!("T {id}"),
            content: content.into(),
            version,
            provenance: None,
            editor: None,
        };
        sink.emit(artifact("rapport", "document", "v1 body", 1)).await;
        sink.emit(artifact("notat", "document", "memo", 1)).await;
        sink.emit(artifact("rapport", "document", "v2 body", 2)).await;
        // A generated file is a data: URI and never belongs in metadata.
        sink.emit(artifact("chart", "image", "data:image/png;base64,AAAA", 1)).await;
        sink.emit(artifact("sheet", "spreadsheet", "data:application/x;base64,AA", 1)).await;

        let recorded = sink.recorded_artifacts();
        assert_eq!(recorded.len(), 2, "one entry per text artifact id");
        assert_eq!(recorded[0].id, "rapport", "first-seen order is kept");
        assert_eq!(recorded[0].version, 2);
        assert_eq!(recorded[0].content, "v2 body", "the latest draft wins");
        assert_eq!(recorded[1].id, "notat");
    }

    #[test]
    fn plain_path_emits_only_control_events() {
        // Empty features = profile:"chat" plain path. Rich events are suppressed;
        // the terminal control event (Stopped) still passes.
        let no_features: Vec<String> = vec![];
        assert!(!ChatEvent::ReasoningDelta { delta: "x".into() }.should_emit(&no_features));
        assert!(!ChatEvent::ToolCall {
            id: "1".into(),
            name: "shell".into(),
            args: json!({})
        }
        .should_emit(&no_features));
        assert!(!ChatEvent::Usage {
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            calls: 1,
            unpriced_calls: 0,
            cost_by_category: None,
            input_tokens: 1,
            output_tokens: 2,
            cost_usd: None,
            cost_basis: None,
            latency_ms: 10,
            confidence: None,
            served_by: None,
            retention_class: None,
            routing_decision: None,
        }
        .should_emit(&no_features));
        assert!(ChatEvent::Stopped {
            reason: "user".into()
        }
        .should_emit(&no_features));
        // Title is a control event: a generated title must reach the plain
        // `chat` profile — the sidebar is exactly that surface.
        let title = ChatEvent::Title {
            title: "Visma fakturastatus".into(),
        };
        assert!(title.should_emit(&no_features));
        assert_eq!(title.name(), "title");
        assert_eq!(title.payload("req-9")["title"], "Visma fakturastatus");
        assert_eq!(title.payload("req-9")["request_id"], "req-9");
        // FollowUps is a control event too — same reasoning as Title: the
        // composer chips are a plain-chat surface, not an opt-in feature.
        let follow_ups = ChatEvent::FollowUps {
            suggestions: vec!["Hva med frakt til Bergen?".into()],
        };
        assert!(follow_ups.should_emit(&no_features));
        assert_eq!(follow_ups.name(), "follow_ups");
        assert_eq!(
            follow_ups.payload("req-9")["suggestions"][0],
            "Hva med frakt til Bergen?"
        );
        assert_eq!(follow_ups.payload("req-9")["request_id"], "req-9");
        // plan_proposed is a control event too (plan-gate §2.1): essential
        // for PlanApprovalControl's edit surface, not an opt-in enhancement.
        let plan_proposed = ChatEvent::PlanProposed {
            plan_id: "plan_abc123".into(),
            summary: "Send the overdue invoice".into(),
            steps: json!([{ "id": "step_1", "title": "Draft the email", "operation": "" }]),
        };
        assert!(plan_proposed.should_emit(&no_features));
        assert_eq!(plan_proposed.name(), "plan_proposed");
        assert_eq!(
            plan_proposed.payload("req-9")["plan_id"],
            "plan_abc123"
        );
        assert_eq!(
            plan_proposed.payload("req-9")["summary"],
            "Send the overdue invoice"
        );
        assert_eq!(
            plan_proposed.payload("req-9")["steps"][0]["title"],
            "Draft the email"
        );
        assert_eq!(plan_proposed.payload("req-9")["request_id"], "req-9");
        // Error is a control event too — it must reach the plain path so a
        // profile:"chat" client still learns the stream failed (chat-parity §20).
        let err = ChatEvent::Error {
            code: "model_plane_unavailable".into(),
            message: "boom".into(),
            retryable: true,
        };
        assert!(err.should_emit(&no_features));
        assert_eq!(err.name(), "error");
        let payload = err.payload("req-1");
        assert_eq!(payload["code"], "model_plane_unavailable");
        assert_eq!(payload["message"], "boom");
        assert_eq!(payload["retryable"], true);
        assert_eq!(payload["request_id"], "req-1");
    }

    /// WS22-S6: the sandbox_state frame is a steps-family frame whose payload is its closed tokens and the
    /// request id, and nothing else. Its constructor admits only members of the closed lists.
    #[test]
    fn sandbox_state_frame_contract() {
        use mp_contracts::lifecycle_notice::{SANDBOX_CAUSES, SANDBOX_SCOPES, SANDBOX_STATES, SANDBOX_STATE_KEYS};
        let kept = ChatEvent::sandbox_state("thread", "kept", None).expect("closed scope and state");
        assert_eq!(kept.name(), "sandbox_state");
        assert_eq!(kept.family(), Some("steps"));
        assert!(!kept.should_emit(&[]), "a client that asked for no steps must not see the workspace");
        assert!(kept.should_emit(&["steps".to_string()]));
        let payload = kept.payload("req-1");
        let keys: std::collections::BTreeSet<&str> =
            payload.as_object().expect("an object").keys().map(String::as_str).collect();
        assert_eq!(keys, ["request_id", "scope", "state"].into_iter().collect(), "{payload}");

        let conflict = ChatEvent::sandbox_state("thread", "conflict", Some("workspace_conflict"))
            .expect("closed tokens");
        let payload = conflict.payload("req-1");
        assert_eq!(payload["cause"], "workspace_conflict");
        for (key, value) in payload.as_object().expect("an object") {
            if key == "request_id" {
                continue;
            }
            assert!(SANDBOX_STATE_KEYS.contains(&key.as_str()), "{key} is not a sandbox_state key");
            let token = value.as_str().expect("every sandbox_state value is a closed token");
            let closed: &[&str] = match key.as_str() {
                "scope" => &SANDBOX_SCOPES,
                "state" => &SANDBOX_STATES,
                _ => &SANDBOX_CAUSES,
            };
            assert!(closed.contains(&token), "{key}={token} is not a closed token");
        }

        // A workspace, a state or a cause outside its closed list, or one that carries a path or free
        // text, is refused rather than shown.
        for (scope, state, cause) in [
            ("workspace", "kept", None),
            ("thread", "kept; rm -rf /", None),
            ("thread", "conflict", Some("/home/user/a.txt")),
            ("thread", "conflict", Some("workspace_conflict ")),
        ] {
            assert!(ChatEvent::sandbox_state(scope, state, cause).is_none(), "{scope} {state} {cause:?}");
        }
    }

    #[test]
    fn run_parked_is_a_control_event() {
        // A park reaches a plain client, and its keys are the ones WS03 shipped and the SPA reads.
        let no_features: Vec<String> = vec![];
        let parked = ChatEvent::RunParked {
            run_id: "run-42".into(),
            reason: mp_contracts::loop_fold::ParkReason::ProviderExhausted,
            resume_after: Some("2026-10-20T12:00:00Z".into()),
        };
        assert_eq!(parked.family(), None);
        assert!(parked.should_emit(&no_features));
        assert_eq!(parked.name(), "run_parked");
        assert_eq!(
            parked.payload("req-1"),
            json!({
                "run_id": "run-42",
                "request_id": "req-1",
                "status": "parked",
                "reason": "provider_exhausted",
                "resume_after": "2026-10-20T12:00:00Z",
            })
        );
        // An unreadable time is null, not an omitted key.
        let unreadable = ChatEvent::RunParked {
            run_id: "run-42".into(),
            reason: mp_contracts::loop_fold::ParkReason::CredentialWindow,
            resume_after: None,
        };
        assert!(unreadable.payload("req-1")["resume_after"].is_null());
    }

    #[test]
    fn opt_in_unlocks_only_the_requested_families() {
        let feats = families(&["reasoning", "usage"]);
        assert!(ChatEvent::ReasoningDelta {
            delta: "thinking".into()
        }
        .should_emit(&feats));
        assert!(ChatEvent::Usage {
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            calls: 1,
            unpriced_calls: 0,
            cost_by_category: None,
            input_tokens: 1,
            output_tokens: 2,
            cost_usd: Some(0.01),
            cost_basis: Some(crate::pricing::CostBasis::Metered),
            latency_ms: 5,
            confidence: Some(0.9),
            served_by: None,
            retention_class: None,
            routing_decision: None,
        }
        .should_emit(&feats));
        let progress = ChatEvent::UsageProgress {
            phase: crate::usage_meter::CallPhase::ToolRound,
            snapshot: running_total(&[(crate::pricing::CostBasis::Metered, Some(0.01))]),
            ceiling: None,
            routing: None,
            routing_decision: None,
        };
        assert!(progress.should_emit(&feats));
        // Not opted in:
        assert!(!progress.should_emit(&families(&["reasoning"])));
        assert!(!ChatEvent::Citation {
            id: "c".into(),
            title: "t".into(),
            url: "u".into(),
            snippet: "s".into(),
            source_group_id: "web:u".into(),
            source_group_label: None,
            number: Some(1),
            claim_id: None,
            start: None,
            end: None,
            origin: "web".into(),
            passage: None,
        }
        .should_emit(&feats));
        assert!(!ChatEvent::StepUpdate {
            id: "s".into(),
            title: "t".into(),
            detail: "d".into(),
            status: "active".into(),
        }
        .should_emit(&feats));
    }

    /// The running total a sink's meter states after `calls`, each a `(basis, price)`: a metered call
    /// is priced at its figure (`None` is unpriced), and a subscription call is priced at a known zero.
    fn running_total(
        calls: &[(crate::pricing::CostBasis, Option<f64>)],
    ) -> crate::usage_meter::Snapshot {
        use crate::pricing::{CategoryCost, CostBasis, PriceSource, Priced};
        use crate::usage_meter::{CallPhase, CallUsage, UsageMeter};
        let mut meter = UsageMeter::default();
        let mut total = meter.snapshot();
        for &(basis, usd) in calls {
            let call = CallUsage {
                phase: CallPhase::ToolRound,
                model: "gpt-5".into(),
                provider: "openai".into(),
                basis,
                input: 1_000,
                output: 100,
                cache_read: 200,
                cache_write: 0,
                priority: false,
            };
            let priced = match basis {
                CostBasis::Subscription => Priced {
                    usd: Some(0.0),
                    by_category: None,
                    source: PriceSource::Subscription,
                    note: None,
                },
                CostBasis::CustomerBilled => Priced {
                    usd: Some(0.0),
                    by_category: None,
                    source: PriceSource::CustomerBilled,
                    note: None,
                },
                CostBasis::Metered => Priced {
                    usd,
                    by_category: usd.map(|usd| CategoryCost {
                        input_usd: usd,
                        cache_read_usd: 0.0,
                        cache_write_usd: 0.0,
                        output_usd: 0.0,
                    }),
                    source: if usd.is_some() { PriceSource::Exact } else { PriceSource::Default },
                    note: None,
                },
            };
            total = meter.record(&call, &priced);
        }
        total
    }

    /// The keys a progress frame of `calls` carries, and the frame's own payload.
    fn progress_payload(
        calls: &[(crate::pricing::CostBasis, Option<f64>)],
    ) -> serde_json::Map<String, Value> {
        ChatEvent::UsageProgress {
            phase: crate::usage_meter::CallPhase::Overhead,
            snapshot: running_total(calls),
            ceiling: None,
            routing: None,
            routing_decision: None,
        }
        .payload("req-1")
        .as_object()
        .expect("a progress payload is an object")
        .clone()
    }

    /// Each progress frame states the closed keys its total supports and no others: `cost_basis` only
    /// for one basis, `unpriced_calls` only once a call went unpriced, and `cost_usd` null otherwise.
    #[test]
    fn usage_progress_has_only_closed_keys() {
        use crate::pricing::CostBasis::{Metered, Subscription};
        use std::collections::BTreeSet;
        let closed: BTreeSet<String> = [
            "calls",
            "phase",
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cache_write_tokens",
            "cost_usd",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        // The closed keys, and the optional ones a frame states beside them.
        let with = |extra: &[&str]| -> BTreeSet<String> {
            let mut keys = closed.clone();
            keys.extend(extra.iter().map(|key| (*key).to_owned()));
            keys
        };
        let keys_of = |payload: &serde_json::Map<String, Value>| -> BTreeSet<String> {
            payload.keys().cloned().collect()
        };

        // Every call priced and metered: a total in one basis, stated as such.
        let priced = progress_payload(&[(Metered, Some(0.25)), (Metered, Some(0.5))]);
        assert_eq!(keys_of(&priced), with(&["cost_basis"]));
        assert_eq!(priced["cost_usd"], 0.75);
        assert_eq!(priced["cost_basis"], "metered");
        assert_eq!(priced["phase"], "overhead");

        // One unpriced call: the cost is null, never the priced calls' sum, and the count is stated.
        let unpriced = progress_payload(&[(Metered, Some(0.25)), (Metered, None)]);
        assert_eq!(keys_of(&unpriced), with(&["cost_basis", "unpriced_calls"]));
        assert!(unpriced["cost_usd"].is_null());
        assert_eq!(unpriced["unpriced_calls"], 1);

        // A subscription turn has no dollar figure, and its basis says so.
        let subscription = progress_payload(&[(Subscription, None)]);
        assert_eq!(keys_of(&subscription), with(&["cost_basis"]));
        assert!(subscription["cost_usd"].is_null());
        assert_eq!(subscription["cost_basis"], "subscription");

        // Mixed bases state no basis, and the metered part's cost stays.
        let mixed = progress_payload(&[(Metered, Some(0.25)), (Subscription, None)]);
        assert_eq!(keys_of(&mixed), with(&[]));
        assert_eq!(mixed["cost_usd"], 0.25);

        // Before any call the total states no basis and no cost.
        let empty = progress_payload(&[]);
        assert_eq!(keys_of(&empty), with(&[]));
        assert!(empty["cost_usd"].is_null());
        assert_eq!(empty["calls"], 0);
    }

    /// A progress frame belongs to the `usage` family, so it is gated exactly as the usage frame is.
    #[test]
    fn usage_progress_is_gated_with_the_usage_family() {
        let event = ChatEvent::UsageProgress {
            phase: crate::usage_meter::CallPhase::Answer,
            snapshot: running_total(&[(crate::pricing::CostBasis::Metered, Some(0.25))]),
            ceiling: None,
            routing: None,
            routing_decision: None,
        };
        assert_eq!(event.family(), Some("usage"));
        assert_eq!(event.name(), "usage_progress");
        assert!(event.should_emit(&families(&["usage"])));
        assert!(!event.should_emit(&families(&["steps", "tools"])), "not asked for, not sent");
        assert!(!event.should_emit(&[]), "the plain chat profile never receives it");
    }

    /// A sink streams one frame for each call it records, and only while it is switched on and its
    /// client asked for usage.
    #[tokio::test]
    async fn a_recorded_call_streams_the_running_total_only_when_the_sink_streams_it() {
        async fn frames_sent(streams: bool, features: &[&str]) -> usize {
            let (tx, mut rx) = tokio::sync::mpsc::channel(8);
            let sink = RichEventSink::new(tx, families(features), "req-1".to_owned())
                .with_usage_progress(streams);
            let call = crate::usage_meter::CallUsage {
                phase: crate::usage_meter::CallPhase::ToolRound,
                model: "gpt-5".into(),
                provider: "openai".into(),
                basis: crate::pricing::CostBasis::Metered,
                input: 1_000,
                output: 100,
                cache_read: 0,
                cache_write: 0,
                priority: false,
            };
            let priced = crate::pricing::Priced {
                usd: Some(0.25),
                by_category: Some(crate::pricing::CategoryCost {
                    input_usd: 0.25,
                    cache_read_usd: 0.0,
                    cache_write_usd: 0.0,
                    output_usd: 0.0,
                }),
                source: crate::pricing::PriceSource::Exact,
                note: None,
            };
            sink.record_call(&call, &priced, None).await;
            sink.record_call(&call, &priced, None).await;
            drop(sink);
            let mut sent = 0;
            while rx.try_recv().is_ok() {
                sent += 1;
            }
            sent
        }
        assert_eq!(frames_sent(true, &["usage"]).await, 2, "one frame per recorded call");
        assert_eq!(frames_sent(false, &["usage"]).await, 0, "the switch is off");
        assert_eq!(frames_sent(true, &[]).await, 0, "a client that did not ask for usage gets none");
    }

    /// A sink whose ceiling is enforced counts the calls it is given without a progress stream, and the
    /// tool phase ends once a recorded call takes the run to its cap.
    #[tokio::test]
    async fn run_ceiling_stops_the_tool_phase_once_a_recorded_call_reaches_the_cap() {
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let cap = crate::sse_events::run_ceiling::RunCeiling {
            usd: Some(0.2),
            tokens: None,
        };
        let sink = RichEventSink::new(tx, Vec::new(), "req-ceiling".to_owned()).with_run_ceiling(
            crate::sse_events::run_ceiling::CeilingTracker::for_turn(cap, false, true),
        );
        assert!(
            sink.records_calls(),
            "an enforced ceiling counts calls without a stream"
        );
        assert!(!sink.streams_usage_progress());
        let call = crate::usage_meter::CallUsage {
            phase: crate::usage_meter::CallPhase::ToolRound,
            model: "gpt-5".into(),
            provider: "openai".into(),
            basis: crate::pricing::CostBasis::Metered,
            input: 1_000,
            output: 100,
            cache_read: 0,
            cache_write: 0,
            priority: false,
        };
        let priced = crate::pricing::Priced {
            usd: Some(0.1),
            by_category: Some(crate::pricing::CategoryCost {
                input_usd: 0.1,
                cache_read_usd: 0.0,
                cache_write_usd: 0.0,
                output_usd: 0.0,
            }),
            source: crate::pricing::PriceSource::Exact,
            note: None,
        };
        sink.record_call(&call, &priced, None).await;
        assert!(
            !sink.run_ceiling_stops_tool_phase(),
            "half of the cap is not the cap"
        );
        sink.record_call(&call, &priced, None).await;
        assert!(
            sink.run_ceiling_stops_tool_phase(),
            "two calls reach the cap"
        );
    }

    /// The ceiling's keys ride a frame that carries a ceiling, and no other frame states them.
    #[test]
    fn usage_progress_states_the_ceiling_keys_only_when_it_carries_one() {
        let carried = ChatEvent::UsageProgress {
            phase: crate::usage_meter::CallPhase::ToolRound,
            snapshot: running_total(&[(crate::pricing::CostBasis::Metered, Some(0.04))]),
            ceiling: Some(crate::sse_events::run_ceiling::CeilingProgress {
                ceiling_usd: Some(0.05),
                ceiling_tokens: Some(20_000),
                fraction: Some(0.8),
                warning: Some(crate::sse_events::run_ceiling::CeilingWarning::Eighty),
            }),
            routing: None,
            routing_decision: None,
        }
        .payload("req-1");
        assert_eq!(carried["ceiling_usd"], json!(0.05));
        assert_eq!(carried["ceiling_tokens"], json!(20_000));
        assert_eq!(carried["fraction"], json!(0.8));
        assert_eq!(carried["warning"], json!("run_ceiling_80"));
        let plain = progress_payload(&[(crate::pricing::CostBasis::Metered, Some(0.04))]);
        for key in ["ceiling_usd", "ceiling_tokens", "fraction", "warning"] {
            assert!(
                !plain.contains_key(key),
                "{key} is stated only on a frame that carries a ceiling"
            );
        }
    }

    /// A token cap counts the input and output of metered calls only: a subscription call's tokens are
    /// usage, and cost-core never lets them exhaust the cap.
    #[test]
    fn a_token_cap_counts_metered_calls_and_never_a_subscription_call() {
        let call = |basis| crate::usage_meter::CallUsage {
            phase: crate::usage_meter::CallPhase::ToolRound,
            model: "m".into(),
            provider: "p".into(),
            basis,
            input: 800,
            output: 200,
            cache_read: 500,
            cache_write: 0,
            priority: false,
        };
        assert_eq!(
            metered_tokens(&call(crate::pricing::CostBasis::Metered)),
            1_000,
            "the input already folds in the cache legs, and the output adds to it"
        );
        assert_eq!(
            metered_tokens(&call(crate::pricing::CostBasis::Subscription)),
            0
        );
        assert_eq!(
            metered_tokens_of([
                &call(crate::pricing::CostBasis::Metered),
                &call(crate::pricing::CostBasis::Subscription),
                &call(crate::pricing::CostBasis::Metered),
            ]),
            2_000
        );
    }

    #[test]
    fn tools_family_covers_both_call_and_result() {
        let feats = families(&["tools"]);
        assert!(ChatEvent::ToolCall {
            id: "1".into(),
            name: "py".into(),
            args: json!({"a":1})
        }
        .should_emit(&feats));
        assert!(ChatEvent::ToolResult {
            id: "1".into(),
            status: "ok".into(),
            output: "42".into(),
            error: None,
        }
        .should_emit(&feats));
    }

    #[test]
    fn names_and_payload_shapes_are_stable() {
        let usage = ChatEvent::Usage {
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            calls: 1,
            unpriced_calls: 0,
            cost_by_category: None,
            input_tokens: 12,
            output_tokens: 34,
            cost_usd: Some(0.002),
            cost_basis: Some(crate::pricing::CostBasis::Metered),
            latency_ms: 880,
            confidence: Some(0.77),
            served_by: None,
            retention_class: None,
            routing_decision: None,
        };
        assert_eq!(usage.name(), "usage");
        let p = usage.payload("req-1");
        assert_eq!(p["input_tokens"], 12);
        assert_eq!(p["output_tokens"], 34);
        assert_eq!(p["latency_ms"], 880);
        assert!((p["cost_usd"].as_f64().unwrap() - 0.002).abs() < 1e-9);
        assert_eq!(p["cost_basis"], "metered");

        let progress = ChatEvent::UsageProgress {
            phase: crate::usage_meter::CallPhase::Answer,
            snapshot: running_total(&[(crate::pricing::CostBasis::Metered, Some(0.01))]),
            ceiling: None,
            routing: None,
            routing_decision: None,
        };
        assert_eq!(progress.name(), "usage_progress");

        let cite = ChatEvent::Citation {
            id: "c1".into(),
            title: "Doc".into(),
            url: "https://x".into(),
            snippet: "hello".into(),
            source_group_id: "kb:sharepoint".into(),
            source_group_label: Some("Microsoft 365".into()),
            number: Some(1),
            claim_id: None,
            start: None,
            end: None,
            origin: "knowledge".into(),
            passage: None,
        };
        assert_eq!(cite.name(), "citation");
        let cite_payload = cite.payload("r");
        assert_eq!(cite_payload["url"], "https://x");
        assert_eq!(cite_payload["source_group_id"], "kb:sharepoint");
        assert_eq!(cite_payload["source_group_label"], "Microsoft 365");
        assert_eq!(cite_payload["number"], 1);
        assert_eq!(cite_payload["origin"], "knowledge");
        // Absent claim_id/start/end are left out of the object entirely
        // (not even `null`), so the common pre-anchor-pass frame stays small.
        assert!(cite_payload.get("claim_id").is_none());
        assert!(cite_payload.get("start").is_none());
        assert!(cite_payload.get("end").is_none());
        // No passage, no keys: the frame is the one sent before EP-2-S4.
        assert!(cite_payload.get("quote").is_none());
        assert!(cite_payload.get("locator").is_none());

        let anchored = ChatEvent::Citation {
            id: "c1".into(),
            title: "Doc".into(),
            url: "https://x".into(),
            snippet: "hello".into(),
            source_group_id: "kb:sharepoint".into(),
            source_group_label: Some("Microsoft 365".into()),
            number: Some(1),
            claim_id: Some("turn-1:claim-0".into()),
            start: Some(12),
            end: Some(15),
            origin: "knowledge".into(),
            passage: None,
        };
        let anchored_payload = anchored.payload("r");
        assert_eq!(anchored_payload["claim_id"], "turn-1:claim-0");
        assert_eq!(anchored_payload["start"], 12);
        assert_eq!(anchored_payload["end"], 15);

        let grounding = ChatEvent::Grounding {
            grounding: crate::retrieval::Grounding {
                mode: "hybrid".into(),
                query: "refund policy".into(),
                trace_id: Some("trace-1".into()),
                low_confidence: false,
                fact_count: 1,
                source_count: 1,
                suggested_next_tools: Vec::new(),
                zdr_actions_applied: Vec::new(),
                notice: None,
                facts: vec![crate::retrieval::GroundingFact {
                    knowledge_id: "kid-1".into(),
                    document_id: "doc-1".into(),
                    text: "Refunds are accepted within 30 days.".into(),
                    score: 0.93,
                    source_title: "Refund policy".into(),
                    source_type: "policy".into(),
                    provider: "Notion".into(),
                    chunk_index: 0,
                }],
                sources: vec![crate::retrieval::GroundingSource {
                    id: "doc-1".into(),
                    kind: "knowledge".into(),
                    title: "Refund policy".into(),
                    snippet: "Refunds are accepted within 30 days.".into(),
                    provider: "Notion".into(),
                    source_type: "policy".into(),
                    document_id: "doc-1".into(),
                    href: "/knowledge".into(),
                    score: 0.93,
                }],
                graph: Some(crate::retrieval::GroundingGraph {
                    trace_id: Some("graph-1".into()),
                    community_summaries: vec!["Refund policy connects with return workflow.".into()],
                    edge_count: 0,
                    nodes: vec![crate::retrieval::GroundingGraphNode {
                        id: "node-1".into(),
                        label: "Refund policy".into(),
                        kind: "policy".into(),
                    }],
                }),
                context_block: String::new(),
                citations: Vec::new(),
            },
        };
        assert_eq!(grounding.family(), Some("citations"));
        assert_eq!(grounding.name(), "grounding");
        assert_eq!(grounding.payload("r")["mode"], "hybrid");
        assert_eq!(grounding.payload("r")["graph"]["traceId"], "graph-1");

        // attachment maps mime -> "type" (matches the brief's payload).
        let att = ChatEvent::Attachment {
            id: "a".into(),
            name: "f.png".into(),
            mime: "image/png".into(),
            url: "blob:1".into(),
            size: 99,
            check: None,
            preview: None,
        };
        assert_eq!(att.payload("r")["type"], "image/png");
    }

    /// The check rides on the attachment frame only when the verifier made one, and it carries the report's
    /// own level and findings. Nothing in the frame says the file was verified in Office.
    #[test]
    fn attachment_event_carries_check_only_when_present() {
        let report = mp_contracts::office_verification::OfficeVerification::from_untrusted_json(
            r#"{"kind":"docx","level":"warnings","checks":[{"id":"rels","level":"warnings","message":"a relationship points nowhere"}],"stats":{"paragraphs":2}}"#,
            "letter.docx",
            mp_contracts::office_verification::OfficeKind::Docx,
        )
        .expect("a well-formed report");
        let attachment = |check| ChatEvent::Attachment {
            id: "a".into(),
            name: "letter.docx".into(),
            mime: "application/vnd.openxmlformats-officedocument.wordprocessingml.document".into(),
            url: "data:application/octet-stream;base64,AA".into(),
            size: 2,
            check,
            preview: None,
        };

        let plain = attachment(None).payload("r");
        assert!(plain.get("check").is_none(), "no verifier ran, so the key is absent: {plain}");
        assert_eq!(plain["size"], 2);

        let checked = attachment(Some(report)).payload("r");
        assert_eq!(checked["check"]["level"], "warnings");
        assert_eq!(checked["check"]["checks"][0]["id"], "rels");
        assert_eq!(checked["check"]["checks"][0]["message"], "a relationship points nowhere");
        assert_eq!(checked["check"]["stats"]["paragraphs"], 2);
        assert_eq!(checked["name"], "letter.docx");
    }

    /// The preview rides on the attachment frame only when the render layer made one. The download URL stays the
    /// file itself, so the viewer is never the deliverable.
    #[test]
    fn attachment_event_carries_preview_only_when_present() {
        let preview = AttachmentPreview {
            mime: "application/pdf".into(),
            url: "data:application/pdf;base64,AA".into(),
            pages: 3,
            fonts_substituted: true,
            thumbs: vec!["data:image/png;base64,AA".into()],
        };
        let attachment = |preview| ChatEvent::Attachment {
            id: "a".into(),
            name: "budget.xlsx".into(),
            mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
            url: "data:application/octet-stream;base64,AA".into(),
            size: 2,
            check: None,
            preview,
        };

        let plain = attachment(None).payload("r");
        assert!(
            plain.get("preview").is_none(),
            "no render ran, so the key is absent: {plain}"
        );

        let rendered = attachment(Some(preview)).payload("r");
        assert_eq!(rendered["preview"]["mime"], "application/pdf");
        assert_eq!(rendered["preview"]["url"], "data:application/pdf;base64,AA");
        assert_eq!(rendered["preview"]["pages"], 3);
        assert_eq!(rendered["preview"]["fonts_substituted"], true);
        assert_eq!(rendered["preview"]["thumbs"][0], "data:image/png;base64,AA");
        assert_eq!(rendered["url"], "data:application/octet-stream;base64,AA");
    }

    #[test]
    fn carries_binary_is_true_for_a_preview_attachment() {
        let preview = AttachmentPreview {
            mime: "application/pdf".into(),
            url: "data:application/pdf;base64,AA".into(),
            pages: 1,
            fonts_substituted: false,
            thumbs: Vec::new(),
        };
        let attachment = |url: &str, preview| ChatEvent::Attachment {
            id: "a".into(),
            name: "budget.xlsx".into(),
            mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
            url: url.into(),
            size: 2,
            check: None,
            preview,
        };
        assert!(carries_binary(&attachment("blob:1", Some(preview))));
        assert!(!carries_binary(&attachment(
            "https://example.test/budget.xlsx",
            None
        )));
    }

    #[test]
    fn usage_cost_basis_explains_a_subscription_cost_and_is_otherwise_optional() {
        // A subscription turn keeps `cost_usd` at 0.0 for older clients and
        // says why; a turn with no established basis keeps the old keys.
        let subscription = ChatEvent::Usage {
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            calls: 1,
            unpriced_calls: 0,
            cost_by_category: None,
            input_tokens: 1_800,
            output_tokens: 120,
            cost_usd: Some(0.0),
            cost_basis: Some(crate::pricing::CostBasis::Subscription),
            latency_ms: 880,
            confidence: None,
            served_by: None,
            retention_class: None,
            routing_decision: None,
        }
        .payload("req-2");
        assert_eq!(subscription["cost_usd"], 0.0);
        assert_eq!(subscription["cost_basis"], "subscription");
        assert_eq!(subscription["input_tokens"], 1_800);
        let unestablished = ChatEvent::Usage {
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            calls: 1,
            unpriced_calls: 0,
            cost_by_category: None,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: None,
            cost_basis: None,
            latency_ms: 880,
            confidence: None,
            served_by: None,
            retention_class: None,
            routing_decision: None,
        }
        .payload("req-3");
        let mut keys: Vec<&str> = unestablished
            .as_object()
            .expect("usage payload is an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "calls",
                "confidence",
                "cost_usd",
                "input_tokens",
                "latency_ms",
                "output_tokens",
                "unpriced_calls"
            ]
        );
    }

    fn usage_with(served_by: Option<ServedBy>) -> serde_json::Value {
        ChatEvent::Usage {
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            calls: 1,
            unpriced_calls: 0,
            cost_by_category: None,
            input_tokens: 10,
            output_tokens: 5,
            cost_usd: Some(0.0),
            cost_basis: Some(crate::pricing::CostBasis::Metered),
            latency_ms: 700,
            confidence: None,
            served_by,
            retention_class: None,
            routing_decision: None,
        }
        .payload("req-served")
    }

    /// The usage payload carries the three provenance fields when the server
    /// stamped them, and exactly the keys it always had when it did not.
    #[test]
fn usage_carries_cache_tokens_and_omits_them_when_zero() {
    let frame = |read: u32, write: u32| {
        ChatEvent::Usage {
            cache_read_tokens: read,
            cache_write_tokens: write,
            calls: 1,
            unpriced_calls: 0,
            cost_by_category: None,
            input_tokens: 1200,
            output_tokens: 80,
            cost_usd: None,
            cost_basis: None,
            latency_ms: 10,
            confidence: None,
            served_by: None,
            retention_class: None,
            routing_decision: None,
        }
        .payload("req-1")
    };
    let hit = frame(900, 0);
    assert_eq!(hit["cache_read_tokens"], 900);
    assert_eq!(hit["cache_write_tokens"], 0);
    let written = frame(0, 300);
    assert_eq!(written["cache_write_tokens"], 300);
    // The zero-retention shape (and every provider that reports no cache) is byte-identical to before.
    let none = frame(0, 0);
    assert!(none.get("cache_read_tokens").is_none());
    assert!(none.get("cache_write_tokens").is_none());
}

#[test]
    fn usage_carries_server_stamped_provenance_and_omits_it_when_empty() {
        let baseline = usage_with(None);
        let mut baseline_keys: Vec<&str> = baseline
            .as_object()
            .expect("usage payload is an object")
            .keys()
            .map(String::as_str)
            .collect();
        baseline_keys.sort_unstable();
        assert_eq!(
            baseline_keys,
            [
                "calls",
                "confidence",
                "cost_basis",
                "cost_usd",
                "input_tokens",
                "latency_ms",
                "output_tokens",
                "unpriced_calls"
            ],
            "no provenance: the provenance keys stay out"
        );

        let azure = usage_with(ServedBy::stamped("azure-openai", "eu", 3));
        assert_eq!(azure["provider_used"], "azure-openai");
        assert_eq!(azure["residency"], "eu");
        assert_eq!(azure["privacy_tier"], "zdr_contractual");
        // The fields sit beside the old ones; none of those moved.
        assert_eq!(azure["input_tokens"], 10);
        assert_eq!(azure["cost_basis"], "metered");

        // Per field: an empty value is left out, not sent as "" or null.
        let bare = usage_with(Some(ServedBy {
            provider_used: "azure-openai".into(),
            residency: String::new(),
            privacy_tier: None,
        }));
        assert_eq!(bare["provider_used"], "azure-openai");
        assert!(bare.get("residency").is_none());
        assert!(bare.get("privacy_tier").is_none());
        let empty = usage_with(Some(ServedBy::default()));
        for key in ["provider_used", "residency", "privacy_tier"] {
            assert!(empty.get(key).is_none(), "{key} must be omitted when empty");
        }
        assert_eq!(empty, baseline, "an all-empty stamp is the old payload");
    }

    /// A subscription answer reads `global` with no tier above it. The
    /// provider itself returns both fields empty; inference-core's chain
    /// stamps them (pinned by its own test), and this is what the gateway
    /// makes of that stamp.
    #[test]
    fn a_subscription_answer_reads_global_with_no_eu_or_zdr_tier() {
        let served = ServedBy::stamped("openai-codex-subscription", "global", 0)
            .expect("a provider served the answer");
        assert_eq!(served.provider_used, "openai-codex-subscription");
        assert_eq!(served.residency, "global");
        assert_eq!(served.privacy_tier, Some("global"));
        let payload = usage_with(Some(served));
        assert_eq!(payload["residency"], "global");
        assert_eq!(payload["privacy_tier"], "global");
        let text = payload.to_string();
        assert!(
            !text.contains("eu_resident") && !text.contains("zdr") && !text.contains("sovereign"),
            "the subscription route must never carry a stronger tier: {text}"
        );
    }

    /// The tier is a lower bound: what the stamped residency implies, raised
    /// to the floor the request enforced, never more.
    #[test]
    fn served_by_asserts_only_what_residency_and_the_enforced_floor_support() {
        let tier = |provider: &str, residency: &str, floor: i32| {
            ServedBy::stamped(provider, residency, floor).and_then(|served| served.privacy_tier)
        };
        // Residency alone.
        assert_eq!(tier("azure-openai", "eu", 0), Some("eu_resident"));
        assert_eq!(tier("azure-openai", " EU ", 0), Some("eu_resident"));
        assert_eq!(tier("some-sovereign", "norway", 0), Some("eu_resident"));
        assert_eq!(tier("openai", "global", 0), Some("global"));
        // The enforced floor raises it: inference-core skipped every provider
        // below it, so the answer met it.
        assert_eq!(tier("azure-openai", "eu", 3), Some("zdr_contractual"));
        assert_eq!(tier("some-sovereign", "norway", 4), Some("sovereign"));
        // A floor can never lower what the residency already implies.
        assert_eq!(tier("azure-openai", "eu", 1), Some("eu_resident"));
        // Out-of-range ordinals express no floor at all.
        assert_eq!(tier("azure-openai", "eu", 99), Some("eu_resident"));
        assert_eq!(tier("azure-openai", "eu", -1), Some("eu_resident"));
        // An undeclared residency asserts nothing by itself.
        assert_eq!(tier("mystery", "", 0), None);
        assert_eq!(tier("mystery", "atlantis", 0), None);
        assert_eq!(
            ServedBy::stamped("mystery", "", 0).map(|served| served.residency),
            Some(String::new())
        );
        // No serving provider, no provenance: a synthetic answer that made
        // no provider call, or an inference-core that stamped nothing.
        assert_eq!(ServedBy::stamped("", "eu", 3), None);
        assert_eq!(ServedBy::stamped("   ", "global", 2), None);
    }

    /// A ZDR answer names the retention it was produced under, and an answer that
    /// asserts none leaves the key out, so every earlier payload is unchanged.
    #[test]
    fn usage_names_its_retention_class_only_when_one_is_stamped() {
        let zdr = ChatEvent::Usage {
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            calls: 1,
            unpriced_calls: 0,
            cost_by_category: None,
            input_tokens: 3,
            output_tokens: 4,
            cost_usd: None,
            cost_basis: None,
            latency_ms: 5,
            confidence: None,
            served_by: None,
            retention_class: Some(RetentionClass::Zdr),
            routing_decision: None,
        }
        .payload("req-zdr");
        assert_eq!(zdr["retention_class"], "zdr");
        assert_eq!(RetentionClass::Zdr.as_str(), "zdr");

        let unstamped = usage_with(None);
        assert!(unstamped.get("retention_class").is_none(), "{unstamped}");
    }

    /// A waived turn states its class and the resolver's reason on the usage frame, so the answer
    /// line can say the turn ran on one connection under a waiver, and why. A ZDR turn states no
    /// reason, and a persistent turn states nothing.
    #[test]
    fn a_waived_usage_frame_states_its_class_and_reason() {
        let waived = ChatEvent::Usage {
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            calls: 1,
            unpriced_calls: 0,
            cost_by_category: None,
            input_tokens: 3,
            output_tokens: 4,
            cost_usd: None,
            cost_basis: None,
            latency_ms: 5,
            confidence: None,
            served_by: None,
            retention_class: Some(RetentionClass::ZdrWaived {
                reason: "connection_not_zdr_capable",
            }),
            routing_decision: None,
        }
        .payload("req-waived");
        assert_eq!(waived["retention_class"], "zdr_waived");
        assert_eq!(waived["retention_reason"], "connection_not_zdr_capable");
        assert_eq!(
            RetentionClass::ZdrWaived {
                reason: "connection_not_zdr_capable",
            }
            .as_str(),
            "zdr_waived"
        );

        let zdr = ChatEvent::Usage {
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            calls: 1,
            unpriced_calls: 0,
            cost_by_category: None,
            input_tokens: 3,
            output_tokens: 4,
            cost_usd: None,
            cost_basis: None,
            latency_ms: 5,
            confidence: None,
            served_by: None,
            retention_class: Some(RetentionClass::Zdr),
            routing_decision: None,
        }
        .payload("req-zdr");
        assert_eq!(zdr["retention_class"], "zdr");
        assert!(zdr.get("retention_reason").is_none(), "{zdr}");
        assert!(usage_with(None).get("retention_reason").is_none());
    }

    /// `turn_timing` rides with `usage`: a client that did not ask for usage
    /// never sees it, live or on resume.
    #[test]
    fn turn_timing_is_gated_exactly_like_usage() {
        let timing = crate::turn_timing::TurnTiming::start("req-t");
        let report = timing
            .report(crate::turn_timing::Outcome::Completed)
            .unwrap();
        let event = ChatEvent::TurnTiming { report };
        let usage = ChatEvent::Usage {
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            calls: 1,
            unpriced_calls: 0,
            cost_by_category: None,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: None,
            cost_basis: None,
            latency_ms: 0,
            confidence: None,
            served_by: None,
            retention_class: None,
            routing_decision: None,
        };
        assert_eq!(event.name(), "turn_timing");
        assert_eq!(event.family(), usage.family());
        for features in [
            families(&[]),
            families(&["tools", "steps", "artifacts"]),
            families(&["usage"]),
        ] {
            assert_eq!(
                event.should_emit(&features),
                usage.should_emit(&features),
                "{features:?}"
            );
        }
        let payload = event.payload("ignored");
        assert_eq!(payload["request_id"], "req-t");
        assert_eq!(payload["outcome"], "completed");
        assert!(payload["phases"].is_array());
    }

    fn retrieval_notice() -> ChatEvent {
        ChatEvent::RetrievalNotice {
            notice: crate::retrieval_metadata::notice_for_code("zdr_dense_unavailable")
                .expect("the ZDR notice is catalogued"),
        }
    }

    /// The notice is a control event: a client that never opted into a feature family still hears that
    /// its answer was reduced, exactly like a refusal does.
    #[test]
    fn a_reduced_answers_notice_reaches_a_client_that_opted_into_nothing() {
        let event = retrieval_notice();
        assert_eq!(event.name(), "retrieval_notice");
        assert_eq!(event.family(), None);
        for features in [families(&[]), families(&["citations", "usage", "tools"])] {
            assert!(event.should_emit(&features), "{features:?}");
        }
    }

    /// The payload is the typed code and its wording key, and nothing else a client could mistake for
    /// content: no sentence, no document text, no Data Plane message.
    #[test]
    fn the_notice_payload_carries_only_the_code_and_its_wording_key() {
        let payload = retrieval_notice().payload("req-n");
        assert_eq!(
            payload,
            json!({
                "code": "zdr_dense_unavailable",
                "wording_key": "retrieval.zdr_dense_unavailable",
                "request_id": "req-n",
            })
        );
    }

    /// A declined retrieval's reason (D1-242) travels as the same event: a client that opted into
    /// nothing hears it, and the payload is the typed code and the wording key only.
    #[test]
    fn a_retrieval_decline_notice_is_the_same_control_event_with_a_code_and_a_key() {
        let event = ChatEvent::RetrievalNotice {
            notice: crate::retrieval_metadata::RetrievalDeclineReason::EntitlementOff.notice(),
        };
        assert_eq!(event.name(), "retrieval_notice");
        assert!(event.should_emit(&families(&[])));
        assert_eq!(
            event.payload("req-d"),
            json!({
                "code": "retrieval_declined_entitlement_off",
                "wording_key": "retrieval.declined.entitlement_off",
                "request_id": "req-d",
            })
        );
    }
}

#[cfg(test)]
mod memory_recall_payload_tests {
    use super::ChatEvent;
    use crate::memory_provenance::{MemoryOrigin, MemoryRole, RecalledMemoryView};

    fn view(origin: MemoryOrigin, role: MemoryRole) -> RecalledMemoryView {
        RecalledMemoryView {
            memory_id: "mem-1".to_owned(),
            role,
            origin,
            label: "USER".to_owned(),
            preview: "Prefers metric units".to_owned(),
        }
    }

    fn payload(memories: Vec<RecalledMemoryView>) -> serde_json::Value {
        ChatEvent::MemoryRecall {
            count: memories.len() as u32,
            latency_ms: 12,
            memories,
        }
        .payload("req-1")
    }

    #[test]
    fn the_wire_carries_what_was_recalled_not_only_how_much() {
        let body = payload(vec![view(MemoryOrigin::Stated, MemoryRole::Recall)]);
        assert_eq!(body["count"], 1);
        let memory = &body["memories"][0];
        assert_eq!(memory["memory_id"], "mem-1");
        assert_eq!(memory["label"], "USER");
        assert_eq!(memory["preview"], "Prefers metric units");
        // The id is on the wire so a reader can act on the exact row.
        assert!(memory["memory_id"].is_string());
    }

    /// The honesty rule, asserted at the boundary a client reads: three origin
    /// states, and "unrecorded" must never serialize as "stated".
    #[test]
    fn unrecorded_origin_is_distinguishable_on_the_wire() {
        for (origin, expected) in [
            (MemoryOrigin::Stated, "stated"),
            (MemoryOrigin::Inferred, "inferred"),
            (MemoryOrigin::Unrecorded, "unrecorded"),
        ] {
            let body = payload(vec![view(origin, MemoryRole::Recall)]);
            assert_eq!(body["memories"][0]["origin"], expected);
        }
    }

    #[test]
    fn the_role_split_survives_serialization() {
        let body = payload(vec![
            view(MemoryOrigin::Stated, MemoryRole::Recall),
            view(MemoryOrigin::Stated, MemoryRole::Inject),
        ]);
        assert_eq!(body["memories"][0]["role"], "recall");
        assert_eq!(body["memories"][1]["role"], "inject");
    }

    /// A turn that recalled nothing describable still reports its count, so an
    /// empty list never reads as "memory was not used".
    #[test]
    fn an_empty_description_list_is_still_a_valid_event() {
        let body = payload(Vec::new());
        assert_eq!(body["memories"].as_array().map(Vec::len), Some(0));
        assert!(body["latency_ms"].is_number());
    }
}

/// The key, type and presence golden of every `ChatEvent` payload (WS12-S7): what the SPA's
/// `chat-client.ts` may rely on, generated from the real `payload()` rather than written by hand.
/// Each variant is sampled twice, minimal (every optional empty) and full (every optional set), so a
/// conditionally written key reads as `optional` and a renamed or retyped key changes the file.
/// Names, types and presence only: no sample text is recorded, so the golden carries no content.
#[cfg(test)]
mod payload_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use serde::Serialize;
    use serde_json::{json, Value};

    use super::{AttachmentPreview, ChatEvent, RetentionClass, ServedBy};
    use crate::knowledge_receipt::{KnowledgeMode, KnowledgeReceipt};
    use crate::search_provenance::{ProviderUse, Route, SearchProvenance};

    const REQUEST_ID: &str = "req-sample";

    /// The committed golden. Regenerate with `MP_UPDATE_GOLDEN=1`; the repo-level
    /// `scripts/tests/zdr-contract-sync.test.sh` then needs its pinned digest, the `.sha256` pin and the
    /// SPA's vendored copy refreshed, which is the deliberate second step of changing the contract.
    const GOLDEN_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../api/contracts/sse-event-payloads.json"
    );

    /// Every variant name `covered` can return. A variant added to `ChatEvent` does not compile until
    /// `covered` names it, and `every_variant_has_a_minimal_and_a_full_sample` then fails until it is
    /// listed here with its samples.
    const VARIANTS: &[&str] = &[
        "ResultReceipt",
        "ReasoningDelta",
        "StepUpdate",
        "ToolCall",
        "ToolResult",
        "Citation",
        "SourceScopeApplied",
        "SearchProvenance",
        "Grounding",
        "Artifact",
        "Attachment",
        "Usage",
        "UsageProgress",
        "TurnTiming",
        "Title",
        "FollowUps",
        "PlanProposed",
        "UserQuestion",
        "ConnectorSuggestion",
        "ResearchPlan",
        "ResearchRun",
        "Verification",
        "MemoryRecall",
        "QueuedInput",
        "QueuedInputReceipted",
        "RetrievalNotice",
        "Stopped",
        "Error",
        "ZdrError",
        "SubscriptionError",
        "RetryScheduled",
        "Compaction",
        "ContextChange",
        "RunParked",
        "AwaitingInput",
        "GoalProgress",
        "SandboxState",
    ];

    /// The variant's name. No wildcard arm, so a new `ChatEvent` variant is a compile error here.
    fn covered(event: &ChatEvent) -> &'static str {
        match event {
            ChatEvent::ResultReceipt { .. } => "ResultReceipt",
            ChatEvent::ReasoningDelta { .. } => "ReasoningDelta",
            ChatEvent::StepUpdate { .. } => "StepUpdate",
            ChatEvent::ToolCall { .. } => "ToolCall",
            ChatEvent::ToolResult { .. } => "ToolResult",
            ChatEvent::Citation { .. } => "Citation",
            ChatEvent::SourceScopeApplied { .. } => "SourceScopeApplied",
            ChatEvent::SearchProvenance { .. } => "SearchProvenance",
            ChatEvent::Grounding { .. } => "Grounding",
            ChatEvent::Artifact { .. } => "Artifact",
            ChatEvent::Attachment { .. } => "Attachment",
            ChatEvent::Usage { .. } => "Usage",
            ChatEvent::UsageProgress { .. } => "UsageProgress",
            ChatEvent::TurnTiming { .. } => "TurnTiming",
            ChatEvent::Title { .. } => "Title",
            ChatEvent::FollowUps { .. } => "FollowUps",
            ChatEvent::PlanProposed { .. } => "PlanProposed",
            ChatEvent::UserQuestion { .. } => "UserQuestion",
            ChatEvent::ConnectorSuggestion { .. } => "ConnectorSuggestion",
            ChatEvent::ResearchPlan { .. } => "ResearchPlan",
            ChatEvent::ResearchRun { .. } => "ResearchRun",
            ChatEvent::Verification { .. } => "Verification",
            ChatEvent::MemoryRecall { .. } => "MemoryRecall",
            ChatEvent::QueuedInput { .. } => "QueuedInput",
            ChatEvent::QueuedInputReceipted { .. } => "QueuedInputReceipted",
            ChatEvent::RetrievalNotice { .. } => "RetrievalNotice",
            ChatEvent::Stopped { .. } => "Stopped",
            ChatEvent::Error { .. } => "Error",
            ChatEvent::ZdrError { .. } => "ZdrError",
            ChatEvent::SubscriptionError { .. } => "SubscriptionError",
            ChatEvent::RetryScheduled { .. } => "RetryScheduled",
            ChatEvent::Compaction { .. } => "Compaction",
            ChatEvent::ContextChange { .. } => "ContextChange",
            ChatEvent::RunParked { .. } => "RunParked",
            ChatEvent::AwaitingInput { .. } => "AwaitingInput",
            ChatEvent::GoalProgress { .. } => "GoalProgress",
            ChatEvent::SandboxState { .. } => "SandboxState",
        }
    }

    fn s(text: &str) -> String {
        text.to_owned()
    }

    /// The structural check of a generated file that failed, the optional `check` key the full attachment sample sets.
    fn office_check() -> mp_contracts::office_verification::OfficeVerification {
        mp_contracts::office_verification::OfficeVerification::from_untrusted_json(
            r#"{"kind":"docx","level":"failed","checks":[{"id":"container","level":"failed","message":"the file is not a readable zip package"}],"stats":{}}"#,
            "f.docx",
            mp_contracts::office_verification::OfficeKind::Docx,
        )
        .expect("a well-formed report")
    }

    /// The render of the file `office_check` judges, the optional `preview` key the full attachment sample sets.
    fn office_preview() -> AttachmentPreview {
        AttachmentPreview {
            mime: s("application/pdf"),
            url: s("data:application/pdf;base64,AA"),
            pages: 1,
            fonts_substituted: false,
            thumbs: vec![s("data:image/png;base64,AA")],
        }
    }

    fn citation(full: bool) -> ChatEvent {
        ChatEvent::Citation {
            id: s("c1"),
            title: s("Doc"),
            url: s("https://example.test/doc"),
            snippet: s("snippet"),
            source_group_id: s("kb:sharepoint"),
            source_group_label: full.then(|| s("Microsoft 365")),
            number: full.then_some(1),
            claim_id: full.then(|| s("turn-1:claim-0")),
            start: full.then_some(12),
            end: full.then_some(15),
            origin: s("knowledge"),
            passage: full.then(|| super::CitationPassage {
                quote: Some(s("quote")),
                locator: Some(super::CitationLocator { document_id: s("doc-1"), chunk_index: 0 }),
            }),
        }
    }

    fn grounding(full: bool) -> ChatEvent {
        let grounding = if full {
            crate::retrieval::Grounding {
                mode: s("hybrid"),
                query: s("query"),
                trace_id: Some(s("trace-1")),
                low_confidence: true,
                fact_count: 1,
                source_count: 1,
                facts: vec![crate::retrieval::GroundingFact {
                    knowledge_id: s("kid-1"),
                    document_id: s("doc-1"),
                    text: s("fact"),
                    score: 0.9,
                    source_title: s("Title"),
                    source_type: s("policy"),
                    provider: s("Notion"),
                    chunk_index: 0,
                }],
                sources: vec![crate::retrieval::GroundingSource {
                    id: s("doc-1"),
                    kind: s("knowledge"),
                    title: s("Title"),
                    snippet: s("snippet"),
                    provider: s("Notion"),
                    source_type: s("policy"),
                    document_id: s("doc-1"),
                    href: s("/knowledge"),
                    score: 0.9,
                }],
                graph: Some(crate::retrieval::GroundingGraph {
                    trace_id: Some(s("graph-1")),
                    community_summaries: vec![s("summary")],
                    edge_count: 1,
                    nodes: vec![crate::retrieval::GroundingGraphNode {
                        id: s("node-1"),
                        label: s("Label"),
                        kind: s("policy"),
                    }],
                }),
                suggested_next_tools: vec![s("knowledge_search")],
                zdr_actions_applied: vec![s("ephemeral_no_trace_persist")],
                ..Default::default()
            }
        } else {
            crate::retrieval::Grounding::default()
        };
        ChatEvent::Grounding { grounding }
    }

    /// The routing decision an answer's route note states (W5-073-b): one of each closed token `route_decision`
    /// writes. The golden records the key as an object, so the tokens themselves never reach it.
    fn route_decision_sample() -> Value {
        json!({
            "mode": "balance",
            "complexity": "moderate",
            "reason": "mode_cell",
            "role": "follow_ups",
            "role_source": "table",
        })
    }

    fn usage(full: bool) -> ChatEvent {
        ChatEvent::Usage {
            input_tokens: 12,
            output_tokens: 34,
            cost_usd: full.then_some(0.002),
            cost_basis: full.then_some(crate::pricing::CostBasis::Metered),
            latency_ms: 880,
            confidence: full.then_some(0.77),
            served_by: full.then(|| ServedBy {
                provider_used: s("provider-a"),
                residency: s("eu"),
                privacy_tier: Some("eu_resident"),
            }),
            retention_class: full.then_some(RetentionClass::ZdrWaived {
                reason: "connection_not_zdr_capable",
            }),
            cache_read_tokens: u32::from(full) * 5,
            cache_write_tokens: u32::from(full) * 3,
            calls: 1 + 2 * u32::from(full),
            unpriced_calls: 0,
            // The breakdown is stated only where the total is priced, so the golden
            // records the key as present on some payloads and absent on others.
            cost_by_category: full.then_some(crate::pricing::CategoryCost {
                input_usd: 0.001,
                cache_read_usd: 0.0005,
                cache_write_usd: 0.0,
                output_usd: 0.0005,
            }),
            // The full sample states its route note's decision; the minimal one keeps its shape.
            routing_decision: full.then(route_decision_sample),
        }
    }

    /// A progress frame of a turn whose tokens are fixed, so only its calls, its cost and the optional
    /// keys that go with them vary.
    fn usage_progress(
        calls: u32,
        cost_usd: Option<f64>,
        basis: Option<crate::pricing::CostBasis>,
        unpriced_calls: u32,
    ) -> ChatEvent {
        usage_progress_with_ceiling(calls, cost_usd, basis, unpriced_calls, None, None, None)
    }

    /// A progress frame of the same turn, carrying the run ceiling it states, the budget's routing token and
    /// the routing decision its route note states (W5-073-b).
    fn usage_progress_with_ceiling(
        calls: u32,
        cost_usd: Option<f64>,
        basis: Option<crate::pricing::CostBasis>,
        unpriced_calls: u32,
        ceiling: Option<crate::sse_events::run_ceiling::CeilingProgress>,
        routing: Option<&'static str>,
        routing_decision: Option<Value>,
    ) -> ChatEvent {
        ChatEvent::UsageProgress {
            phase: crate::usage_meter::CallPhase::ToolRound,
            snapshot: crate::usage_meter::Snapshot {
                calls,
                input: 2_042,
                output: 207,
                cache_read: 5,
                cache_write: 3,
                cost_usd,
                unpriced_calls,
                basis,
                by_category: cost_usd.map(|_| crate::pricing::CategoryCost {
                    input_usd: 0.001,
                    cache_read_usd: 0.0005,
                    cache_write_usd: 0.0,
                    output_usd: 0.0005,
                }),
            },
            ceiling,
            routing,
            routing_decision,
        }
    }

    fn turn_timing(full: bool) -> ChatEvent {
        use crate::turn_timing::{Counts, Draft, Marks, Outcome, Phase, PhaseSpan, ProviderSummary};
        let report = crate::turn_timing::TurnTimingReport {
            request_id: s(REQUEST_ID),
            outcome: Outcome::Completed,
            server_ms: 900,
            provider_ms: 700,
            orchestration_ms: 200,
            marks: if full {
                Marks {
                    connected_ms: Some(1),
                    source_ready_ms: Some(2),
                    first_token_ms: Some(3),
                    first_draft_ms: Some(4),
                    first_validated_ms: Some(5),
                    durable_ms: Some(6),
                }
            } else {
                Marks::default()
            },
            counts: Counts { model_rounds: u32::from(full), ..Default::default() },
            tokens_reported: full,
            phases: if full {
                vec![PhaseSpan {
                    phase: Phase::Generation,
                    start_ms: 1,
                    end_ms: 9,
                    round: Some(1),
                    pass: Some(1),
                    batch: Some(1),
                    attempt: Some(1),
                    outcome: Some("ok"),
                    tools: vec![s("create_artifact")],
                    provider: Some(ProviderSummary {
                        handler_ms: 8,
                        provider_ms: 7,
                        first_token_ms: Some(2),
                        attempts: 1,
                        failed_attempts_ms: 0,
                        retry_wait_ms: 0,
                        response_cache_hit: false,
                        upstream_ms: BTreeMap::from([(s("connect"), 1)]),
                    }),
                    budget_ms: Some(100),
                }]
            } else {
                Vec::new()
            },
            phases_dropped: u32::from(full),
            draft: if full {
                Draft { model_writes: 1, model_write_chars: 10, stored_chars: 9, ..Default::default() }
            } else {
                Draft::default()
            },
        };
        ChatEvent::TurnTiming { report }
    }

    fn memory_recall(full: bool) -> ChatEvent {
        use crate::memory_provenance::{MemoryOrigin, MemoryRole, RecalledMemoryView};
        let memories = if full {
            vec![RecalledMemoryView {
                memory_id: s("mem-1"),
                role: MemoryRole::Recall,
                origin: MemoryOrigin::Stated,
                label: s("USER"),
                preview: s("preview"),
            }]
        } else {
            Vec::new()
        };
        ChatEvent::MemoryRecall { count: memories.len() as u32, latency_ms: 12, memories }
    }

    /// A minimal and a full sample of every variant, minimal first.
    fn samples() -> Vec<ChatEvent> {
        let refusal = |feature: Option<&'static str>| match feature {
            Some(key) => mp_contracts::zdr_refusal::Refusal::feature_not_supported(key),
            None => mp_contracts::zdr_refusal::Refusal::UNAVAILABLE,
        };
        vec![
            ChatEvent::ResultReceipt { receipt: json!({}) },
            ChatEvent::ResultReceipt { receipt: json!({ "checks": [] }) },
            ChatEvent::ReasoningDelta { delta: s("") },
            ChatEvent::ReasoningDelta { delta: s("thinking") },
            ChatEvent::StepUpdate { id: s("1"), title: s(""), detail: s(""), status: s("running") },
            ChatEvent::StepUpdate { id: s("1"), title: s("Title"), detail: s("Detail"), status: s("done") },
            ChatEvent::ToolCall { id: s("1"), name: s("web_search"), args: json!({}) },
            ChatEvent::ToolCall { id: s("1"), name: s("web_search"), args: json!({ "query": "q" }) },
            ChatEvent::ToolResult { id: s("1"), status: s("ok"), output: s(""), error: None },
            ChatEvent::ToolResult { id: s("1"), status: s("error"), output: s("out"), error: Some(s("boom")) },
            citation(false),
            citation(true),
            ChatEvent::SourceScopeApplied { scope: s("web_only"), excluded_tools: Vec::new(), knowledge: None, reason: None },
            ChatEvent::SourceScopeApplied {
                scope: s("knowledge_only"),
                excluded_tools: vec![s("web_search")],
                knowledge: None,
                reason: None,
            },
            ChatEvent::SourceScopeApplied {
                scope: s("auto"),
                excluded_tools: Vec::new(),
                knowledge: Some(KnowledgeReceipt { mode: KnowledgeMode::NotEntitled, sources: None }),
                reason: None,
            },
            ChatEvent::SourceScopeApplied {
                scope: s("knowledge_only"),
                excluded_tools: vec![s("web_search")],
                knowledge: Some(KnowledgeReceipt { mode: KnowledgeMode::Retrieval, sources: Some(3) }),
                reason: None,
            },
            ChatEvent::SearchProvenance {
                provenance: SearchProvenance {
                    providers: vec![ProviderUse { id: "brave", label: "Brave Search", hits: 2 }],
                    route: Route::FreeChain,
                },
            },
            ChatEvent::SearchProvenance {
                provenance: SearchProvenance {
                    providers: vec![
                        ProviderUse { id: "verevon", label: "Verevon search", hits: 1 },
                        ProviderUse { id: "other", label: "other provider", hits: 0 },
                    ],
                    route: Route::ZdrFreeOnly,
                },
            },
            grounding(false),
            grounding(true),
            ChatEvent::Artifact { id: s("a1"), kind: s("document"), title: s(""), content: s(""), version: 1, provenance: None, editor: None },
            ChatEvent::Artifact {
                id: s("a1"),
                kind: s("code"),
                title: s("T"),
                content: s("body"),
                version: 2,
                provenance: Some(Box::new(crate::deep_research::ResearchProvenance::sample())),
                editor: Some(s("model")),
            },
            ChatEvent::Attachment {
                id: s("f1"),
                name: s("f.png"),
                mime: s("image/png"),
                url: s("blob:1"),
                size: 0,
                check: None,
                preview: None,
            },
            ChatEvent::Attachment {
                id: s("f1"),
                name: s("f.docx"),
                mime: s("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
                url: s("data:application/octet-stream;base64,AA"),
                size: 2048,
                check: Some(office_check()),
                preview: Some(office_preview()),
            },
            usage(false),
            usage(true),
            usage_progress(1, None, Some(crate::pricing::CostBasis::Subscription), 0),
            usage_progress(
                3,
                Some(0.002_456),
                Some(crate::pricing::CostBasis::Metered),
                0,
            ),
            usage_progress(2, None, Some(crate::pricing::CostBasis::Metered), 1),
            // A mixed total states its cost and no basis, which is why the golden must mark `cost_basis` optional.
            usage_progress(4, Some(0.25), None, 0),
            // A frame that states the run ceiling: the share used, and the 80% warning it is the first to carry.
            usage_progress_with_ceiling(
                3,
                Some(0.002_456),
                Some(crate::pricing::CostBasis::Metered),
                0,
                Some(crate::sse_events::run_ceiling::CeilingProgress {
                    ceiling_usd: Some(0.05),
                    ceiling_tokens: Some(20_000),
                    fraction: Some(0.8),
                    warning: Some(crate::sse_events::run_ceiling::CeilingWarning::Eighty),
                }),
                None,
                None,
            ),
            // A frame whose call the budget moved states its routing (WS27-S7).
            usage_progress_with_ceiling(
                2,
                Some(0.001),
                Some(crate::pricing::CostBasis::Metered),
                0,
                None,
                Some("tier_shifted"),
                None,
            ),
            // The answer's own frame states the routing decision its route note stated (W5-073-b).
            usage_progress_with_ceiling(
                2,
                Some(0.001),
                Some(crate::pricing::CostBasis::Metered),
                0,
                None,
                None,
                Some(route_decision_sample()),
            ),
            turn_timing(false),
            turn_timing(true),
            ChatEvent::Title { title: s("") },
            ChatEvent::Title { title: s("A title") },
            ChatEvent::FollowUps { suggestions: Vec::new() },
            ChatEvent::FollowUps { suggestions: vec![s("Why?")] },
            ChatEvent::PlanProposed { plan_id: s("p1"), summary: s(""), steps: json!([]) },
            ChatEvent::PlanProposed {
                plan_id: s("p1"),
                summary: s("Summary"),
                steps: json!([{ "id": "s1", "title": "T", "operation": "write" }]),
            },
            ChatEvent::UserQuestion {
                question_id: s("q_1"),
                delivery: s("next_turn"),
                expires_at: None,
                items: json!([]),
                origin: None,
                origin_label: None,
            },
            ChatEvent::UserQuestion {
                question_id: s("q_2"),
                delivery: s("next_turn"),
                expires_at: Some(s("2026-11-02T10:00:00Z")),
                items: json!([{
                    "id": "carrier",
                    "kind": "single",
                    "prompt": "Which carrier?",
                    "options": [{ "id": "bring", "label": "Bring", "description": null }],
                    "allow_other": true,
                }]),
                origin: Some(s("mcp_elicitation")),
                origin_label: Some(s("Fixture server")),
            },
            ChatEvent::ConnectorSuggestion { suggestion_id: s("sug_1"), provider: s("google"), reason: s("") },
            ChatEvent::ConnectorSuggestion {
                suggestion_id: s("sug_2"),
                provider: s("microsoft"),
                reason: s("Your calendar would show the meeting."),
            },
            ChatEvent::ResearchPlan { sub_queries: Vec::new(), questions: Vec::new(), max_sub_queries: 1 },
            ChatEvent::ResearchRun { id: s("rsr_sample"), status: "running" },
            ChatEvent::ResearchRun { id: s("rsr_sample"), status: "cancelled" },
            ChatEvent::ResearchPlan {
                sub_queries: vec![s("norsk havbruk 2026"), s("lakselus tiltak")],
                questions: vec![s("Hvilken region?")],
                max_sub_queries: 9,
            },
            ChatEvent::Verification { verdict: "unrelated", kb_citations: 0, web_citations: 0, web_allowed: false },
            ChatEvent::Verification { verdict: "supports", kb_citations: 2, web_citations: 1, web_allowed: true },
            memory_recall(false),
            memory_recall(true),
            ChatEvent::QueuedInput { messages: Vec::new() },
            ChatEvent::QueuedInput { messages: vec![s("one more thing")] },
            ChatEvent::QueuedInputReceipted { messages: Vec::new(), ids: Vec::new() },
            ChatEvent::QueuedInputReceipted { messages: vec![s("one more thing")], ids: vec![s("msg-1")] },
            ChatEvent::RetrievalNotice {
                notice: crate::retrieval_metadata::notice_for_code("zdr_dense_unavailable")
                    .expect("the ZDR notice is catalogued"),
            },
            ChatEvent::RetrievalNotice {
                notice: crate::retrieval_metadata::notice_for_code("another_reduction")
                    .expect("the generic notice is catalogued"),
            },
            ChatEvent::Stopped { reason: s("") },
            ChatEvent::Stopped { reason: s("user") },
            ChatEvent::Error { code: s("internal"), message: s(""), retryable: false },
            ChatEvent::Error { code: s("upstream_unavailable"), message: s("Try again"), retryable: true },
            ChatEvent::ZdrError { refusal: refusal(None) },
            ChatEvent::ZdrError { refusal: refusal(Some("attachments")) },
            ChatEvent::RetryScheduled {
                scope: "tool",
                tool: s("knowledge_search"),
                attempt: 2,
                max_attempts: 3,
                wait_ms: 0,
                reason: "other",
            },
            ChatEvent::RetryScheduled {
                scope: "tool",
                tool: s("knowledge_search"),
                attempt: 2,
                max_attempts: 3,
                wait_ms: 250,
                reason: "rate_limited",
            },
            ChatEvent::Compaction {
                scope: "history",
                outcome: "truncated",
                messages_compacted: None,
                messages_kept: None,
                pinned_kept: None,
            },
            ChatEvent::Compaction {
                scope: "tool_results",
                outcome: "summarized",
                messages_compacted: Some(12),
                messages_kept: Some(6),
                pinned_kept: Some(2),
            },
            ChatEvent::ContextChange {
                change: "history_shed",
                messages_dropped: None,
                skills_truncated: None,
                skills_dropped: None,
                layer: None,
                state: None,
            },
            ChatEvent::ContextChange {
                change: "run_context_skipped",
                messages_dropped: Some(4),
                skills_truncated: Some(1),
                skills_dropped: Some(0),
                layer: Some("memory"),
                state: Some("skipped_by_policy"),
            },
            ChatEvent::RunParked {
                run_id: s("run-1"),
                reason: mp_contracts::loop_fold::ParkReason::CredentialWindow,
                resume_after: None,
            },
            ChatEvent::RunParked {
                run_id: s("run-1"),
                reason: mp_contracts::loop_fold::ParkReason::ProviderExhausted,
                resume_after: Some(s("2026-10-20T12:00:00Z")),
            },
            ChatEvent::AwaitingInput { run_id: s("run-1"), question_id: s("") },
            ChatEvent::AwaitingInput { run_id: s("run-1"), question_id: s("q_1") },
            ChatEvent::GoalProgress {
                goal_id: s("goal_1"),
                turn_no: 1,
                tokens_used: 0,
                token_budget: None,
                cost_used_usd_micros: 0,
                cost_budget_usd_micros: None,
                turns_used: 1,
                max_turns: 10,
                status: "active",
                status_reason: "",
                last_verdict: "",
            },
            ChatEvent::GoalProgress {
                goal_id: s("goal_1"),
                turn_no: 3,
                tokens_used: 31_000,
                token_budget: Some(30_000),
                cost_used_usd_micros: 120_000,
                cost_budget_usd_micros: Some(500_000),
                turns_used: 3,
                max_turns: 10,
                status: "budget_reached",
                status_reason: "token_budget",
                last_verdict: "not_met",
            },
            ChatEvent::SandboxState {
                scope: "thread",
                state: "kept",
                cause: None,
            },
            ChatEvent::SandboxState {
                scope: "thread",
                state: "conflict",
                cause: Some("workspace_conflict"),
            },
            ChatEvent::SubscriptionError {
                code: s("rate_limited"),
                message: s(""),
                retryable: false,
                retry_after_ms: None,
                resets_at: None,
                retry_at: None,
            },
            ChatEvent::SubscriptionError {
                code: s("rate_limited"),
                message: s("Wait"),
                retryable: true,
                retry_after_ms: Some(1000),
                resets_at: Some(s("2026-01-01T00:00:00Z")),
                retry_at: Some(s("2026-01-01T00:00:01Z")),
            },
        ]
    }

    fn json_type(value: &Value) -> &'static str {
        match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(number) if number.is_f64() => "number",
            Value::Number(_) => "integer",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    }

    #[derive(Serialize)]
    struct KeyShape {
        types: Vec<&'static str>,
        presence: &'static str,
    }

    #[derive(Serialize)]
    struct EventShape {
        family: Option<&'static str>,
        variants: Vec<&'static str>,
        keys: BTreeMap<String, KeyShape>,
    }

    #[derive(Serialize)]
    struct Contract {
        contract: &'static str,
        version: u32,
        generated_by: &'static str,
        events: BTreeMap<&'static str, EventShape>,
    }

    /// What the samples say about each SSE event name: the families and variants behind it and, per
    /// payload key, the JSON types it took and whether every sample carried it. The three `error`
    /// variants are one event name, so they merge into one entry.
    fn render() -> String {
        struct Seen {
            family: Option<&'static str>,
            variants: BTreeSet<&'static str>,
            samples: usize,
            keys: BTreeMap<String, (BTreeSet<&'static str>, usize)>,
        }
        let mut seen: BTreeMap<&'static str, Seen> = BTreeMap::new();
        for event in samples() {
            let entry = seen.entry(event.name()).or_insert_with(|| Seen {
                family: event.family(),
                variants: BTreeSet::new(),
                samples: 0,
                keys: BTreeMap::new(),
            });
            assert_eq!(entry.family, event.family(), "{} mixes feature families", event.name());
            entry.variants.insert(covered(&event));
            entry.samples += 1;
            let payload = event.payload(REQUEST_ID);
            let object = payload.as_object().expect("an SSE payload is a JSON object");
            for (key, value) in object {
                let slot = entry.keys.entry(key.clone()).or_default();
                slot.0.insert(json_type(value));
                slot.1 += 1;
            }
        }
        let events = seen
            .into_iter()
            .map(|(name, seen)| {
                let keys = seen
                    .keys
                    .into_iter()
                    .map(|(key, (types, count))| {
                        let presence = if count == seen.samples { "always" } else { "optional" };
                        (key, KeyShape { types: types.into_iter().collect(), presence })
                    })
                    .collect();
                (name, EventShape { family: seen.family, variants: seen.variants.into_iter().collect(), keys })
            })
            .collect();
        let contract = Contract {
            contract: "model-gateway.sse-event-payloads",
            version: 1,
            generated_by: "model-gateway sse_events payload_contract tests (MP_UPDATE_GOLDEN=1)",
            events,
        };
        let mut text = serde_json::to_string_pretty(&contract).expect("the contract serializes");
        text.push('\n');
        text
    }

    /// `user_question` is a control event that carries the card's keys and the question itself, and
    /// nothing else (WS14 S4).
    #[test]
    fn user_question_is_a_control_event_with_the_card_keys() {
        let event = ChatEvent::UserQuestion {
            question_id: "q_abc".into(),
            delivery: "next_turn".into(),
            expires_at: Some("2026-11-02T10:00:00Z".into()),
            items: json!([{ "id": "carrier", "kind": "single" }]),
            origin: None,
            origin_label: None,
        };
        assert_eq!(event.name(), "user_question");
        assert!(
            event.family().is_none(),
            "a question is a control event, never feature-gated"
        );
        assert!(event.should_emit(&[]));
        let payload = event.payload("req-4");
        assert_eq!(payload["question_id"], "q_abc");
        assert_eq!(payload["delivery"], "next_turn");
        assert_eq!(payload["expires_at"], "2026-11-02T10:00:00Z");
        assert_eq!(payload["items"][0]["id"], "carrier");
        assert_eq!(payload["request_id"], "req-4");
    }

    /// `connector_suggestion` is a control event that carries the card's three keys and nothing else
    /// (EP-10 S8).
    #[test]
    fn connector_suggestion_is_a_control_event_with_the_card_keys() {
        let event = ChatEvent::ConnectorSuggestion {
            suggestion_id: "sug_1".into(),
            provider: "google".into(),
            reason: "Your mail would answer this.".into(),
        };
        assert_eq!(event.name(), "connector_suggestion");
        assert!(event.family().is_none(), "the card is a control event, never feature-gated");
        assert!(event.should_emit(&[]));
        assert_eq!(
            event.payload("req-5"),
            json!({
                "suggestion_id": "sug_1",
                "provider": "google",
                "reason": "Your mail would answer this.",
                "request_id": "req-5",
            })
        );
    }

    /// A model question's frame has no origin key, so an older client and today's read it the same way
    /// (EP-4 S6).
    #[test]
    fn user_question_without_origin_is_byte_identical() {
        let event = ChatEvent::UserQuestion {
            question_id: "q_abc".into(),
            delivery: "next_turn".into(),
            expires_at: None,
            items: json!([]),
            origin: None,
            origin_label: None,
        };
        assert_eq!(
            event.payload("req-5"),
            json!({
                "question_id": "q_abc",
                "delivery": "next_turn",
                "expires_at": null,
                "items": [],
                "request_id": "req-5",
            })
        );
    }

    /// A server's question carries the server's name, so the card can say who asked it (EP-4 S6).
    #[test]
    fn user_question_with_server_origin_carries_it() {
        let event = ChatEvent::UserQuestion {
            question_id: "q_srv".into(),
            delivery: "resume".into(),
            expires_at: None,
            items: json!([]),
            origin: Some("mcp_elicitation".into()),
            origin_label: Some("Fixture server".into()),
        };
        let payload = event.payload("req-6");
        assert_eq!(payload["question_id"], "q_srv");
        assert_eq!(payload["origin"], "mcp_elicitation");
        assert_eq!(payload["origin_label"], "Fixture server");
    }

    /// `research_plan` is a control event carrying the plan, the questions and the cap the card
    /// enforces, and nothing else (EP-2 S5).
    #[test]
    fn research_plan_is_a_control_event_with_the_card_keys() {
        let event = ChatEvent::ResearchPlan {
            sub_queries: vec!["a".into(), "b".into()],
            questions: vec!["Which year?".into()],
            max_sub_queries: 9,
        };
        assert_eq!(event.name(), "research_plan");
        assert!(event.family().is_none(), "the plan card is never feature-gated");
        assert!(event.should_emit(&[]));
        let payload = event.payload("req-5");
        assert_eq!(payload["sub_queries"], json!(["a", "b"]));
        assert_eq!(payload["questions"], json!(["Which year?"]));
        assert_eq!(payload["caps"], json!({ "max_sub_queries": 9 }));
        assert_eq!(payload["request_id"], "req-5");
        assert_eq!(payload.as_object().map(serde_json::Map::len), Some(4));
    }

    /// `search_provenance` is a `citations`-family frame that names providers and a route, and nothing else (EP-9 W1).
    #[test]
    fn search_provenance_names_providers_and_a_route_only() {
        let event = ChatEvent::SearchProvenance {
            provenance: SearchProvenance {
                providers: vec![ProviderUse { id: "brave", label: "Brave Search", hits: 2 }],
                route: Route::PaidAllowed,
            },
        };
        assert_eq!(event.name(), "search_provenance");
        assert_eq!(event.family(), Some("citations"));
        assert!(!event.should_emit(&[]), "a client that asked for no citations does not see it");
        assert!(event.should_emit(&["citations".to_string()]));
        let payload = event.payload("req-w1");
        assert_eq!(payload["providers"], json!([{ "id": "brave", "label": "Brave Search", "hits": 2 }]));
        assert_eq!(payload["route"], "paid_allowed");
        assert_eq!(payload["request_id"], "req-w1");
        assert_eq!(payload.as_object().map(serde_json::Map::len), Some(3));
    }

    #[test]
    fn every_variant_has_a_minimal_and_a_full_sample() {
        let mut count: BTreeMap<&str, usize> = BTreeMap::new();
        for event in samples() {
            *count.entry(covered(&event)).or_default() += 1;
        }
        for variant in VARIANTS {
            assert!(
                count.get(variant).copied().unwrap_or(0) >= 2,
                "{variant} needs a minimal and a full sample in samples()"
            );
        }
        assert_eq!(
            count.keys().copied().collect::<BTreeSet<_>>(),
            VARIANTS.iter().copied().collect::<BTreeSet<_>>(),
            "samples() and VARIANTS name different variants"
        );
    }

    /// The golden states `routing_decision` on both usage families: the terminal frame and the answer's progress
    /// frame carry it, so the key the SPA reads is in the contract (W5-073-b).
    #[test]
    fn the_golden_states_a_routing_decision_on_both_usage_families() {
        let carries = |family: &str| {
            samples().iter().any(|event| {
                event.name() == family && event.payload(REQUEST_ID)["routing_decision"].is_object()
            })
        };
        for family in ["usage", "usage_progress"] {
            assert!(carries(family), "{family} states no routing_decision");
        }
    }

    #[test]
    fn the_golden_carries_names_and_types_and_never_sample_content() {
        let text = render();
        for content in ["thinking", "Try again", "one more thing", "A title", "Wait"] {
            assert!(!text.contains(content), "the golden records sample text: {content}");
        }
    }

    /// The frames that carry retention and refusal facts keep the keys the SPA reads them by.
    #[test]
    fn the_zdr_bearing_frames_keep_their_keys() {
        let contract: Value = serde_json::from_str(&render()).expect("the golden is JSON");
        let key = |event: &str, key: &str| contract["events"][event]["keys"].get(key).cloned();
        assert_eq!(key("usage", "retention_class").unwrap()["presence"], "optional");
        assert_eq!(key("usage", "retention_reason").unwrap()["presence"], "optional");
        assert_eq!(key("error", "code").unwrap()["presence"], "always");
        assert_eq!(key("error", "action").unwrap()["presence"], "optional");
        assert_eq!(key("error", "feature").unwrap()["presence"], "optional");
        assert_eq!(key("retrieval_notice", "wording_key").unwrap()["presence"], "always");
    }

    #[test]
    fn source_scope_applied_names_the_org_web_switch_only_when_it_withheld_a_tool() {
        let withheld = ChatEvent::SourceScopeApplied {
            scope: s("auto"),
            excluded_tools: vec![s("web_search"), s("fetch_url")],
            knowledge: None,
            reason: Some(s("org_web_off")),
        };
        let payload = withheld.payload("req-1");
        assert_eq!(payload["reason"], "org_web_off");
        assert_eq!(payload["excluded_tools"], json!(["web_search", "fetch_url"]));

        // A turn no organisation switch touched keeps its frame as it was: no reason key at all.
        let plain = ChatEvent::SourceScopeApplied {
            scope: s("web_only"),
            excluded_tools: Vec::new(),
            knowledge: None,
            reason: None,
        };
        assert!(plain.payload("req-1").get("reason").is_none());
    }

    #[test]
    fn sse_event_payload_golden() {
        let generated = render();
        if std::env::var("MP_UPDATE_GOLDEN").is_ok_and(|value| value == "1") {
            std::fs::create_dir_all(std::path::Path::new(GOLDEN_PATH).parent().unwrap())
                .expect("the contracts directory");
            std::fs::write(GOLDEN_PATH, &generated).expect("write the SSE payload golden");
            return;
        }
        let committed = std::fs::read_to_string(GOLDEN_PATH).unwrap_or_else(|error| {
            panic!(
                "cannot read {GOLDEN_PATH} ({error}). Generate it with MP_UPDATE_GOLDEN=1 cargo test \
                 -p model-gateway --lib sse_event_payload_golden, then pin its digest in \
                 scripts/tests/zdr-contract-sync.test.sh."
            )
        });
        assert!(
            !committed.contains('\r'),
            "the SSE payload golden must keep LF line endings (see .gitattributes)"
        );
        assert_eq!(
            committed, generated,
            "the committed SSE payload golden differs from what ChatEvent::payload produces. If the \
             change is intended, regenerate it with MP_UPDATE_GOLDEN=1 cargo test -p model-gateway \
             --lib sse_event_payload_golden, refresh its .sha256 pin and the SPA's vendored copy, and \
             update the pinned digest in scripts/tests/zdr-contract-sync.test.sh."
        );
    }
}

#[cfg(test)]
mod connector_turn_tests {
    use super::*;
    use crate::tool_loop::connector_reads::ConnectorTurn;

    /// The selection rides on the sink, and a connected-account source is recorded with its origin and
    /// no number, so the turn's evidence never gives it a number another source could share.
    #[tokio::test]
    async fn a_connector_citation_is_recorded_unnumbered_on_the_turn_sink() {
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let sink = RichEventSink::new(tx, Vec::new(), "req-1".to_owned()).with_connector_turn(
            ConnectorTurn::new(vec!["conn_1".to_owned()], true),
        );
        assert_eq!(
            sink.connector_turn().map(|turn| turn.selection.clone()),
            Some(vec!["conn_1".to_owned()])
        );
        sink.emit(ChatEvent::Citation {
            id: "conn-0123456789abcdef".to_owned(),
            title: "Invoice 42".to_owned(),
            url: String::new(),
            snippet: "Ada · 2026-10-28".to_owned(),
            source_group_id: "connector:microsoft".to_owned(),
            source_group_label: Some("Microsoft 365".to_owned()),
            number: None,
            claim_id: None,
            start: None,
            end: None,
            origin: "connector".to_owned(),
            passage: None,
        })
        .await;
        let recorded = sink.recorded_citations();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].origin, "connector");
        assert_eq!(recorded[0].number, None);
    }
}

#[cfg(test)]
mod citation_passage_tests {
    use super::*;

    fn fact(document_id: &str, chunk_index: i32, text: &str) -> crate::retrieval::GroundingFact {
        crate::retrieval::GroundingFact {
            knowledge_id: "kid-1".to_owned(),
            document_id: document_id.to_owned(),
            text: text.to_owned(),
            score: 0.9,
            source_title: "Policy".to_owned(),
            source_type: "policy".to_owned(),
            provider: "Notion".to_owned(),
            chunk_index,
        }
    }

    fn citation(passage: Option<CitationPassage>) -> ChatEvent {
        ChatEvent::Citation {
            id: "doc-1".to_owned(),
            title: "Policy".to_owned(),
            url: "notion".to_owned(),
            snippet: "Refunds within 30 days.".to_owned(),
            source_group_id: "kb:notion".to_owned(),
            source_group_label: Some("Notion".to_owned()),
            number: Some(1),
            claim_id: None,
            start: None,
            end: None,
            origin: "knowledge".to_owned(),
            passage,
        }
    }

    #[test]
    fn citation_passages_switch_reads_only_the_literal_true() {
        assert!(parse_citation_passages(Some("true")));
        assert!(parse_citation_passages(Some(" true ")));
        for raw in [None, Some(""), Some("1"), Some("on"), Some("TRUE"), Some("yes")] {
            assert!(!parse_citation_passages(raw), "{raw:?} must stay off");
        }
    }

    #[test]
    fn citation_passage_quote_is_collapsed_bounded_and_never_empty() {
        let long = format!("  {}  ", "ord ".repeat(300));
        let passage = CitationPassage::new(&long, None).expect("a quote");
        let quote = passage.quote.expect("quote set");
        assert!(quote.chars().count() <= MAX_QUOTE_CHARS);
        assert!(!quote.contains("  ") && !quote.ends_with(' '));
        assert_eq!(CitationPassage::new("   ", None), None, "neither half: no passage at all");
        let locator = CitationLocator { document_id: "doc-1".to_owned(), chunk_index: 2 };
        let only_locator = CitationPassage::new("", Some(locator.clone())).expect("locator alone");
        assert_eq!(only_locator.quote, None);
        assert_eq!(only_locator.locator, Some(locator));
    }

    #[test]
    fn citation_passage_for_knowledge_takes_the_first_fact_of_the_document() {
        let facts = vec![
            fact("doc-2", 0, "Other document."),
            fact("doc-1", 4, "Refunds are accepted within 30 days."),
            fact("doc-1", 9, "A later chunk of the same document."),
        ];
        let passage = CitationPassage::knowledge(&facts, "doc-1").expect("doc-1 has a fact");
        assert_eq!(passage.quote.as_deref(), Some("Refunds are accepted within 30 days."));
        assert_eq!(
            passage.locator,
            Some(CitationLocator { document_id: "doc-1".to_owned(), chunk_index: 4 })
        );
        assert_eq!(CitationPassage::knowledge(&facts, "doc-3"), None);
    }

    #[test]
    fn citation_frame_without_a_passage_is_byte_identical_and_with_one_carries_both_keys() {
        let plain = citation(None).payload("r");
        assert!(plain.get("quote").is_none());
        assert!(plain.get("locator").is_none());

        let passage = CitationPassage::knowledge(&[fact("doc-1", 3, "Refunds within 30 days.")], "doc-1");
        let rich = citation(passage).payload("r");
        assert_eq!(rich["quote"], "Refunds within 30 days.");
        assert_eq!(rich["locator"]["document_id"], "doc-1");
        assert_eq!(rich["locator"]["chunk_index"], 3);
        // Every other key is the plain frame's, unchanged.
        let mut stripped = rich.clone();
        let object = stripped.as_object_mut().expect("an object");
        object.remove("quote");
        object.remove("locator");
        assert_eq!(stripped, plain);
    }

    #[tokio::test]
    async fn citation_passage_is_recorded_under_the_stream_keys_and_strippable() {
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let sink = RichEventSink::new(tx, Vec::new(), "r".to_owned());
        let passage = CitationPassage::new("Refunds within 30 days.", None);
        sink.emit(citation(passage)).await;
        let recorded = sink.recorded_citations();
        let value = serde_json::to_value(&recorded[0]).expect("serializable");
        assert_eq!(value["quote"], "Refunds within 30 days.");
        assert!(value.get("passage").is_none(), "flattened, never nested");
        assert!(value.get("locator").is_none());

        let stripped = serde_json::to_value(recorded[0].clone().without_passage()).expect("serializable");
        assert!(stripped.get("quote").is_none());
        assert_eq!(stripped["number"], 1);
    }

    #[test]
    fn citation_artifact_provenance_streams_records_and_strips() {
        let event = ChatEvent::Artifact {
            id: "research-r".to_owned(),
            kind: "document".to_owned(),
            title: "Dyp research".to_owned(),
            content: "# Rapport".to_owned(),
            version: 1,
            provenance: Some(Box::new(crate::deep_research::ResearchProvenance::sample())),
            editor: None,
        };
        let payload = event.payload("r");
        assert_eq!(payload["provenance"]["pipeline"], "deep_research_v1");

        let recorded = RecordedArtifact::from_event(&event).expect("a text artifact");
        assert!(recorded.provenance.is_some());
        assert_eq!(recorded.clone().into_event(), event, "the staged publish streams the same frame");
        let stripped = recorded.without_provenance();
        let value = serde_json::to_value(&stripped).expect("serializable");
        assert!(value.get("provenance").is_none());

        let plain = ChatEvent::Artifact {
            id: "a".to_owned(),
            kind: "document".to_owned(),
            title: "T".to_owned(),
            content: "c".to_owned(),
            version: 1,
            provenance: None,
            editor: None,
        };
        assert!(plain.payload("r").get("provenance").is_none());
    }

    /// EP-2-S6: the claim ledger streams on the report's stamp and leaves with it (D12).
    #[test]
    fn research_ledger_streams_on_the_stamp_and_is_stripped_with_it() {
        let event = ChatEvent::Artifact {
            id: "research-r".to_owned(),
            kind: "document".to_owned(),
            title: "Dyp research".to_owned(),
            content: "# Rapport".to_owned(),
            version: 1,
            provenance: Some(Box::new(crate::deep_research::ResearchProvenance::sample_with_ledger())),
            editor: None,
        };
        let payload = event.payload("r");
        assert_eq!(payload["provenance"]["passes"], 2);
        assert_eq!(payload["provenance"]["research_ledger"]["version"], 1);
        assert_eq!(payload["provenance"]["research_ledger"]["claims"][0]["status"], "single_source");
        assert_eq!(payload["provenance"]["claims_by_status"]["single_source"], 1);
        assert!(payload["provenance"]["ledger_digest"].is_string());

        let recorded = RecordedArtifact::from_event(&event).expect("a text artifact");
        let stored = serde_json::to_value(&recorded).expect("serializable");
        assert_eq!(stored["provenance"]["research_ledger"]["pass_count"], 2);
        let stripped = serde_json::to_value(recorded.without_provenance()).expect("serializable");
        assert!(stripped.get("provenance").is_none(), "no ledger without its stamp");
    }

    #[test]
    fn artifact_event_omits_editor_when_none() {
        let plain = ChatEvent::Artifact {
            id: "a".to_owned(),
            kind: "document".to_owned(),
            title: "T".to_owned(),
            content: "c".to_owned(),
            version: 3,
            provenance: None,
            editor: None,
        };
        assert!(plain.payload("r").get("editor").is_none(), "older clients see the frame unchanged");
        assert_eq!(plain.clone().with_editor(None), plain);

        let attributed = plain.clone().with_editor(Some("model"));
        assert_eq!(attributed.payload("r")["editor"], "model");
        assert_eq!(attributed.payload("r")["version"], 3);
        let recorded = RecordedArtifact::from_event(&attributed).expect("a text artifact");
        assert!(
            serde_json::to_value(&recorded).expect("serializable").get("editor").is_none(),
            "the message metadata never carries the editor"
        );

        let other = ChatEvent::Error { code: "x".into(), message: "y".into(), retryable: false };
        assert!(other.clone().with_editor(Some("model")).payload("r").get("editor").is_none());
    }

    /// A deck as the `deck` tool keeps it: created, one slide, canonical JSON.
    fn deck_manifest_json(deck_id: &str) -> String {
        use mp_contracts::deck_manifest::{DeckManifest, DeckOp, SlideInput, SlideLayout};
        let mut deck = DeckManifest::empty(deck_id);
        deck.apply(DeckOp::Create {
            title: "Kvartalstall".to_owned(),
            locale: "nb-NO".to_owned(),
            slides: vec![SlideInput {
                layout: SlideLayout::TitleBullets,
                title: "Salg".to_owned(),
                bullets: vec!["Opp 8 %".to_owned()],
                left: Vec::new(),
                right: Vec::new(),
                image: None,
                notes: Some("Kilde: Q3-rapporten".to_owned()),
            }],
        })
        .expect("a valid create");
        deck.canonical_json()
    }

    fn deck_event(id: &str, content: String, version: u32) -> ChatEvent {
        ChatEvent::Artifact {
            id: id.to_owned(),
            kind: "deck".to_owned(),
            title: "Kvartalstall".to_owned(),
            content,
            version,
            provenance: None,
            editor: None,
        }
    }

    /// EP-5 S3: a deck's manifest is kept with the message, with its version, so a reload and a
    /// later turn can continue it.
    #[test]
    fn recorded_artifact_keeps_a_deck_manifest() {
        let manifest = deck_manifest_json("deck-01test");
        let recorded = RecordedArtifact::from_event(&deck_event("deck-01test", manifest.clone(), 3))
            .expect("a deck manifest is kept");
        assert_eq!(recorded.kind, "deck");
        assert_eq!(recorded.version, 3);
        assert_eq!(recorded.content, manifest);
        let stored = serde_json::to_value(&recorded).expect("serializable");
        assert_eq!(stored["kind"], "deck");
        assert_eq!(stored["version"], 3);
    }

    /// A deck that would not read back is never kept: over the manifest cap, not a manifest, a
    /// `data:` URI, or a manifest of another deck.
    #[test]
    fn recorded_artifact_drops_a_deck_over_the_cap() {
        let manifest = deck_manifest_json("deck-01test");
        let padded = format!(
            "{manifest}{}",
            " ".repeat(mp_contracts::deck_manifest::MAX_MANIFEST_CHARS)
        );
        assert!(padded.chars().count() < RecordedArtifact::MAX_PERSISTED_CHARS, "the deck's own cap decides");
        assert!(RecordedArtifact::from_event(&deck_event("deck-01test", padded, 1)).is_none());
        assert!(RecordedArtifact::from_event(&deck_event("deck-01test", "# not json".to_owned(), 1)).is_none());
        assert!(RecordedArtifact::from_event(&deck_event("deck-01test", "{\"version\":1}".to_owned(), 1)).is_none());
        assert!(RecordedArtifact::from_event(&deck_event(
            "deck-01test",
            "data:application/json;base64,e30=".to_owned(),
            1
        ))
        .is_none());
        assert!(
            RecordedArtifact::from_event(&deck_event("deck-other", manifest, 1)).is_none(),
            "a manifest is kept only under its own deck id"
        );
    }
}
