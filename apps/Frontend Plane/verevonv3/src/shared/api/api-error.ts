// The typed transport error and its helpers, kept free of DOM and config imports so the
// Node-side tooling (studio-demo) and the browser clients can both import it.

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

/**
 * The {@link ApiError} for a status-only refusal from a client that does not read
 * the gateway envelope (the Marketing and Visibility panels): the client's own
 * outcome code is both `code` and `message`, so a caller branches on `.code`
 * instead of matching message text, and {@link translateApiError} still answers
 * by status class for a code it has no copy for.
 */
export function clientError(status: number, code: string): ApiError {
  return new ApiError(code, status, code)
}

/** Whether `err` is an {@link ApiError} carrying one of `codes`. */
export function hasErrorCode(err: unknown, ...codes: string[]): boolean {
  return err instanceof ApiError && err.code !== null && codes.includes(err.code)
}
