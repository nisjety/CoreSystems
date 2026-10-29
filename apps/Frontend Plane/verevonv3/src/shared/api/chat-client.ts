import { ApiError, requestJson } from './http'
import { readSseStream, type SseEvent } from './sse'
import {
  isSelectablePrivacyTier,
  normalizePrivacyTier,
  type PrivacyTier,
} from './privacy-tier'
import { createSelectedAgentToolSpecs } from '@/shared/actions/agent-tools'
import { isSupportChatThread } from '@/shared/chat/support-chat-thread'
import {
  applyServerRetention,
  forgetThreadLocally,
  reconcileLocalThreads,
} from '@/features/chat/lib/chat-retention'

// ── Wire contract ───────────────────────────────────────────────────────────
// The gateway forwards the chat body verbatim to model-gateway `/v1/invoke/*`,
// whose `InvokeRequest` is snake_case with `tools: ToolSpec[]`. This client owns
// the logical→wire translation so callers stay ergonomic (camelCase + intent).

export type ChatToolSpec = {
  name: string
  description?: string
  /** JSON Schema for the tool's parameters, as a JSON string. */
  parametersJson?: string
}

/** A specialized action activated via the composer "/" menu. */
export type ChatAction = {
  id: string
  name: string
  kind: 'skill' | 'capability' | 'connector' | 'tool'
}

export type ChatAttachment = {
  data_base64?: string
  kind?: string
  mime_type?: string
  url?: string
}

export type ChatInvokeRequest = {
  content: string
  model?: string
  threadId?: string
  /** The requested personal Space for a new durable chat thread. The BFF
   * exchanges this selection for a Control-signed, effect-bound decision. */
  spaceRef?: string
  /**
   * The Control `subject_id` of a Space agent addressed by `@` mention this
   * turn (`docs/space-defenition.md`, "Invocation rule"). The gateway is the
   * one that decides whether this agent may actually answer — it re-resolves
   * Control membership and the Application binding before injecting a
   * persona; a mention naming an unbound or inactive agent is rejected, never
   * silently ignored or silently granted. Meaningless without `spaceRef`.
   */
  mentionedAgentRef?: string
  sessionKey?: string
  features?: string[]
  generateImage?: boolean
  browseWeb?: boolean
  /** Deep research mode: gateway runs plan -> concurrent searches -> page reads -> a cited report. */
  deepResearch?: boolean
  /**
   * Extended-thinking effort: `quick` | `standard` | `deep`.
   *
   * The gateway derives the token budget from this profile — the client picks an
   * effort, never a raw budget. `standard` (and unset) requests no thinking, so
   * the ordinary turn is unchanged; only `deep`/`quick` cost the extra latency
   * and tokens. Reasoning arrives on `reasoning_delta`, which requires the
   * `reasoning` feature (already in `DEFAULT_FEATURES`).
   */
  effort?: 'quick' | 'standard' | 'deep'
  attachments?: ChatAttachment[]
  /** Explicit tool definitions (advanced); usually derived from actions/browseWeb. */
  tools?: ChatToolSpec[]
  /** Skills/capabilities/connectors activated in the composer. */
  actions?: ChatAction[]
  profile?: string
  zdr?: boolean
  /**
   * Plan mode — when true, opt into the Model Plane's agentic run path so the
   * agent plans + executes tools under human-approval gates (risky tools pause
   * for Approve/Reject in chat). Adds the `agentic` feature family.
   */
  planMode?: boolean
  /**
   * This turn is a REGENERATE of the previous answer, not a new question.
   *
   * Client-declared because the server cannot infer it: a regenerate arrives as
   * an ordinary turn carrying the same text. model-gateway feeds it to the
   * implicit-dissatisfaction classifier, where it is deliberately WEAK evidence
   * — a regenerate often just means "give me another style" — so asserting it
   * can only nudge one skill's score, never condemn it.
   */
  regenerated?: boolean
  /** This turn is an EDITED resubmit of the previous question. Same reasoning. */
  editResubmit?: boolean
  /**
   * Minimum Venice-style privacy tier this turn may be served by
   * (`min_privacy_tier` on the wire). Enforcement is server-side: ineligible
   * providers are skipped and an empty remainder fails closed — never a silent
   * downgrade. OMITTED from the wire unless explicitly set: unspecified means
   * today's behavior byte-identically.
   */
  minPrivacyTier?: PrivacyTier
}

// ── SSE events (mapped from model-gateway's real event names) ────────────────

export type ChatConnectedEvent = {
  ok?: boolean
  requestId?: string
  threadId?: string
  model?: string
  /** Present on agentic runs — the orchestration run id to stream console events for. */
  runId?: string
}
export type ChatMessageEvent = { content: string; requestId?: string }
export type ChatDoneEvent = {
  requestId?: string
  modelUsed?: string
  outputTokens?: number
  /**
   * Why generation stopped, when the backend reported it. `'stream_incomplete'`
   * means the provider connection broke before any proper termination signal —
   * the answer may be truncated. See model_plane.v1.InferChunk.stop_reason.
   */
  stopReason?: string
}
/**
 * The run was stopped before finishing. Distinct from `onDone` on purpose: a
 * server-side stop used to be routed to `onDone`, which rendered a halted
 * answer as a normally-completed one.
 */
export type ChatStoppedEvent = { requestId?: string; reason?: string }
/**
 * Long-term memory was injected into this turn's prompt (Model Plane's
 * `memory_recall` event, `memory` feature family). Emitted at most once per
 * turn and only when memory was genuinely recalled, so `count` is never 0.
 */
/**
 * How a recalled memory came to exist.
 *
 * THREE states, not a boolean. `unrecorded` covers rows written before
 * provenance was tracked, and the backend contract is explicit that those must
 * never be presented as `stated` — saying "you told me this" about a row that
 * does not record it manufactures consent. Any value this build does not
 * recognise is read as `unrecorded` for the same reason.
 */
export type MemoryOrigin = 'stated' | 'inferred' | 'unrecorded'

/** Whether the memory came from another conversation or is org-level context. */
export type MemoryRole = 'recall' | 'inject'

export type RecalledMemory = {
  /** Stable id, so the reader can act on this exact row. */
  memoryId: string
  role: MemoryRole
  origin: MemoryOrigin
  label: string
  preview: string
}

export type ChatMemoryRecallEvent = {
  count: number
  latencyMs?: number
  /** What was recalled. Empty when the backend reported only a count. */
  memories: RecalledMemory[]
}

