import {
  type DashboardComposerSubmitPayload,
} from '@/features/dashboard/home/DashboardComposer'
import {
  type AgentModeId,
  type ArtifactEditorKind,
  type AutonomyRung,
  type ChatAction,
  type ChatCostBasis,
  type ChatCostByCategory,
  type ChatTurnTiming,
  type ChatUsagePhase,
  type ChatUsageProgressEvent,
  type ChatVerificationEvent,
  type CitationLocator,
  type EffortLevel,
  type EffortProfile,
  type RecalledMemory,
  type ResearchProvenance,
} from '@/shared/api/chat-client'
import {
  type AnswerServedBy,
  type PrivacyTier,
} from '@/shared/api/privacy-tier'
import {
  type Approval,
} from '@/shared/api/orchestration-client'
import {
  Briefcase,
  Code2,
  GraduationCap,
  Laptop,
  Palette,
  PenLine,
  Presentation,
  WandSparkles,
  type LucideProps,
} from '@/shared/icons'
import type { JSX } from '@solidjs/web'
import type { ChatEffectClass } from '@/shared/chat/effect-class'

export type ComposerToolId = DashboardComposerSubmitPayload['tools'][number]
export type ComposerAttachment = DashboardComposerSubmitPayload['attachments'][number]
/**
 * A user attachment as shown in the live transcript. `previewUrl` is an
 * in-memory data URL materialized before the composer revokes its object URL;
 * it is intentionally stripped by `turnsToTranscript` and never persisted.
 */
export type ChatTurnAttachment = ComposerAttachment & { previewUrl?: string }
export type ChatTab = 'chat' | 'sources' | 'artifacts' | 'steps' | 'trace'
export type TaskStepStatus = 'done' | 'active' | 'waiting' | 'error' | 'stopped'
/** A cancel request a Stop sent that the server did not confirm. */
export type ChatCancelFailure = {
  /** `invocation` is the live stream's cancel; `run` is the durable run cancel. */
  target: 'invocation' | 'run'
  /** HTTP status, when the request got an answer at all. */
  status?: number
  code?: string
  at: string
}
export type IconComponent = (props: LucideProps) => JSX.Element

/** One revision of an artifact, as emitted by a single `artifact` SSE event or read from the server's history. */
export type ChatArtifactVersion = {
  content: string
  title: string
  version: number
  /** Absent for a revision the server has not attributed (history off, or an older frame). */
  editorKind?: ArtifactEditorKind
  /** For a restore, the version whose text it brought back. */
  restoredFromVersion?: number
}

export type ChatArtifact = {
  id: string
  kind: string
  content: string
  title: string
  version: number
  /**
   * Every revision received for this `id`, oldest → newest. The backend can
   * re-emit the same `id` with a higher `version` when the model rewrites the
   * artifact (canvas iteration); the top-level `content`/`title`/`version`
   * always mirror the NEWEST revision so existing readers need no change, and
   * this list is what the panel's `‹ v2/3 ›` stepper walks.
   *
   * Optional because artifacts restored from a thread snapshot written before
   * versioning existed carry none — `artifactVersions()` in chat-artifacts.ts
   * then treats the current fields as a single revision.
   */
  history?: ChatArtifactVersion[]
  /** A research report's provenance stamp (EP-2-S4), in the wire shape. */
  provenance?: ResearchProvenance
  /** Who made the newest revision, when the frame named it (EP-3 S3). */
  editor?: ArtifactEditorKind
}

export type ChatToolCall = {
  id: string
  name: string
  args?: unknown
  status?: string
  output?: string
  error?: string
}

