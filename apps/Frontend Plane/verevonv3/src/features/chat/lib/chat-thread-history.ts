import {
  readClientJson,
  readClientValue,
  removeClientValue,
  writeClientJson,
  writeClientValue,
} from '@/shared/session/client-storage'
import type { ChatEffectClass } from '@/shared/chat/effect-class'
import type { ConversationNode } from '@/shared/chat-nodes'
import type { ThreadGoalStatus } from '@/shared/api/thread-goal-client'
import { isAttentionState, type AttentionState } from '@/shared/attention/attention-state'

export const CHAT_ACTIVE_THREAD_KEY = 'verevon.chat.threadId'
export const CHAT_THREAD_HISTORY_KEY = 'verevon.chat.threadHistory.v1'
export const CHAT_THREAD_TRANSCRIPTS_KEY = 'verevon.chat.threadTranscripts.v1'
export const CHAT_THREAD_HISTORY_CHANGED_EVENT = 'verevon:chat-thread-history-changed'
export const CHAT_ACTIVE_THREAD_CHANGED_EVENT = 'verevon:chat-active-thread-changed'

const maxThreadHistoryItems = 40
const maxThreadTranscriptItems = 40
const maxTranscriptTurns = 120
const maxTranscriptSteps = 240
const maxTitleLength = 96
const maxPreviewLength = 180
const maxTranscriptContentLength = 32_000
const maxTranscriptDetailLength = 32_000

/**
 * The cap a rename UI should enforce on the input itself.
 *
 * Exported rather than re-declared: `normalizeHistoryInput` already truncates
 * a stored title to this length (with an ellipsis), so a rename control that
 * used a different number could accept text the store would then silently
 * shorten out from under it.
 */
export const CHAT_THREAD_TITLE_MAX_LENGTH = maxTitleLength

/**
 * How a history item's title was produced.
 * - `generated`: AI summary from the model-gateway `title` SSE event — locked:
 *   a later `preview` write must not clobber it.
 * - `preview`: truncated first user message (the fallback).
 * Absent on legacy stored items; treated as `preview`.
 */
export type ChatThreadTitleKind = 'generated' | 'preview'

/**
 * A thread's own memory mode (WS07-S6): `inherit` follows the person's memory settings, `off` keeps this thread
 * out of memory. Absent reads as `inherit`. Only these two closed values are ever stored or sent.
 */
export type ChatThreadMemoryMode = 'inherit' | 'off'

export type ChatThreadHistoryItem = {
  preview: string
  threadId: string
  title: string
  titleKind?: ChatThreadTitleKind
  updatedAt: string
  /** Server-owned latest run hint used by the calm thread rail. */
  latestRunId?: string
  latestRunStatus?: string
  latestRunUpdatedAt?: string
  /** User-pinned to the top of the sidebar (see `togglePinnedChatThread`). Local-only — never synced from the server. */
  pinned?: boolean
  /**
   * Persona spec — the per-thread override of the user's global persona
   * switch. Absent means enabled (the opposite convention from `pinned`,
   * which stores `true` and omits `false`): see `withPersonaEnabledCarry`.
   * Server-owned via `ChatThreadSession.personaEnabled`
   * (`chat-client.ts`), same as `pinned`.
   */
  personaEnabled?: boolean
  /**
   * The thread's memory mode (WS07-S6). Server-owned via `ChatThreadSession.memoryMode`; a local write keeps
   * the stored mode (see `withMemoryModeCarry`), and a server list is what it reads.
   */
  memoryMode?: ChatThreadMemoryMode
  /** Which surface created this thread (session-core's `ThreadSummary.origin`
   * — see `ChatThreadSession.origin` in chat-client.ts). Set once at
   * creation and never changes, so it always carries forward once known
   * (see `withOriginCarry`) rather than needing an explicit resend like
   * `pinned` does. */
  origin?: string
  /**
   * The thread's goal status (EP-7 S5): one closed token (`active`, `paused`, `met`,
   * `budget_reached`, `turns_reached` or `blocked`), read through {@link chatThreadGoalStatus}.
   * Server-owned, and present only while the server's goal and attention switches are on. A server list
   * replaces it; a local write keeps it.
   */
  goalStatus?: string
  /** Whether the owner starred the thread. Server-owned like `goalStatus`: a server list replaces it, a local write keeps it (see `withThreadStateCarry`). Absent when not starred. */
  starred?: boolean
  /** Whether the thread holds a reply the owner has not read. Server-owned: the client never derives it. Absent when read. */
  unread?: boolean
  /** The thread this one was copied from (WS17 S6). Set once at creation, so a local write keeps it. Absent for a thread that was not forked. */
  forkedFromThreadId?: string
  /**
   * How many threads were forked from this one (F-f05-23), as the server's thread list states it while the thread-state
   * switch is on. A local write keeps it (see `withThreadStateCarry`); absent when no thread was forked from it.
   */
  forkCount?: number
  /**
   * The server's attention state of the thread (WS16 S1), as a thread list states it while the attention switch is on.
   * A local write keeps it only while it leaves the run as it was (see `withAttentionCarry`). Once a local write moves
   * the run, the row reads its legacy run status (`threadAttentionState`) until the next list read.
   */
  attentionState?: AttentionState
}

