import {
  type ChatThreadTranscriptStep,
  type ChatThreadTranscriptTurn,
} from '@/features/chat/lib/chat-thread-history'
import {
  type DashboardComposerSubmitPayload,
} from '@/features/dashboard/home/DashboardComposer'
import {
  type ChatAction,
  type ChatMessage,
  type RecalledMemory,
} from '@/shared/api/chat-client'
import {
  blobToDataUrl,
  parseDataUrl,
} from '@/shared/lib/blob-data'
import {
  readClientValue,
  writeClientValue,
} from '@/shared/session/client-storage'
import { supportQuestionFromPrompt } from '@/shared/chat/support-context-envelope'
import {
  isChatArtifactVersion,
  mergeArtifactVersion,
} from './chat-artifacts'
import {
  artifactTitle,
  capitalize,
  createPreview,
  formatLatency,
  hostname,
  inferArtifactKind,
  isValidUrl,
  normalizeTaskStatus,
  objectValue,
  stringValue,
  truncateText,
} from './chat-media-markdown'
import {
  type AgentTaskStep,
  type AgentTaskStepEvidence,
  CHAT_BROWSE_WEB_KEY,
  type ChatArtifact,
  type ChatKnowledgeGrounding,
  type ChatToolCall,
  type ChatTurn,
  type Citation,
  type ComposerAttachment,
  type ComposerToolId,
  type GeneratedFile,
  type StreamAttachment,
  TOOL_LABELS,
  type TaskStepStatus,
} from './chat-types'

export function messageToTurn(msg: ChatMessage): ChatTurn {
  return {
    id: msg.id,
    role: msg.role,
    content: visibleTurnContent(msg.role, msg.content),
    // Keep an absent server timestamp EMPTY instead of fabricating "now":
    // session-core messages carry no timestamps, so stamping the fetch time
    // here made every thread selection look like fresh activity (the history
    // list re-ordered and re-dated on a mere click). The cached-metadata merge
    // backfills the real time; loadThread fills any remainder from the stored
    // history entry before the turns reach the UI.
    createdAt: msg.createdAt || '',
    streaming: false,
    model: msg.model,
    tools: [],
    attachments: [],
  }
}

export function turnsToTranscript(turns: ChatTurn[]): ChatThreadTranscriptTurn[] {
  return turns
    .filter((turn) => turn.role === 'user' || turn.role === 'assistant')
    .map((turn) => ({
      id: turn.id,
      role: turn.role,
      content: turn.content,
      createdAt: turn.createdAt,
      model: turn.model,
      modelUsed: turn.modelUsed,
      requestId: turn.requestId,
      status: turn.status,
      inputTokens: turn.inputTokens,
      outputTokens: turn.outputTokens,
      latencyMs: turn.latencyMs,
      costUsd: turn.costUsd,
      confidence: turn.confidence,
      reasoning: turn.reasoning,
      citations: turn.citations,
      toolCalls: turn.toolCalls,
      artifacts: turn.artifacts,
      files: turn.files,
      grounding: turn.grounding,
      memoryRecallCount: turn.memoryRecallCount,
      recalledMemories: turn.recalledMemories,
      stopReason: turn.stopReason,
      tools: turn.tools,
      attachments: turn.attachments,
    }))
}

export function taskStepsToTranscript(steps: AgentTaskStep[]): ChatThreadTranscriptStep[] {
  return steps.map((step) => ({
    id: step.id,
    title: step.title,
    detail: step.detail,
    status: step.status,
    createdAt: step.createdAt,
    expandedDetail: step.expandedDetail,
    evidence: step.evidence,
    turnId: step.turnId,
    turnTitle: step.turnTitle,
  }))
}