export type Citation = {
  id: string
  title: string
  url: string
  snippet: string
  /**
   * The `[n]` the server numbered this source with. A marker binds by this, never by array position: the
   * numbering has gaps, because a source that was not read takes no number. Absent from an older gateway and
   * from a stored turn written before numbers were kept.
   */
  number?: number
  /** Optional server-issued claim binding. The current gateway may omit it;
   * the UI must never infer these values from prose. */
  claimId?: string
  /**
   * Machine-stable grouping key (chat-wave2 citations spec §2.2/§2.6):
   * `kb:{provider}` for a knowledge-base source, `web_domain(url)`-shaped for
   * a web/deep-research one. Absent from a not-yet-updated gateway — group
   * by `hostname(url)` in that case (`InlineCitationMarker`,
   * `collectEvidenceSources`), never leave a citation ungrouped.
   */
  sourceGroupId?: string
  /**
   * Human, already-localized label for `sourceGroupId` (currently only set
   * for a knowledge-base source, e.g. "Microsoft 365"). Absent for a web/
   * deep-research citation, which renders its hostname as the label instead.
   */
  sourceGroupLabel?: string
  /**
   * The marker's own UTF-8 BYTE offset range within the complete answer
   * text (Rust `regex::Match::start`/`end` — NOT a JS string index: a
   * Norwegian character like æ/ø/å is 2 UTF-8 bytes but 1 UTF-16 code unit,
   * so these numbers do not line up with a JS `string.slice()` call), set
   * once by the server's post-stream anchor pass alongside `claimId`.
   * INFORMATIONAL ONLY — never use these to slice or highlight the answer:
   * chip placement is still the client's own regex match over already-split
   * markdown blocks, whose own start offset within the full answer text is
   * not tracked, so `start`/`end` cannot be resolved against a block
   * substring without recomputing a block-relative offset first (and, even
   * then, without first converting from a UTF-8 byte offset to a UTF-16
   * index).
   */
  start?: number
  end?: number
  /**
   * Source-scope provenance (chat-wave2 scope spec §2.2): `'web'` for a
   * web_search/deep-research/fetch_url source, `'knowledge'` for a
   * knowledge-base or org-connected-search source. Always set by an updated
   * gateway; absent from a not-yet-updated one. Drives the "Kilder: kun
   * organisasjonskunnskap" / "Kilder: kun nett" receipt badge and lets the
   * Kilder tab defensively flag a citation whose family disagrees with the
   * turn's active `retrievalScope`.
   *
   * `'connector'` (WS23 S6) is a mail message, file or calendar item from one
   * of the person's connected accounts. The gateway sends its title and a
   * metadata line only, never body text, so the card shows those and nothing more.
   */
  origin?: 'web' | 'knowledge' | 'connector'
  /** The passage this source contributed (EP-2-S4), when the server sent one. */
  quote?: string
  /** Where a knowledge source's passage sits in Data Plane, in the wire shape. */
  locator?: CitationLocator
}

export type GeneratedFileCheckLevel = 'passed' | 'warnings' | 'failed' | 'skipped'

/**
 * The gateway's structural check of a generated Office or PDF file: what the
 * check found in the file's container. It never says the file opens in Office,
 * and no surface may say it does. A `skipped` check has no verdict.
 */
export type GeneratedFileCheck = {
  level: GeneratedFileCheckLevel
  checks: { id: string; level: GeneratedFileCheckLevel; message: string }[]
}

/**
 * The render of a generated Office file for the artifact panel: its pages as a
 * PDF, with thumbnails of the first pages. It shows the file; it is never the
 * download, and it is not kept with the thread.
 */
export type GeneratedFilePreview = {
  mime: 'application/pdf'
  /** A `data:` URI of the rendered PDF. */
  url: string
  pages: number
  /** True when a font the file names was drawn with a stand-in, so the viewer says so. */
  fontsSubstituted: boolean
  /** `data:` URIs of the first pages, in page order. */
  thumbs: string[]
}

export type GeneratedFile = {
  id: string
  name: string
  mime: string
  size: number
  url: string
  /** Present only when `code_interpreter` ran the structural check on this file. */
  check?: GeneratedFileCheck
  /** Present only when the render layer made a preview of this Office file. */
  preview?: GeneratedFilePreview
}

export type GeneratedImagePreview = {
  id: string
  title: string
  src: string
  downloadName: string
  size: number
  artifactId?: string
}

export type ArtifactPanelItem = {
  artifact: ChatArtifact
  file?: GeneratedFile
  turn: ChatTurn
}

export type ChatGroundingSource = {
  id: string
  kind: 'knowledge'
  title: string
  snippet: string
  provider: string
  sourceType: string
  documentId: string
  href: string
  score: number
}

export type ChatGroundingFact = {
  knowledgeId: string
  documentId: string
  text: string
  score: number
  sourceTitle: string
  sourceType: string
  provider: string
  chunkIndex: number
}

export type ChatGroundingGraphNode = {
  id: string
  label: string
  kind: string
}

export type ChatGroundingGraph = {
  traceId?: string
  communitySummaries: string[]
  edgeCount: number
  nodes: ChatGroundingGraphNode[]
}

export type ChatKnowledgeGrounding = {
  mode: 'retrieve' | 'hybrid'
  query: string
  traceId?: string
  lowConfidence: boolean
  factCount: number
  sourceCount: number
  facts: ChatGroundingFact[]
  sources: ChatGroundingSource[]
  graph?: ChatGroundingGraph
}

/**
 * Below this answer-confidence score, the chat UI must surface a visible
 * caveat on the message bubble itself rather than leaving it discoverable
 * only in the Reasoning/Steps popover.
 *
 * Reference point (Model Plane `confidence.rs`): a clean, ungrounded answer
 * scores exactly `BASE = 0.72` — confident-sounding text with no hedging and
 * no knowledge-base citations. A genuinely knowledge-base-grounded answer
 * gets `GROUNDED_BONUS = 0.15` on top, landing around `0.87`. Setting the
 * threshold at `0.75` — just above the ungrounded baseline — means any
 * answer that is fluent but NOT grounded in this org's knowledge base gets
 * flagged, while a real grounded answer does not.
 *
 * Incident (2026-07-20): "tell me about coresystem what do they do and sell"
 * returned a confidently wrong, uncited answer scored exactly 0.72; that
 * score was computed but only ever visible by opening the Reasoning popover
 * and clicking into its "Oversikt" tab — never in the actual chat bubble.
 */