/** Mid-run user messages the agent has just been handed. */
export type ChatQueuedInputEvent = { messages: string[] }
export type ChatErrorEvent = {
  code: string
  message: string
  retryable?: boolean
}
export type ChatArtifactEvent = {
  id?: string
  kind?: string
  title?: string
  content?: string
  version?: number
}
export type ChatReasoningEvent = { delta: string }
export type ChatToolCallEvent = { id?: string; name?: string; args?: unknown }
export type ChatToolResultEvent = {
  id?: string
  output?: string
  error?: string
  status?: string
}
export type ChatCitationEvent = {
  id?: string
  title?: string
  url?: string
  snippet?: string
}
export type ChatGroundingEvent = { value: unknown }
export type ChatStepEvent = {
  id?: string
  title?: string
  detail?: string
  status?: string
}
export type ChatAttachmentEvent = {
  id?: string
  name?: string
  mime?: string
  type?: string
  url?: string
  size?: number
}
export type ChatUsageEvent = {
  inputTokens?: number
  outputTokens?: number
  costUsd?: number
  latencyMs?: number
  confidence?: number
}
/**
 * AI-generated thread title, emitted once by model-gateway after a thread's
 * FIRST exchange completes (before the terminal `done`). Already sanitized
 * server-side: single line, no quotes/emoji, ≤64 chars.
 */
export type ChatTitleEvent = { title: string; requestId?: string }
/**
 * AI-generated follow-up question suggestions, emitted by model-gateway after
 * (almost) every non-ZDR exchange completes (before the terminal `done`).
 * Already sanitized server-side: 0-3 short questions, no quotes/emoji/
 * numbering. Never emitted for a ZDR turn.
 */
export type ChatFollowUpsEvent = { suggestions: string[]; requestId?: string }

/**
 * Read the recalled-memory list off the wire.
 *
 * Unknown `origin`/`role` values degrade rather than being dropped — the
 * backend's vocabulary may already be wider than this build's, and a row the
 * reader cannot see is worse than one labelled conservatively. `origin`
 * specifically degrades to `unrecorded`, never to `stated`: reading an
 * unrecognised value as "you told me this" is the one mistake that misleads.
 */
function normalizeRecalledMemories(raw: unknown): RecalledMemory[] {
  if (!Array.isArray(raw)) return []
  return raw.flatMap((entry): RecalledMemory[] => {
    if (!entry || typeof entry !== 'object') return []
    const record = entry as Record<string, unknown>
    const memoryId =
      typeof record.memory_id === 'string' ? record.memory_id : ''
    const preview = typeof record.preview === 'string' ? record.preview : ''
    // Without an id the row cannot be acted on, and without a preview there is
    // nothing to read; either way it earns no line.
    if (!memoryId || !preview.trim()) return []
    const origin = record.origin
    const role = record.role
    return [
      {
        memoryId,
        role: role === 'inject' ? 'inject' : 'recall',
        origin:
          origin === 'stated' || origin === 'inferred' ? origin : 'unrecorded',
        label:
          typeof record.label === 'string' && record.label
            ? record.label
            : 'Minne',
        preview,
      },
    ]
  })
}

export type ChatStreamHandlers = {
  onConnected?: (event: ChatConnectedEvent) => void
  onMessage?: (event: ChatMessageEvent) => void
  onDone?: (event: ChatDoneEvent) => void
  onError?: (event: ChatErrorEvent) => void
  onArtifact?: (event: ChatArtifactEvent) => void
  onReasoning?: (event: ChatReasoningEvent) => void
  onCitation?: (event: ChatCitationEvent) => void
  onGrounding?: (event: ChatGroundingEvent) => void
  onStep?: (event: ChatStepEvent) => void
  onToolCall?: (event: ChatToolCallEvent) => void
  onToolResult?: (event: ChatToolResultEvent) => void
  onAttachment?: (event: ChatAttachmentEvent) => void
  onUsage?: (event: ChatUsageEvent) => void
  onTitle?: (event: ChatTitleEvent) => void
  onFollowUps?: (event: ChatFollowUpsEvent) => void
  onStopped?: (event: ChatStoppedEvent) => void
  onMemoryRecall?: (event: ChatMemoryRecallEvent) => void
  /**
   * A message the user sent mid-run has reached the agent. Emitted at delivery
   * rather than at enqueue: the POST already confirmed acceptance, and what the
   * client cannot otherwise know is when the agent actually saw it.
   */
  onQueuedInput?: (event: ChatQueuedInputEvent) => void
  /**
   * Any SSE event this client has no case for. The gateway relays upstream
   * events verbatim with no allowlist, so a new backend event reaches the
   * browser and — before this hook — died here silently. Log, never drop.
   */
  onUnknownEvent?: (event: {
    name: string
    payload: Record<string, unknown>
  }) => void
  /**
   * The SSE `id:` of every frame that carries one, in arrival order.
   *
   * Resume needs this. The server buffers each frame under a monotonic seq and
   * replays those with `seq > Last-Event-ID`, so without tracking the highest id
   * we saw, a reconnect can only replay the stream **from the beginning** — which
   * is what it did: the controller dropped the cached partial answer on the
   * first replayed delta precisely because it knew everything was coming again.
   */
  onFrameId?: (id: string) => void
}

export type ChatMessage = {
  id: string
  role: 'user' | 'assistant'
  content: string
  model?: string
  createdAt: string
}

export type ChatThreadSession = {
  preview: string
  threadId: string
  title: string
  updatedAt: string
  /**
   * Whether the user pinned this thread. Server-owned so a pin follows the user
   * across devices, and defaults to false for an index written before pins
   * existed.
   */
  pinned: boolean
  /** Non-secret Space routing reference for a scoped thread. The BFF still
   * resolves fresh Control authority before an append. */
  spaceRef?: string
}

export type ChatThreadTranscriptSnapshot = {
  taskSteps?: unknown[]
  threadId: string
  turns: unknown[]
  updatedAt: string
}

export type SaveChatThreadSnapshotRequest = {
  preview?: string
  taskSteps?: unknown[]
  title?: string
  turns?: unknown[]
  updatedAt?: string
  /**
   * OMIT to leave the pin untouched. The SPA saves a snapshot on every turn to
   * refresh title/preview, so sending `false` by default would silently unpin
   * the thread on the user's next message.
   */
  pinned?: boolean
}

export type ModelModality = 'chat' | 'image' | 'video' | 'audio' | 'embedding' | 'other'

// ── Verevon intent modes ──────────────────────────────────────────────────────
// Three first-class pseudo-models. They are NOT catalog entries: the backend
// (inference-core) resolves each id SERVER-SIDE via its intent layer (complexity
// + budget → a concrete model, with model-router as the cheap fallback). The
// frontend only offers them and sends the chosen id as `model`. They are always
// pinned at the top of the picker, independent of what `/v1/models` returns.

export type VerevonModeBadge = 'cheap' | 'premium'

export type VerevonMode = {
  id: string
  /** Display label, e.g. "Verevon Balance". */
  label: string
  /** Short one-liner shown under the label in the picker. */
  description: string
  /** Which cost badge to show next to the row. */
  badge: VerevonModeBadge
}

