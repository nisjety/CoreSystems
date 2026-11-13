import { createSignal, untrack } from 'solid-js'

/**
 * What the chat composer may honestly say about the connection (F-t3-078). Two facts feed it and neither is guessed:
 *
 * - the browser's own `online`/`offline` events (`navigator.onLine`), and
 * - the outcome of the chat stream: a stream that dropped and could not be resumed marks the chat `unreachable`
 *   even though the browser still believes it is online (a service restart, a proxy cut). A stream that opens, or
 *   a reply from the service, clears it.
 *
 * The shown state lags the facts, so a blip does not flicker: a degraded state shows only after it has held for
 * `DEGRADED_HOLD_MS`, and `online` replaces a shown degraded state only after `RESTORED_HOLD_MS` of steady `online`.
 *
 * Presentation only. It never gates a send (a send that fails is reported by its own exchange) and is never an
 * authorization input. Dark behind `VITE_CHAT_CONNECTION_PILL`: off, nothing is noted and nothing is shown.
 */
export type ChatConnectivity = 'online' | 'offline' | 'unreachable'

/** A degraded state shows only after it has held this long (ms). */
const DEGRADED_HOLD_MS = 1500
/** `online` replaces a shown degraded state only after it has held this long (ms). */
const RESTORED_HOLD_MS = 3000

/**
 * Whether the connection pill is on for this build. `VITE_CHAT_CONNECTION_PILL` counts only as the exact string
 * `true`, as the other chat switches do.
 */
export const chatConnectionPillEnabled = (): boolean => import.meta.env.VITE_CHAT_CONNECTION_PILL === 'true'

function browserOnline(): boolean {
  return typeof navigator === 'undefined' || navigator.onLine !== false
}

const [browserIsOnline, setBrowserIsOnline] = createSignal(browserOnline())
const [streamLost, setStreamLost] = createSignal(false)
// What the pill shows. It follows the facts only once their hold time has run out.
const [shown, setShown] = createSignal<ChatConnectivity>('online')
let degradeTimer: ReturnType<typeof setTimeout> | undefined
let restoreTimer: ReturnType<typeof setTimeout> | undefined

/** What the facts say right now, before any hold. Offline wins over an unreachable stream. */
function factsNow(): ChatConnectivity {
  if (!untrack(browserIsOnline)) return 'offline'
  return untrack(streamLost) ? 'unreachable' : 'online'
}

function clearHolds(): void {
  clearTimeout(degradeTimer)
  clearTimeout(restoreTimer)
  degradeTimer = undefined
  restoreTimer = undefined
}

/** A hold ran out: show what the facts say now. */
function commit(): void {
  const facts = factsNow()
  if (untrack(shown) !== facts) setShown(facts)
}

/**
 * Bring the shown state in line with the facts, starting or clearing a hold. A change of fact clears the hold it no
 * longer needs. Safe to call at any time: it changes nothing when the shown state already matches.
 */
function settle(): void {
  const facts = factsNow()
  const current = untrack(shown)
  if (facts === 'online') {
    clearTimeout(degradeTimer)
    degradeTimer = undefined
    if (current === 'online') {
      clearTimeout(restoreTimer)
      restoreTimer = undefined
      return
    }
    restoreTimer ??= setTimeout(() => {
      restoreTimer = undefined
      commit()
    }, RESTORED_HOLD_MS)
    return
  }
  clearTimeout(restoreTimer)
  restoreTimer = undefined
  if (current !== 'online') {
    // Already showing a degraded state: a change between offline and unreachable shows at once.
    if (current !== facts) setShown(facts)
    return
  }
  degradeTimer ??= setTimeout(() => {
    degradeTimer = undefined
    commit()
  }, DEGRADED_HOLD_MS)
}

/** What the pill shows now. `online` renders nothing. */
export function chatConnectivity(): ChatConnectivity {
  return shown()
}

/**
 * A chat stream dropped and resuming it did not reattach, and a durable wait for its answer follows: the chat cannot
 * reach the service right now. The caller says so only when that wait will run, because a turn settled without one
 * reconnects to nothing.
 */
export function noteChatStreamLost(): void {
  if (!chatConnectionPillEnabled() || untrack(streamLost)) return
  setStreamLost(true)
  settle()
}

/** A chat stream opened, or the service answered: the chat reaches the service again. */
export function noteChatStreamOk(): void {
  if (!chatConnectionPillEnabled() || !untrack(streamLost)) return
  setStreamLost(false)
  settle()
}

/**
 * Follow the browser's connectivity events for as long as the caller lives. Returns the stop function, which also
 * clears any pending hold. Coming back online clears a stream loss too: the next stream finds out for itself, and a
 * pill that outlives its cause is worse than none.
 */
export function watchChatConnectivity(): () => void {
  if (typeof window === 'undefined') return () => undefined
  // Writes only on a change: a write while a component body is still running halts Solid 2
  // (REACTIVE_WRITE_IN_OWNED_SCOPE). The pill starts this in onSettled, so the first sync runs after render.
  const sync = () => {
    const now = browserOnline()
    if (untrack(browserIsOnline) !== now) setBrowserIsOnline(now)
    settle()
  }
  const back = () => {
    setBrowserIsOnline(true)
    setStreamLost(false)
    settle()
  }
  sync()
  window.addEventListener('offline', sync)
  window.addEventListener('online', back)
  return () => {
    window.removeEventListener('offline', sync)
    window.removeEventListener('online', back)
    clearHolds()
  }
}