export const LOW_CONFIDENCE_ANSWER_THRESHOLD = 0.75

/**
 * What the reader can do about a failed turn.
 *  - `retry`: try again now (the default when absent).
 *  - `reconnect`: renew the ChatGPT sign-in first, then try again.
 *  - `wait`: the refusing service accepts calls again at a known instant; the
 *    retry control stays disabled until then and never fires by itself.
 *  - `open_integrations`: review the connection the turn used (a zero-retention
 *    refusal: the reader's own ChatGPT connection), then try again.
 *  - `none`: sending the same turn again cannot help, so no retry is offered
 *    (a model or a setting has to change first).
 */
export type StreamFailureAction = 'retry' | 'reconnect' | 'wait' | 'open_integrations' | 'none'

/**
 * The running cost and usage of one turn or agent run (WS27-S4): the latest `usage_progress` frame,
 * replaced by the turn's `usage` frame. Counts and closed tokens only; every key is optional because
 * a frame carries only what it has. Session-only: never written to the thread transcript, so a reload
 * shows the turn's own usage fields and no breakdown.
 */
export type RunMeter = {
  /** Model calls the total covers so far. */
  calls?: number
  /** The provider call the latest progress frame reports. */
  phase?: ChatUsagePhase
  inputTokens?: number
  outputTokens?: number
  cacheReadTokens?: number
  cacheWriteTokens?: number
  /** The metered cost. Never set on a subscription turn, whose wire 0.0 is a placeholder. */
  costUsd?: number
  costBasis?: ChatCostBasis
  /** Of `calls`, the metered ones whose price was unknown. A total with any has no `costUsd`. */
  unpricedCalls?: number
  /** The cost by what was billed. Only the `usage` frame states it, and only for a priced total. */
  costByCategory?: ChatCostByCategory
  /** The run's caps and the share of them used (WS27-S6). */
  ceilingUsd?: number
  ceilingTokens?: number
  fraction?: number
  warning?: NonNullable<ChatUsageProgressEvent['warning']>
  /** How the budget moved the turn's route (WS27-S7), once a call states it. */
  routing?: NonNullable<ChatUsageProgressEvent['routing']>
}

/**
 * A run lifecycle notice on one turn (WS15): something the gateway did to the turn or to a tool call while it
 * ran, from the `retry_scheduled`, `compaction`, `context_change` and `retrieval_notice` frames. Only the
 * gateway's counts and closed tokens are kept, never text. Session-only: `turnsToTranscript` does not copy it,
 * so nothing is stored locally or on the server, and a reload shows none.
 */
export type LifecycleNotice =
  /** `callId` is the tool call the retry was matched to when it arrived, if one was out. */
  | { kind: 'retry'; tool?: string; attempt: number; maxAttempts: number; reason: string; callId?: string }
  | { kind: 'compaction'; scope: string; outcome: string }
  | { kind: 'context'; change: string; messagesDropped?: number }
  /** `wordingKey` is the gateway's catalogued key for why the answer was reduced. */
  | { kind: 'retrieval'; wordingKey: string }

/** The most notices one turn keeps. The oldest go first. */
export const LIFECYCLE_NOTICE_CAP = 20

/** The providers a connected-app suggestion may name: mp-contracts `connector_suggest`'s closed list. */
export const CONNECTOR_SUGGESTION_PROVIDERS = ['google', 'microsoft'] as const
export type ConnectorSuggestionProvider = (typeof CONNECTOR_SUGGESTION_PROVIDERS)[number]

/**
 * One connected-app suggestion (`connector_suggestion`, EP-10 S8). The reason is the gateway's scrubbed line,
 * rendered as text. `restored` marks a suggestion read back from the cache after a reload: the moment it
 * answered has passed, so the card shows it as a note and offers nothing.
 */
export type ChatConnectorSuggestion = {
  suggestionId: string
  provider: ConnectorSuggestionProvider
  reason: string
  requestId?: string
  restored?: boolean
}

/**
 * `notices` with `notice` appended. An exact repeat of a notice the turn already holds is not added again: a
 * resumed stream replays what the turn saw. The list keeps its newest `LIFECYCLE_NOTICE_CAP` entries.
 */
export function addLifecycleNotice(
  notices: readonly LifecycleNotice[] | undefined,
  notice: LifecycleNotice,
): LifecycleNotice[] {
  const held = notices ?? []
  const key = JSON.stringify(notice)
  if (held.some((item) => JSON.stringify(item) === key)) return [...held]
  return [...held, notice].slice(-LIFECYCLE_NOTICE_CAP)
}

