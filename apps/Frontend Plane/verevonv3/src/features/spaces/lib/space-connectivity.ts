import { createSignal, untrack } from 'solid-js'

/**
 * What the room's composer may honestly say about the connection (WS01-S8,
 * F-t1-033). Two facts feed it and neither is guessed:
 *
 * - the browser's own `online`/`offline` events (`navigator.onLine`), and
 * - the outcome of the last send's transport: a stream that dropped and could
 *   not be resumed marks the room `unreachable` even though the browser still
 *   believes it is online (a service restart, a proxy cut); the next send that
 *   reaches the server clears it.
 *
 * `online` says nothing and renders nothing. The state is presentation only: it
 * never gates a send (a send that fails is reported by its own exchange) and is
 * never an authorization input.
 */
export type SpaceConnectivity = 'online' | 'offline' | 'unreachable'

function browserOnline(): boolean {
  return typeof navigator === 'undefined' || navigator.onLine !== false
}

const [browserIsOnline, setBrowserIsOnline] = createSignal(browserOnline())
const [sendUnreachable, setSendUnreachable] = createSignal(false)

/** The current state: offline wins, then an unresumable send, else online. */
export function spaceConnectivity(): SpaceConnectivity {
  if (!browserIsOnline()) return 'offline'
  return sendUnreachable() ? 'unreachable' : 'online'
}

/** A send's stream dropped and resuming it did not work. */
export function noteSendTransportLost(): void {
  setSendUnreachable(true)
}

/** A send reached the server (its stream opened, or the server answered). */
export function noteSendTransportOk(): void {
  setSendUnreachable(false)
}

/**
 * Follow the browser's connectivity events for as long as the caller lives.
 * Returns the stop function. Coming back online also clears `unreachable`: the
 * next send finds out for itself, and a banner that outlives its cause is worse
 * than none.
 */
export function watchBrowserConnectivity(): () => void {
  if (typeof window === 'undefined') return () => undefined
  // Writes only on a change: the first sync usually finds nothing to do, and a write while a component
  // body is still running halts the reactive system under Solid 2 (REACTIVE_WRITE_IN_OWNED_SCOPE).
  const sync = () => {
    const now = browserOnline()
    if (untrack(browserIsOnline) !== now) setBrowserIsOnline(now)
  }
  const back = () => {
    setBrowserIsOnline(true)
    setSendUnreachable(false)
  }
  sync()
  window.addEventListener('offline', sync)
  window.addEventListener('online', back)
  return () => {
    window.removeEventListener('offline', sync)
    window.removeEventListener('online', back)
  }
}

/**
 * Build-time switch for the connectivity banner until the BFF delivers
 * `features.connectivity_banner` (WS01-S5). On in dev, off in production unless
 * `VITE_SPACE_CONNECTIVITY_BANNER_UI=true`; `=false` turns it off in dev too. A
 * rollout control only, never an authorization input.
 */
export function spaceConnectivityBannerEnabled(): boolean {
  const flag = import.meta.env.VITE_SPACE_CONNECTIVITY_BANNER_UI
  if (flag === 'true') return true
  if (flag === 'false') return false
  return import.meta.env.DEV === true
}

/**
 * Build-time switch for the send key on the agent path until the BFF delivers
 * `features.send_idempotency` (WS01-S5). Same rule as the banner switch, with
 * `VITE_SPACE_SEND_IDEMPOTENCY_API`. With it off a room send is byte-identical
 * to before: no `idempotency_key`, no `session_key`, no resume.
 */
export function spaceSendIdempotencyEnabled(): boolean {
  const flag = import.meta.env.VITE_SPACE_SEND_IDEMPOTENCY_API
  if (flag === 'true') return true
  if (flag === 'false') return false
  return import.meta.env.DEV === true
}
