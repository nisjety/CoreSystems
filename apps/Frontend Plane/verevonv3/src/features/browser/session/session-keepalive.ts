/**
 * Keeps a person's browser session grant alive while the session is open and
 * the tab is visible (WS8.D).
 *
 * The gateway advertises `human_browsing` only for a human_browse session. The
 * timer makes one keep-alive call per interval and none while the page is
 * hidden, the session is not live, or the previous call has not returned. A
 * call that reports the grant can no longer be renewed stops the timer once.
 */

/** Five minutes: well inside the broker's default 15-minute grant TTL. */
export const SESSION_KEEPALIVE_INTERVAL_MS = 300_000

export type KeepAliveOutcome = 'alive' | 'expired' | 'failed'

export function sessionHasHumanBrowsing(capabilities: readonly string[] | null | undefined): boolean {
  return Boolean(capabilities?.includes('human_browsing'))
}

export type SessionKeepAliveOptions = {
  /** Defaults to {@link SESSION_KEEPALIVE_INTERVAL_MS}. */
  intervalMs?: number
  /** False skips the tick: a hidden tab or a session that is not live makes no call. */
  isActive: () => boolean
  /** One keep-alive call. A rejected call counts as a failed tick. */
  tick: () => Promise<KeepAliveOutcome>
  /** Called once, when the grant can no longer be renewed. The timer has stopped by then. */
  onExpired?: () => void
}

/** Starts the timer. The returned function stops it and is safe to call twice. */
export function startSessionKeepAlive(options: SessionKeepAliveOptions): () => void {
  const intervalMs = options.intervalMs ?? SESSION_KEEPALIVE_INTERVAL_MS
  let inFlight = false
  let stopped = false
  let handle: ReturnType<typeof setInterval> | undefined

  const stop = (): void => {
    stopped = true
    if (handle !== undefined) clearInterval(handle)
    handle = undefined
  }

  handle = setInterval(() => {
    if (stopped || inFlight || !options.isActive()) return
    inFlight = true
    options.tick().then(
      (outcome) => {
        inFlight = false
        if (outcome === 'expired' && !stopped) {
          stop()
          options.onExpired?.()
        }
      },
      () => {
        inFlight = false
      },
    )
  }, intervalMs)

  return stop
}
