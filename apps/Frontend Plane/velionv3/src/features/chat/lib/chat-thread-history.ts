import {
  readClientJson,
  readClientValue,
  removeClientValue,
  writeClientJson,
  writeClientValue,
} from '@/shared/session/client-storage'

export const CHAT_ACTIVE_THREAD_KEY = 'velion.chat.threadId'
export const CHAT_THREAD_HISTORY_KEY = 'velion.chat.threadHistory.v1'
export const CHAT_THREAD_TRANSCRIPTS_KEY = 'velion.chat.threadTranscripts.v1'
export const CHAT_THREAD_HISTORY_CHANGED_EVENT = 'velion:chat-thread-history-changed'
export const CHAT_ACTIVE_THREAD_CHANGED_EVENT = 'velion:chat-active-thread-changed'

const maxThreadHistoryItems = 40
const maxThreadTranscriptItems = 40
const maxTranscriptTurns = 120
const maxTranscriptSteps = 240
const maxTitleLength = 96
const maxPreviewLength = 180
const maxTranscriptContentLength = 32_000
const maxTranscriptDetailLength = 32_000

/**
 * How a history item's title was produced.
 * - `generated`: AI summary from the model-gateway `title` SSE event — locked:
 *   a later `preview` write must not clobber it.
 * - `preview`: truncated first user message (the fallback).
 * Absent on legacy stored items; treated as `preview`.
 */
export type ChatThreadTitleKind = 'generated' | 'preview'

export type ChatThreadHistoryItem = {
  preview: string
  threadId: string
  title: string
  titleKind?: ChatThreadTitleKind
  updatedAt: string
  /** User-pinned to the top of the sidebar (see `togglePinnedChatThread`). Local-only — never synced from the server. */
  pinned?: boolean
}

export type ChatThreadHistoryInput = {
  preview?: string
  threadId: string
  title?: string
  titleKind?: ChatThreadTitleKind
  updatedAt?: string
  /** Omit to leave the stored pin state untouched (see `withPinnedCarry`). */
  pinned?: boolean
}

export type ChatThreadTranscriptTurn = {
  attachments?: unknown[]
  artifacts?: unknown[]
  citations?: unknown[]
  content: string
  createdAt: string
  costUsd?: number
  confidence?: number
  files?: unknown[]
  grounding?: unknown
  id: string
  inputTokens?: number
  latencyMs?: number
  model?: string
  modelUsed?: string
  outputTokens?: number
  reasoning?: string
  requestId?: string
  role: 'assistant' | 'user'
  status?: 'error' | 'stopped' | 'waiting'
  toolCalls?: unknown[]
  tools?: string[]
}

export type ChatThreadTranscriptStep = {
  createdAt: string
  detail: string
  evidence?: ChatThreadTranscriptStepEvidence[]
  expandedDetail?: string
  id: string
  status: 'active' | 'done' | 'error' | 'stopped' | 'waiting'
  title: string
  turnId?: string
  turnTitle?: string
}

export type ChatThreadTranscriptStepEvidence = {
  href?: string
  id: string
  label: string
  value: string
}

export type ChatThreadTranscript = {
  taskSteps?: ChatThreadTranscriptStep[]
  threadId: string
  turns: ChatThreadTranscriptTurn[]
  updatedAt: string
}

export type ChatThreadTranscriptInput = {
  taskSteps?: ChatThreadTranscriptStep[]
  threadId: string
  turns: ChatThreadTranscriptTurn[]
  updatedAt?: string
}

export function readActiveChatThreadId(): string | null {
  return normalizeId(readClientValue(CHAT_ACTIVE_THREAD_KEY))
}

export function setActiveChatThreadId(threadId: string): void {
  const normalized = normalizeId(threadId)
  if (!normalized) return
  writeClientValue(CHAT_ACTIVE_THREAD_KEY, normalized)
  dispatchClientEvent(CHAT_ACTIVE_THREAD_CHANGED_EVENT, { threadId: normalized })
}

export function clearActiveChatThreadId(): void {
  removeClientValue(CHAT_ACTIVE_THREAD_KEY)
  dispatchClientEvent(CHAT_ACTIVE_THREAD_CHANGED_EVENT, { threadId: null })
}