export type ChatThreadHistoryInput = {
  preview?: string
  threadId: string
  title?: string
  titleKind?: ChatThreadTitleKind
  updatedAt?: string
  /** Optional server-owned latest run hint; omitted fields carry forward. */
  latestRunId?: string
  latestRunStatus?: string
  latestRunUpdatedAt?: string
  /** Omit to leave the stored pin state untouched (see `withPinnedCarry`). */
  pinned?: boolean
  /** Omit to leave the stored persona-enabled state untouched (see
   * `withPersonaEnabledCarry`). */
  personaEnabled?: boolean
  /** Omit to leave the stored memory mode untouched (see `withMemoryModeCarry`). */
  memoryMode?: ChatThreadMemoryMode
  /** Omit to leave the stored origin untouched (see `withOriginCarry`) — a
   * local snapshot write never knows it and must not erase it. */
  origin?: string
  /** Omit to leave the stored goal status untouched (see `withGoalStatusCarry`). Only a closed
   * token is kept; any other value reads as no goal. */
  goalStatus?: string
  /** Omit to leave the stored star untouched (see `withThreadStateCarry`). Only `true` is kept. */
  starred?: boolean
  /** Omit to leave the stored unread state untouched. Only `true` is kept. */
  unread?: boolean
  /** Omit to leave the stored lineage untouched. A blank value is dropped. */
  forkedFromThreadId?: string
  /** Omit to leave the stored fork count untouched (see `withThreadStateCarry`). Only a count above zero is kept. */
  forkCount?: number
  /** The server's attention state from a thread list (see `ChatThreadHistoryItem.attentionState`). Only a closed state is kept. */
  attentionState?: AttentionState
}

/** The server's flags for one thread, as the cache holds them. Absent means not starred, read, or forked. */
export type ChatThreadState = Pick<ChatThreadHistoryInput, 'starred' | 'unread' | 'forkedFromThreadId' | 'forkCount'>

export type ChatThreadTranscriptTurn = {
  /** The connected-app suggestion card (EP-10 S8), read back as a restored note. */
  connectorSuggestion?: unknown
  sourceScope?: import('@/shared/actions/chat-source-scope').ChatSourceScope
  /** See `ChatMessage.retrievalScope`. */
  retrievalScope?: import('@/shared/actions/chat-source-scope').RetrievalScope
  /** The chips a user turn was sent with (EP-3 S5), as the BFF re-emits `metadata.context_refs`: kind, id, label only. */
  contextRefs?: unknown[]
  /** The plan an assistant plan turn ended with (EP-2 S5), as the BFF re-emits `metadata.research_plan`: sub-queries and questions only. */
  researchPlan?: { sub_queries?: unknown; questions?: unknown }
  attachments?: unknown[]
  artifacts?: unknown[]
  citations?: unknown[]
  content: string
  createdAt: string
  costUsd?: number
  /**
   * How the cost was reached. Persisted because a subscription turn stores no
   * `costUsd` at all, and without its basis a reload could no longer say
   * "included in the subscription": the cost row would silently vanish.
   */
  costBasis?: 'metered' | 'subscription'
  /**
   * The `turn_timing` totals (wire `server_ms`, `provider_ms`,
   * `orchestration_ms`), so "Modelltid" and "Systemtid" survive a reload. The
   * span list stays live-only.
   */
  timing?: ChatThreadTranscriptTiming
  /** Server-derived effect evidence from the durable run proof bundle. */
  effectClass?: ChatEffectClass
  confidence?: number
  /**
   * What the backend's verification pass found, persisted so the notice
   * survives a reload — the same reason `memoryRecallCount` below is
   * stored. Shaped from the confidence node rather than restated, so the
   * snapshot cannot drift from what the renderer reads.
   */
  verification?: Extract<ConversationNode, { kind: 'confidence' }>['verification']
  files?: unknown[]
  grounding?: unknown
  id: string
  inputTokens?: number
  latencyMs?: number
  /**
   * How many long-term memories this turn recalled. Persisted so the notice
   * survives a reload — without it the chip appeared live and vanished on
   * reopen, which reads as the recall not having happened.
   */
  memoryRecallCount?: number
  /**
   * Which memories the turn recalled. Persisted for the same reason the count
   * is: a disclosure that vanishes on reopen reads as though the recall never
   * happened, and the whole point is being able to go back and correct a wrong
   * remembered fact.
   */
  recalledMemories?: unknown[]
  model?: string
  modelUsed?: string
  /** Explicit model route used when this turn was submitted. */
  provider?: string
  /** Opaque user-owned model subscription connection id. */
  subscriptionConnectionId?: string
  /** Opaque customer-gateway connection id this turn was served on (W4-BYO-6), kept so a reopened thread still knows it. */
  modelConnectionId?: string
  outputTokens?: number
  /** Durable orchestration run id for an agentic/Do turn. */
  runId?: string
  /** Whether this turn was submitted as a plan/Do run. */
  planMode?: boolean
  /** Durable autonomy grant recorded on a plan/Do run. */
  grantedRung?: 'read_only' | 'workspace_write' | 'danger_full_access'
  reasoning?: string
  requestId?: string
  role: 'assistant' | 'user'
  status?: 'error' | 'stopped' | 'waiting'
  /**
   * Why generation stopped. Persisted because the truncation notice is a
   * CORRECTNESS warning about the content: an answer that "may be cut off"
   * silently becoming a normal-looking answer on reload is the worst version of
   * losing it.
   */
  stopReason?: string
  toolCalls?: unknown[]
  tools?: string[]
}