export function transcriptTurnToChatTurn(turn: ChatThreadTranscriptTurn): ChatTurn {
  return {
    id: turn.id,
    role: turn.role,
    content: visibleTurnContent(turn.role, turn.content),
    createdAt: turn.createdAt,
    streaming: false,
    model: turn.model,
    modelUsed: turn.modelUsed,
    requestId: turn.requestId,
    status: turn.status,
    inputTokens: turn.inputTokens,
    outputTokens: turn.outputTokens,
    latencyMs: turn.latencyMs,
    costUsd: turn.costUsd,
    confidence: turn.confidence,
    reasoning: turn.reasoning,
    citations: (turn.citations ?? []).filter(isCitation),
    toolCalls: (turn.toolCalls ?? []).filter(isChatToolCall),
    artifacts: (turn.artifacts ?? []).filter(isChatArtifact),
    files: (turn.files ?? []).filter(isGeneratedFile),
    grounding: isChatKnowledgeGrounding(turn.grounding) ? turn.grounding : undefined,
    memoryRecallCount: turn.memoryRecallCount,
    recalledMemories: (turn.recalledMemories ?? []).filter(isRecalledMemory),
    stopReason: turn.stopReason,
    tools: (turn.tools ?? []).filter(isComposerToolId),
    attachments: (turn.attachments ?? []).filter(isComposerAttachment),
  }
}

function visibleTurnContent(role: ChatTurn['role'], content: string): string {
  if (role !== 'user') return content
  return supportQuestionFromPrompt(content) ?? content
}

export function transcriptStepToTaskStep(step: ChatThreadTranscriptStep): AgentTaskStep {
  return {
    id: step.id,
    title: step.title,
    detail: step.detail,
    status: step.status,
    createdAt: step.createdAt,
    expandedDetail: step.expandedDetail,
    evidence: step.evidence,
    turnId: step.turnId,
    turnTitle: step.turnTitle,
  }
}

export function dedupeChatTurns(turns: ChatTurn[]): ChatTurn[] {
  return turns.reduce<ChatTurn[]>((next, turn) => {
    const previous = next.at(-1)
    if (
      previous &&
      previous.role === turn.role &&
      previous.content === turn.content &&
      // Two timestamp-less turns (session-core sends none) are the case the
      // dedupe existed for back when both were stamped "now" — keep treating
      // them as close.
      (timestampsAreClose(previous.createdAt, turn.createdAt) ||
        (!previous.createdAt && !turn.createdAt))
    ) {
      return next
    }
    return [...next, turn]
  }, [])
}

export function timestampsAreClose(left: string, right: string): boolean {
  const leftTime = Date.parse(left)
  const rightTime = Date.parse(right)
  if (Number.isNaN(leftTime) || Number.isNaN(rightTime)) return false
  return Math.abs(leftTime - rightTime) < 60_000
}