export function readChatThreadHistory(): ChatThreadHistoryItem[] {
  return readClientJson(CHAT_THREAD_HISTORY_KEY, isChatThreadHistory) ?? []
}

export function upsertChatThreadHistory(input: ChatThreadHistoryInput): ChatThreadHistoryItem[] {
  const item = normalizeHistoryInput(input)
  if (!item) return readChatThreadHistory()

  const existing = readChatThreadHistory()
  const current = existing.find((candidate) => candidate.threadId === item.threadId)
  const resolved = withPinnedCarry(withTitleLock(item, current), current)

  // Replace in place, then order by activity. Position derives from
  // `updatedAt`, never from write order — selecting an old thread rewrites its
  // entry (self-heal) without hoisting it to the top.
  const merged = current
    ? existing.map((candidate) => (candidate.threadId === resolved.threadId ? resolved : candidate))
    : [resolved, ...existing]
  const next = sortByUpdatedAtDesc(merged).slice(0, maxThreadHistoryItems)

  writeClientJson(CHAT_THREAD_HISTORY_KEY, next)
  dispatchClientEvent(CHAT_THREAD_HISTORY_CHANGED_EVENT, { sessions: next })
  return next
}

/**
 * Toggle whether a thread is pinned to the top of the sidebar. A no-op when
 * the thread has no history entry (nothing to pin). Goes through
 * {@link upsertChatThreadHistory} so the title lock, sort, and
 * change-event dispatch all stay in the one place that already owns them.
 */
export function togglePinnedChatThread(threadId: string): ChatThreadHistoryItem[] {
  const normalized = normalizeId(threadId)
  if (!normalized) return readChatThreadHistory()
  const current = readChatThreadHistory().find((item) => item.threadId === normalized)
  if (!current) return readChatThreadHistory()
  return upsertChatThreadHistory({
    threadId: current.threadId,
    title: current.title,
    titleKind: current.titleKind,
    preview: current.preview,
    updatedAt: current.updatedAt,
    pinned: !current.pinned,
  })
}

export function replaceChatThreadHistory(inputs: ChatThreadHistoryInput[]): ChatThreadHistoryItem[] {
  const stored = readChatThreadHistory()
  const seen = new Set<string>()
  const collected: ChatThreadHistoryItem[] = []
  for (const input of inputs) {
    const item = normalizeHistoryInput(input)
    if (!item || seen.has(item.threadId)) continue
    seen.add(item.threadId)
    // Server session lists carry no titleKind or pin state (pinning is
    // local-only); without these locks a sidebar refresh racing the debounced
    // server snapshot would clobber a freshly generated title, and a full
    // server-session resync would silently unpin every pinned thread.
    const matchingStored = stored.find((candidate) => candidate.threadId === item.threadId)
    collected.push(withPinnedCarry(withTitleLock(item, matchingStored), matchingStored))
  }
  // Sort BEFORE truncating, exactly as `upsertChatThreadHistory` already does.
  // Truncating first — which this used to do, breaking out of the loop at the
  // cap in raw server order — silently destroyed pins: the gateway returns up
  // to 80 threads, `sortByUpdatedAtDesc` puts pinned ones first, and a user
  // with more than 40 threads who pinned an old one had it cut before the sort
  // could ever rescue it. Because the truncated list is then written straight
  // back to storage, the pin AND the thread were gone from the sidebar for good
  // on the next resync.
  const next = sortByUpdatedAtDesc(collected).slice(0, maxThreadHistoryItems)

  writeClientJson(CHAT_THREAD_HISTORY_KEY, next)
  dispatchClientEvent(CHAT_THREAD_HISTORY_CHANGED_EVENT, { sessions: next })
  return next
}

export function removeChatThreadHistoryItem(threadId: string): ChatThreadHistoryItem[] {
  const normalized = normalizeId(threadId)
  if (!normalized) return readChatThreadHistory()

  const next = readChatThreadHistory().filter((item) => item.threadId !== normalized)
  writeClientJson(CHAT_THREAD_HISTORY_KEY, next)
  removeChatThreadTranscript(normalized)
  dispatchClientEvent(CHAT_THREAD_HISTORY_CHANGED_EVENT, { sessions: next })
  return next
}

