import { gatewayBaseUrl } from './config'
import { ApiError, type ApiErrorHints } from './api-error'

export { ApiError, clientError, hasErrorCode, type ApiErrorHints } from './api-error'

type Envelope<T> = { data: T } | T

/**
 * Browser-wide signal for a protected request whose server-side session is no
 * longer valid. Keeping this as a DOM event prevents the transport layer from
 * importing the session store (which would create an auth-client ↔ transport
 * cycle), while still letting the application shell return the user to login.
 */
export const SESSION_EXPIRED_EVENT = 'verevon:session-expired'

/** A retry hint in whole seconds, or null when it is missing or not sane (1..3600). */
function boundedSeconds(value: unknown): number | null {
  const parsed =
    typeof value === 'number' ? value : typeof value === 'string' && /^\d{1,5}$/.test(value.trim()) ? Number(value) : Number.NaN
  return Number.isFinite(parsed) && parsed >= 1 && parsed <= 3600 ? Math.ceil(parsed) : null
}

function unwrap<T>(payload: Envelope<T>): T {
  if (payload && typeof payload === 'object' && 'data' in payload) {
    return (payload as { data: T }).data
  }
  return payload as T
}

function applyDevAuth(headers: Headers): void {
  if (headers.has('Authorization')) return
  if (import.meta.env.VITE_ALLOW_DEV_AUTH_BYPASS !== 'true') return
  headers.set('Authorization', 'Bearer dev-bypass')
}

/**
 * Extract a `{ code, message }` pair from any error body shape we receive:
 *  - gateway standard envelope: `{ error: { code, message } }`
 *  - gateway string error:      `{ error: "...", code? }` — upstream bodies the
 *    gateway passes through, e.g. model-gateway's 422
 *    `{ error, code: "no_speech_detected" }`, keep their top-level code
 *  - Better Auth passthrough:   `{ code, message }` (auth routes are proxied
 *    verbatim by the gateway, so the SPA sees Better Auth's native shape)
 */
export function extractError(
  body: unknown,
  status: number,
): { code: string | null; message: string; hints: ApiErrorHints } {
  if (body && typeof body === 'object') {
    const record = body as Record<string, unknown>

    const errorField = record.error
    if (typeof errorField === 'string' && errorField.trim()) {
      return {
        code: typeof record.code === 'string' && record.code.trim() ? record.code : null,
        message: errorField,
        hints: {},
      }
    }
    if (errorField && typeof errorField === 'object') {
      const e = errorField as Record<string, unknown>
      const details =
        e.details && typeof e.details === 'object' && !Array.isArray(e.details) ? (e.details as Record<string, unknown>) : null
      const blockReason = e.blockReason ?? details?.block_reason
      return {
        code: typeof e.code === 'string' ? e.code : null,
        message: typeof e.message === 'string' && e.message.trim() ? e.message : `Request failed (${status})`,
        hints: {
          details,
          retryAfterSeconds: boundedSeconds(e.retryAfterSeconds) ?? boundedSeconds(details?.retry_after_seconds),
          resumable: e.resumable === true || details?.resumable === true,
          blockReason: typeof blockReason === 'string' ? blockReason : null,
        },
      }
    }

    // Better Auth top-level { code, message }
    if (typeof record.message === 'string' && record.message.trim()) {
      return { code: typeof record.code === 'string' ? record.code : null, message: record.message, hints: {} }
    }
  }

  return { code: null, message: `Request failed (${status})`, hints: {} }
}

/**
 * The {@link ApiError} for a non-2xx response: the envelope's code and typed
 * hints, plus the `Retry-After` header (whole seconds) when the body carried no
 * retry hint of its own. One place, so `requestJson`, `requestForm` and the SSE
 * client cannot drift apart on which fields survive.
 */
export function apiErrorFromResponse(body: unknown, response: Pick<Response, 'status' | 'headers'>): ApiError {
  const { code, message, hints } = extractError(body, response.status)
  const retryAfterSeconds = hints.retryAfterSeconds ?? boundedSeconds(response.headers?.get('Retry-After'))
  return new ApiError(message, response.status, code, { ...hints, retryAfterSeconds })
}

/**
 * Tell the application shell that a protected request was refused because the
 * server-side session is gone: one path back to sign-in for every transport.
 *
 * Only the gateway's own 401 `unauthorized` counts, and never on `/api/v1/auth/*`:
 * a failed sign-in or two-factor challenge is an expected credential error that must
 * stay on its form, and the session probe itself answering 401 must not re-trigger
 * the probe (that would loop). A 503 `session_verification_unavailable`, a timeout or
 * any 5xx says nothing about the session and never ends it. The streaming transports
 * (`sse.ts`) do not call this: the agent stream maps the model gateway's own 401 to the
 * same code, and a credential drift there is not a sign-out.
 */
export function signalSessionExpiry(failure: ApiError, path: string): void {
  if (
    failure.status === 401
    && failure.code === 'unauthorized'
    && !path.startsWith('/api/v1/auth/')
    && typeof window !== 'undefined'
  ) {
    window.dispatchEvent(new Event(SESSION_EXPIRED_EVENT))
  }
}

export async function requestJson<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(init?.headers)
  headers.set('Accept', 'application/json')
  if (init?.body && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }
  applyDevAuth(headers)

  const response = await fetch(`${gatewayBaseUrl()}${path}`, {
    ...init,
    credentials: 'include',
    headers,
  })

  const body = (await response.json().catch(() => null)) as Envelope<T> | Record<string, unknown> | null

  if (!response.ok) {
    const failure = apiErrorFromResponse(body, response)
    // Other gateway `unauthorized` responses mean the existing browser session is
    // no longer accepted, so leaving the user on a page with controls that will
    // all fail is misleading.
    signalSessionExpiry(failure, path)
    throw failure
  }

  return unwrap(body as Envelope<T>)
}

/**
 * Multipart variant of {@link requestJson}. The browser sets the
 * `multipart/form-data` boundary itself, so we must NOT set a Content-Type
 * header (doing so would corrupt the boundary). Used for file uploads.
 */
export async function requestForm<T>(
  path: string,
  form: FormData,
  init?: Omit<RequestInit, 'body'>,
): Promise<T> {
  const headers = new Headers(init?.headers)
  headers.set('Accept', 'application/json')
  headers.delete('Content-Type')
  applyDevAuth(headers)

  const response = await fetch(`${gatewayBaseUrl()}${path}`, {
    ...init,
    method: init?.method ?? 'POST',
    credentials: 'include',
    headers,
    body: form,
  })

  const body = (await response.json().catch(() => null)) as Envelope<T> | Record<string, unknown> | null

  if (!response.ok) {
    const failure = apiErrorFromResponse(body, response)
    signalSessionExpiry(failure, path)
    throw failure
  }

  return unwrap(body as Envelope<T>)
}