/** Verevon Balance is the default selection (backend treats `''` as balance too). */
export const VEREVON_BALANCE_MODE_ID = 'verevon-balance'

/** The pinned "Verevon" group, authoritative + always shown, most-default first. */
export const VEREVON_MODES: readonly VerevonMode[] = [
  {
    id: 'verevon-budget',
    label: 'Verevon Budget',
    description: 'Cheapest, fast',
    badge: 'cheap',
  },
  {
    id: VEREVON_BALANCE_MODE_ID,
    label: 'Verevon Balance',
    description: 'Smart balance of cost & quality',
    badge: 'cheap',
  },
  {
    id: 'verevon-genius',
    label: 'Verevon Genius',
    description: 'Most capable, premium',
    badge: 'premium',
  },
]

const VEREVON_MODE_IDS: ReadonlySet<string> = new Set(VEREVON_MODES.map((mode) => mode.id))

/** True when the id is one of the pinned Verevon intent modes. */
export function isVerevonModeId(id: string): boolean {
  return VEREVON_MODE_IDS.has(id)
}

/** Look up a Verevon mode by id (for label/description/badge rendering). */
export function verevonModeById(id: string): VerevonMode | undefined {
  return VEREVON_MODES.find((mode) => mode.id === id)
}

export type ModelInfo = {
  id: string
  name: string
  capabilities?: string[]
  /** Upstream provider, e.g. 'openai', 'anthropic', 'deepseek', 'microsoft'. */
  provider?: string
  /** Primary modality of the model. Used to filter chat-only choices. */
  modality?: ModelModality
  /** Whether the model is a cheap / cost-efficient option (cost-aware default). */
  cheap?: boolean
  /** Relative cost tier hint, when the backend reports it. */
  costTier?: 'low' | 'medium' | 'high'
  /**
   * Venice-style programmatic privacy tier for this model's provider
   * (`privacy_tier` on `/v1/models`). `undefined` when absent/unknown — the
   * UI then renders no claim at all rather than guessing one.
   */
  privacyTier?: PrivacyTier
  /** Data-residency region the backend reports alongside the tier (`residency`). */
  residency?: string
}

// Opt-in rich SSE families the model-gateway understands (chat-parity §2).
// `memory` opts this client in to the `memory_recall` visibility event. The
// memory INJECTION itself is unconditional server-side (prompt quality is not
// a client choice) — this only asks to be TOLD when it happened, so the UI can
// show "recalled N memories" instead of silently benefiting from it.
const DEFAULT_FEATURES = [
  'usage',
  'citations',
  'reasoning',
  'steps',
  'artifacts',
  'memory',
]
function str(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined
}

function strOrJson(value: unknown): string | undefined {
  if (typeof value === 'string') return value
  if (value == null) return undefined
  try {
    return JSON.stringify(value)
  } catch {
    return undefined
  }
}

function num(value: unknown): number | undefined {
  return typeof value === 'number' ? value : undefined
}

function objectValue(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : undefined
}

function emitCitation(value: unknown, handlers: ChatStreamHandlers): void {
  const payload = objectValue(value)
  if (!payload) return
  handlers.onCitation?.({
    id: str(payload.id),
    title: str(payload.title),
    url: str(payload.url) ?? str(payload.href),
    snippet: str(payload.snippet) ?? str(payload.description),
  })
}

type WireToolSpec = { name: string; description: string; parameters_json: string }

function buildToolSpecs(request: ChatInvokeRequest): WireToolSpec[] {
  return createSelectedAgentToolSpecs({
    browseWeb: request.browseWeb,
    actions: request.actions,
    explicitTools: request.tools,
  }).map((tool) => ({
    name: tool.name,
    description: tool.description,
    parameters_json: tool.parametersJson,
  }))
}

/**
 * Keep Global Chat workspace-aware without turning every message into a
 * support query. The gateway re-checks the authenticated organization and
 * removes this control field before forwarding to Model Plane.
 */
export function shouldRequestSupportContext(content: string): boolean {
  const query = content.trim().toLocaleLowerCase()
  if (!query) return false
  return [
    /\bsla\b/,
    /\bsupport (ticket|conversation|issue)/,
    /\bticket(s)? at risk\b/,
    /\bunresolved (customer )?issues?\b/,
    /\b(recurring|shipping) problem\b/,
    /\b(follow[- ]?up|needs follow[- ]?up)\b/,
    /\b(relevant|knowledge) (support |knowledge )?(source|article|link|document)s?\b/,
    /\b(find|relevant).{0,80}\b(source|sources|article|articles|knowledge)\b/,
    /\b(støtte|saker|samtaler|oppfølging|kunnskapsbase)\b/,
  ].some((pattern) => pattern.test(query))
}