export function clearChatThreadHistory(): void {
  removeClientValue(CHAT_THREAD_HISTORY_KEY)
  removeClientValue(CHAT_THREAD_TRANSCRIPTS_KEY)
  dispatchClientEvent(CHAT_THREAD_HISTORY_CHANGED_EVENT, { sessions: [] })
}

export function selectChatThread(threadId: string): void {
  setActiveChatThreadId(threadId)
}

export function readChatThreadTranscript(threadId: string): ChatThreadTranscript | null {
  const normalized = normalizeId(threadId)
  if (!normalized) return null
  return readChatThreadTranscripts().find((item) => item.threadId === normalized) ?? null
}

export function upsertChatThreadTranscript(input: ChatThreadTranscriptInput): ChatThreadTranscript[] {
  const item = normalizeTranscriptInput(input)
  if (!item) return readChatThreadTranscripts()

  const next = [
    item,
    ...readChatThreadTranscripts().filter((candidate) => candidate.threadId !== item.threadId),
  ].slice(0, maxThreadTranscriptItems)

  writeClientJson(CHAT_THREAD_TRANSCRIPTS_KEY, next)
  return next
}

export function removeChatThreadTranscript(threadId: string): ChatThreadTranscript[] {
  const normalized = normalizeId(threadId)
  if (!normalized) return readChatThreadTranscripts()

  const next = readChatThreadTranscripts().filter((item) => item.threadId !== normalized)
  writeClientJson(CHAT_THREAD_TRANSCRIPTS_KEY, next)
  return next
}

function readChatThreadTranscripts(): ChatThreadTranscript[] {
  return readClientJson(CHAT_THREAD_TRANSCRIPTS_KEY, isChatThreadTranscripts) ?? []
}

function normalizeHistoryInput(input: ChatThreadHistoryInput): ChatThreadHistoryItem | null {
  const threadId = normalizeId(input.threadId)
  if (!threadId) return null

  const titleKind = normalizeTitleKind(input.titleKind)
  return {
    threadId,
    title: normalizeDisplayText(input.title, 'Velion Chat', maxTitleLength),
    ...(titleKind ? { titleKind } : {}),
    preview: normalizeDisplayText(input.preview, 'Open live session', maxPreviewLength),
    updatedAt: normalizeTimestamp(input.updatedAt),
    // Only present when the caller explicitly passed a boolean — absent
    // (undefined) means "unspecified", which `withPinnedCarry` resolves
    // against whatever is already stored rather than treating it as unpin.
    ...(typeof input.pinned === 'boolean' ? { pinned: input.pinned } : {}),
  }
}

function normalizeTitleKind(value: ChatThreadTitleKind | undefined): ChatThreadTitleKind | undefined {
  return value === 'generated' || value === 'preview' ? value : undefined
}

/**
 * Title lock: an AI-generated title survives every write whose own title is
 * merely a preview (periodic snapshots, server session syncs, legacy callers).
 * Only a newer `generated` title may replace it.
 */
function withTitleLock(
  item: ChatThreadHistoryItem,
  current: ChatThreadHistoryItem | undefined,
): ChatThreadHistoryItem {
  if (current?.titleKind === 'generated' && item.titleKind !== 'generated') {
    return { ...item, title: current.title, titleKind: 'generated' }
  }
  return item
}

/**
 * Pin carry, mirroring {@link withTitleLock}: the vast majority of writes
 * (periodic snapshots, server-session resyncs) never mention `pinned` at
 * all, and must not silently unpin an item the user pinned earlier. Only an
 * input that explicitly set `pinned` (`togglePinnedChatThread`, or a caller
 * that legitimately knows the pin state) may change it — a `false` there
 * still drops the key entirely so an unpinned item's stored shape is
 * identical to one that was never pinned.
 */
function withPinnedCarry(
  item: ChatThreadHistoryItem,
  current: ChatThreadHistoryItem | undefined,
): ChatThreadHistoryItem {
  if (item.pinned !== undefined) {
    if (item.pinned) return item
    const unpinned: ChatThreadHistoryItem = { ...item }
    delete unpinned.pinned
    return unpinned
  }
  return current?.pinned ? { ...item, pinned: true } : item
}

