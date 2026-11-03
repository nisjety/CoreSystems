import { createRoot, createSignal } from 'solid-js'

import { ApiError } from '@/shared/api/api-error'
import { announce } from '@/shared/a11y/announcer'
import {
  getSpacesAttention,
  type AttentionRoom,
  type AttentionTotals,
  type SpacesAttention,
} from '@/shared/api/spaces-attention-client'

/**
 * Cross-room attention, shared by every consumer in the shell (WS11-S4).
 *
 * # One poller per browser, and only while it is seen
 *
 * The aggregate is one request that covers every room, so it is fetched ONCE per browser, not once
 * per tab or per component. A VISIBLE tab that holds the Web Lock `verevon.space-attention.leader`
 * polls every 30 seconds and broadcasts each answer on a `BroadcastChannel`; every other tab only
 * listens. A hidden tab never asks for the lock and gives it back when it is hidden, so a background
 * tab costs the server nothing and the next visible tab takes over at once. Without `navigator.locks`
 * (old browsers, tests) every visible tab polls for itself: correct, just wasteful.
 *
 * # Requests are spaced
 *
 * A reader who marks something read asks for a fresh answer, but two aggregate requests from one
 * browser are never closer than {@link ATTENTION_REFRESH_GAP_MS}. A request asked for during a poll
 * waits for that poll and then goes once.
 *
 * # The badge is the server's
 *
 * `AttentionRoom.badge` is computed once, in the gateway, and is rendered as given. Nothing here (or
 * in a component) adds counts up or derives a badge: that is how `space-unread.ts` drifted from
 * Convex once, and the plan forbids a second implementation.
 *
 * # Absence is not zero
 *
 * No snapshot yet, a failed poll or a feature-off gateway all read as "unknown": the sidebar shows
 * no badge rather than a calm one. The age of an answer is the time this browser (or the leader tab
 * it heard from) GOT it, so a follower tab never calls an old answer fresh. After
 * {@link ATTENTION_STALE_AFTER_MS} without a fresh answer the store reports `stale`, and the UI says
 * so instead of presenting old numbers as current.
 */

export const ATTENTION_POLL_MS = 30_000
export const ATTENTION_MAX_BACKOFF_MS = 300_000
export const ATTENTION_STALE_AFTER_MS = 90_000
/** Two aggregate requests from one browser are at least this far apart, however often a reader asks. */
export const ATTENTION_REFRESH_GAP_MS = 10_000
const CLOCK_TICK_MS = 15_000
const LEADER_LOCK = 'verevon.space-attention.leader'
const CHANNEL = 'verevon.space-attention'

type Translate = (no: string, en: string) => string

type ChannelMessage =
  | { readonly type: 'snapshot'; readonly at: number; readonly snapshot: SpacesAttention }
  | { readonly type: 'disabled' }
  | { readonly type: 'refresh' }
  | { readonly type: 'hello' }

interface State {
  readonly snapshot: SpacesAttention | undefined
  /** When this browser last received an answer (its clock, or the leader's), or null. */
  readonly receivedAt: number | null
  /** The gateway said the surface is off (404 `space_attention_disabled`); polling stopped. */
  readonly disabled: boolean
}

/** The last good answer and when it was received. Kept outside the signal, so a read is never behind a write. */
interface Answer {
  readonly snapshot: SpacesAttention
  readonly at: number
}

const store = createRoot(() => {
  const [state, setState] = createSignal<State>({ snapshot: undefined, receivedAt: null, disabled: false })
  const [now, setNow] = createSignal(Date.now())
  return { state, setState, now, setNow }
})

// ── reads (reactive) ─────────────────────────────────────────────────────────────────────────────

/** The server's numbers for one room, or undefined when nothing is known about it (not "zero"). */
export function spaceAttentionRoom(spaceRef: string): AttentionRoom | undefined {
  const snapshot = store.state().snapshot
  if (!snapshot || snapshot.mode !== 'live') return undefined
  return snapshot.rooms.find((room) => room.spaceRef === spaceRef)
}

/** The sums across rooms, or undefined when unknown. `badge` is the server's, rendered as given. */
export function spaceAttentionTotals(): AttentionTotals | undefined {
  const snapshot = store.state().snapshot
  return snapshot && snapshot.mode === 'live' ? snapshot.totals : undefined
}

/** True once the last good answer is older than 90 s (or the poller keeps failing). */
export function spaceAttentionStale(): boolean {
  const { receivedAt, snapshot } = store.state()
  if (!snapshot || receivedAt === null) return false
  return store.now() - receivedAt > ATTENTION_STALE_AFTER_MS
}