/**
 * One conversation turn as the chat renders it.
 *
 * Every field is named. Server message metadata is never spread onto a turn:
 * keys such as `head_summary` (the gateway's compaction summary of earlier
 * turns, persisted on an assistant message) are model context, not something
 * to render, and have no field here on purpose.
 */
export type ChatTurn = {
  sourceScope?: import('@/shared/actions/chat-source-scope').ChatSourceScope
  id: string
  role: 'user' | 'assistant'
  content: string
  createdAt: string
  /** The server's message-tree position for this turn, on a versioned thread (WS17 S9). Absent everywhere else. */
  version?: import('@/shared/api/chat-client').ChatMessageVersion
  /**
   * When this client saw the turn's terminal frame (done, stopped or error), as an ISO time. Session-only: the
   * transcript record does not keep it, so a reloaded turn has none and its fold row shows counts only (EP-14 S1).
   */
  settledAt?: string
  streaming: boolean
  tools: ComposerToolId[]
  attachments: ChatTurnAttachment[]
  status?: 'waiting' | 'stopped' | 'error'
  model?: string
  /** Explicit route retained so retry/edit/regenerate cannot leave a user-owned subscription. */
  provider?: string
  /** Opaque Integration Core connection id paired with a subscription route. */
  subscriptionConnectionId?: string
  /** Opaque Integration Core id of the customer gateway this turn is served on (W4-BYO-6). Kept so a retry stays on it. */
  modelConnectionId?: string
  requestId?: string
  /**
   * Highest SSE frame `id:` seen for this turn, sent as `Last-Event-ID` on
   * resume so the server replays only what we missed.
   *
   * Without it a reconnect replays the stream from the beginning — which is why
   * the resume path used to clear the cached partial answer on its first
   * replayed delta. Not persisted in the thread snapshot: a cursor is only
   * meaningful against a buffer that is still alive (10-minute TTL), and a stale
   * one would skip frames a fresh resume needs.
   */
  lastFrameId?: string
  modelUsed?: string
  inputTokens?: number
  outputTokens?: number
  latencyMs?: number
  costUsd?: number
  /**
   * How `costUsd` was reached. On a subscription-carried turn the gateway
   * computes no list-price cost and `costUsd` is left unset here, so no surface
   * can render the wire's compatibility 0.0 as a real "$0".
   */
  costBasis?: ChatCostBasis
  /**
   * Where the answer was processed, as the gateway stamped it on `usage`
   * (serving provider, declared residency, privacy tier). Absent when the
   * gateway did not stamp one, and then nothing is rendered. Session-only:
   * not written to the thread snapshot, so a reload shows no line rather than
   * a stale or reconstructed one.
   */
  servedBy?: AnswerServedBy
  /**
   * Where the turn's server time went (`turn_timing`). Absent when the gateway
   * did not report it; the metrics rows that read it are then not shown at all.
   * A restored transcript keeps only the three totals.
   */
  timing?: ChatTurnTiming
  /**
   * The turn's running cost and usage (see `RunMeter`), for the Work panel and the agent console.
   * Session-only: `turnsToTranscript` does not copy it.
   */
  meter?: RunMeter
  /**
   * Answer text that streamed live and was then NOT saved: the server ended
   * the turn with an error that says so (`session_liveness_failed`, for one,
   * can arrive after the whole answer streamed). `content` then carries the
   * explanation, so the turn cannot read as a saved answer, and this keeps the
   * text reachable behind an explicit "not saved" disclosure. Session-only:
   * `turnsToTranscript` drops it, so a reload never resurrects it.
   */
  unsavedContent?: string
  /**
   * What the reader can do about this failed turn, from the server's failure
   * code (see `streamFailurePresentation`). Absent means an ordinary retry.
   * Session-only, like `unsavedContent`: `turnsToTranscript` drops it.
   */
  failureAction?: StreamFailureAction
  /**
   * With `failureAction: 'wait'`, the instant (ISO) the refusing service
   * accepts calls again, such as the ChatGPT plan's usage-limit reset.
   * Session-only: `turnsToTranscript` drops it.
   */
  retryAt?: string
  /**
   * The gateway's publication receipt (`result_receipt`) for a checked answer
   * or document. Session-only: the durable copy lives on the saved assistant
   * message, and this is only what the live stream reported.
   */
  resultReceipt?: Record<string, unknown>
  /**
   * Cancel requests a Stop sent for this turn that the server did not confirm.
   * Metadata only — status and code, never content — so a Stop the server may
   * not have honoured is on record while the reconciliation that follows it
   * decides what the turn really became.
   */
  cancelFailures?: ChatCancelFailure[]
  confidence?: number
  /**
   * What the backend's verification pass found after scoring this answer low.
   * Absent when the pass did not run — which is itself the point: a reader can
   * otherwise not tell "we checked and found nothing" from "we never looked",
   * and those justify very different trust in the same number.
   */
  verification?: ChatVerificationEvent
  reasoning?: string
  citations?: Citation[]
  toolCalls?: ChatToolCall[]
  artifacts?: ChatArtifact[]
  files?: GeneratedFile[]
  grounding?: ChatKnowledgeGrounding
  /**
   * How many long-term memories were injected into this turn's prompt, from
   * the `memory_recall` SSE event. Absent means none were — the backend emits
   * the event only when memory genuinely contributed, so there is no
   * "recalled 0" state to render.
   */
  memoryRecallCount?: number
  /**
   * WHICH memories this turn recalled, with their provenance.
   *
   * The count alone tells a reader that memory was used; this tells them what
   * was used, so a wrong remembered fact can actually be found and corrected.
   * Persisted alongside the count so the record survives a reload — a notice
   * that disappears on reopen reads as though the recall never happened.
   */
  recalledMemories?: RecalledMemory[]
  /**
   * The run lifecycle notices this turn has had, in arrival order, capped (see `LifecycleNotice`). Session-only:
   * `turnsToTranscript` drops it. Absent means none.
   */
  lifecycle?: LifecycleNotice[]
  /**
   * Why generation stopped, when the backend reported it. `'stream_incomplete'`
   * means the provider connection broke before any proper termination signal,
   * so the answer may be cut off mid-thought — rendered as an honest notice
   * rather than left to look like a complete reply.
   */
  stopReason?: string
  /**
   * Durable Model run id. Captured from the `connected` SSE event (and, as a
   * fallback, from a `paused` step). Approval cards use it for plan turns;
   * the live Work panel only subscribes when the turn is plan/research so
   * ordinary Ask runs do not make the base chat feel like an IDE.
   */
  runId?: string
  /** Pending human-approval requests gating this agentic run's next tool. */
  pendingApprovals?: Approval[]
  /**
   * This turn ran in plan mode — it described what it would do rather than
   * doing it.
   *
   * Captured at SEND time, not read from the composer toggle later: a toggle
   * flipped afterwards must not retroactively change what a finished turn was.
   * Drives the plan-approval control, which is the only thing that grants a run
   * the authority to execute.
   */
  planMode?: boolean
  /**
   * The agent mode this turn was sent under, when the person chose one (WS25-S11). Captured at send time, as
   * `planMode` is, so a later picker change does not relabel a turn that already ran.
   */
  agentMode?: AgentModeId
  /**
   * This turn ran the plan the person had approved (WS14 S2a): an EXECUTING run
   * under the grant the Model Plane holds for the thread, not a planning one.
   *
   * Session-only, like the armed state that produced it: set by `sendContent`
   * when it sent the turn as an execute turn, never read back from server
   * metadata and not part of the local thread record. It makes the Work surface
   * treat the turn as a run (steps, run id, effect class) even though
   * `planMode` is false.
   */
  executeGranted?: boolean
  /**
   * The rung a person granted this run, once they approved its plan. Absent
   * means no grant has been made — never "granted everything".
   */
  grantedRung?: AutonomyRung
  /**
   * The model's structured plan (plan-gate §2.1), from the `plan_proposed`
   * SSE event — populated by `onPlanProposed` alongside the existing
   * `onFollowUps` wiring. Absent while the model is still deciding (a
   * finished plan-mode turn with no proposed plan yet derives no
   * `plan-approval` node — see `chat-nodes/derive.ts`). A second
   * `plan.propose` call in the same turn REPLACES this (last-proposed-wins,
   * owner ruling 2026-09-27), it does not append.
   */
  proposedPlan?: {
    planId: string
    summary: string
    steps: import('@/shared/api/chat-client').ChatPlanStep[]
  }
  /**
   * The person rejected this turn's proposed plan (plan-gate §2.3), and the
   * server recorded it. Session-only, like `proposedPlan`: the server is the
   * durable record and refuses any further decision on a rejected plan, so
   * this only exists so the card can say what happened instead of sitting
   * there unchanged with an Approve button that would now be refused.
   */
  planRejected?: boolean
  /**
   * AI-generated follow-up question suggestions (composer chips), from the
   * `follow_ups` SSE event. Session-only by design — not persisted into the
   * thread transcript/history, so a reloaded thread simply shows none rather
   * than stale suggestions for an answer the user has moved past.
   */
  followUps?: string[]
  /**
   * The connected-app suggestion this turn's model made (`connector_suggestion`, EP-10 S8): at most one
   * per turn. Kept with the turn's cached metadata so a reloaded thread shows it as a resolved note.
   */
  connectorSuggestion?: ChatConnectorSuggestion
  /** Server-derived effect evidence from the durable run proof bundle. */
  effectClass?: ChatEffectClass
  /**
   * Per-turn retrieval-source filter this turn was SENT with (chat-wave2
   * scope spec). Captured at send time, like `planMode` above — a picker
   * changed afterward must not retroactively relabel a finished turn. Also
   * read back on history playback (`ChatMessage.retrievalScope`).
   */
  retrievalScope?: import('@/shared/actions/chat-source-scope').RetrievalScope
  /** The context chips this turn was sent with (EP-3 S5): kind, id and label only. */
  contextRefs?: import('@/shared/actions/chat-context-refs').ChatContextRef[]
  /**
   * The deep-research plan this assistant turn ended with (EP-2 S5), read back from the persisted
   * `research_plan` on a reload. Shown as a read-only card once a later turn exists; never set while
   * the live plan card is still open.
   */
  researchPlan?: import('@/shared/api/chat-client').ChatResearchPlanEvent
  /**
   * The server's own confirmation of `retrievalScope` (`source_scope_applied`
   * SSE event) — the concrete "shown in the receipt" surface. Absent until
   * the event arrives. A turn whose scope was `auto`/omitted gets one only
   * when a Space scoped it, and then its scope is `auto` with a `knowledge`
   * receipt (WS18-S3). `excludedTools` names the builtin tool classes this
   * turn's offered set did NOT include because of the filter.
   */
  scopeApplied?: import('@/shared/api/chat-client').ChatSourceScopeAppliedEvent
  /**
   * The search providers that served this turn's web searches, and the route its entitlement allowed
   * (`search_provenance` SSE event, EP-9 W1). Live only: nothing stores it, so a reloaded turn has none.
   */
  searchProvenance?: import('@/shared/api/chat-client').ChatSearchProvenanceEvent
}