export function buildChatWireBody(request: ChatInvokeRequest): Record<string, unknown> {
  const threadId = request.threadId?.trim() || undefined
  const supportReadOnly = isSupportChatThread(threadId)
  const tools = supportReadOnly ? [] : buildToolSpecs(request)
  const features = new Set(request.features ?? DEFAULT_FEATURES)
  // Request the tools family by default, even with zero client-declared specs.
  // Support-derived threads are the exception because their durable history
  // contains customer-authored transcript text and must remain read-only.
  // model-gateway merges its own builtins (tool_loop::builtin_tool_defs —
  // fetch_url, knowledge_search) into the tool list whenever this feature is
  // present, regardless of what the client sent (sse.rs). Without it, a plain
  // chat turn never gets knowledge_search attached, so the model can't ground
  // answers in the org's own ingested knowledge base unless the user happens
  // to also toggle Browse or an action first. web_search stays gated behind
  // its own explicit-Search-toggle check server-side, so this does not grant
  // unrestricted web access — only the safe, always-useful builtins turn on.
  if (supportReadOnly) features.delete('tools')
  else features.add('tools')
  // Plan mode → agentic run path (orchestration-backed, supports approval gates
  // + run pause/resume). Without it the gateway uses the direct tool loop, which
  // never pauses for human approval.
  if (request.planMode && !supportReadOnly) features.add('agentic')
  else features.delete('agentic')

  const supportContextQuery = !supportReadOnly && shouldRequestSupportContext(request.content)
    ? request.content.trim().slice(0, 240)
    : undefined

  return {
    content: request.content,
    model: request.model,
    profile: request.profile ?? 'chat',
    thread_id: threadId,
    session_key: request.sessionKey?.trim() || threadId,
    space_ref: request.spaceRef?.trim() || undefined,
    mentioned_agent_ref: request.mentionedAgentRef?.trim() || undefined,
    browse_web: supportReadOnly ? false : request.browseWeb ?? false,
    generate_image: supportReadOnly ? false : request.generateImage ?? false,
    // Sent as a real field, not just as the `agentic` feature above: the
    // gateway needs it to mark the run itself (in-memory plan-mode store +
    // session-core's durable `run.mode`). Without this the toggle only widened
    // the feature set and nothing server-side could tell a planning run from an
    // executing one.
    plan_mode: supportReadOnly ? false : request.planMode ?? false,
    // Deep research is its OWN field, not just `browse_web`. "Dyp research"
    // used to set only browseWeb, so it was indistinguishable from a plain
    // Search turn — the same failure planMode had. The gateway keys the
    // multi-round research pipeline off this flag.
    deep_research: supportReadOnly ? false : (request.deepResearch ?? false),
    // Omitted entirely when unset or `standard`: the gateway treats an absent
    // effort as "no thinking", and sending `standard` explicitly would be a
    // no-op field on every ordinary turn.
    ...(request.effort && request.effort !== 'standard'
      ? { effort: request.effort }
      : {}),
    attachments: request.attachments ?? [],
    ...(supportContextQuery ? { support_context_query: supportContextQuery } : {}),
    features: [...features],
    tools,
    zdr: request.zdr ?? false,
    // Snake_case on the wire; model-gateway also accepts the camelCase aliases
    // (http_routes.rs `alias = "regenerate"` / `alias = "editResubmit"`), but
    // matching the rest of this body keeps one convention.
    regenerated: request.regenerated ?? false,
    edited_resubmit: request.editResubmit ?? false,
    // Tier selection is opt-in ONLY: omitted unless the user explicitly picked
    // a tier, so a default send stays byte-identical to today's behavior
    // (UNSPECIFIED ⇒ no constraint server-side). Same pattern as
    // `support_context_query` above. `isSelectablePrivacyTier` (not mere
    // truthiness) keeps a literal `'unspecified'` OFF the wire too.
    ...(isSelectablePrivacyTier(request.minPrivacyTier)
      ? { min_privacy_tier: request.minPrivacyTier }
      : {}),
  }
}

function dispatchEvent(event: SseEvent, handlers: ChatStreamHandlers): void {
  if (!event.data) return
  let payload: Record<string, unknown>
  try {
    payload = JSON.parse(event.data) as Record<string, unknown>
  } catch {
    return
  }

  switch (event.event) {
    case 'connected':
      handlers.onConnected?.({
        ok: typeof payload.ok === 'boolean' ? payload.ok : undefined,
        requestId: str(payload.request_id),
        threadId: str(payload.thread_id),
        model: str(payload.model_used) ?? str(payload.model),
        runId: str(payload.run_id),
      })
      break
    case 'chunk': {
      const delta = str(payload.delta) ?? str(payload.content) ?? ''
      if (delta) handlers.onMessage?.({ content: delta, requestId: str(payload.request_id) })
      break
    }
    case 'done':
      handlers.onDone?.({
        requestId: str(payload.request_id),
        modelUsed: str(payload.model_used) ?? str(payload.modelUsed),
        outputTokens: num(payload.output_tokens) ?? num(payload.outputTokens),
        stopReason: str(payload.stop_reason) ?? str(payload.stopReason),
      })
      break
    case 'stopped':
      // Routed to its OWN handler. Previously this called `onDone`, so a
      // server-side stop (cancelled upstream, budget exhausted) rendered as a
      // normally-completed answer and the user had no way to tell.
      // `onStopped` falls back to `onDone` only for callers that predate it,
      // preserving their existing behaviour rather than silently going quiet.
      if (handlers.onStopped) {
        handlers.onStopped({
          requestId: str(payload.request_id),
          reason: str(payload.reason),
        })
      } else {
        handlers.onDone?.({ requestId: str(payload.request_id) })
      }
      break
    case 'error':
      handlers.onError?.({
        code: str(payload.code) ?? 'error',
        message: str(payload.message) ?? 'Stream error',
        retryable: typeof payload.retryable === 'boolean' ? payload.retryable : undefined,
      })
      break
    case 'artifact':
      handlers.onArtifact?.({
        id: str(payload.id),
        kind: str(payload.kind),
        title: str(payload.title),
        content: str(payload.content),
        version: num(payload.version),
      })
      break
    case 'attachment':
      handlers.onAttachment?.({
        id: str(payload.id),
        name: str(payload.name),
        mime: str(payload.mime),
        type: str(payload.type),
        url: str(payload.url),
        size: num(payload.size),
      })
      break
    case 'reasoning_delta':
      handlers.onReasoning?.({ delta: str(payload.delta) ?? '' })
      break
    case 'tool_call':
      handlers.onToolCall?.({
        id: str(payload.id) ?? str(payload.tool_call_id) ?? str(payload.call_id),
        name: str(payload.name) ?? str(payload.tool) ?? str(payload.function_name),
        args: payload.args ?? payload.arguments,
      })
      break
    case 'tool_result':
      handlers.onToolResult?.({
        id: str(payload.id) ?? str(payload.tool_call_id) ?? str(payload.call_id),
        output: strOrJson(payload.output) ?? strOrJson(payload.result) ?? strOrJson(payload.data),
        error: strOrJson(payload.error),
        status: str(payload.status),
      })
      break
    case 'citation':
      emitCitation(payload, handlers)
      break
    case 'citations':
    case 'search_results': {
      const citations = Array.isArray(payload.citations)
        ? payload.citations
        : Array.isArray(payload.results)
          ? payload.results
          : []
      for (const citation of citations) emitCitation(citation, handlers)
      break
    }
    case 'grounding':
      handlers.onGrounding?.({ value: payload.grounding ?? payload })
      break
    case 'step_update':
      handlers.onStep?.({
        id: str(payload.id) ?? str(payload.step_id),
        title: str(payload.title) ?? str(payload.name),
        detail: str(payload.detail) ?? str(payload.message),
        status: str(payload.status),
      })
      break
    case 'usage':
      handlers.onUsage?.({
        inputTokens: num(payload.input_tokens),
        outputTokens: num(payload.output_tokens),
        costUsd: num(payload.cost_usd),
        latencyMs: num(payload.latency_ms),
        confidence: num(payload.confidence),
      })
      break
    case 'title': {
      const title = str(payload.title)?.trim()
      if (title) handlers.onTitle?.({ title, requestId: str(payload.request_id) })
      break
    }
    case 'follow_ups': {
      const suggestions = Array.isArray(payload.suggestions)
        ? payload.suggestions.filter((item): item is string => typeof item === 'string' && item.trim().length > 0)
        : []
      if (suggestions.length > 0) {
        handlers.onFollowUps?.({ suggestions, requestId: str(payload.request_id) })
      }
      break
    }
    case 'memory_recall': {
      const count = num(payload.count)
      // The backend only emits this when memory was genuinely injected, so a
      // 0/absent count means a malformed payload — not "recalled nothing".
      // Dropping it is better than rendering "recalled 0 memories".
      if (count !== undefined && count > 0) {
        handlers.onMemoryRecall?.({
          count,
          latencyMs: num(payload.latency_ms),
          memories: normalizeRecalledMemories(payload.memories),
        })
      }
      break
    }
    case 'queued_input': {
      const messages = Array.isArray(payload.messages)
        ? payload.messages.filter(
            (item): item is string =>
              typeof item === 'string' && item.trim().length > 0,
          )
        : []
      // A delivery with no readable text is a malformed frame, not a delivery.
      // Reporting it would tell the user their message arrived when we cannot
      // show which one.
      if (messages.length > 0) {
        handlers.onQueuedInput?.({ messages })
      }
      break
    }
    default: {
      // STREAM_* envelopes are emitted by some upstreams and are noise here.
      // Everything else is a backend event this client has not learned yet:
      // surface it instead of dropping it, so adding an event server-side is
      // discoverable rather than invisible.
      //
      // `event.event` is optional on the wire type, and an unnamed frame is not
      // a discoverable new event — it is malformed. Reporting it as one would
      // put `undefined` in the log line this hook exists to make useful.
      const name = event.event
      if (name && !name.startsWith('STREAM_')) {
        handlers.onUnknownEvent?.({ name, payload })
      }
      break
    }
  }
}