/** False once the gateway reported the attention surface off. */
export function spaceAttentionAvailable(): boolean {
  return !store.state().disabled
}

// ── lifecycle ────────────────────────────────────────────────────────────────────────────────────

let retainers = 0
let translate: Translate = (_no, en) => en
let channel: BroadcastChannel | undefined
/** This tab polls: it is visible and holds the lock (or there is no lock to hold). */
let leader = false
let leaderAbort: AbortController | undefined
let releaseLeader: (() => void) | undefined
let pollTimer: ReturnType<typeof setTimeout> | undefined
let refreshTimer: ReturnType<typeof setTimeout> | undefined
let tickTimer: ReturnType<typeof setInterval> | undefined
let failures = 0
let inflight = false
let queued = false
let lastPollAt = 0
let lastAnswer: Answer | undefined
/** Bumped on every stop, so a request that was on its way then cannot touch the next session's state. */
let generation = 0

/**
 * Start (or join) the shared poller. Returns the release function; the poller stops when the last
 * retainer releases. `tr` is the viewer's translator, kept for the polite live-region sentence.
 */
export function retainSpaceAttention(tr: Translate): () => void {
  translate = tr
  retainers += 1
  if (retainers === 1) start()
  let released = false
  return () => {
    if (released) return
    released = true
    retainers = Math.max(0, retainers - 1)
    if (retainers === 0) stop()
  }
}

/** Ask for a fresh answer soon (after the reader marks something read). Safe to call from anywhere. */
export function refreshSpaceAttention(): void {
  if (retainers === 0 || store.state().disabled) return
  if (leader) scheduleRefresh()
  else post({ type: 'refresh' })
}

function documentVisible(): boolean {
  return typeof document === 'undefined' || document.visibilityState === 'visible'
}

function start(): void {
  if (typeof window === 'undefined') return
  failures = 0
  store.setNow(Date.now())
  tickTimer = setInterval(() => store.setNow(Date.now()), CLOCK_TICK_MS)
  try {
    if (typeof BroadcastChannel === 'function') {
      channel = new BroadcastChannel(CHANNEL)
      channel.onmessage = (event: MessageEvent<ChannelMessage>) => onMessage(event.data)
    }
  } catch {
    channel = undefined
  }
  if (typeof document !== 'undefined') document.addEventListener('visibilitychange', onVisibilityChange)
  post({ type: 'hello' })
  requestLeadership()
}

function stop(): void {
  if (tickTimer) clearInterval(tickTimer)
  tickTimer = undefined
  if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', onVisibilityChange)
  generation += 1
  inflight = false
  queued = false
  yieldLeadership()
  try {
    channel?.close()
  } catch {
    // closing twice is harmless
  }
  channel = undefined
}

/**
 * Take the lead while this tab is visible. A visible tab queues for the lock; a hidden one never
 * does, so no background tab asks the server anything. With no lock, or no channel to hear the
 * leader on, the visible tab polls for itself.
 */
function requestLeadership(): void {
  if (retainers === 0 || leader || leaderAbort || !documentVisible()) return
  const locks = typeof navigator !== 'undefined' ? navigator.locks : undefined
  if (!locks || !channel) {
    becomeLeader()
    return
  }
  const abort = new AbortController()
  leaderAbort = abort
  void locks
    .request(LEADER_LOCK, { mode: 'exclusive', signal: abort.signal }, () =>
      new Promise<void>((resolve) => {
        // Superseded, stopped, or hidden while it waited: pass the lock straight on.
        if (abort.signal.aborted || retainers === 0 || !documentVisible()) {
          resolve()
          return
        }
        releaseLeader = resolve
        becomeLeader()
      }),
    )
    .catch(() => undefined) // an aborted request is the normal way out
    .finally(() => {
      if (leaderAbort === abort) leaderAbort = undefined
    })
}

/** Give up polling: cancel a pending lock request and release a held one. Safe to call when not leader. */
function yieldLeadership(): void {
  leader = false
  queued = false
  clearTimeout(pollTimer)
  pollTimer = undefined
  clearTimeout(refreshTimer)
  refreshTimer = undefined
  leaderAbort?.abort()
  leaderAbort = undefined
  releaseLeader?.()
  releaseLeader = undefined
}

function onVisibilityChange(): void {
  if (retainers === 0) return
  if (!documentVisible()) {
    yieldLeadership()
  } else if (leader) {
    scheduleRefresh()
  } else {
    requestLeadership()
  }
}

function becomeLeader(): void {
  leader = true
  scheduleRefresh()
}

function post(message: ChannelMessage): void {
  try {
    channel?.postMessage(message)
  } catch {
    // a closed channel is the caller leaving
  }
}