/** A turn's server time split, in milliseconds on the gateway's own clock. */
export type ChatThreadTranscriptTiming = {
  serverMs: number
  providerMs: number
  orchestrationMs: number
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

/** The goal statuses a thread row may show (EP-7 S5): the closed set the server reports. */
const CHAT_THREAD_GOAL_STATUSES: readonly ThreadGoalStatus[] = [
  'active',
  'paused',
  'met',
  'budget_reached',
  'turns_reached',
  'blocked',
]

/**
 * The goal status when it is one of the closed tokens, otherwise `undefined`. A word outside the set
 * reads as no goal, so nothing but a status token reaches a thread row through the history store.
 */
export function chatThreadGoalStatus(value: string | undefined): ThreadGoalStatus | undefined {
  return CHAT_THREAD_GOAL_STATUSES.find((candidate) => candidate === value)
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
  const carriedRun = withLatestRunCarry(withPersonaEnabledCarry(withPinnedCarry(withThreadStateCarry(withTitleLock(item, current), current), current), current), current)
  const resolved = withMemoryModeCarry(withGoalStatusCarry(withOriginCarry(withAttentionCarry(carriedRun, current), current), current), current)

  // Replace in place, then order by activity. Position derives from
  // `updatedAt`, never from write order — selecting an old thread rewrites its
  // entry (self-heal) without hoisting it to the top.
  const merged = current
    ? existing.map((candidate) => (candidate.threadId === resolved.threadId ? resolved : candidate))
    : [resolved, ...existing]
  const next = sortByUpdatedAtDesc(merged).slice(0, historyRowLimit())

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
    latestRunId: current.latestRunId,
    latestRunStatus: current.latestRunStatus,
    latestRunUpdatedAt: current.latestRunUpdatedAt,
    pinned: !current.pinned,
  })
}

/**
 * Toggle a thread's per-thread persona override. Mirrors
 * {@link togglePinnedChatThread} exactly (same no-op guard, same
 * `upsertChatThreadHistory` plumbing) — see `withPersonaEnabledCarry` for
 * why the toggled value here is fine to pass explicitly even though it
 * carries the opposite default from `pinned`.
 */
export function togglePersonaEnabledChatThread(threadId: string): ChatThreadHistoryItem[] {
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
    latestRunId: current.latestRunId,
    latestRunStatus: current.latestRunStatus,
    latestRunUpdatedAt: current.latestRunUpdatedAt,
    pinned: current.pinned,
    personaEnabled: !(current.personaEnabled ?? true),
  })
}

/** The switch the sidebar's memory control reads (WS07-S6): only the literal `true` turns it on. Read at call time. */
export function chatThreadMemoryModeEnabled(): boolean {
  return import.meta.env.VITE_CHAT_THREAD_MEMORY_MODE === 'true'
}

/**
 * The paged sidebar (VITE_CHAT_SIDEBAR_SERVER_PAGING): the active list comes one server page at a time, and the local
 * cache keeps every row it has loaded. Only the literal `true` turns it on. Read at call time.
 */
export function chatSidebarServerPagingEnabled(): boolean {
  return import.meta.env.VITE_CHAT_SIDEBAR_SERVER_PAGING === 'true'
}

/**
 * The server thread state in the sidebar (VITE_CHAT_SIDEBAR_THREAD_STATE): the unread dot, the Starred filter, the Forked
 * chip and the search opener read the server's flags. Only the literal `true` turns it on. Read at call time.
 */
export function chatSidebarThreadStateEnabled(): boolean {
  return import.meta.env.VITE_CHAT_SIDEBAR_THREAD_STATE === 'true'
}

/** This browser's record that its local chats have moved to the server once (see CoreSidebar's migration step). */
export const CHAT_SIDEBAR_MIGRATED_KEY = 'verevon.chat.sidebar-migrated.v1'

/**
 * Whether the local chats have already moved to the server, so the paged sidebar may start from the first page. Blocked
 * storage answers no, and the whole listing is read as before.
 */
export function chatSidebarMigrated(): boolean {
  try {
    return window.localStorage.getItem(CHAT_SIDEBAR_MIGRATED_KEY) === 'true'
  } catch {
    return false
  }
}