export async function streamChat(
  request: ChatInvokeRequest,
  handlers: ChatStreamHandlers,
  signal?: AbortSignal,
  lastEventId?: string,
): Promise<void> {
  let connError: unknown

  await readSseStream(
    '/api/v1/chat/stream',
    {
      method: 'POST',
      body: JSON.stringify(buildChatWireBody(request)),
      signal,
      lastEventId,
    },
    (event) => {
      if (event.id) handlers.onFrameId?.(event.id)
      dispatchEvent(event, handlers)
    },
    (err) => {
      connError = err
    },
  )

  if (connError) {
    const msg = connError instanceof Error ? connError.message : 'Connection failed'
    handlers.onError?.({ code: 'connection_error', message: msg })
  }
}

export async function resumeStream(
  requestId: string,
  handlers: ChatStreamHandlers,
  signal?: AbortSignal,
  lastEventId?: string,
): Promise<void> {
  let connError: unknown

  await readSseStream(
    `/api/v1/chat/stream/resume/${encodeURIComponent(requestId)}`,
    { method: 'GET', signal, lastEventId },
    (event) => {
      if (event.id) handlers.onFrameId?.(event.id)
      dispatchEvent(event, handlers)
    },
    (err) => {
      connError = err
    },
  )

  if (connError) {
    const msg = connError instanceof Error ? connError.message : 'Resume failed'
    handlers.onError?.({ code: 'connection_error', message: msg })
  }
}

export async function cancelInvocation(requestId: string): Promise<void> {
  await requestJson(`/api/v1/chat/invocations/${encodeURIComponent(requestId)}/cancel`, {
    method: 'POST',
  })
}

/**
 * How much a run is permitted to do. A strictly ordered ladder — each rung
 * includes everything below it.
 *
 * Mirrors the Model Plane's `AutonomyRung`. Sent as a keyword, not a number: a
 * number that means something else on the other side is a silent mis-grant.
 */
export type AutonomyRung = 'read_only' | 'workspace_write' | 'danger_full_access'

/**
 * Shortest justification the Model Plane accepts.
 *
 * Mirrored here so the UI can say so BEFORE the request. Not zero-plus-one on
 * purpose: 'ok' and 'ja' are non-empty and say nothing, and a person has to read
 * this to decide.
 */
export const MIN_PLAN_JUSTIFICATION_CHARS = 12

export type PlanApprovalResult = {
  grantedRung: AutonomyRung
  /** False when the run was not actually in plan mode — an idempotent re-approve. */
  wasInPlanMode: boolean
  /**
   * False when the grant reached the running agent but was not written to the
   * run. Surfaced rather than swallowed: the next turn would then not carry it,
   * and the agent would refuse work it was just authorized to do.
   */
  persisted: boolean
}

/**
 * Approve a planning run: grant it the authority to execute, at a named rung,
 * with a stated reason.
 *
 * Both are required by the Model Plane — an approval with no reason, or a reason
 * granting nothing, is refused rather than recorded as an empty justification.
 */
export async function approvePlan(
  runId: string,
  grantedRung: AutonomyRung,
  justification: string,
): Promise<PlanApprovalResult> {
  const raw = await requestJson<Record<string, unknown>>(
    `/api/v1/agents/runs/${encodeURIComponent(runId)}/plan-approval`,
    {
      method: 'POST',
      body: JSON.stringify({ granted_rung: grantedRung, justification }),
    },
  )
  return {
    grantedRung: (typeof raw.granted_rung === 'string'
      ? raw.granted_rung
      : grantedRung) as AutonomyRung,
    wasInPlanMode: raw.was_in_plan_mode === true,
    persisted: raw.persisted !== false,
  }
}

/**
 * What happened to a message sent while a run was still streaming.
 *
 * Every arm is distinct because each needs a different response from the UI, and
 * collapsing them is what made the original behaviour invisible: the send path
 * simply returned, and the user's words were gone.
 */
export type QueuedInputResult =
  /** Delivered to the running agent at its next tool-round boundary. */
  | { outcome: 'queued'; pending: number; persisted: boolean }
  /**
   * The run finished before the message landed. Not an error — the caller should
   * send it as an ordinary turn instead, so nothing is lost.
   */
  | { outcome: 'run_ended' }
  /** Refused with a reason worth showing: too many pending, or too long. */
  | {
      outcome: 'refused'
      reason: 'too_many' | 'too_long' | 'other'
      message: string
    }

/**
 * Send a message to a run that is already streaming.
 *
 * `spaceRef` and `threadId` are hints, not authority: the BFF strips every
 * authority field and mints a fresh, content-bound append decision from Control
 * for this exact message, and the Model Plane appends to the thread its own
 * stream registration recorded — never to a thread named here.
 */