export type AgentTaskStep = {
  id: string
  title: string
  detail: string
  status: TaskStepStatus
  createdAt: string
  expandedDetail?: string
  evidence?: AgentTaskStepEvidence[]
  turnId?: string
  turnTitle?: string
}

export type AgentTaskStepEvidence = {
  id: string
  label: string
  value: string
  href?: string
}

export type AgentTaskStepSection = {
  id: string
  title: string
  createdAt: string
  steps: AgentTaskStep[]
}

export type ChatStatus = 'idle' | 'streaming' | 'error'

/**
 * A message the user typed while a run was streaming.
 *
 * Session-only and deliberately not persisted: the durable record of the message
 * is written by the Model Plane on the thread itself (before it is delivered,
 * with send receipts on). This is only the live status of a delivery in flight,
 * which is meaningless after a reload — by then it either arrived (and is in the
 * transcript) or it did not.
 */
export type QueuedInput = {
  id: string
  content: string
  /**
   * `pending` — accepted, not yet handed to the agent.
   * `delivered` — the agent has it (the `queued_input` SSE event arrived).
   * `resent` — the run ended without taking it, so it went out again as an
   * ordinary turn under the same `clientMessageId`: the transcript holds it once.
   * `refused` — it was not accepted, and `note` says why. Never silent.
   */
  state: 'pending' | 'delivered' | 'resent' | 'refused'
  note?: string
  /**
   * The id the send was named with. The gateway stores the message under it and
   * names it in the delivery event, so a delivery is matched to its message by id
   * (text is the fallback for a gateway that sends no ids).
   */
  clientMessageId?: string
  /**
   * The gateway accepted the message (and, with receipts on, stored it). Only an
   * accepted message that the run never took is resent when the run ends: one
   * that was refused or never reached the gateway is not.
   */
  accepted?: boolean
}