export function markChatSidebarMigrated(): void {
  try {
    window.localStorage.setItem(CHAT_SIDEBAR_MIGRATED_KEY, 'true')
  } catch {
    // Blocked storage: the next load reads the whole listing again, which is the path before paging.
  }
}

/**
 * Whether the sidebar lists its active chats through the paged listing: the switch is on and the local chats have moved to
 * the server. Until both hold, the sidebar reads the whole listing and the cache keeps its first 40 rows, as before.
 */
export function chatSidebarPagedListing(): boolean {
  return chatSidebarServerPagingEnabled() && chatSidebarMigrated()
}

/**
 * The mode a memory toggle moves to: a thread that follows the person's memory settings (`inherit`, or absent)
 * turns it off for this thread, and an off thread inherits again. Pure: it reads only its argument.
 */
export function toggleThreadMemoryMode(current: ChatThreadMemoryMode | undefined): ChatThreadMemoryMode {
  return current === 'off' ? 'inherit' : 'off'
}

/**
 * Whether the sidebar's thread menu offers the memory control for one row (WS07-S6). It needs the switch on, a
 * chat thread (a Space, mail-task or other surface's thread is not one), and a device that may keep conversation
 * content: a zero-retention posture hides it. A temporary chat never reaches this list, so it has no row here.
 */
export function chatThreadMemoryControlVisible(
  item: Pick<ChatThreadHistoryItem, 'origin'>,
  options: { enabled: boolean; localRetentionAllowed: boolean },
): boolean {
  return options.enabled && options.localRetentionAllowed && item.origin === 'chat'
}

/**
 * Set one thread's memory mode in the local cache, ahead of the server's answer, as the other optimistic writes
 * do: it goes through {@link upsertChatThreadHistory}. A no-op for a thread with no history entry.
 */
export function setChatThreadMemoryModeLocally(threadId: string, mode: ChatThreadMemoryMode): ChatThreadHistoryItem[] {
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
    latestRunId: current.latestRunId,
    latestRunStatus: current.latestRunStatus,
    latestRunUpdatedAt: current.latestRunUpdatedAt,
    pinned: current.pinned,
    memoryMode: mode,
  })
}

/**
 * Apply a user-typed rename to a thread's local cache entry.
 *
 * A no-op when the thread has no history entry yet — same guard as
 * {@link togglePinnedChatThread}, which this otherwise mirrors. Goes through
 * {@link upsertChatThreadHistory} for the sort/change-event plumbing, but
 * always writes `titleKind: 'generated'` rather than passing the caller's own
 * kind through.
 *
 * An explicit rename is a stronger signal than either kind
 * {@link ChatThreadTitleKind} was built for — it is neither the model's own
 * summary nor the untouched first-message fallback — and it must survive the
 * exact same title lock a real AI title relies on ({@link withTitleLock}):
 * without it, the next routine preview snapshot (a composer autosave, a
 * server resync) would silently overwrite what the person just typed. The one
 * cost is that undoing a rename after a failed server write has to go through
 * this same function (with the pre-rename title) rather than
 * `upsertChatThreadHistory` directly — otherwise the revert write would itself
 * be blocked by the lock it just installed.
 */