export async function queueInvocationInput(
  requestId: string,
  content: string,
  options: { threadId?: string; spaceRef?: string } = {},
): Promise<QueuedInputResult> {
  try {
    const raw = await requestJson<Record<string, unknown>>(
      `/api/v1/chat/invocations/${encodeURIComponent(requestId)}/queue`,
      {
        method: 'POST',
        body: JSON.stringify({
          content,
          ...(options.threadId ? { thread_id: options.threadId } : {}),
          ...(options.spaceRef ? { space_ref: options.spaceRef } : {}),
        }),
      },
    )
    return {
      outcome: 'queued',
      pending: typeof raw.pending === 'number' ? raw.pending : 1,
      // `false` means the run WILL read the message but the thread does not
      // record it. Surfaced rather than swallowed: the reply would otherwise
      // appear in history answering nothing.
      persisted: raw.persisted !== false,
    }
  } catch (error) {
    if (!(error instanceof ApiError)) {
      return {
        outcome: 'refused',
        reason: 'other',
        message:
          error instanceof Error
            ? error.message
            : 'Kunne ikke levere meldingen.',
      }
    }
    // 404 is the ordinary race, not a failure: the stream ended between the
    // keystroke and the request.
    if (error.status === 404) return { outcome: 'run_ended' }
    if (error.status === 429) {
      return { outcome: 'refused', reason: 'too_many', message: error.message }
    }
    if (error.status === 413) {
      return { outcome: 'refused', reason: 'too_long', message: error.message }
    }
    return { outcome: 'refused', reason: 'other', message: error.message }
  }
}

/** One segment of the assembled context window. */
export type ContextSegment = {
  kind: string
  content: string
  estimatedTokens: number
}

/**
 * What is actually in the model's context window for a thread.
 *
 * session-core has itemized this all along and the gateway already called it to
 * BUILD prompts — it was simply never exposed, so the one surface that could
 * answer "why did it answer from that?" was unreachable from the UI.
 */
export type ThreadContext = {
  threadId: string
  /** The assembler's own total — reported, not summed from the segments. */
  estimatedTokens: number
  /** The budget the segments were assembled against. */
  budgetTokens: number
  segments: ContextSegment[]
}

export async function getThreadContext(
  threadId: string,
  runId?: string,
): Promise<ThreadContext> {
  const query = runId?.trim()
    ? `?run_id=${encodeURIComponent(runId.trim())}`
    : ''
  const raw = await requestJson<Record<string, unknown>>(
    `/api/v1/chat/threads/${encodeURIComponent(threadId)}/context${query}`,
  )
  const segments = Array.isArray(raw?.segments) ? raw.segments : []
  return {
    threadId: typeof raw?.thread_id === 'string' ? raw.thread_id : threadId,
    estimatedTokens:
      typeof raw?.estimated_tokens === 'number' ? raw.estimated_tokens : 0,
    budgetTokens:
      typeof raw?.budget_tokens === 'number' ? raw.budget_tokens : 0,
    segments: segments.flatMap((entry): ContextSegment[] => {
      if (!entry || typeof entry !== 'object') return []
      const segment = entry as Record<string, unknown>
      const kind = typeof segment.kind === 'string' ? segment.kind : ''
      // A segment with no kind cannot be labelled, and an unlabelled block of
      // prompt text in an inspector is worse than omitting it.
      if (!kind) return []
      return [
        {
          kind,
          content: typeof segment.content === 'string' ? segment.content : '',
          estimatedTokens:
            typeof segment.estimated_tokens === 'number'
              ? segment.estimated_tokens
              : 0,
        },
      ]
    }),
  }
}

export async function getThreadMessages(
  threadId: string,
): Promise<ChatMessage[]> {
  const raw = await requestJson<unknown>(
    `/api/v1/chat/threads/${encodeURIComponent(threadId)}/messages`,
  )
  return normalizeThreadMessages(raw)
}

/**
 * Apply the server's retention verdict carried on a threads response.
 *
 * Deliberately applied HERE rather than at each call site. `listChatThreads`
 * has three independent callers (the sidebar, the dashboard composer, the chat
 * controller); a policy any one of them can forget to apply is the same class
 * of defect as the client-side ZDR gate this replaces. This is the single point
 * every server answer passes through.
 */
function applyRetentionFrom(raw: unknown): void {
  const record = objectValue(raw)
  const data = objectValue(record?.data) ?? record
  const retention = objectValue(data?.retention)
  const zdr = retention?.zdr
  applyServerRetention(typeof zdr === 'boolean' ? zdr : undefined)
}

export async function listChatThreads(): Promise<ChatThreadSession[]> {
  const raw = await requestJson<unknown>('/api/v1/chat/threads')
  applyRetentionFrom(raw)
  const sessions = normalizeChatThreadSessions(raw)
  // Reached only on a SUCCESSFUL listing — `requestJson` throws otherwise — so
  // an empty array here really means "the server has no threads for you", not
  // "the request failed". That distinction is what makes this safe to act on:
  // a thread the server no longer lists has been erased upstream, and its local
  // copy must go with it. This is the only way a GDPR erasure reaches the
  // device at all.
  reconcileLocalThreads(sessions.map((session) => session.threadId))
  return sessions
}

export async function saveChatThreadSnapshot(
  threadId: string,
  snapshot: SaveChatThreadSnapshotRequest,
): Promise<ChatThreadSession | null> {
  const raw = await requestJson<unknown>(
    `/api/v1/chat/threads/${encodeURIComponent(threadId)}`,
    {
      method: 'PUT',
      body: JSON.stringify(snapshot),
    },
  )
  const record = objectValue(raw)
  const data = objectValue(record?.data) ?? record
  // `retained: false` means the server understood the request and deliberately
  // kept nothing — a ZDR posture it derived server-side (org policy, the
  // thread's own ZDR marker, or this request's own flag). If the server kept
  // nothing, neither may the device.
  if (data?.retained === false) {
    forgetThreadLocally(threadId)
    return null
  }
  return normalizeChatThreadSession(record?.session)
}

export async function getChatThreadTranscript(threadId: string): Promise<ChatThreadTranscriptSnapshot | null> {
  const raw = await requestJson<unknown>(
    `/api/v1/chat/threads/${encodeURIComponent(threadId)}/transcript`,
  )
  const record = objectValue(raw)
  return normalizeChatThreadTranscript(record?.transcript)
}

export async function deleteChatThread(threadId: string): Promise<ChatThreadSession[]> {
  const raw = await requestJson<unknown>(
    `/api/v1/chat/threads/${encodeURIComponent(threadId)}`,
    { method: 'DELETE' },
  )
  applyRetentionFrom(raw)
  return normalizeChatThreadSessions(raw)
}

export async function clearChatThreads(): Promise<void> {
  await requestJson('/api/v1/chat/threads', { method: 'DELETE' })
}

function normalizeThreadMessages(raw: unknown): ChatMessage[] {
  const source = Array.isArray(raw)
    ? raw
    : raw && typeof raw === 'object' && Array.isArray((raw as { messages?: unknown }).messages)
      ? (raw as { messages: unknown[] }).messages
      : []

  return source
    .filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === 'object')
    .map((item, index) => ({
      id: str(item.id) ?? str(item.message_id) ?? `msg-${index}`,
      role: item.role === 'assistant' ? 'assistant' : 'user',
      content: str(item.content) ?? '',
      model: str(item.model) ?? str(item.model_used),
      createdAt: str(item.created_at) ?? str(item.createdAt) ?? '',
    }))
}