export function mergeServerTurnsWithCachedMetadata(serverTurns: ChatTurn[], cachedTurns: ChatTurn[]): ChatTurn[] {
  if (cachedTurns.length === 0) return serverTurns
  const usedCachedIds = new Set<string>()
  const roleOffsets: Record<ChatTurn['role'], number> = { assistant: 0, user: 0 }

  const merged = serverTurns.map((serverTurn) => {
    const roleIndex = roleOffsets[serverTurn.role]
    roleOffsets[serverTurn.role] += 1
    const cachedTurn = selectCachedTurnForServerTurn(serverTurn, cachedTurns, usedCachedIds, roleIndex)
    if (!cachedTurn) return serverTurn
    usedCachedIds.add(cachedTurn.id)
    return {
      ...serverTurn,
      // The cached transcript recorded the REAL send time during streaming;
      // a server turn without a timestamp takes it instead of staying empty.
      createdAt: serverTurn.createdAt || cachedTurn.createdAt,
      model: serverTurn.model ?? cachedTurn.model,
      modelUsed: serverTurn.modelUsed ?? cachedTurn.modelUsed,
      requestId: serverTurn.requestId ?? cachedTurn.requestId,
      // A server-persisted assistant message is by definition COMPLETE
      // (session-core stores it once, at stream end), so a cached in-flight
      // 'waiting' must never leak onto it — it would render a finished
      // answer as an eternal spinner. Terminal cached statuses still carry.
      status: serverTurn.status ?? (cachedTurn.status === 'waiting' ? undefined : cachedTurn.status),
      inputTokens: serverTurn.inputTokens ?? cachedTurn.inputTokens,
      outputTokens: serverTurn.outputTokens ?? cachedTurn.outputTokens,
      latencyMs: serverTurn.latencyMs ?? cachedTurn.latencyMs,
      costUsd: serverTurn.costUsd ?? cachedTurn.costUsd,
      confidence: serverTurn.confidence ?? cachedTurn.confidence,
      reasoning: serverTurn.reasoning ?? cachedTurn.reasoning,
      citations: metadataArray(serverTurn.citations, cachedTurn.citations),
      toolCalls: metadataArray(serverTurn.toolCalls, cachedTurn.toolCalls),
      artifacts: metadataArray(serverTurn.artifacts, cachedTurn.artifacts),
      files: metadataArray(serverTurn.files, cachedTurn.files),
      grounding: serverTurn.grounding ?? cachedTurn.grounding,
      // Both are session-derived: the server's persisted message carries
      // neither, so without falling back to the cache a thread refresh would
      // drop the recall chip and — worse — the truncation warning, turning a
      // "may be cut off" answer into one that looks complete.
      memoryRecallCount: serverTurn.memoryRecallCount ?? cachedTurn.memoryRecallCount,
      recalledMemories: metadataArray(serverTurn.recalledMemories, cachedTurn.recalledMemories),
      stopReason: serverTurn.stopReason ?? cachedTurn.stopReason,
      tools: serverTurn.tools.length > 0 ? serverTurn.tools : cachedTurn.tools,
      attachments: serverTurn.attachments.length > 0 ? serverTurn.attachments : cachedTurn.attachments,
    }
  })

  // Resumable tail (chat-parity §3b): a reload mid-answer leaves the waiting
  // assistant turn ONLY in the cache — the server persists the assistant
  // message at stream end, so mapping over serverTurns alone silently drops
  // it, and the resume path (`maybeResumeStream`) never sees a waiting turn to
  // reattach to. Carry that one turn over: strictly the LAST cached turn, an
  // assistant still 'waiting', unconsumed by the merge above, and holding the
  // requestId the resume endpoint needs (no requestId → nothing to resume →
  // appending would render an unsettleable spinner). Its user question is
  // already in serverTurns: the gateway persists the user message during
  // prepare, before any requestId exists.
  const tail = cachedTurns.at(-1)
  if (
    tail &&
    tail.role === 'assistant' &&
    tail.status === 'waiting' &&
    tail.requestId &&
    !usedCachedIds.has(tail.id)
  ) {
    return [...merged, tail]
  }
  return merged
}

export function metadataArray<T>(serverItems: T[] | undefined, cachedItems: T[] | undefined): T[] | undefined {
  return serverItems && serverItems.length > 0 ? serverItems : cachedItems
}

export function selectCachedTurnForServerTurn(
  serverTurn: ChatTurn,
  cachedTurns: ChatTurn[],
  usedCachedIds: Set<string>,
  roleIndex: number,
): ChatTurn | undefined {
  const unusedSameRole = cachedTurns.filter((turn) => turn.role === serverTurn.role && !usedCachedIds.has(turn.id))
  return unusedSameRole.find((turn) => turn.id === serverTurn.id)
    ?? unusedSameRole.find((turn) => turnsProbablyMatch(serverTurn, turn))
    ?? unusedSameRole[roleIndex]
    ?? (serverTurn.role === 'assistant' ? [...unusedSameRole].reverse().find(hasCachedTurnMetadata) : undefined)
}

export function turnsProbablyMatch(left: ChatTurn, right: ChatTurn): boolean {
  const leftContent = normalizeTurnContentForMatch(left.content)
  const rightContent = normalizeTurnContentForMatch(right.content)
  if (!leftContent || !rightContent) return false
  if (leftContent === rightContent) return true
  const leftSnippet = leftContent.slice(0, 120)
  const rightSnippet = rightContent.slice(0, 120)
  return leftSnippet.length > 40 && rightContent.includes(leftSnippet)
    ? true
    : rightSnippet.length > 40 && leftContent.includes(rightSnippet)
}

