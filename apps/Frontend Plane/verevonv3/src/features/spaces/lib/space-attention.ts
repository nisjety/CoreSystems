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
 * # One poller per browser
 *
 * The aggregate is one request that covers every room, so it is fetched ONCE per browser, not once
 * per tab or per component. A tab that holds the Web Lock `verevon.space-attention.leader` polls and
 * broadcasts each answer on a `BroadcastChannel`; every other tab only listens. When the leader tab
 * closes the browser hands the lock to the next waiting tab, which then starts polling. Without
 * `navigator.locks` (old browsers, tests) every tab polls for itself, which is correct, just wasteful.
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
 * no badge rather than a calm one. After {@link ATTENTION_STALE_AFTER_MS} without a fresh answer the
 * store reports `stale`, and the UI says so instead of presenting old numbers as current.
 */

export const ATTENTION_POLL_MS = 30_000
export const ATTENTION_MAX_BACKOFF_MS = 300_000
export const ATTENTION_STALE_AFTER_MS = 90_000
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
  /** When this browser last received an answer (local clock), or null. */
  readonly receivedAt: number | null
  /** The gateway said the surface is off (404 `space_attention_disabled`); polling stopped. */
  readonly disabled: boolean
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
let leader = false
let leaderAbort: AbortController | undefined
let releaseLeader: (() => void) | undefined
let pollTimer: ReturnType<typeof setTimeout> | undefined
let tickTimer: ReturnType<typeof setInterval> | undefined
let failures = 0
let inflight = false
let lastRefreshAt = 0

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
  const at = Date.now()
  if (at - lastRefreshAt < 1000) return
  lastRefreshAt = at
  if (leader) void pollNow()
  else post({ type: 'refresh' })
}

function start(): void {
  if (typeof window === 'undefined') return
  failures = 0
  store.setNow(Date.now())
  tickTimer = setInterval(() => store.setNow(Date.now()), 15_000)
  try {
    if (typeof BroadcastChannel === 'function') {
      channel = new BroadcastChannel(CHANNEL)
      channel.onmessage = (event: MessageEvent<ChannelMessage>) => onMessage(event.data)
    }
  } catch {
    channel = undefined
  }
  const locks = typeof navigator !== 'undefined' ? navigator.locks : undefined
  if (!locks || !channel) {
    // No way to elect (or no way to hear the leader): poll for ourselves.
    becomeLeader()
    return
  }
  post({ type: 'hello' })
  leaderAbort = new AbortController()
  locks
    .request(LEADER_LOCK, { mode: 'exclusive', signal: leaderAbort.signal }, () =>
      new Promise<void>((resolve) => {
        releaseLeader = resolve
        becomeLeader()
      }),
    )
    .catch(() => undefined) // an aborted request is the normal way out
}

function stop(): void {
  clearTimeout(pollTimer)
  pollTimer = undefined
  if (tickTimer) clearInterval(tickTimer)
  tickTimer = undefined
  leader = false
  leaderAbort?.abort()
  leaderAbort = undefined
  releaseLeader?.()
  releaseLeader = undefined
  try {
    channel?.close()
  } catch {
    // closing twice is harmless
  }
  channel = undefined
}

function becomeLeader(): void {
  leader = true
  if (store.state().disabled) return
  void pollNow()
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
      if (!leader && message.snapshot && typeof message.snapshot === 'object') apply(message.snapshot)
      return
    case 'disabled':
      store.setState((current) => ({ ...current, disabled: true }))
      return
    case 'refresh':
      if (leader) void pollNow()
      return
    case 'hello': {
      const { snapshot } = store.state()
      if (leader && snapshot) post({ type: 'snapshot', at: Date.now(), snapshot })
      return
    }
  }
}

// ── polling (leader only) ────────────────────────────────────────────────────────────────────────

async function pollNow(): Promise<void> {
  if (!leader || inflight || store.state().disabled) return
  inflight = true
  clearTimeout(pollTimer)
  let nextMs = ATTENTION_POLL_MS
  try {
    const snapshot = await getSpacesAttention()
    failures = 0
    apply(snapshot)
    post({ type: 'snapshot', at: Date.now(), snapshot })
  } catch (error) {
    if (error instanceof ApiError && error.status === 404 && error.code === 'space_attention_disabled') {
      // Off in this deployment: stop for the session instead of asking every 30 seconds.
      store.setState((current) => ({ ...current, disabled: true }))
      post({ type: 'disabled' })
      inflight = false
      return
    }
    failures += 1
    nextMs = Math.min(ATTENTION_POLL_MS * 2 ** failures, ATTENTION_MAX_BACKOFF_MS)
    if (error instanceof ApiError && error.retryAfterSeconds) {
      nextMs = Math.max(nextMs, error.retryAfterSeconds * 1000)
    }
  } finally {
    inflight = false
  }
  if (leader && retainers > 0) pollTimer = setTimeout(() => void pollNow(), nextMs)
}

function apply(snapshot: SpacesAttention): void {
  const previous = store.state().snapshot
  store.setState({ snapshot, receivedAt: Date.now(), disabled: false })
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
  lastRefreshAt = 0
  store.setState({ snapshot: undefined, receivedAt: null, disabled: false })
}
