import { requestJson } from './http'
import { readSseStream, type SseEvent } from './sse'
import { createSelectedAgentToolSpecs } from '@/shared/actions/agent-tools'

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
  sessionKey?: string
  features?: string[]
  generateImage?: boolean
  browseWeb?: boolean
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
export type ChatDoneEvent = { requestId?: string; modelUsed?: string; outputTokens?: number }
export type ChatErrorEvent = { code: string; message: string; retryable?: boolean }
export type ChatArtifactEvent = { id?: string; kind?: string; title?: string; content?: string; version?: number }
export type ChatReasoningEvent = { delta: string }
export type ChatToolCallEvent = { id?: string; name?: string; args?: unknown }
export type ChatToolResultEvent = { id?: string; output?: string; error?: string; status?: string }
export type ChatCitationEvent = { id?: string; title?: string; url?: string; snippet?: string }
export type ChatGroundingEvent = { value: unknown }
export type ChatStepEvent = { id?: string; title?: string; detail?: string; status?: string }
export type ChatAttachmentEvent = { id?: string; name?: string; mime?: string; type?: string; url?: string; size?: number }
export type ChatUsageEvent = {
  inputTokens?: number
  outputTokens?: number
  costUsd?: number
  latencyMs?: number
  confidence?: number
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
}

export type ModelModality = 'chat' | 'image' | 'video' | 'audio' | 'embedding' | 'other'

// ── Velion intent modes ──────────────────────────────────────────────────────
// Three first-class pseudo-models. They are NOT catalog entries: the backend
// (inference-core) resolves each id SERVER-SIDE via its intent layer (complexity
// + budget → a concrete model, with model-router as the cheap fallback). The
// frontend only offers them and sends the chosen id as `model`. They are always
// pinned at the top of the picker, independent of what `/v1/models` returns.

export type VelionModeBadge = 'cheap' | 'premium'

export type VelionMode = {
  id: string
  /** Display label, e.g. "Velion Balance". */
  label: string
  /** Short one-liner shown under the label in the picker. */
  description: string
  /** Which cost badge to show next to the row. */
  badge: VelionModeBadge
}

/** Velion Balance is the default selection (backend treats `''` as balance too). */
export const VELION_BALANCE_MODE_ID = 'velion-balance'

/** The pinned "Velion" group, authoritative + always shown, most-default first. */
export const VELION_MODES: readonly VelionMode[] = [
  {
    id: 'velion-budget',
    label: 'Velion Budget',
    description: 'Cheapest, fast',
    badge: 'cheap',
  },
  {
    id: VELION_BALANCE_MODE_ID,
    label: 'Velion Balance',
    description: 'Smart balance of cost & quality',
    badge: 'cheap',
  },
  {
    id: 'velion-genius',
    label: 'Velion Genius',
    description: 'Most capable, premium',
    badge: 'premium',
  },
]

const VELION_MODE_IDS: ReadonlySet<string> = new Set(VELION_MODES.map((mode) => mode.id))

/** True when the id is one of the pinned Velion intent modes. */
export function isVelionModeId(id: string): boolean {
  return VELION_MODE_IDS.has(id)
}

/** Look up a Velion mode by id (for label/description/badge rendering). */
export function velionModeById(id: string): VelionMode | undefined {
  return VELION_MODES.find((mode) => mode.id === id)
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
}

// Opt-in rich SSE families the model-gateway understands (chat-parity §2).
const DEFAULT_FEATURES = ['usage', 'citations', 'reasoning', 'steps', 'artifacts']
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

export function buildChatWireBody(request: ChatInvokeRequest): Record<string, unknown> {
  const tools = buildToolSpecs(request)
  const features = new Set(request.features ?? DEFAULT_FEATURES)
  if (tools.length > 0) features.add('tools')
  // Plan mode → agentic run path (orchestration-backed, supports approval gates
  // + run pause/resume). Without it the gateway uses the direct tool loop, which
  // never pauses for human approval.
  if (request.planMode) features.add('agentic')
  const threadId = request.threadId?.trim() || undefined

  return {
    content: request.content,
    model: request.model,
    profile: request.profile ?? 'chat',
    thread_id: threadId,
    session_key: request.sessionKey?.trim() || threadId,
    browse_web: request.browseWeb ?? false,
    generate_image: request.generateImage ?? false,
    attachments: request.attachments ?? [],
    features: [...features],
    tools,
    zdr: request.zdr ?? false,
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
      })
      break
    case 'stopped':
      handlers.onDone?.({ requestId: str(payload.request_id) })
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
    // STREAM_* envelopes are emitted by some upstreams and ignored gracefully.
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
    (event) => dispatchEvent(event, handlers),
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
    (event) => dispatchEvent(event, handlers),
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

export async function getThreadMessages(threadId: string): Promise<ChatMessage[]> {
  const raw = await requestJson<unknown>(
    `/api/v1/chat/threads/${encodeURIComponent(threadId)}/messages`,
  )
  return normalizeThreadMessages(raw)
}

export async function listChatThreads(): Promise<ChatThreadSession[]> {
  const raw = await requestJson<unknown>('/api/v1/chat/threads')
  return normalizeChatThreadSessions(raw)
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
        cheap: readBool(item.cheap) ?? readBool(item.is_cheap),
      }
    })
    .filter((model) => model.id.length > 0)
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
 * The cost-aware default model id. Velion Balance is the first-class default:
 * the backend resolves `velion-balance` server-side (complexity + budget →
 * concrete model, with model-router as the cheap fallback), so we don't need to
 * pick a concrete catalog model. `models` is accepted for API compatibility and
 * future heuristics, but the balance mode is authoritative.
 *
 * The composer/ChatPage fall back to `''` if anything is off — the backend treats
 * an empty model as balance too — so a bad value can never crash the picker.
 */
export function cheapDefaultModelId(models: readonly ModelInfo[]): string {
  // `models` is intentionally not consulted today: Velion Balance is resolved
  // server-side. Kept in the signature for API compatibility + future heuristics.
  void models
  return VELION_BALANCE_MODE_ID
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
 * Velion intent modes are filtered out here: the backend may advertise them
 * synthetically (`provider === 'velion'` or a `velion-*` mode id), but the pinned
 * Velion group in the UI is authoritative, so we drop the duplicates from the
 * normal catalog groups.
 */
export function groupChatModels(models: readonly ModelInfo[]): ModelGroup[] {
  const chat = models
    .filter(isChatModel)
    .filter((model) => model.provider !== 'velion' && !isVelionModeId(model.id))
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

export async function submitFeedback(
  requestId: string,
  rating: 'positive' | 'negative',
  note?: string,
): Promise<void> {
  await requestJson('/api/v1/chat/feedback', {
    method: 'POST',
    body: JSON.stringify({ requestId, rating, note }),
  })
}
