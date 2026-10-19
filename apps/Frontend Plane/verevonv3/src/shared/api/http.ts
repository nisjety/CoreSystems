import { gatewayBaseUrl } from './config'

type Envelope<T> = { data: T } | T

/**
 * Browser-wide signal for a protected request whose server-side session is no
 * longer valid. Keeping this as a DOM event prevents the transport layer from
 * importing the session store (which would create an auth-client ↔ transport
 * cycle), while still letting the application shell return the user to login.
 */
export const SESSION_EXPIRED_EVENT = 'verevon:session-expired'

/**
 * Typed transport error. Carries the HTTP status and — when the backend
 * provides one — a machine-readable `code` (e.g. Better Auth's
 * `EMAIL_NOT_VERIFIED`, `INVALID_EMAIL_OR_PASSWORD`) so callers can branch on
 * intent instead of string-matching messages.
 */
export class ApiError extends Error {
  readonly status: number
  readonly code: string | null
  /** The typed `details` object of the envelope, when the body had one. Never rendered. */
  readonly details: Record<string, unknown> | null
  /** When to try again, in whole seconds (1..3600), from the envelope or the `Retry-After` header. */
  readonly retryAfterSeconds: number | null
  /** The failed run can be resumed (`resumable` in the envelope or its details). */
  readonly resumable: boolean
  /** The typed bot-wall reason (`blockReason`, or Quarry's `details.block_reason`), as sent. */
  readonly blockReason: string | null

  constructor(message: string, status: number, code: string | null, hints: ApiErrorHints = {}) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
    this.details = hints.details ?? null
    this.retryAfterSeconds = hints.retryAfterSeconds ?? null
    this.resumable = hints.resumable === true
    this.blockReason = hints.blockReason ?? null
  }
}

/** The machine-readable parts of an error body beyond `code` and `message`. */
export type ApiErrorHints = {
  details?: Record<string, unknown> | null
  retryAfterSeconds?: number | null
  resumable?: boolean
  blockReason?: string | null
}

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
    const { code } = failure
    // A failed sign-in or 2FA challenge is an expected credential error and
    // must remain on its form. Other gateway `unauthorized` responses mean the
    // existing browser session is no longer accepted, so leaving the user on a
    // page with controls that will all fail is misleading.
    if (
      response.status === 401
      && code === 'unauthorized'
      && !path.startsWith('/api/v1/auth/')
      && typeof window !== 'undefined'
    ) {
      window.dispatchEvent(new Event(SESSION_EXPIRED_EVENT))
    }
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
    throw apiErrorFromResponse(body, response)
  }

  return unwrap(body as Envelope<T>)
}