export type ChatState = {
  turns: ChatTurn[]
  taskSteps: AgentTaskStep[]
  status: ChatStatus
  error: string | null
  requestId: string | null
  threadId: string | null
  activeModel: string
  branchCount: number
  /** Mid-run messages for the CURRENT run, cleared when a new turn starts. */
  queuedInputs: QueuedInput[]
}

export type StreamAttachment =
  | {
      data_base64: string
      kind: 'image'
      mime_type: string
    }
  /**
   * A document's own bytes, for the code sandbox to read (read-only, under
   * `attachments/`). Sent only when the server declares `modes.sandbox_files`;
   * the document's text is inlined into the turn as before, so a server that
   * stages nothing loses nothing.
   */
  | {
      data_base64: string
      kind: 'file'
      mime_type: string
      name: string
    }

/**
 * The client's pointer to the plan grant the Model Plane holds for a thread
 * (WS14 S2a), set when the person approves a plan.
 *
 * Session-only: never written to local storage, a thread snapshot or the
 * server, and it carries no authority. The gateway's grant is the authority and
 * decides whether an execute turn runs; this only remembers that one was given,
 * for which thread and rung, so the composer can offer the "run the approved
 * plan" pill and the controller can refuse to send an execute turn on a thread
 * the grant was not made for.
 */
export type ThreadPlanGrant = {
  threadId: string
  rung: AutonomyRung
  /** The structured plan that was approved, when the turn proposed one. */
  planId?: string
  /** The plan card's turn: where a refusal or an expiry is explained. */
  turnId: string
  approvedAtMs: number
  /** When the gateway's grant lapses, when it said so; absent for an older gateway. */
  expiresAtMs?: number
}

