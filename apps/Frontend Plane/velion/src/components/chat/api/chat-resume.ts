/**
 * Browser-side chat resume-on-reload (HARNESS_PHASE1 §3b).
 *
 * When a streaming turn starts, the route surfaces the gateway stream
 * `request_id` via a `meta` SSE frame. We remember `{sessionId → requestId}`
 * in sessionStorage and clear it when the turn finishes. On reload, the chat
 * provider checks for a pending entry and replays the buffered answer from the
 * gateway via `/api/chat/resume/{requestId}` (EventSource), concatenating the
 * raw deltas to recover the in-flight text — then clears the entry.
 *
 * sessionStorage (not localStorage): a resume only makes sense within the same
 * tab/session; a closed tab's in-flight turn is abandoned.
 */

const KEY = 'velion.chat.pendingStream.v1'

interface PendingMap {
  [sessionId: string]: string // requestId
}

function read(): PendingMap {
  if (typeof window === 'undefined') return {}
  try {
    const raw = window.sessionStorage.getItem(KEY)
    return raw ? (JSON.parse(raw) as PendingMap) : {}
  } catch {
    return {}
  }
}

function write(map: PendingMap): void {
  if (typeof window === 'undefined') return
  try {
    window.sessionStorage.setItem(KEY, JSON.stringify(map))
  } catch {
    // Storage full / disabled — resume is best-effort.
  }
}

/** Record that a streaming turn is in flight for this session. */
export function rememberPendingStream(sessionId: string, requestId: string): void {
  if (!sessionId || !requestId) return
  const map = read()
  map[sessionId] = requestId
  write(map)
}

/** Clear the pending marker once a turn completes (or is abandoned). */
export function clearPendingStream(sessionId: string): void {
  if (!sessionId) return
  const map = read()
  if (map[sessionId]) {
    delete map[sessionId]
    write(map)
  }
}

/** The gateway request_id of a pending turn for this session, if any. */
export function getPendingStream(sessionId: string): string | null {
  if (!sessionId) return null
  return read()[sessionId] ?? null
}

interface GatewayResumeChunk {
  delta?: string
  done?: boolean
}

/**
 * Resume a pending turn by replaying its buffered deltas from the gateway.
 * Calls `onText` with the cumulative recovered text as deltas arrive and
 * resolves when the stream ends. Clears the pending marker on completion.
 *
 * Returns `false` immediately when there's nothing to resume.
 */
export async function resumePendingStream(
  sessionId: string,
  onText: (cumulative: string) => void,
  options: { signal?: AbortSignal } = {},
): Promise<boolean> {
  const requestId = getPendingStream(sessionId)
  if (!requestId) return false

  let cumulative = ''
  const response = await fetch(`/api/chat/resume/${encodeURIComponent(requestId)}`, {
    method: 'GET',
    cache: 'no-store',
    headers: { Accept: 'text/event-stream' },
    signal: options.signal,
  }).catch(() => null)

  // 404 / error → buffer expired or unknown; nothing to recover.
  if (!response || !response.ok || !response.body) {
    clearPendingStream(sessionId)
    return false
  }

  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''

  try {
    for (;;) {
      const { value, done } = await reader.read()
      if (done) break
      buffer += decoder.decode(value, { stream: true })

      let sep = buffer.indexOf('\n\n')
      while (sep !== -1) {
        const rawEvent = buffer.slice(0, sep)
        buffer = buffer.slice(sep + 2)
        sep = buffer.indexOf('\n\n')

        const dataLines: string[] = []
        for (const line of rawEvent.split('\n')) {
          if (line.startsWith('data:')) dataLines.push(line.slice(5).trim())
        }
        if (dataLines.length === 0) continue

        let chunk: GatewayResumeChunk
        try {
          chunk = JSON.parse(dataLines.join('\n')) as GatewayResumeChunk
        } catch {
          continue
        }

        if (chunk.delta) {
          cumulative += chunk.delta
          onText(cumulative)
        }
        if (chunk.done) {
          clearPendingStream(sessionId)
          return true
        }
      }
    }
  } finally {
    reader.releaseLock()
  }

  // Stream ended without an explicit done — keep whatever we recovered and
  // clear so we don't loop on reload.
  clearPendingStream(sessionId)
  return true
}