function normalizeChatThreadSessions(raw: unknown): ChatThreadSession[] {
  const source = Array.isArray(raw)
    ? raw
    : raw && typeof raw === 'object' && Array.isArray((raw as { sessions?: unknown }).sessions)
      ? (raw as { sessions: unknown[] }).sessions
      : []

  return source
    .map(normalizeChatThreadSession)
    .filter((item): item is ChatThreadSession => Boolean(item))
}

function normalizeChatThreadSession(raw: unknown): ChatThreadSession | null {
  const item = objectValue(raw)
  if (!item) return null
  const threadId = str(item.threadId) ?? str(item.thread_id)
  const title = str(item.title)
  if (!threadId || !title) return null
  return {
    threadId,
    title,
    preview: str(item.preview) ?? '',
    updatedAt: normalizeIsoTimestamp(str(item.updatedAt) ?? str(item.updated_at)),
    pinned: item.pinned === true,
    spaceRef: str(item.spaceRef) ?? str(item.space_ref),
  }
}

function normalizeChatThreadTranscript(raw: unknown): ChatThreadTranscriptSnapshot | null {
  const item = objectValue(raw)
  if (!item) return null
  const threadId = str(item.threadId) ?? str(item.thread_id)
  const turns = Array.isArray(item.turns) ? item.turns : null
  if (!threadId || !turns) return null
  const taskSteps = Array.isArray(item.taskSteps)
    ? item.taskSteps
    : Array.isArray(item.task_steps)
      ? item.task_steps
      : undefined
  return {
    threadId,
    turns,
    taskSteps,
    updatedAt: normalizeIsoTimestamp(str(item.updatedAt) ?? str(item.updated_at)),
  }
}

function normalizeIsoTimestamp(value: string | undefined): string {
  if (!value) return new Date().toISOString()
  const parsed = Date.parse(value)
  return Number.isNaN(parsed) ? new Date().toISOString() : new Date(parsed).toISOString()
}

// ── Model catalog normalization ──────────────────────────────────────────────
// `/api/v1/models` historically returned `{ id, name, features }`. The backend is
// growing the payload to expose the full Azure Foundry catalog with provider +
// modality + cost metadata. We parse BOTH shapes defensively: every new field is
// optional and recovered from name/id heuristics when absent, so an old-shape
// response never breaks the picker.

const CHAT_MODALITIES: ReadonlySet<ModelModality> = new Set(['chat'])

/** Non-chat ids/names we must never offer as a chat model, even with no metadata. */
const NON_CHAT_NAME_PATTERNS: readonly RegExp[] = [
  /\bimage\b/i,
  /\bsora\b/i,
  /\bvideo\b/i,
  /\bvision-only\b/i,
  /\btranscrib/i,
  /\bwhisper\b/i,
  /\btts\b/i,
  /\bspeech\b/i,
  /\bembed/i,
  /text-embedding/i,
  /\bdall-?e\b/i,
  /\brerank/i,
]

function normalizeModality(value: string | undefined, id: string, name: string): ModelModality | undefined {
  const explicit = (value ?? '').toLowerCase()
  if (explicit) {
    if (explicit.includes('chat') || explicit.includes('text') || explicit.includes('reason')) return 'chat'
    if (explicit.includes('image') || explicit.includes('vision')) return 'image'
    if (explicit.includes('video')) return 'video'
    if (explicit.includes('audio') || explicit.includes('speech') || explicit.includes('transcri')) return 'audio'
    if (explicit.includes('embed')) return 'embedding'
    return 'other'
  }
  // No explicit modality → infer from id/name heuristics.
  const haystack = `${id} ${name}`
  if (NON_CHAT_NAME_PATTERNS.some((re) => re.test(haystack))) {
    if (/embed/i.test(haystack)) return 'embedding'
    if (/sora|video/i.test(haystack)) return 'video'
    if (/transcrib|whisper|tts|speech/i.test(haystack)) return 'audio'
    if (/image|dall-?e/i.test(haystack)) return 'image'
    return 'other'
  }
  return undefined
}

function normalizeCostTier(value: unknown): ModelInfo['costTier'] {
  const tier = str(value)?.toLowerCase()
  if (tier === 'low' || tier === 'cheap' || tier === 'economy') return 'low'
  if (tier === 'medium' || tier === 'standard') return 'medium'
  if (tier === 'high' || tier === 'premium' || tier === 'expensive') return 'high'
  return undefined
}

function readBool(value: unknown): boolean | undefined {
  if (typeof value === 'boolean') return value
  if (typeof value === 'string') {
    const v = value.toLowerCase()
    if (v === 'true' || v === 'yes') return true
    if (v === 'false' || v === 'no') return false
  }
  return undefined
}

export async function listModels(): Promise<ModelInfo[]> {
  const raw = await requestJson<unknown>('/api/v1/models')
  const source = Array.isArray(raw)
    ? raw
    : raw && typeof raw === 'object' && Array.isArray((raw as { models?: unknown }).models)
      ? (raw as { models: unknown[] }).models
      : []

  return source
    .filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === 'object')
    .map((item) => {
      const id = str(item.id) ?? ''
      const name = str(item.name) ?? str(item.display_name) ?? str(item.id) ?? 'model'
      const provider = str(item.provider) ?? str(item.vendor) ?? str(item.owned_by)
      const modality = normalizeModality(
        str(item.modality) ?? str(item.type) ?? str(item.kind) ?? str(item.category),
        id,
        name,
      )
      const costTier = normalizeCostTier(item.cost_tier ?? item.cost ?? item.tier)
      // Unknown/garbage tier values normalize to undefined → neutral UI, never
      // a fabricated residency claim.
      const privacyTier = normalizePrivacyTier(item.privacy_tier ?? item.privacyTier)
      return {
        id,
        name,
        capabilities: Array.isArray(item.features)
          ? (item.features as unknown[]).filter((f): f is string => typeof f === 'string')
          : Array.isArray(item.capabilities)
            ? (item.capabilities as unknown[]).filter((f): f is string => typeof f === 'string')
            : undefined,
        provider,
        modality,
        costTier,
        privacyTier,
        residency: str(item.residency),
        cheap: readBool(item.cheap) ?? readBool(item.is_cheap),
      }
    })
    .filter((model) => model.id.length > 0)
}

/**
 * The per-model capability flag inference-core emits for a deployment whose
 * zero-retention contract an operator has explicitly attested.
 *
 * It is NOT derived from the region or the provider brand. `feature_flags()` in
 * inference-core only adds it when `supports_zdr` is true, which for Azure
 * OpenAI means the operator set `AZURE_OPENAI_ZDR_CONFIRMED`, and which the
 * Anthropic provider currently never sets.
 */