export type SendOptions = {
  sourceScope?: import('@/shared/actions/chat-source-scope').ChatSourceScope
  /** See `ChatInvokeRequest.retrievalScope`. Per-turn, unlike `sourceScope`. */
  retrievalScope?: import('@/shared/actions/chat-source-scope').RetrievalScope
  /** See `ChatInvokeRequest.connectorScope`. Per-turn, like `retrievalScope`. */
  connectorScope?: string[]
  /** See `ChatInvokeRequest.contextRefs`. Per-turn, like `connectorScope`. */
  contextRefs?: import('@/shared/actions/chat-context-refs').ChatContextRef[]
  /**
   * The brand deck template this turn applies (EP-5 S7): a skill id from `listDeckTemplates`. See
   * `ChatInvokeRequest.deckTemplate`; per-turn, like `contextRefs`, and never sent on a zero-retention turn.
   */
  deckTemplate?: string
  /**
   * This send is not the composer's text (a deck template pick from the deck view, EP-5 S7), so the composer keeps its
   * draft. Like the approved-plan turn and a wide-research retry, which are never typed in the composer either.
   */
  keepDraft?: boolean
  /** See `ChatInvokeRequest.agentMode`. Per-turn, like `connectorScope`. */
  agentMode?: AgentModeId
  actions?: ChatAction[]
  /** Explicit provider route for a connected user-owned model subscription. */
  provider?: string
  /** Opaque Integration Core connection id; never a ChatGPT token. */
  subscriptionConnectionId?: string
  /** Opaque Integration Core id of a customer gateway (W4-BYO-6). Text-only, never zero retention. */
  modelConnectionId?: string
  /** Reasoning effort from the composer's response-mode selector. */
  effort?: EffortProfile | EffortLevel
  /**
   * Response-style dial from composer settings — how much prose the answer
   * spends, independent of `effort`'s thinking budget.
   */
  tone?: 'concise' | 'detailed'

  attachments?: StreamAttachment[]
  /**
   * What the MODEL receives as the user turn, when it differs from what the
   * user typed: the typed text followed by the full text of small attached
   * documents. The transcript keeps showing the typed text (with the
   * attachment chips); the model gets the content in the same turn instead of
   * waiting for asynchronous indexing that finishes after the answer
   * (RUN-LOG finding 1). Absent means "send the typed text as-is".
   */
  modelContent?: string
  browseWeb?: boolean
  deepResearch?: boolean
  /**
   * Wide research from the composer's "Bred forskning" entry (EP-2 S8/S9): the choice a start sends, or a one-item
   * retry. Set only by `startWideResearch` and `retryWideResearchItem` on an agentic Do turn; see
   * `ChatInvokeRequest.wideResearch` for what the wire carries.
   */
  wideResearch?: import('@/shared/api/chat-client').WideResearchStart
  createdAt?: string
  displayAttachments?: ChatTurnAttachment[]
  generateImage?: boolean
  appendUser?: boolean
  /**
   * The id this send is named with (the receipt the transcript dedupes on). Minted
   * by `sendContent` for an ordinary send; carried here only by a send that
   * repeats an earlier one, a queued message the run never took, so both land as
   * one transcript row. Never set for a zero-retention turn, a regenerate or an
   * edited resubmit.
   */
  clientMessageId?: string
  /**
   * This send is a queued message going out again as an ordinary turn. Its
   * mid-run strip entry stays on screen, as "resent", instead of being cleared
   * with the previous run's strip.
   */
  resendOfQueued?: boolean
  tools?: ComposerToolId[]
  /**
   * Temporary chat (Zero Data Retention): the request opts the whole turn
   * out of persistence server-side, and the thread this turn belongs to is
   * marked temporary client-side too (see `isTemporaryThread` in
   * use-chat-controller.ts) — no history entry, no transcript cache, no
   * server snapshot, no title/follow-up generation.
   */
  zdr?: boolean
  /**
   * This send is a REGENERATE of the previous answer, or an EDITED resubmit of
   * the previous question.
   *
   * Declared here because only the client knows: both arrive at the server as an
   * ordinary turn carrying (near-)identical text. model-gateway feeds them to
   * the implicit-dissatisfaction classifier, which is what turns a regenerate
   * click into a weak negative signal against the skills that served the answer
   * being replaced. Without these the two strongest behavioural signals the
   * classifier defines are unreachable.
   */
  regenerated?: boolean
  editResubmit?: boolean
  /**
   * A REQUEST that this send runs the approved plan. Set only by the composer
   * submit, from the armed state, and re-validated by `sendContent`, which is
   * the authority boundary: a send that fails any check goes out as an ordinary
   * turn. Regenerate, edit-resubmit, continue-generation and retry never set it.
   */
  executeGranted?: boolean
  /**
   * Minimum privacy tier requested for this turn (Venice-style tiering).
   * Only set when the user explicitly picked a tiered catalog model; omitted
   * otherwise, so unspecified stays byte-identical on the wire.
   */
  minPrivacyTier?: PrivacyTier
  /**
   * "Fortsett" (continue-after-stop): pre-fills the new assistant turn's
   * DISPLAYED content with this text instead of starting from `''`, so the
   * partial answer a stopped turn already produced is never lost — streamed
   * deltas simply append after it, exactly as they would after an empty turn.
   * Absent for every other send. See `continueGeneration` in
   * use-chat-controller.ts, the only caller.
   */
  seedContent?: string
}