export function normalizeTurnContentForMatch(content: string): string {
  return content.replace(/\s+/g, ' ').trim()
}

export function hasCachedTurnMetadata(turn: ChatTurn): boolean {
  return (
    (turn.citations?.length ?? 0) > 0 ||
    (turn.toolCalls?.length ?? 0) > 0 ||
    (turn.artifacts?.length ?? 0) > 0 ||
    (turn.files?.length ?? 0) > 0 ||
    Boolean(turn.grounding) ||
    Boolean(turn.reasoning)
  )
}

export function isComposerToolId(value: string): value is ComposerToolId {
  return value === 'image' || value === 'reason' || value === 'research' || value === 'search'
}

/**
 * A persisted recalled-memory row, validated on the way back in.
 *
 * `origin` is re-narrowed here rather than trusted: a stored transcript may have
 * been written by a build whose vocabulary was wider, and an unrecognised value
 * must read as `unrecorded` rather than as `stated` — the same rule the wire
 * normalizer applies, for the same reason.
 */
export function isRecalledMemory(value: unknown): value is RecalledMemory {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.memoryId === 'string' &&
    record.memoryId.length > 0 &&
    typeof record.label === 'string' &&
    typeof record.preview === 'string' &&
    (record.role === 'recall' || record.role === 'inject') &&
    (record.origin === 'stated' || record.origin === 'inferred' || record.origin === 'unrecorded')
  )
}

export function isCitation(value: unknown): value is Citation {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.id === 'string' &&
    typeof record.title === 'string' &&
    typeof record.url === 'string' &&
    typeof record.snippet === 'string'
  )
}

export function isChatToolCall(value: unknown): value is ChatToolCall {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.id === 'string' &&
    typeof record.name === 'string' &&
    (record.args === undefined || typeof record.args === 'object') &&
    (record.status === undefined || typeof record.status === 'string') &&
    (record.output === undefined || typeof record.output === 'string') &&
    (record.error === undefined || typeof record.error === 'string')
  )
}

export function isChatArtifact(value: unknown): value is ChatArtifact {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.id === 'string' &&
    typeof record.kind === 'string' &&
    typeof record.content === 'string' &&
    typeof record.title === 'string' &&
    typeof record.version === 'number' &&
    // `history` rides along in the thread snapshot as untyped JSON; a snapshot
    // written by an older build simply has none.
    (record.history === undefined ||
      (Array.isArray(record.history) && record.history.every(isChatArtifactVersion)))
  )
}

export function isGeneratedFile(value: unknown): value is GeneratedFile {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.id === 'string' &&
    typeof record.name === 'string' &&
    typeof record.mime === 'string' &&
    typeof record.size === 'number' &&
    typeof record.url === 'string'
  )
}

export function isChatKnowledgeGrounding(value: unknown): value is ChatKnowledgeGrounding {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    (record.mode === 'retrieve' || record.mode === 'hybrid') &&
    typeof record.query === 'string' &&
    typeof record.lowConfidence === 'boolean' &&
    typeof record.factCount === 'number' &&
    typeof record.sourceCount === 'number' &&
    Array.isArray(record.facts) &&
    Array.isArray(record.sources)
  )
}

export function isComposerAttachment(value: unknown): value is ComposerAttachment {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.id === 'string' &&
    typeof record.name === 'string' &&
    typeof record.size === 'number' &&
    typeof record.type === 'string' &&
    typeof record.url === 'string'
  )
}

export async function toStreamAttachments(
  attachments: DashboardComposerSubmitPayload['attachments'],
): Promise<StreamAttachment[]> {
  const out: StreamAttachment[] = []
  for (const attachment of attachments) {
    if (!attachment.url || !attachment.type.startsWith('image/')) continue
    try {
      let dataUrl = attachment.url
      if (!dataUrl.startsWith('data:')) {
        const response = await fetch(dataUrl)
        dataUrl = await blobToDataUrl(await response.blob())
      }
      const parsed = parseDataUrl(dataUrl)
      if (parsed) out.push({ kind: 'image', data_base64: parsed.base64, mime_type: parsed.mime || attachment.type })
    } catch {
      // skip unreadable attachments
    }
  }
  return out
}

