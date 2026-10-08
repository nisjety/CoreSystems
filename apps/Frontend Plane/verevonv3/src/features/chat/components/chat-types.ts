import {
  type DashboardComposerSubmitPayload,
} from '@/features/dashboard/home/DashboardComposer'
import {
  type AutonomyRung,
  type ChatAction,
  type ChatCostBasis,
  type ChatTurnTiming,
  type ChatVerificationEvent,
  type RecalledMemory,
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

/** One revision of an artifact, as emitted by a single `artifact` SSE event. */
export type ChatArtifactVersion = {
  content: string
  title: string
  version: number
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
   */
  origin?: 'web' | 'knowledge'
}

export type GeneratedFile = {
  id: string
  name: string
  mime: string
  size: number
  url: string
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
 *  - `none`: sending the same turn again cannot help, so no retry is offered.
 */
export type StreamFailureAction = 'retry' | 'reconnect' | 'wait' | 'none'

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
  streaming: boolean
  tools: ComposerToolId[]
  attachments: ChatTurnAttachment[]
  status?: 'waiting' | 'stopped' | 'error'
  model?: string
  /** Explicit route retained so retry/edit/regenerate cannot leave a user-owned subscription. */
  provider?: string
  /** Opaque Integration Core connection id paired with a subscription route. */
  subscriptionConnectionId?: string
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
  /** Server-derived effect evidence from the durable run proof bundle. */
  effectClass?: ChatEffectClass
  /**
   * Per-turn retrieval-source filter this turn was SENT with (chat-wave2
   * scope spec). Captured at send time, like `planMode` above — a picker
   * changed afterward must not retroactively relabel a finished turn. Also
   * read back on history playback (`ChatMessage.retrievalScope`).
   */
  retrievalScope?: import('@/shared/actions/chat-source-scope').RetrievalScope
  /**
   * The server's own confirmation of `retrievalScope` (`source_scope_applied`
   * SSE event) — the concrete "shown in the receipt" surface. Absent until
   * the event arrives, and never emitted at all for a turn whose effective
   * scope was `auto`/omitted. `excludedTools` names the builtin tool classes
   * this turn's offered set did NOT include because of the filter.
   */
  scopeApplied?: { scope: 'knowledge_only' | 'web_only'; excludedTools: string[] }
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
 * is written by the Model Plane at enqueue time, on the thread itself. This is
 * only the live status of a delivery in flight, which is meaningless after a
 * reload — by then it either arrived (and is in the transcript) or it did not.
 */
export type QueuedInput = {
  id: string
  content: string
  /**
   * `pending` — accepted, not yet handed to the agent.
   * `delivered` — the agent has it (the `queued_input` SSE event arrived).
   * `refused` — it was not accepted, and `note` says why. Never silent.
   */
  state: 'pending' | 'delivered' | 'refused'
  note?: string
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

export type StreamAttachment = {
  data_base64: string
  kind: 'image'
  mime_type: string
}

export type SendOptions = {
  sourceScope?: import('@/shared/actions/chat-source-scope').ChatSourceScope
  /** See `ChatInvokeRequest.retrievalScope`. Per-turn, unlike `sourceScope`. */
  retrievalScope?: import('@/shared/actions/chat-source-scope').RetrievalScope
  actions?: ChatAction[]
  /** Explicit provider route for a connected user-owned model subscription. */
  provider?: string
  /** Opaque Integration Core connection id; never a ChatGPT token. */
  subscriptionConnectionId?: string
  /** Reasoning effort from the composer's response-mode selector. */
  effort?: 'quick' | 'deep'
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
  createdAt?: string
  displayAttachments?: ChatTurnAttachment[]
  generateImage?: boolean
  appendUser?: boolean
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
