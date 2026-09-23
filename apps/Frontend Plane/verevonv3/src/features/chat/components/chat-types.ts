import {
  type DashboardComposerSubmitPayload,
} from '@/features/dashboard/home/DashboardComposer'
import {
  type AutonomyRung,
  type ChatAction,
  type ChatVerificationEvent,
  type RecalledMemory,
} from '@/shared/api/chat-client'
import {
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
  sourceGroupId?: string
  start?: number
  end?: number
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
   * AI-generated follow-up question suggestions (composer chips), from the
   * `follow_ups` SSE event. Session-only by design — not persisted into the
   * thread transcript/history, so a reloaded thread simply shows none rather
   * than stale suggestions for an answer the user has moved past.
   */
  followUps?: string[]
  /** Server-derived effect evidence from the durable run proof bundle. */
  effectClass?: ChatEffectClass
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