/**
 * Order history by pin state, then by activity (`updatedAt` descending)
 * within each tier: pinned items first, unpinned after. Stable within each
 * tier: equal timestamps keep their relative order, so a rewrite that
 * preserves the timestamp also preserves position — selection never reads as
 * activity. With no pinned items this is exactly the original single-tier
 * sort, so every caller that never pins anything is unaffected.
 */
function sortByUpdatedAtDesc(items: ChatThreadHistoryItem[]): ChatThreadHistoryItem[] {
  const byUpdatedAtDesc = (left: ChatThreadHistoryItem, right: ChatThreadHistoryItem) =>
    Date.parse(right.updatedAt) - Date.parse(left.updatedAt)
  const pinned = items.filter((item) => item.pinned).sort(byUpdatedAtDesc)
  const unpinned = items.filter((item) => !item.pinned).sort(byUpdatedAtDesc)
  return [...pinned, ...unpinned]
}

function normalizeId(value: string | null | undefined): string | null {
  const normalized = value?.trim()
  return normalized ? normalized : null
}

function normalizeDisplayText(value: string | undefined, fallback: string, maxLength: number): string {
  const normalized = value?.replace(/\s+/g, ' ').trim()
  const text = normalized || fallback
  return text.length > maxLength ? `${text.slice(0, maxLength - 3).trimEnd()}...` : text
}

function normalizeTimestamp(value: string | undefined): string {
  if (!value) return new Date().toISOString()
  const parsed = Date.parse(value)
  return Number.isNaN(parsed) ? new Date().toISOString() : new Date(parsed).toISOString()
}

function normalizeTranscriptInput(input: ChatThreadTranscriptInput): ChatThreadTranscript | null {
  const threadId = normalizeId(input.threadId)
  if (!threadId) return null
  const turns = input.turns.map(normalizeTranscriptTurn).filter((turn): turn is ChatThreadTranscriptTurn => Boolean(turn))
  return {
    threadId,
    taskSteps: normalizeTranscriptSteps(input.taskSteps),
    turns: turns.slice(-maxTranscriptTurns),
    updatedAt: normalizeTimestamp(input.updatedAt),
  }
}

function normalizeTranscriptTurn(turn: ChatThreadTranscriptTurn): ChatThreadTranscriptTurn | null {
  const id = normalizeId(turn.id)
  const role = turn.role === 'assistant' ? 'assistant' : turn.role === 'user' ? 'user' : null
  if (!id || !role) return null
  return {
    id,
    role,
    artifacts: Array.isArray(turn.artifacts) ? turn.artifacts : undefined,
    content: normalizeTranscriptContent(turn.content),
    citations: Array.isArray(turn.citations) ? turn.citations : undefined,
    confidence: normalizeOptionalNumber(turn.confidence),
    costUsd: normalizeOptionalNumber(turn.costUsd),
    createdAt: normalizeTimestamp(turn.createdAt),
    files: Array.isArray(turn.files) ? turn.files : undefined,
    grounding: isRecord(turn.grounding) ? turn.grounding : undefined,
    inputTokens: normalizeOptionalNumber(turn.inputTokens),
    latencyMs: normalizeOptionalNumber(turn.latencyMs),
    model: normalizeOptionalText(turn.model),
    modelUsed: normalizeOptionalText(turn.modelUsed),
    outputTokens: normalizeOptionalNumber(turn.outputTokens),
    reasoning: normalizeOptionalText(turn.reasoning),
    requestId: normalizeOptionalText(turn.requestId),
    status: normalizeTranscriptTurnStatus(turn.status),
    toolCalls: Array.isArray(turn.toolCalls) ? turn.toolCalls : undefined,
    tools: normalizeToolIds(turn.tools),
    attachments: Array.isArray(turn.attachments) ? turn.attachments : undefined,
  }
}

