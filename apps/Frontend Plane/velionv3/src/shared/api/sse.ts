import { gatewayBaseUrl } from './config'
import { ApiError, extractError } from './http'

export type SseEvent = {
  event?: string
  data: string
  id?: string
}

export type SseOptions = {
  method?: 'GET' | 'POST'
  body?: string
  headers?: Record<string, string>
  signal?: AbortSignal
  lastEventId?: string
}

function applyDevAuth(headers: Headers): void {
  if (headers.has('Authorization')) return
  if (!import.meta.env.DEV && import.meta.env.VITE_ALLOW_DEV_AUTH_BYPASS !== 'true') return
  headers.set('Authorization', 'Bearer dev-bypass')
}

/**
 * POST-capable SSE reader using fetch. Supports `Last-Event-ID` resume
 * and multi-line data joining. Calls `onEvent` for each parsed event,
 * `onError` on connection errors, and `onDone` when the stream closes.
 */
export async function readSseStream(
  path: string,
  options: SseOptions,
  onEvent: (event: SseEvent) => void,
  onError?: (err: unknown) => void,
  onDone?: () => void,
): Promise<void> {
  const headers = new Headers({
    Accept: 'text/event-stream',
    'Cache-Control': 'no-cache',
    ...options.headers,
  })

  if (options.body && options.method !== 'GET') {
    headers.set('Content-Type', 'application/json')
  }

  if (options.lastEventId) {
    headers.set('Last-Event-ID', options.lastEventId)
  }
  applyDevAuth(headers)

  try {
    const response = await fetch(`${gatewayBaseUrl()}${path}`, {
      method: options.method ?? 'GET',
      credentials: 'include',
      headers,
      body: options.body,
      signal: options.signal,
    })

    if (!response.ok) {
      // Surface the gateway's JSON error envelope ({ error: { code, message } })
      // instead of a generic "connection failed", so onError reports the real
      // upstream code/message (e.g. unauthorized, model_error, upstream_error).
      const errorBody = await response.json().catch(() => null)
      const { code, message } = extractError(errorBody, response.status)
      throw new ApiError(message, response.status, code)
    }
    if (!response.body) {
      throw new Error(`SSE connection failed (${response.status})`)
    }

    const reader = response.body.getReader()
    const decoder = new TextDecoder()
    let buffer = ''

    // Current event fields accumulate until an empty line flushes them.
    let currentEvent: Partial<SseEvent> = {}
    const dataLines: string[] = []

    while (true) {
      const { done, value } = await reader.read()
      if (done) break

      buffer += decoder.decode(value, { stream: true })

      const lines = buffer.split('\n')
      buffer = lines.pop() ?? ''

      for (const raw of lines) {
        const line = raw.endsWith('\r') ? raw.slice(0, -1) : raw

        if (line === '') {
          // Empty line: dispatch event if we have data.
          if (dataLines.length > 0) {
            onEvent({
              event: currentEvent.event,
              data: dataLines.join('\n'),
              id: currentEvent.id,
            })
          }
          currentEvent = {}
          dataLines.length = 0
          continue
        }

        if (line.startsWith(':')) continue // comment

        const colonIndex = line.indexOf(':')
        const [field, value] =
          colonIndex === -1
            ? [line, '']
            : [line.slice(0, colonIndex), line.slice(colonIndex + 1).replace(/^ /, '')]

        if (field === 'data') {
          dataLines.push(value)
        } else if (field === 'event') {
          currentEvent.event = value
        } else if (field === 'id') {
          currentEvent.id = value
        }
      }
    }
  } catch (err) {
    if (err instanceof DOMException && err.name === 'AbortError') return
    onError?.(err)
  } finally {
    onDone?.()
  }
}