function onMessage(message: ChannelMessage | undefined): void {
  if (!message || typeof message !== 'object') return
  switch (message.type) {
    case 'snapshot':
      // Only a follower takes another tab's answer; the leader has its own.
      if (!leader && message.snapshot && typeof message.snapshot === 'object') {
        const at = typeof message.at === 'number' && Number.isFinite(message.at) ? message.at : Date.now()
        apply(message.snapshot, at)
      }
      return
    case 'disabled':
      store.setState((current) => ({ ...current, disabled: true }))
      return
    case 'refresh':
      if (leader) scheduleRefresh()
      return
    case 'hello':
      // A new tab asks for the answer it missed. Replies carry when WE got it, never "now".
      if (leader && lastAnswer) post({ type: 'snapshot', at: lastAnswer.at, snapshot: lastAnswer.snapshot })
      return
  }
}

// ── polling (leader only) ────────────────────────────────────────────────────────────────────────

/** Poll now, or at the end of the spacing gap when the last request was too recent. */
function scheduleRefresh(): void {
  if (!leader || retainers === 0 || refreshTimer !== undefined) return
  const wait = Math.max(0, lastPollAt + ATTENTION_REFRESH_GAP_MS - Date.now())
  if (wait === 0) {
    void pollNow()
    return
  }
  refreshTimer = setTimeout(() => {
    refreshTimer = undefined
    void pollNow()
  }, wait)
}

async function pollNow(): Promise<void> {
  if (!leader || store.state().disabled || !documentVisible()) return
  if (inflight) {
    queued = true
    return
  }
  inflight = true
  queued = false
  const session = generation
  clearTimeout(pollTimer)
  pollTimer = undefined
  lastPollAt = Date.now()
  let nextMs = ATTENTION_POLL_MS
  let off = false
  try {
    const snapshot = await getSpacesAttention()
    if (session !== generation) return // stopped while on its way: the answer belongs to a session that ended
    failures = 0
    const at = Date.now()
    apply(snapshot, at)
    post({ type: 'snapshot', at, snapshot })
  } catch (error) {
    if (session !== generation) return
    if (error instanceof ApiError && error.status === 404 && error.code === 'space_attention_disabled') {
      // Off in this deployment: stop for the session instead of asking every 30 seconds.
      store.setState((current) => ({ ...current, disabled: true }))
      post({ type: 'disabled' })
      off = true
    } else {
      failures += 1
      nextMs = Math.min(ATTENTION_POLL_MS * 2 ** failures, ATTENTION_MAX_BACKOFF_MS)
      if (error instanceof ApiError && error.retryAfterSeconds) {
        nextMs = Math.max(nextMs, error.retryAfterSeconds * 1000)
      }
    }
  } finally {
    if (session === generation) inflight = false
  }
  if (off || !leader || retainers === 0) return
  if (queued) {
    // Someone asked for a fresh answer while this one was on its way: that answer may predate the change.
    scheduleRefresh()
    return
  }
  pollTimer = setTimeout(() => void pollNow(), nextMs)
}

function apply(snapshot: SpacesAttention, at: number): void {
  const previous = lastAnswer?.snapshot
  lastAnswer = { snapshot, at }
  store.setState({ snapshot, receivedAt: at, disabled: false })
  store.setNow(Date.now())
  announceIncrease(previous, snapshot)
}

/**
 * A polite sentence when the number waiting for the reader goes UP between two answers. Only in the
 * tab the reader is looking at, never for the first answer, and never the decrease.
 */
function announceIncrease(previous: SpacesAttention | undefined, next: SpacesAttention): void {
  if (!previous || previous.mode !== 'live' || next.mode !== 'live') return
  if (typeof document !== 'undefined' && document.visibilityState !== 'visible') return
  const gained = next.totals.badge - previous.totals.badge
  if (gained <= 0) return
  const total = next.totals.badge
  announce(
    gained === 1
      ? translate(`Nytt varsel i rom. ${total} venter på deg.`, `New alert in a room. ${total} waiting for you.`)
      : translate(`${gained} nye varsler i rom. ${total} venter på deg.`, `${gained} new alerts in rooms. ${total} waiting for you.`),
    { key: 'space-attention-increase', coalesceMs: 10_000, kind: 'status' },
  )
}

/** Test hook: forget everything, stop the poller. */
export function resetSpaceAttentionForTests(): void {
  retainers = 0
  stop()
  failures = 0
  inflight = false
  queued = false
  lastPollAt = 0
  lastAnswer = undefined
  store.setState({ snapshot: undefined, receivedAt: null, disabled: false })
}