function normalizeTranscriptSteps(value: ChatThreadTranscriptStep[] | undefined): ChatThreadTranscriptStep[] | undefined {
  if (!Array.isArray(value)) return undefined
  const steps = value.map(normalizeTranscriptStep).filter((step): step is ChatThreadTranscriptStep => Boolean(step))
  return steps.length > 0 ? steps.slice(-maxTranscriptSteps) : undefined
}

function normalizeTranscriptStep(step: ChatThreadTranscriptStep): ChatThreadTranscriptStep | null {
  const id = normalizeId(step.id)
  const title = normalizeOptionalText(step.title)
  const status = normalizeTranscriptStepStatus(step.status)
  if (!id || !title || !status) return null
  return {
    id,
    title,
    detail: normalizeTranscriptDetail(step.detail),
    status,
    createdAt: normalizeTimestamp(step.createdAt),
    expandedDetail: normalizeOptionalLongText(step.expandedDetail),
    evidence: normalizeTranscriptStepEvidence(step.evidence),
    turnId: normalizeOptionalText(step.turnId),
    turnTitle: normalizeOptionalText(step.turnTitle),
  }
}

function normalizeTranscriptStepEvidence(value: ChatThreadTranscriptStepEvidence[] | undefined): ChatThreadTranscriptStepEvidence[] | undefined {
  if (!Array.isArray(value)) return undefined
  const evidence = value.map(normalizeTranscriptStepEvidenceItem).filter((item): item is ChatThreadTranscriptStepEvidence => Boolean(item))
  return evidence.length > 0 ? evidence : undefined
}

function normalizeTranscriptStepEvidenceItem(item: ChatThreadTranscriptStepEvidence): ChatThreadTranscriptStepEvidence | null {
  const id = normalizeId(item.id)
  const label = normalizeOptionalText(item.label)
  const value = normalizeOptionalText(item.value)
  if (!id || !label || !value) return null
  return {
    id,
    label,
    value,
    href: normalizeOptionalText(item.href),
  }
}

function normalizeTranscriptContent(value: string): string {
  const text = typeof value === 'string' ? value : ''
  return text.length > maxTranscriptContentLength ? text.slice(0, maxTranscriptContentLength) : text
}

function normalizeTranscriptDetail(value: string): string {
  const text = typeof value === 'string' ? value : ''
  return text.length > maxTranscriptDetailLength ? text.slice(0, maxTranscriptDetailLength) : text
}

function normalizeOptionalLongText(value: string | undefined): string | undefined {
  const normalized = normalizeOptionalText(value)
  if (!normalized) return undefined
  return normalized.length > maxTranscriptDetailLength ? normalized.slice(0, maxTranscriptDetailLength) : normalized
}

function normalizeOptionalText(value: string | undefined): string | undefined {
  const normalized = value?.trim()
  return normalized || undefined
}

function normalizeOptionalNumber(value: number | undefined): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined
}

function normalizeTranscriptTurnStatus(value: ChatThreadTranscriptTurn['status'] | undefined): ChatThreadTranscriptTurn['status'] | undefined {
  return value === 'error' || value === 'stopped' || value === 'waiting' ? value : undefined
}

function normalizeTranscriptStepStatus(value: ChatThreadTranscriptStep['status'] | undefined): ChatThreadTranscriptStep['status'] | null {
  return value === 'active' || value === 'done' || value === 'error' || value === 'stopped' || value === 'waiting'
    ? value
    : null
}

function normalizeToolIds(value: string[] | undefined): string[] | undefined {
  if (!Array.isArray(value)) return undefined
  const tools = [...new Set(value.map((item) => item.trim()).filter(Boolean))]
  return tools.length > 0 ? tools : undefined
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}

function isChatThreadHistory(value: unknown): value is ChatThreadHistoryItem[] {
  return Array.isArray(value) && value.every(isChatThreadHistoryItem)
}

function isChatThreadHistoryItem(value: unknown): value is ChatThreadHistoryItem {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.threadId === 'string' &&
    Boolean(record.threadId.trim()) &&
    typeof record.title === 'string' &&
    Boolean(record.title.trim()) &&
    // Optional so items stored before titleKind/pinned existed keep validating.
    (record.titleKind === undefined || record.titleKind === 'generated' || record.titleKind === 'preview') &&
    typeof record.preview === 'string' &&
    typeof record.updatedAt === 'string' &&
    !Number.isNaN(Date.parse(record.updatedAt)) &&
    (record.pinned === undefined || typeof record.pinned === 'boolean')
  )
}