export function createId(prefix: string) {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return `${prefix}-${crypto.randomUUID()}`
  }
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`
}

export function buildTaskSteps(
  content: string,
  tools: ComposerToolId[],
  mode: 'submit' | 'regenerate',
  turnId: string,
  actions: ChatAction[] = [],
): AgentTaskStep[] {
  const now = new Date().toISOString()
  const turnTitle = createPreview(content, 58)
  const intro = mode === 'regenerate' ? 'Regenerating latest response.' : `Preparing "${createPreview(content)}".`
  return [
    { id: `${turnId}:connect`, title: 'Connect stream', detail: intro, status: 'active', createdAt: now, turnId, turnTitle },
    ...tools.map((tool) => ({
      id: `${turnId}:tool-${tool}`,
      title: TOOL_LABELS[tool],
      detail: tool === 'search'
        ? 'Web search is available; it runs only if the answer needs fresh data.'
        : `${TOOL_LABELS[tool]} is enabled for this answer.`,
      status: 'waiting' as const,
      createdAt: now,
      turnId,
      turnTitle,
    })),
    ...actions.map((action) => ({
      id: `${turnId}:action-${action.id}`,
      title: `${capitalize(action.kind)}: ${action.name || action.id}`,
      detail: `Selected action ${action.id}.`,
      status: 'waiting' as const,
      createdAt: now,
      turnId,
      turnTitle,
    })),
    { id: `${turnId}:answer`, title: 'Compose response', detail: 'Waiting for model output.', status: 'waiting', createdAt: now, turnId, turnTitle },
  ]
}

export function createTurnStep(
  turnId: string,
  turnTitle: string,
  id: string,
  title: string,
  detail: string,
  status: TaskStepStatus,
): AgentTaskStep {
  return {
    id: `${turnId}:${id}`,
    title,
    detail,
    status,
    createdAt: new Date().toISOString(),
    turnId,
    turnTitle,
  }
}

export function normalizeArtifact(event: { id?: string; kind?: string; title?: string; content?: string; version?: number }): ChatArtifact | null {
  const content = event.content ?? ''
  // An artifact with no content AND no id carries nothing to show or update —
  // drop it. One WITH an id was genuinely announced by the backend, so it is
  // kept: the panel then renders an explicit "kunne ikke lastes" state instead
  // of silently losing an artifact the user was told about.
  if (!content && !event.id) return null
  const kind = event.kind ?? inferArtifactKind(content)
  return {
    id: event.id ?? createId('artifact'),
    kind,
    title: event.title ?? artifactTitle(kind),
    content,
    version: event.version ?? 0,
  }
}

export function normalizeGeneratedFile(event: {
  id?: string
  name?: string
  mime?: string
  type?: string
  url?: string
  size?: number
}): GeneratedFile | null {
  if (!event.url || !event.name) return null
  return {
    id: event.id ?? event.url,
    name: event.name,
    mime: event.mime ?? event.type ?? 'application/octet-stream',
    size: event.size ?? 0,
    url: event.url,
  }
}

export function normalizeCitation(event: { id?: string; title?: string; url?: string; snippet?: string }): Citation | null {
  if (!event.url) return null
  return {
    id: event.id ?? event.url,
    title: event.title ?? hostname(event.url),
    url: event.url,
    snippet: event.snippet ?? '',
  }
}

export function normalizeStep(
  event: { id?: string; title?: string; detail?: string; status?: string },
  turnId?: string,
  turnTitle?: string,
): AgentTaskStep | null {
  if (!event.id && !event.title) return null
  const rawId = event.id ?? createId('step')
  return {
    id: turnId ? `${turnId}:event-${rawId}` : rawId,
    title: event.title ?? 'Agent step',
    detail: event.detail ?? event.status ?? 'Updated.',
    status: normalizeTaskStatus(event.status),
    createdAt: new Date().toISOString(),
    turnId,
    turnTitle,
  }
}

export function normalizeToolCall(event: { id?: string; name?: string; args?: unknown }): ChatToolCall | null {
  if (!event.id && !event.name) return null
  return {
    id: event.id ?? event.name ?? createId('tool'),
    name: event.name ?? 'Tool call',
    args: event.args,
    status: 'running',
  }
}

export function applyToolResult(calls: ChatToolCall[], event: { id?: string; output?: string; error?: string; status?: string }): ChatToolCall[] {
  const index = calls.findIndex((call) => call.id === event.id)
  if (index < 0) {
    return [
      ...calls,
      {
        id: event.id ?? createId('tool'),
        name: 'Tool result',
        output: event.output,
        error: event.error,
        status: event.status ?? (event.error ? 'error' : 'done'),
      },
    ]
  }
  return calls.map((call, itemIndex) => itemIndex === index
    ? { ...call, output: event.output, error: event.error, status: event.status ?? (event.error ? 'error' : 'done') }
    : call)
}

export function toolNameForResult(calls: ChatToolCall[], id: string): string | undefined {
  return calls.find((call) => call.id === id)?.name ?? (id.includes('web-search') ? 'web_search' : undefined)
}

export function composerToolIdForToolName(name?: string): ComposerToolId | null {
  if (name === 'web_search') return 'search'
  if (name === 'image') return 'image'
  if (name === 'reason' || name === 'reasoning') return 'reason'
  if (name === 'research' || name === 'deep_research') return 'research'
  return null
}

export function humanizeToolName(name: string): string {
  if (name === 'web_search') return 'Web search'
  if (name === 'fetch_url') return 'Fetch URL'
  if (name === 'knowledge_search') return 'Knowledge search'
  return name
    .replace(/^mcp__/, 'MCP ')
    .replace(/[_.:-]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/^\w/, (match) => match.toUpperCase()) || 'Tool'
}

/**
 * Compact, single-line summary of tool-call arguments for dense run timelines.
 *
 * Agent tool args are heterogeneous (integration tools carry `provider` /
 * `operation`; web tools carry `query` / `provider`; others are free-form), so
 * this surfaces the salient routing keys first, then a few remaining primitive
 * params — e.g. "provider: slack · operation: send_message · channel: #ops".
 * Returns '' when there is nothing meaningful to show, so callers can fall back
 * to a neutral running label instead of a fabricated one.
 */
export function summarizeToolArgs(args: unknown, maxLength = 180): string {
  if (args == null) return ''
  if (typeof args === 'string') return truncateText(args.trim(), maxLength)
  if (typeof args !== 'object') return truncateText(String(args), maxLength)
  if (Array.isArray(args)) {
    try {
      return truncateText(JSON.stringify(args), maxLength)
    } catch {
      return ''
    }
  }

  const record = args as Record<string, unknown>
  const priorityKeys = ['provider', 'operation', 'action', 'capability', 'query', 'url', 'path', 'id']
  const parts: string[] = []
  const seen = new Set<string>()

  const pushPart = (key: string, value: unknown): void => {
    const rendered = renderArgValue(value)
    if (rendered) parts.push(`${key}: ${rendered}`)
  }

  for (const key of priorityKeys) {
    if (key in record) {
      seen.add(key)
      pushPart(key, record[key])
    }
  }
  for (const [key, value] of Object.entries(record)) {
    if (parts.length >= 6) break
    if (seen.has(key)) continue
    pushPart(key, value)
  }

  return truncateText(parts.join(' · '), maxLength)
}

function renderArgValue(value: unknown): string {
  if (value == null) return ''
  if (typeof value === 'string') return truncateText(value.trim(), 64)
  if (typeof value === 'number' || typeof value === 'boolean') return String(value)
  try {
    return truncateText(JSON.stringify(value), 64)
  } catch {
    return ''
  }
}

export function summarizeToolResult(event: { output?: string; error?: string; status?: string }): string {
  if (event.error) return truncateText(event.error, 260)
  if (event.output) return truncateText(event.output, 260)
  if (event.status) return `Tool completed with status ${event.status}.`
  return 'Tool call completed.'
}

export function summarizeGrounding(grounding: ChatKnowledgeGrounding): string {
  const parts = [
    `${grounding.sourceCount} source${grounding.sourceCount === 1 ? '' : 's'}`,
    `${grounding.factCount} fact${grounding.factCount === 1 ? '' : 's'}`,
  ]
  if (grounding.traceId) parts.push(`trace ${grounding.traceId}`)
  return parts.join(' · ')
}

export function formatUsageSummary(usage: {
  inputTokens?: number
  outputTokens?: number
  costUsd?: number
  latencyMs?: number
  confidence?: number
}): string {
  const parts: string[] = []
  if (usage.inputTokens != null || usage.outputTokens != null) {
    parts.push(`${usage.inputTokens ?? 0} in / ${usage.outputTokens ?? 0} out tokens`)
  }
  if (usage.latencyMs != null) parts.push(formatLatency(usage.latencyMs))
  if (usage.costUsd != null) parts.push(`$${usage.costUsd.toFixed(4)}`)
  if (usage.confidence != null) parts.push(`${Math.round(usage.confidence * 100)}% confidence`)
  return parts.join(' · ') || 'Usage metadata received.'
}

export function extractCitationsFromToolOutput(output: string): Citation[] {
  const trimmed = output.trim()
  if (!trimmed) return []

  const jsonCitations = extractJsonCitations(trimmed)
  if (jsonCitations.length > 0) return jsonCitations

  const lines = trimmed.split(/\r?\n/).map((line) => line.trim()).filter(Boolean)
  const citations: Citation[] = []
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]
    if (!line) continue
    const match = line.match(/https?:\/\/[^\s)"'<>,]+/i)
    if (!match) continue
    const url = match[0].replace(/[)\].,;]+$/, '')
    if (!isValidUrl(url) || citations.some((citation) => citation.url === url)) continue
    const previous = lines[index - 1]?.replace(/^\d+[.)]\s*/, '').trim()
    const nextLine = lines[index + 1]
    const next = nextLine && !/^https?:\/\//i.test(nextLine)
      ? nextLine
      : ''
    citations.push({
      id: `web-${citations.length + 1}-${url}`,
      title: previous || hostname(url),
      url,
      snippet: next,
    })
    if (citations.length >= 8) break
  }
  return citations
}

export function extractJsonCitations(output: string): Citation[] {
  try {
    const parsed = JSON.parse(output) as unknown
    const root = objectValue(parsed)
    const list = Array.isArray(parsed)
      ? parsed
      : Array.isArray(root?.results)
        ? root.results
        : Array.isArray(root?.citations)
          ? root.citations
          : []
    const direct = citationFromUnknown(parsed, 0)
    if (direct) return [direct]
    return list
      .map(citationFromUnknown)
      .filter((citation): citation is Citation => Boolean(citation))
      .slice(0, 8)
  } catch {
    return []
  }
}

export function citationFromUnknown(value: unknown, index: number): Citation | null {
  const item = objectValue(value)
  if (!item) return null
  const url = stringValue(item.url) ?? stringValue(item.href) ?? stringValue(item.final_url) ?? stringValue(item.finalUrl)
  if (!url || !isValidUrl(url)) return null
  const title = stringValue(item.title) ?? hostname(url)
  const snippet = stringValue(item.snippet) ?? stringValue(item.description) ?? ''
  if (isFailedFetchCitation(item, title, snippet)) return null
  return {
    id: stringValue(item.id) ?? `web-${index + 1}-${url}`,
    title,
    url,
    snippet,
  }
}

export function isFailedFetchCitation(item: Record<string, unknown>, title: string, snippet: string): boolean {
  const status = Number(item.status ?? item.status_code ?? item.statusCode)
  if (status === 404) return true
  const content = stringValue(item.content) ?? stringValue(item.markdown) ?? stringValue(item.text) ?? ''
  const label = `${title} ${snippet}`.toLowerCase()
  return !content.trim() && (label.includes('404') || label.includes('content not found') || label.includes('not found'))
}

export function upsertToolCall(calls: ChatToolCall[], call: ChatToolCall): ChatToolCall[] {
  const index = calls.findIndex((item) => item.id === call.id)
  if (index < 0) return [...calls, call]
  return calls.map((item, itemIndex) => itemIndex === index ? { ...item, ...call } : item)
}

/**
 * Adds or UPDATES an artifact in a turn's list.
 *
 * A repeated `id` never duplicates the entry: the incoming revision is folded
 * into the existing one by `mergeArtifactVersion`, which keeps the full version
 * history and leaves the top-level fields on the newest revision. `carried` is
 * an artifact with the same id found on an EARLIER turn — the model can rewrite
 * a document several turns later, and its earlier revisions must not be lost.
 */
export function upsertArtifact(
  artifacts: ChatArtifact[],
  artifact: ChatArtifact,
  carried?: ChatArtifact,
): ChatArtifact[] {
  const index = artifacts.findIndex((item) => item.id === artifact.id)
  if (index < 0) return [...artifacts, mergeArtifactVersion(carried, artifact)]
  return artifacts.map((item, itemIndex) => (
    itemIndex === index ? mergeArtifactVersion(item, artifact) : item
  ))
}

export function upsertGeneratedFile(files: GeneratedFile[], file: GeneratedFile): GeneratedFile[] {
  const index = files.findIndex((item) => item.id === file.id || item.url === file.url)
  if (index < 0) return [...files, file]
  return files.map((item, itemIndex) => itemIndex === index ? { ...item, ...file } : item)
}

export function upsertCitation(citations: Citation[], citation: Citation): Citation[] {
  if (citations.some((item) => item.id === citation.id || item.url === citation.url)) return citations
  return [...citations, citation]
}

export function upsertStepEvidence(evidence: AgentTaskStepEvidence[], item: AgentTaskStepEvidence): AgentTaskStepEvidence[] {
  if (evidence.some((existing) => existing.id === item.id || existing.href === item.href)) return evidence
  return [...evidence, item]
}

export function citationEvidence(citation: Citation): AgentTaskStepEvidence {
  return {
    id: citation.id || citation.url,
    label: citation.title || hostname(citation.url),
    value: hostname(citation.url),
    href: citation.url,
  }
}

export function missingSearchResultStep(step: AgentTaskStep, status: TaskStepStatus): AgentTaskStep | null {
  if (status !== 'done' || !step.id.endsWith(':tool-search') || step.status !== 'waiting') return null
  // With web search available by default and used per-query (staleness
  // heuristic + model judgment), a turn that completes without searching is
  // the normal outcome for timeless questions — not a failure. Stay honest
  // (the answer is not web-verified) without alarming the user.
  return {
    ...step,
    detail: 'No web search needed — answered from existing knowledge or other tools.',
    expandedDetail: 'Web search was available for this turn, but the query did not require fresh web data, so no search ran. Time-sensitive claims in this answer are not web-verified.',
    status: 'done',
  }
}

export function readBrowseWebPreference(): boolean {
  // Default ON: web search is meant to be available on every turn, with the
  // backend deciding per-query whether it is actually used (staleness
  // heuristic + model judgment). Only an explicit opt-out ('0') disables it.
  return readClientValue(CHAT_BROWSE_WEB_KEY) !== '0'
}

export function writeBrowseWebPreference(enabled: boolean): void {
  writeClientValue(CHAT_BROWSE_WEB_KEY, enabled ? '1' : '0')
}

export function searchCompletionDetail(toolName: string, failed: boolean, sourceCount: number, output?: string): string {
  if (failed) return `${humanizeToolName(toolName)} failed.`
  if (sourceCount > 0) return `Web search returned ${sourceCount} source${sourceCount === 1 ? '' : 's'}.`
  if (output?.trim()) return 'Web search returned raw output.'
  return 'Web search completed, but no source payload was received.'
}

export function searchQueryFromArgs(args: unknown): string | undefined {
  const direct = objectValue(args)
  if (direct) return stringValue(direct.query)
  if (typeof args !== 'string') return undefined
  try {
    return stringValue(objectValue(JSON.parse(args))?.query)
  } catch {
    return undefined
  }
}
