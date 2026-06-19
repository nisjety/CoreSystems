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

export type ChatThreadHistoryItem = {
  preview: string
  threadId: string
  title: string
  updatedAt: string
}

export type ChatThreadHistoryInput = {
  preview?: string
  threadId: string
  title?: string
  updatedAt?: string
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

  const next = [
    item,
    ...readChatThreadHistory().filter((candidate) => candidate.threadId !== item.threadId),
  ].slice(0, maxThreadHistoryItems)

  writeClientJson(CHAT_THREAD_HISTORY_KEY, next)
  dispatchClientEvent(CHAT_THREAD_HISTORY_CHANGED_EVENT, { sessions: next })
  return next
}

export function replaceChatThreadHistory(inputs: ChatThreadHistoryInput[]): ChatThreadHistoryItem[] {
  const seen = new Set<string>()
  const next: ChatThreadHistoryItem[] = []
  for (const input of inputs) {
    const item = normalizeHistoryInput(input)
    if (!item || seen.has(item.threadId)) continue
    seen.add(item.threadId)
    next.push(item)
    if (next.length >= maxThreadHistoryItems) break
  }

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

  return {
    threadId,
    title: normalizeDisplayText(input.title, 'Velion Chat', maxTitleLength),
    preview: normalizeDisplayText(input.preview, 'Open live session', maxPreviewLength),
    updatedAt: normalizeTimestamp(input.updatedAt),
  }
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
    typeof record.preview === 'string' &&
    typeof record.updatedAt === 'string' &&
    !Number.isNaN(Date.parse(record.updatedAt))
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