function isChatThreadTranscripts(value: unknown): value is ChatThreadTranscript[] {
  return Array.isArray(value) && value.every(isChatThreadTranscript)
}

function isChatThreadTranscript(value: unknown): value is ChatThreadTranscript {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.threadId === 'string' &&
    Boolean(record.threadId.trim()) &&
    typeof record.updatedAt === 'string' &&
    !Number.isNaN(Date.parse(record.updatedAt)) &&
    Array.isArray(record.turns) &&
    record.turns.every(isChatThreadTranscriptTurn) &&
    (
      record.taskSteps === undefined ||
      (Array.isArray(record.taskSteps) && record.taskSteps.every(isChatThreadTranscriptStep))
    )
  )
}

function isChatThreadTranscriptTurn(value: unknown): value is ChatThreadTranscriptTurn {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.id === 'string' &&
    Boolean(record.id.trim()) &&
    (record.role === 'assistant' || record.role === 'user') &&
    typeof record.content === 'string' &&
    typeof record.createdAt === 'string' &&
    !Number.isNaN(Date.parse(record.createdAt)) &&
    (record.model === undefined || typeof record.model === 'string') &&
    (record.modelUsed === undefined || typeof record.modelUsed === 'string') &&
    (record.requestId === undefined || typeof record.requestId === 'string') &&
    (record.status === undefined || record.status === 'error' || record.status === 'stopped' || record.status === 'waiting') &&
    (record.inputTokens === undefined || typeof record.inputTokens === 'number') &&
    (record.outputTokens === undefined || typeof record.outputTokens === 'number') &&
    (record.latencyMs === undefined || typeof record.latencyMs === 'number') &&
    (record.costUsd === undefined || typeof record.costUsd === 'number') &&
    (record.confidence === undefined || typeof record.confidence === 'number') &&
    (record.reasoning === undefined || typeof record.reasoning === 'string') &&
    (record.citations === undefined || Array.isArray(record.citations)) &&
    (record.toolCalls === undefined || Array.isArray(record.toolCalls)) &&
    (record.artifacts === undefined || Array.isArray(record.artifacts)) &&
    (record.files === undefined || Array.isArray(record.files)) &&
    (record.grounding === undefined || isRecord(record.grounding)) &&
    (record.tools === undefined || (Array.isArray(record.tools) && record.tools.every((tool) => typeof tool === 'string'))) &&
    (record.attachments === undefined || Array.isArray(record.attachments))
  )
}

function isChatThreadTranscriptStep(value: unknown): value is ChatThreadTranscriptStep {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.id === 'string' &&
    Boolean(record.id.trim()) &&
    typeof record.title === 'string' &&
    Boolean(record.title.trim()) &&
    typeof record.detail === 'string' &&
    (record.status === 'active' || record.status === 'done' || record.status === 'error' || record.status === 'stopped' || record.status === 'waiting') &&
    typeof record.createdAt === 'string' &&
    !Number.isNaN(Date.parse(record.createdAt)) &&
    (record.expandedDetail === undefined || typeof record.expandedDetail === 'string') &&
    (record.turnId === undefined || typeof record.turnId === 'string') &&
    (record.turnTitle === undefined || typeof record.turnTitle === 'string') &&
    (
      record.evidence === undefined ||
      (Array.isArray(record.evidence) && record.evidence.every(isChatThreadTranscriptStepEvidence))
    )
  )
}

function isChatThreadTranscriptStepEvidence(value: unknown): value is ChatThreadTranscriptStepEvidence {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.id === 'string' &&
    Boolean(record.id.trim()) &&
    typeof record.label === 'string' &&
    Boolean(record.label.trim()) &&
    typeof record.value === 'string' &&
    Boolean(record.value.trim()) &&
    (record.href === undefined || typeof record.href === 'string')
  )
}

function dispatchClientEvent(name: string, detail: unknown): void {
  if (typeof window === 'undefined') return
  window.dispatchEvent(new CustomEvent(name, { detail }))
}