export type EvidenceSource = (Citation & { kind: 'web' }) | ChatGroundingSource

export type MarkdownListItem = {
  /** Nesting level derived from leading indentation (0 = top level). */
  depth: number
  /** Marker family of this item; nested items may differ from the block's. */
  ordered: boolean
  text: string
  /**
   * GFM task-list state ("- [ ]" / "- [x]"), undefined for an ordinary item.
   * `text` has already had the "[ ]"/"[x]" marker stripped off the front.
   */
  checked?: boolean
}

export type MarkdownTableAlign = 'center' | 'left' | 'right' | null

/**
 * One line of a (possibly nested) blockquote, mirroring `MarkdownListItem`'s
 * depth field so nested quotes (">" then ">>", or "> >") can recurse the same
 * way nested lists do instead of collapsing to a single level.
 */
export type MarkdownQuoteLine = {
  /** Nesting level derived from the count of leading '>' markers (1 = top level). */
  depth: number
  text: string
}

export type MarkdownBlock =
  | {
      kind: 'code'
      lang: string
      text: string
      /**
       * False when the fence never closed before the source ran out (an
       * in-progress fence mid-stream). A ```mermaid``` block only ever goes
       * through the actual diagram renderer once `closed`; while streaming
       * it renders as a plain code block, same as any other language, so an
       * incomplete/invalid partial diagram never reaches `mermaid.render`.
       */
      closed: boolean
    }
  // Raw `<details>`/`<summary>` HTML written by the model (e.g. "Se alle
  // tilbud" behind a shipping-quote table). Rendered as a real collapsible
  // instead of escaped tag soup; `blocks` is the parsed inner markdown.
  | { kind: 'details'; summary: string; blocks: MarkdownBlock[] }
  | { kind: 'heading'; level: 1 | 2 | 3; text: string }
  | { kind: 'hr' }
  | {
      kind: 'list'
      ordered: boolean
      items: MarkdownListItem[]
      /**
       * The integer parsed off the FIRST item's marker when `ordered`
       * (e.g. "391." -> 391) so a leading number is preserved as `<ol
       * start>` instead of every list silently renumbering from 1.
       */
      startNumber?: number
    }
  // Block ("display") math, `$$...$$`. `closed` mirrors the code fence's
  // field: false while the closing `$$` hasn't arrived yet (mid-stream), so
  // the renderer shows the raw source instead of feeding a half-written
  // LaTeX expression to KaTeX.
  | { kind: 'math'; text: string; closed: boolean }
  | { kind: 'paragraph'; text: string }
  | { kind: 'quote'; lines: MarkdownQuoteLine[] }
  | { kind: 'table'; align: MarkdownTableAlign[]; header: string[]; rows: string[][] }

// ── Prompt chips ──────────────────────────────────────────────────────────────

export const PRIMARY_PROMPTS = [
  { label: 'Create slides', prompt: 'Create a concise slide outline for a customer support leadership update.', icon: Presentation },
  { label: 'Build website', prompt: 'Build a focused website plan for a high-converting support automation page.', icon: Code2 },
  { label: 'Develop apps', prompt: 'Plan a desktop app workflow for agents managing customer conversations.', icon: Laptop },
  { label: 'Design', prompt: 'Design a refined support workflow with clear states, handoffs, and escalation paths.', icon: Palette },
  { label: 'Write', prompt: 'Write a polished customer reply that is concise, helpful, and on-brand.', icon: PenLine },
] as const

export const OVERFLOW_PROMPTS = [
  { label: 'Learn', prompt: 'Teach me the most important concepts behind customer support automation.', icon: GraduationCap },
  { label: 'Code', prompt: 'Help me implement a clean customer support automation feature with tests.', icon: Code2 },
  { label: 'Career chat', prompt: 'Help me prepare for a career conversation about customer experience leadership.', icon: Briefcase },
  { label: "Verevon's choice", prompt: 'Choose the highest-impact next task for improving our support operations.', icon: WandSparkles },
] as const

export const TOOL_LABELS: Record<ComposerToolId, string> = {
  image: 'Create image',
  research: 'Deep research',
  search: 'Search',
}

export const PROSE_ARTIFACT_KINDS = new Set(['markdown', 'md', 'doc', 'text', 'report', 'prose'])
export const CHAT_BROWSE_WEB_KEY = 'verevon.chat.browseWeb.v1'
