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

  constructor(message: string, status: number, code: string | null) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
  }
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
 *  - gateway string error:      `{ error: "..." }`
 *  - Better Auth passthrough:   `{ code, message }` (auth routes are proxied
 *    verbatim by the gateway, so the SPA sees Better Auth's native shape)
 */
export function extractError(body: unknown, status: number): { code: string | null; message: string } {
  if (body && typeof body === 'object') {
    const record = body as Record<string, unknown>

    const errorField = record.error
    if (typeof errorField === 'string' && errorField.trim()) {
      return { code: null, message: errorField }
    }
    if (errorField && typeof errorField === 'object') {
      const e = errorField as Record<string, unknown>
      return {
        code: typeof e.code === 'string' ? e.code : null,
        message: typeof e.message === 'string' && e.message.trim() ? e.message : `Request failed (${status})`,
      }
    }

    // Better Auth top-level { code, message }
    if (typeof record.message === 'string' && record.message.trim()) {
      return { code: typeof record.code === 'string' ? record.code : null, message: record.message }
    }
  }

  return { code: null, message: `Request failed (${status})` }
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
    const { code, message } = extractError(body, response.status)
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
    throw new ApiError(message, response.status, code)
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
    const { code, message } = extractError(body, response.status)
    throw new ApiError(message, response.status, code)
  }

  return unwrap(body as Envelope<T>)
}