export const ZERO_RETENTION_CAPABILITY = 'zdr'

/**
 * Whether ANY catalogue model can serve a no-retention turn.
 *
 * This exists because the alternative is worse than no feature: inference-core
 * fails a ZDR request CLOSED, skipping every provider that is not attested, so
 * offering the temporary-chat toggle against an unattested fleet produces a
 * turn that always dies. Asking the catalogue lets the UI say so before the
 * user spends a message finding out.
 *
 * An EMPTY catalogue returns false. That is deliberate: `listModels()` swallows
 * a gateway outage into `[]`, and in that state we know nothing about provider
 * attestation — offering a mode that fails closed downstream is the wrong guess.
 */
export function hasZeroRetentionModel(models: readonly ModelInfo[]): boolean {
  return models.some((model) => model.capabilities?.includes(ZERO_RETENTION_CAPABILITY) ?? false)
}

/** Patterns that mark a model as expensive enough to warrant a deliberate pick. */
const EXPENSIVE_ID_PATTERNS: readonly RegExp[] = [/opus/i, /sonnet/i, /gpt-5/i, /\bo[13]\b/i, /\bo3\b/i]

/**
 * True when the model is chat-capable. Prefers explicit modality metadata; falls
 * back to name/id heuristics so the old `{ id, name }` shape still filters cleanly.
 */
export function isChatModel(model: ModelInfo): boolean {
  if (model.modality) return CHAT_MODALITIES.has(model.modality)
  const haystack = `${model.id} ${model.name}`
  return !NON_CHAT_NAME_PATTERNS.some((re) => re.test(haystack))
}

/** True for models a user should select deliberately (opus/sonnet/gpt-5.x …). */
export function isExpensiveModel(model: ModelInfo): boolean {
  if (model.costTier === 'high') return true
  if (model.cheap === true) return false
  return EXPENSIVE_ID_PATTERNS.some((re) => re.test(`${model.id} ${model.name}`))
}

/**
 * The cost-aware default model id. Verevon Balance is the first-class default:
 * the backend resolves `verevon-balance` server-side (complexity + budget →
 * concrete model, with model-router as the cheap fallback), so we don't need to
 * pick a concrete catalog model. `models` is accepted for API compatibility and
 * future heuristics, but the balance mode is authoritative.
 *
 * The composer/ChatPage fall back to `''` if anything is off — the backend treats
 * an empty model as balance too — so a bad value can never crash the picker.
 */
export function cheapDefaultModelId(models: readonly ModelInfo[]): string {
  // `models` is intentionally not consulted today: Verevon Balance is resolved
  // server-side. Kept in the signature for API compatibility + future heuristics.
  void models
  return VEREVON_BALANCE_MODE_ID
}

const PROVIDER_GROUP_ORDER: readonly { label: string; test: (m: ModelInfo) => boolean }[] = [
  { label: 'Claude', test: (m) => m.provider === 'anthropic' || /claude/i.test(`${m.id} ${m.name}`) },
  { label: 'OpenAI GPT', test: (m) => m.provider === 'openai' || /gpt|^o[0-9]|model-router/i.test(`${m.id} ${m.name}`) },
  { label: 'DeepSeek', test: (m) => m.provider === 'deepseek' || /deepseek/i.test(`${m.id} ${m.name}`) },
]

export type ModelGroup = { label: string; models: ModelInfo[] }

/**
 * Group chat models by family/provider for the picker. Unmatched providers fall
 * into an "Other models" bucket. Empty groups are dropped. Group order is stable.
 *
 * Verevon intent modes are filtered out here: the backend may advertise them
 * synthetically (`provider === 'verevon'` or a `verevon-*` mode id), but the pinned
 * Verevon group in the UI is authoritative, so we drop the duplicates from the
 * normal catalog groups.
 */
export function groupChatModels(models: readonly ModelInfo[]): ModelGroup[] {
  const chat = models
    .filter(isChatModel)
    .filter((model) => model.provider !== 'verevon' && !isVerevonModeId(model.id))
  const buckets = new Map<string, ModelInfo[]>()
  const otherLabel = 'Other models'

  for (const model of chat) {
    const group = PROVIDER_GROUP_ORDER.find((g) => g.test(model))
    const label = group?.label ?? otherLabel
    const existing = buckets.get(label)
    if (existing) existing.push(model)
    else buckets.set(label, [model])
  }

  const ordered: ModelGroup[] = []
  for (const { label } of PROVIDER_GROUP_ORDER) {
    const models = buckets.get(label)
    if (models && models.length > 0) ordered.push({ label, models })
  }
  const other = buckets.get(otherLabel)
  if (other && other.length > 0) ordered.push({ label: otherLabel, models: other })
  return ordered
}

export type ChatFeedbackRating = 'positive' | 'negative'

/**
 * Rate one chat turn.
 *
 * Wire contract: `{requestId, rating, note}` → gateway → model-gateway
 * `/v1/feedback`. `requestId` is the SSE turn id; model-gateway resolves it
 * server-side to the durable run id and to the skill ids it injected into that
 * turn. The client deliberately does NOT send skill ids — it does not reliably
 * know them, and a client-asserted skill id would let a rating be aimed at any
 * skill in the org.
 *
 * Rejects with {@link ApiError} on failure. Callers must surface that: a
 * swallowed rejection is what made the UI light the thumb up while nothing was
 * recorded.
 */
export async function submitFeedback(
  requestId: string,
  rating: ChatFeedbackRating,
  options: { note?: string; runId?: string } = {},
): Promise<void> {
  await requestJson('/api/v1/chat/feedback', {
    method: 'POST',
    body: JSON.stringify({
      requestId,
      rating,
      note: options.note,
      // Only agentic turns learn their durable run id (the `connected` event
      // carries it). Sending it lets the rating still land as a run-only rating
      // if the gateway no longer holds the turn record. It is not a trust
      // shortcut: model-gateway still verifies run ownership server-side.
      run_id: options.runId,
    }),
  })
}

/**
 * User-facing reason a rating did not stick. Keeps the wording in one place so
 * the chat surface only has to decide *where* to show it.
 */
export function describeFeedbackFailure(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.code === 'turn_not_rateable') {
      return 'Denne meldingen kan ikke vurderes lenger. Vurderingen ble ikke lagret.'
    }
    if (error.status === 401 || error.status === 403) {
      return 'Du har ikke tilgang til å vurdere denne meldingen. Vurderingen ble ikke lagret.'
    }
    if (error.status === 412) {
      return 'Vurderinger lagres ikke når Zero Data Retention er aktivt.'
    }
  }
  return 'Kunne ikke lagre vurderingen. Prøv igjen.'
}