export function renameChatThreadLocally(threadId: string, title: string): ChatThreadHistoryItem[] {
  const normalized = normalizeId(threadId)
  if (!normalized) return readChatThreadHistory()
  const current = readChatThreadHistory().find((item) => item.threadId === normalized)
  if (!current) return readChatThreadHistory()
  return upsertChatThreadHistory({
    threadId: current.threadId,
    title,
    titleKind: 'generated',
    preview: current.preview,
    updatedAt: current.updatedAt,
    latestRunId: current.latestRunId,
    latestRunStatus: current.latestRunStatus,
    latestRunUpdatedAt: current.latestRunUpdatedAt,
    pinned: current.pinned,
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
    // Server session lists carry no titleKind, so the title lock still does
    // real work: a sidebar refresh racing the debounced server snapshot would
    // otherwise clobber a freshly generated title.
    //
    // They DO carry pin state, contrary to what this comment used to claim.
    // `ChatThreadSession.pinned` is non-optional and normalized to a hard
    // boolean (`chat-client.ts`, `pinned: item.pinned === true`), so every
    // server thread arrives with an explicit `pinned` and `withPinnedCarry`
    // below takes its explicit branch — the carry-from-stored branch is
    // unreachable from this caller. That is correct rather than broken: the
    // server owns the pin now (both the sidebar toggle and the composer's
    // History panel write through to `PUT /api/v1/chat/threads/{id}`), so an
    // explicit `false` from the server is the truth, not a lost local pin.
    // Do not "restore" a local carry here — it would resurrect pins the user
    // removed on another device.
    const matchingStored = stored.find((candidate) => candidate.threadId === item.threadId)
    collected.push(withOriginCarry(withLatestRunCarry(withPersonaEnabledCarry(withPinnedCarry(withTitleLock(item, matchingStored), matchingStored), matchingStored), matchingStored), matchingStored))
  }
  // Sort BEFORE truncating, exactly as `upsertChatThreadHistory` already does.
  // Truncating first — which this used to do, breaking out of the loop at the
  // cap in raw server order — silently destroyed pins: the gateway returns up
  // to 80 threads, `sortByUpdatedAtDesc` puts pinned ones first, and a user
  // with more than 40 threads who pinned an old one had it cut before the sort
  // could ever rescue it. Because the truncated list is then written straight
  // back to storage, the pin AND the thread were gone from the sidebar for good
  // on the next resync.
  const next = sortByUpdatedAtDesc(collected).slice(0, historyRowLimit())

  writeClientJson(CHAT_THREAD_HISTORY_KEY, next)
  dispatchClientEvent(CHAT_THREAD_HISTORY_CHANGED_EVENT, { sessions: next })
  return next
}

/**
 * Set one thread's server flags (star, unread, lineage, fork count) in the cache to the server's answer as they stand: a flag
 * the server no longer sets reads as cleared. The sidebar refreshes only the open thread this way. A no-op for a thread with
 * no history entry.
 */
export function applyChatThreadStateLocally(threadId: string, state: ChatThreadState): ChatThreadHistoryItem[] {
  const normalized = normalizeId(threadId)
  const stored = readChatThreadHistory()
  if (!normalized || !stored.some((item) => item.threadId === normalized)) return stored
  const flags = threadStateOf(state)
  const next = stored.map((item) => {
    if (item.threadId !== normalized) return item
    const cleared: ChatThreadHistoryItem = { ...item }
    delete cleared.starred
    delete cleared.unread
    delete cleared.forkedFromThreadId
    delete cleared.forkCount
    return { ...cleared, ...flags }
  })
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

/**
 * Puts a copy a "Fork from here" just made into the local cache and opens it (F-f05-21). The copy joins the sidebar at once,
 * from what the server's session states and nothing more; the server's list stays the source of truth on the next load.
 */
export function openForkedChatThread(
  threadId: string,
  session?: Pick<ChatThreadHistoryInput, 'title' | 'preview' | 'updatedAt'>,
): void {
  upsertChatThreadHistory({ threadId, title: session?.title, preview: session?.preview, updatedAt: session?.updatedAt })
  selectChatThread(threadId)
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

/**
 * Drops the stored transcript of every thread the history cache no longer holds, except the open thread's. The paged sidebar
 * calls this after its first-page replace: a thread that left the cache (erased upstream, or past the first page) would
 * otherwise keep its conversation until the 40-entry cap evicted it, because only a row removal purges a transcript.
 */
export function pruneChatThreadTranscripts(): ChatThreadTranscript[] {
  const cached = new Set(readChatThreadHistory().map((item) => item.threadId))
  const active = readActiveChatThreadId()
  const stored = readChatThreadTranscripts()
  const kept = stored.filter((item) => cached.has(item.threadId) || item.threadId === active)
  if (kept.length !== stored.length) writeClientJson(CHAT_THREAD_TRANSCRIPTS_KEY, kept)
  return kept
}

function readChatThreadTranscripts(): ChatThreadTranscript[] {
  return readClientJson(CHAT_THREAD_TRANSCRIPTS_KEY, isChatThreadTranscripts) ?? []
}

function normalizeHistoryInput(input: ChatThreadHistoryInput): ChatThreadHistoryItem | null {
  const threadId = normalizeId(input.threadId)
  if (!threadId) return null

  const titleKind = normalizeTitleKind(input.titleKind)
  const goalStatus = chatThreadGoalStatus(input.goalStatus)
  return {
    threadId,
    title: normalizeDisplayText(input.title, 'Verevon Chat', maxTitleLength),
    ...(titleKind ? { titleKind } : {}),
    preview: normalizeDisplayText(input.preview, 'Open live session', maxPreviewLength),
    updatedAt: normalizeTimestamp(input.updatedAt),
    ...(normalizeOptionalText(input.latestRunId) ? { latestRunId: normalizeOptionalText(input.latestRunId) } : {}),
    ...(normalizeOptionalText(input.latestRunStatus) ? { latestRunStatus: normalizeOptionalText(input.latestRunStatus) } : {}),
    ...(input.latestRunUpdatedAt ? { latestRunUpdatedAt: normalizeTimestamp(input.latestRunUpdatedAt) } : {}),
    // Only present when the caller explicitly passed a boolean — absent
    // (undefined) means "unspecified", which `withPinnedCarry` resolves
    // against whatever is already stored rather than treating it as unpin.
    ...(typeof input.pinned === 'boolean' ? { pinned: input.pinned } : {}),
    // Same "only when explicit" contract as `pinned` above — absent means
    // "unspecified", which `withPersonaEnabledCarry` resolves against
    // whatever is already stored rather than treating it as enabled.
    ...(typeof input.personaEnabled === 'boolean' ? { personaEnabled: input.personaEnabled } : {}),
    // Only a closed memory mode survives. Absent is "unspecified", which `withMemoryModeCarry` resolves for a local write.
    ...(isChatThreadMemoryMode(input.memoryMode) ? { memoryMode: input.memoryMode } : {}),
    ...(normalizeOptionalText(input.origin) ? { origin: normalizeOptionalText(input.origin) } : {}),
    // Only a closed goal token survives. Absent is "unspecified": a local write keeps the stored token
    // (`withGoalStatusCarry`), and a server list reads it as no goal.
    ...(goalStatus ? { goalStatus } : {}),
    // Only a star, an unread reply and a non-blank lineage survive (see `threadStateOf`).
    ...threadStateOf(input),
    // Only a closed attention state survives. A write that does not state one leaves the row without it.
    ...(isAttentionState(input.attentionState) ? { attentionState: input.attentionState } : {}),
  }
}

/**
 * Rows the cache keeps: the first 40, as always, or every row the paged sidebar has loaded (see
 * {@link chatSidebarPagedListing}). A cap there would drop the rows past 40 on the next local write.
 */
function historyRowLimit(): number {
  return chatSidebarPagedListing() ? Number.POSITIVE_INFINITY : maxThreadHistoryItems
}

/** One server session as a history item, with its server flags kept (while the thread-state switch is on). Null when it has no thread id. */
export function normalizeChatThreadHistoryItem(input: ChatThreadHistoryInput): ChatThreadHistoryItem | null {
  return normalizeHistoryInput(input)
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
 * Persona-enabled carry, mirroring {@link withPinnedCarry} but with the
 * INVERSE stored polarity: `pinned` stores `true` and omits `false` (default
 * unpinned); persona stores `false` and omits `true` (default enabled) — so
 * a routine write that never mentions persona (a snapshot autosave, a
 * rename) cannot silently re-enable a persona the user turned off for this
 * thread, and an item that was never touched stores no key at all, same as
 * one explicitly left enabled.
 */
function withPersonaEnabledCarry(
  item: ChatThreadHistoryItem,
  current: ChatThreadHistoryItem | undefined,
): ChatThreadHistoryItem {
  if (item.personaEnabled !== undefined) {
    if (item.personaEnabled) {
      const enabled: ChatThreadHistoryItem = { ...item }
      delete enabled.personaEnabled
      return enabled
    }
    return item
  }
  return current?.personaEnabled === false ? { ...item, personaEnabled: false } : item
}

/**
 * Snapshot writes usually carry title/preview only. Preserve the last
 * server-owned run hint in that case so a local transcript refresh cannot
 * erase the status chip before the next authoritative thread-list read.
 */
function withLatestRunCarry(
  item: ChatThreadHistoryItem,
  current: ChatThreadHistoryItem | undefined,
): ChatThreadHistoryItem {
  return {
    ...item,
    ...(item.latestRunId === undefined && current?.latestRunId ? { latestRunId: current.latestRunId } : {}),
    ...(item.latestRunStatus === undefined && current?.latestRunStatus ? { latestRunStatus: current.latestRunStatus } : {}),
    ...(item.latestRunUpdatedAt === undefined && current?.latestRunUpdatedAt ? { latestRunUpdatedAt: current.latestRunUpdatedAt } : {}),
  }
}

/**
 * Attention carry, paired with the run hint: the server's attention state is derived from the same approval and plan rows
 * as that hint, so a local write keeps the stored state only while it leaves the run as it was. A write that moves the run
 * (a new run, or a new status) drops it, and the row reads its legacy run status until the next thread-list read.
 */
function withAttentionCarry(
  item: ChatThreadHistoryItem,
  current: ChatThreadHistoryItem | undefined,
): ChatThreadHistoryItem {
  if (item.attentionState !== undefined || !current?.attentionState) return item
  const sameRun = item.latestRunId === current.latestRunId && item.latestRunStatus === current.latestRunStatus
  return sameRun ? { ...item, attentionState: current.attentionState } : item
}

/**
 * Origin carry: unlike `pinned`, origin is set once at thread creation and
 * never changes (see session-core's `ThreadSummary.origin` doc), so a write
 * that does not mention it (any local snapshot, a rename) must always keep
 * whatever was already stored rather than needing an explicit resend.
 */
function withOriginCarry(
  item: ChatThreadHistoryItem,
  current: ChatThreadHistoryItem | undefined,
): ChatThreadHistoryItem {
  if (item.origin === undefined && current?.origin) {
    return { ...item, origin: current.origin }
  }
  return item
}

/**
 * Goal status carry: like the run hint, a local snapshot write (a title, a rename, a pin toggle)
 * does not know the server's goal, so it keeps the stored token rather than erasing the chip until
 * the next server list. A server list does not go through here (see `replaceChatThreadHistory`), so a
 * goal the server has cleared reads as no goal there.
 */
function withGoalStatusCarry(
  item: ChatThreadHistoryItem,
  current: ChatThreadHistoryItem | undefined,
): ChatThreadHistoryItem {
  if (item.goalStatus === undefined && current?.goalStatus) {
    return { ...item, goalStatus: current.goalStatus }
  }
  return item
}

/**
 * Memory-mode carry, like {@link withGoalStatusCarry}: a local write that does not know the thread's mode (a
 * title, a pin, a rename) keeps the stored one rather than erasing it. A server list does not go through here.
 */
function withMemoryModeCarry(
  item: ChatThreadHistoryItem,
  current: ChatThreadHistoryItem | undefined,
): ChatThreadHistoryItem {
  if (item.memoryMode === undefined && current?.memoryMode) {
    return { ...item, memoryMode: current.memoryMode }
  }
  return item
}

/**
 * Star, unread, lineage and fork-count carry, like {@link withGoalStatusCarry}: a local write (a title, a pin, a rename) does not
 * know the server's flags, so it keeps the stored ones. A server list does not go through here (see
 * `replaceChatThreadHistory`), so a flag the server has cleared reads as cleared there.
 */
function withThreadStateCarry(
  item: ChatThreadHistoryItem,
  current: ChatThreadHistoryItem | undefined,
): ChatThreadHistoryItem {
  return {
    ...item,
    ...(item.starred === undefined && current?.starred ? { starred: true } : {}),
    ...(item.unread === undefined && current?.unread ? { unread: true } : {}),
    ...(item.forkedFromThreadId === undefined && current?.forkedFromThreadId
      ? { forkedFromThreadId: current.forkedFromThreadId }
      : {}),
    ...(item.forkCount === undefined && current?.forkCount ? { forkCount: current.forkCount } : {}),
  }
}

/**
 * Keeps only the server answers that mean something: a star, an unread reply, a non-blank lineage, and a fork count above
 * zero. With the thread-state switch off nothing is kept, so the cache is written exactly as it was before the switch existed.
 */
function threadStateOf(state: ChatThreadState): ChatThreadState {
  if (!chatSidebarThreadStateEnabled()) return {}
  const lineage = normalizeOptionalText(state.forkedFromThreadId)
  return {
    ...(state.starred === true ? { starred: true } : {}),
    ...(state.unread === true ? { unread: true } : {}),
    ...(lineage ? { forkedFromThreadId: lineage } : {}),
    ...(state.forkCount !== undefined && state.forkCount > 0 ? { forkCount: state.forkCount } : {}),
  }
}

function isChatThreadMemoryMode(value: unknown): value is ChatThreadMemoryMode {
  return value === 'inherit' || value === 'off'
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
    costBasis: turn.costBasis === 'metered' || turn.costBasis === 'subscription' ? turn.costBasis : undefined,
    timing: normalizeTranscriptTiming(turn.timing),
    effectClass: normalizeEffectClass(turn.effectClass),
    createdAt: normalizeTimestamp(turn.createdAt),
    files: Array.isArray(turn.files) ? turn.files : undefined,
    grounding: isRecord(turn.grounding) ? turn.grounding : undefined,
    inputTokens: normalizeOptionalNumber(turn.inputTokens),
    latencyMs: normalizeOptionalNumber(turn.latencyMs),
    // The four the field comments above promise to keep across a reload.
    // They used to be dropped right here, on the way into storage, so each
    // survived only as long as the tab that streamed it.
    memoryRecallCount: normalizeRecallCount(turn.memoryRecallCount),
    recalledMemories: Array.isArray(turn.recalledMemories) ? turn.recalledMemories.map(recalledMemoryWithoutText) : undefined,
    stopReason: normalizeOptionalText(turn.stopReason),
    verification: normalizeTranscriptVerification(turn.verification),
    model: normalizeOptionalText(turn.model),
    modelUsed: normalizeOptionalText(turn.modelUsed),
    provider: normalizeOptionalText(turn.provider),
    subscriptionConnectionId: normalizeOptionalText(turn.subscriptionConnectionId),
    modelConnectionId: normalizeOptionalText(turn.modelConnectionId),
    outputTokens: normalizeOptionalNumber(turn.outputTokens),
    planMode: typeof turn.planMode === 'boolean' ? turn.planMode : undefined,
    grantedRung: normalizeGrantedRung(turn.grantedRung),
    reasoning: normalizeOptionalText(turn.reasoning),
    requestId: normalizeOptionalText(turn.requestId),
    runId: normalizeOptionalText(turn.runId),
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

/** All three totals, each a finite non-negative number, or nothing at all. */
function normalizeTranscriptTiming(value: unknown): ChatThreadTranscriptTiming | undefined {
  if (!isRecord(value)) return undefined
  const serverMs = normalizeNonNegative(value.serverMs)
  const providerMs = normalizeNonNegative(value.providerMs)
  const orchestrationMs = normalizeNonNegative(value.orchestrationMs)
  if (serverMs === undefined || providerMs === undefined || orchestrationMs === undefined) return undefined
  return { serverMs, providerMs, orchestrationMs }
}

function normalizeNonNegative(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : undefined
}

/**
 * Which memory a turn recalled, never its text. Forgetting or correcting a
 * memory changes the memory, not the turns that recalled it, so a stored
 * preview would bring the old text back on the next reload.
 */
function recalledMemoryWithoutText(value: unknown): unknown {
  if (!isRecord(value)) return value
  return { memoryId: value.memoryId, role: value.role, origin: value.origin, label: value.label, preview: '' }
}

/** The backend emits a recall notice only for a real recall, so 0 is no count. */
function normalizeRecallCount(value: number | undefined): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? Math.floor(value) : undefined
}

/**
 * Only the three verdicts the backend defines, as the wire normalizer does:
 * a stored value this build cannot read must not render as "checked".
 */
function normalizeTranscriptVerification(value: unknown): ChatThreadTranscriptTurn['verification'] {
  if (!isRecord(value)) return undefined
  const verdict = value.verdict
  if (verdict !== 'supports' && verdict !== 'contradicts' && verdict !== 'unrelated') return undefined
  return {
    verdict,
    kbCitations: normalizeNonNegative(value.kbCitations) ?? 0,
    webCitations: normalizeNonNegative(value.webCitations) ?? 0,
    webAllowed: value.webAllowed === true,
  }
}

function normalizeEffectClass(value: ChatEffectClass | undefined): ChatEffectClass | undefined {
  return value === 'read_only'
    || value === 'proposed_effect'
    || value === 'effectful'
    || value === 'external_receipt'
    || value === 'unknown'
    ? value
    : undefined
}

function normalizeTranscriptTurnStatus(value: ChatThreadTranscriptTurn['status'] | undefined): ChatThreadTranscriptTurn['status'] | undefined {
  return value === 'error' || value === 'stopped' || value === 'waiting' ? value : undefined
}

function normalizeGrantedRung(value: ChatThreadTranscriptTurn['grantedRung'] | undefined): ChatThreadTranscriptTurn['grantedRung'] | undefined {
  return value === 'read_only' || value === 'workspace_write' || value === 'danger_full_access' ? value : undefined
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
    (record.latestRunId === undefined || typeof record.latestRunId === 'string') &&
    (record.latestRunStatus === undefined || typeof record.latestRunStatus === 'string') &&
    (record.latestRunUpdatedAt === undefined || (
      typeof record.latestRunUpdatedAt === 'string' && !Number.isNaN(Date.parse(record.latestRunUpdatedAt))
    )) &&
    (record.pinned === undefined || typeof record.pinned === 'boolean') &&
    (record.personaEnabled === undefined || typeof record.personaEnabled === 'boolean') &&
    (record.memoryMode === undefined || isChatThreadMemoryMode(record.memoryMode)) &&
    (record.origin === undefined || typeof record.origin === 'string') &&
    (record.goalStatus === undefined || typeof record.goalStatus === 'string') &&
    (record.starred === undefined || typeof record.starred === 'boolean') &&
    (record.unread === undefined || typeof record.unread === 'boolean') &&
    (record.forkedFromThreadId === undefined || typeof record.forkedFromThreadId === 'string') &&
    (record.forkCount === undefined || typeof record.forkCount === 'number') &&
    (record.attentionState === undefined || isAttentionState(record.attentionState))
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
    (record.provider === undefined || typeof record.provider === 'string') &&
    (record.subscriptionConnectionId === undefined || typeof record.subscriptionConnectionId === 'string') &&
    (record.modelConnectionId === undefined || typeof record.modelConnectionId === 'string') &&
    (record.requestId === undefined || typeof record.requestId === 'string') &&
    (record.runId === undefined || typeof record.runId === 'string') &&
    (record.planMode === undefined || typeof record.planMode === 'boolean') &&
    (record.grantedRung === undefined || record.grantedRung === 'read_only' || record.grantedRung === 'workspace_write' || record.grantedRung === 'danger_full_access') &&
    (record.status === undefined || record.status === 'error' || record.status === 'stopped' || record.status === 'waiting') &&
    (record.inputTokens === undefined || typeof record.inputTokens === 'number') &&
    (record.outputTokens === undefined || typeof record.outputTokens === 'number') &&
    (record.latencyMs === undefined || typeof record.latencyMs === 'number') &&
    (record.costUsd === undefined || typeof record.costUsd === 'number') &&
    (record.costBasis === undefined || record.costBasis === 'metered' || record.costBasis === 'subscription') &&
    (record.timing === undefined || isRecord(record.timing)) &&
    (record.stopReason === undefined || typeof record.stopReason === 'string') &&
    (record.memoryRecallCount === undefined || typeof record.memoryRecallCount === 'number') &&
    (record.recalledMemories === undefined || Array.isArray(record.recalledMemories)) &&
    (record.verification === undefined || isRecord(record.verification)) &&
    (record.effectClass === undefined || record.effectClass === 'read_only' || record.effectClass === 'proposed_effect' || record.effectClass === 'effectful' || record.effectClass === 'external_receipt' || record.effectClass === 'unknown') &&
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
