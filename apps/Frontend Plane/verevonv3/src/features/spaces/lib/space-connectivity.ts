import { createSignal, untrack } from 'solid-js'

import { probeSpaceService } from '@/shared/api/spaces-client'
import { spaceRoomFeatureWithDevOverride } from './space-room-features'

/**
 * What the room's composer may honestly say about the connection (WS01-S8,
 * F-t1-033). Three facts feed it and none is guessed:
 *
 * - the browser's own `online`/`offline` events (`navigator.onLine`);
 * - the outcome of the last send's transport: a stream that dropped and could
 *   not be resumed marks the room `unreachable` even though the browser still
 *   believes it is online (a service restart, a proxy cut); the next send that
 *   reaches the server clears it; and
 * - the optional `/health` probe (`checkSpaceService`). The gateway's `/health`
 *   answers without asking any upstream, so the probe can say only that the
 *   gateway itself cannot be reached before a send does; a room backend behind a
 *   live gateway still shows only when a send or a room read fails. Any sign that
 *   the service answers (the probe, a send that reaches it, a room read or a
 *   stream frame) clears it again.
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
const [serviceDown, setServiceDown] = createSignal(false)

/** The current state: offline wins, then an unresumable send or a down service, else online. */
export function spaceConnectivity(): SpaceConnectivity {
  if (!browserIsOnline()) return 'offline'
  return sendUnreachable() || serviceDown() ? 'unreachable' : 'online'
}

// Counts every sign that the service answers. A down answer from a probe that started before the last sign is stale.
let aliveSeq = 0

/** A sign that the service answers: a send reached it, a room read or a stream frame came back, or a probe did. */
function noteServiceAlive(): void {
  aliveSeq += 1
  // Writes only on a change, as the watcher below does: most signs of life find the service already up.
  if (untrack(serviceDown)) setServiceDown(false)
}

/**
 * Asks the gateway's `/health` whether it answers and records the answer. A down answer is ignored when any sign of
 * life arrived while the probe was out, so a slow answer cannot undo a newer one. Never throws.
 */
export async function checkSpaceService(): Promise<void> {
  const aliveBefore = aliveSeq
  const up = await probeSpaceService()
  if (up) noteServiceAlive()
  else if (aliveSeq === aliveBefore) setServiceDown(true)
}

// The last moment each room heard from its server (a stream frame or a listing read), per room, so a switch between
// rooms or a late read for another room never blanks the one on screen. Memory only: a room ref and a time, no content.
const [lastFresh, setLastFresh] = createSignal<ReadonlyMap<string, number>>(new Map())

/**
 * A frame or a listing read from this room's server: the room is as fresh as this moment, and the service is up. It
 * also clears an earlier failed send's `unreachable`, since the room's server has just answered: a newer stamp beside
 * a sentence saying the room cannot be reached would contradict itself.
 */
export function noteRoomFresh(spaceRef: string, at: number = Date.now()): void {
  setLastFresh((current) => new Map(current).set(spaceRef, at))
  if (untrack(sendUnreachable)) setSendUnreachable(false)
  noteServiceAlive()
}

/**
 * When this room last heard from its server, in epoch milliseconds, or undefined when it has not in this page
 * session. Reactive. The stamp shows how old what is on screen is; it never says the room is current.
 */
export function roomLastFreshAt(spaceRef: string): number | undefined {
  return lastFresh().get(spaceRef)
}

/** A send's stream dropped and resuming it did not work. */
export function noteSendTransportLost(): void {
  setSendUnreachable(true)
}

/** A send reached the server (its stream opened, or the server answered). */
export function noteSendTransportOk(): void {
  setSendUnreachable(false)
  noteServiceAlive()
}

/**
 * Follow the browser's connectivity events for as long as the caller lives.
 * Returns the stop function. Coming back online also clears `unreachable`: the
 * next send finds out for itself, and a banner that outlives its cause is worse
 * than none. The service is probed once when the watch starts online, and again
 * when the browser comes back, so a gateway that cannot be reached shows before a send.
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
    void checkSpaceService()
  }
  sync()
  // A probe writes its answer only after the request returns, never inside the caller's body.
  if (browserOnline()) void checkSpaceService()
  window.addEventListener('offline', sync)
  window.addEventListener('online', back)
  return () => {
    window.removeEventListener('offline', sync)
    window.removeEventListener('online', back)
  }
}

/**
 * Whether the connectivity banner shows: the room's `features.connectivity_banner` from the
 * context read (WS01-S5). `VITE_SPACE_CONNECTIVITY_BANNER_UI=true|false` is an explicit
 * override in a dev build only. A rollout control, never an authorization input.
 */
export function spaceConnectivityBannerEnabled(): boolean {
  return spaceRoomFeatureWithDevOverride('connectivity_banner', import.meta.env.VITE_SPACE_CONNECTIVITY_BANNER_UI)
}

/**
 * Whether a room send on the agent path carries its key: the room's `features.send_idempotency`
 * from the context read (WS01-S5). `VITE_SPACE_SEND_IDEMPOTENCY_API=true|false` is an explicit
 * override in a dev build only. With it off a room send is byte-identical to before: no
 * `idempotency_key`, no `session_key`, no resume.
 */
export function spaceSendIdempotencyEnabled(): boolean {
  return spaceRoomFeatureWithDevOverride('send_idempotency', import.meta.env.VITE_SPACE_SEND_IDEMPOTENCY_API)
}
