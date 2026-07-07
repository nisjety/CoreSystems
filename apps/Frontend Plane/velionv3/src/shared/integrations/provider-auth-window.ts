import { requestJson } from '@/shared/api/http'

/**
 * COOP-safe OAuth popup runner.
 *
 * Providers like X, LinkedIn, and Microsoft serve their login pages with
 * `Cross-Origin-Opener-Policy: same-origin`, which permanently severs the
 * popup ↔ opener relationship the moment the popup navigates there. Two
 * things break under severing:
 *   - `popup.closed` lies (reads `true` while the window is still open), so
 *     any closed-poll both spams the console with COOP warnings and falsely
 *     reports "the window was closed";
 *   - the callback page's `window.opener` is null, so its postMessage
 *     completion signal never arrives.
 *
 * The authoritative completion signal is therefore the connect-session
 * status endpoint (`pending` → `completed` | `failed`), polled server-side
 * via the gateway. postMessage is kept as a fast path for providers that do
 * not sever (GitHub, Google, Meta), and the closed-poll is used ONLY as a
 * fallback when no session id is available to poll.
 */

const STATUS_POLL_INTERVAL_MS = 2000
const FLOW_TIMEOUT_MS = 180000

type ConnectSessionStatus = {
  status?: string
  errorCode?: string
}

function openProviderAuthWindow(): Window | null {
  const width = Math.min(540, window.screen.width)
  const height = Math.min(720, window.screen.height)
  const left = Math.max(window.screen.width / 2 - width / 2, 0)
  const top = Math.max(window.screen.height / 2 - height / 2, 0)

  return window.open(
    '',
    '_blank',
    [
      `left=${left}`,
      `top=${top}`,
      `width=${width}`,
      `height=${height}`,
      'scrollbars=yes',
      'resizable=yes',
      'status=no',
      'toolbar=no',
      'location=no',
      'copyhistory=no',
      'menubar=no',
      'directories=no',
    ].join(','),
  )
}

async function fetchConnectSessionStatus(sessionId: string): Promise<ConnectSessionStatus | null> {
  try {
    const payload = await requestJson<Record<string, unknown>>(
      `/api/v1/integrations/connect-sessions/${encodeURIComponent(sessionId)}/status`,
      { method: 'GET' },
    )
    // requestJson unwraps `data` envelopes; tolerate both shapes.
    const record = (payload && typeof payload === 'object' && 'status' in payload
      ? payload
      : (payload as { data?: Record<string, unknown> } | null)?.data) as Record<string, unknown> | undefined
    if (!record) return null
    return {
      status: typeof record.status === 'string' ? record.status : undefined,
      errorCode: typeof record.errorCode === 'string' ? record.errorCode : undefined,
    }
  } catch {
    // Transient polling failures must not abort a flow the user is mid-login
    // in — the timeout is the backstop.
    return null
  }
}

function connectFailureMessage(status: ConnectSessionStatus): string {
  switch (status.errorCode) {
    case 'access_denied':
      return 'Tilgang ble avvist hos leverandøren. Prøv igjen og godkjenn forespørselen.'
    case 'invalid_state':
      return 'Sikkerhetssjekken feilet (ugyldig state). Start tilkoblingen på nytt.'
    default:
      return status.errorCode
        ? `Leverandøren avviste tilkoblingen (${status.errorCode}).`
        : 'Autorisasjonen ble ikke fullført hos leverandøren.'
  }
}

export async function runDirectOauthWindow(input: {
  connectUrl: string
  /** Connect-session id (returned as `sessionToken`); enables COOP-safe
   * server-side status polling. Without it the runner falls back to the
   * legacy popup-closed heuristic. */
  sessionToken?: string
}): Promise<void> {
  const expectedOrigin = new URL(input.connectUrl).origin
  const authWindow = openProviderAuthWindow()
  if (!authWindow) {
    throw new Error('The provider sign-in window was blocked by the browser.')
  }

  await new Promise<void>((resolve, reject) => {
    let settled = false
    let statusInFlight = false
    const settle = (fn: () => void) => {
      if (settled) return
      settled = true
      window.clearTimeout(timeout)
      if (closePoll !== undefined) window.clearInterval(closePoll)
      if (statusPoll !== undefined) window.clearInterval(statusPoll)
      window.removeEventListener('message', handleMessage)
      // Deliberately NO authWindow.close() here: on success the callback page
      // closes itself, on failure it must stay open so the user can read the
      // provider error — and touching a COOP-severed handle only logs
      // "Cross-Origin-Opener-Policy policy would block the window.close call".
      fn()
    }

    const handleMessage = (event: MessageEvent) => {
      // Fast path for providers that keep the opener relationship. Under COOP
      // severing `event.source` may not compare equal to our handle, so trust
      // origin + payload shape (+ session token match when we have one).
      if (event.origin !== expectedOrigin && event.origin !== window.location.origin) return
      const payload = event.data
      if (!payload || typeof payload !== 'object') return

      const record = payload as Record<string, unknown>
      if (record.type !== 'velion.integration.connected') return
      if (input.sessionToken && record.sessionToken !== input.sessionToken) return

      if (record.status === 'success') {
        settle(resolve)
        return
      }

      const message =
        typeof record.message === 'string' && record.message.trim()
          ? record.message
          : 'The authorization flow did not complete.'
      settle(() => reject(new Error(message)))
    }

    window.addEventListener('message', handleMessage)

    const timeout = window.setTimeout(() => {
      settle(() => reject(new Error('The authorization flow timed out before the provider returned a result.')))
    }, FLOW_TIMEOUT_MS)

    // Authoritative completion signal: server-side session status. Immune to
    // COOP severing because it never touches the popup handle.
    const statusPoll = input.sessionToken
      ? window.setInterval(() => {
          if (statusInFlight || settled) return
          statusInFlight = true
          void fetchConnectSessionStatus(input.sessionToken!)
            .then((status) => {
              statusInFlight = false
              if (!status || settled) return
              if (status.status === 'completed') settle(resolve)
              else if (status.status === 'failed') settle(() => reject(new Error(connectFailureMessage(status))))
            })
            .catch(() => {
              statusInFlight = false
            })
        }, STATUS_POLL_INTERVAL_MS)
      : undefined

    // popup-closed heuristic ONLY when we cannot poll: under COOP `closed`
    // reads true while the window is still open, so it must never gate a
    // pollable flow.
    const closePoll = input.sessionToken
      ? undefined
      : window.setInterval(() => {
          if (!authWindow.closed) return
          settle(() => reject(new Error('The authorization window was closed before the connection finished.')))
        }, 500)

    try {
      authWindow.location.href = input.connectUrl
    } catch {
      settle(() => reject(new Error('The authorization window could not open the provider sign-in page.')))
    }
  })
}
