/**
 * Model Plane harness client.
 *
 * One place for the cross-cutting concerns every velion → model-gateway proxy
 * was reinventing: bearer-token minting, correlation-id propagation, retry with
 * backoff, timeouts, telemetry, and typed errors. See
 * `apps/Model Plane/docs/HARNESS_PHASE1.md` §5.
 *
 * Server-only. Import from Next.js route handlers / server actions, never from
 * a client component (it mints tokens and reads server env).
 *
 * Two entry points:
 *   - {@link harnessFetch}  → returns the raw `Response` after retries. Use for
 *     SSE / streaming consumers that read `response.body` themselves.
 *   - {@link harnessJson}   → awaits + parses JSON, throwing {@link HarnessError}
 *     on non-2xx. Use for unary calls.
 */

import { randomUUID } from 'node:crypto'

import {
  getModelPlaneTokenFromCookie,
  getModelPlaneTokenInternal,
} from './auth-token'

const CORRELATION_HEADER = 'x-correlation-id'

function modelGatewayUrl(): string {
  return (process.env.MODEL_GATEWAY_URL || 'http://localhost:8080').replace(/\/+$/, '')
}

/** How the gateway JWT is obtained for this call. */
export interface HarnessAuth {
  /** Browser session cookie header — forwarded to auth-core to mint a user JWT. */
  cookieHeader?: string
  /** Service-to-service identity — used when there is no browser session. */
  internalClaims?: {
    userId: string
    orgId: string
    email?: string
    scopes?: readonly string[]
  }
}

export interface RetryConfig {
  /** Total attempts including the first. */
  maxAttempts: number
  /** Base delay for exponential backoff. */
  baseDelayMs: number
  /** Upper bound on any single backoff delay. */
  maxDelayMs: number
}

const DEFAULT_RETRY: RetryConfig = {
  maxAttempts: 3,
  baseDelayMs: 200,
  maxDelayMs: 5_000,
}

/** HTTP statuses worth retrying — transient by nature. */
const RETRYABLE_STATUS = new Set([429, 502, 503, 504])

export type HarnessErrorCode =
  | 'timeout'
  | 'network'
  | 'http'
  | 'auth'
  | 'aborted'

/** Typed error surfaced by the harness client. */
export class HarnessError extends Error {
  readonly code: HarnessErrorCode
  readonly status?: number
  readonly retryable: boolean
  readonly correlationId: string
  readonly attempts: number

  constructor(args: {
    code: HarnessErrorCode
    message: string
    correlationId: string
    attempts: number
    status?: number
    retryable?: boolean
    cause?: unknown
  }) {
    super(args.message, args.cause ? { cause: args.cause } : undefined)
    this.name = 'HarnessError'
    this.code = args.code
    this.status = args.status
    this.retryable = args.retryable ?? false
    this.correlationId = args.correlationId
    this.attempts = args.attempts
  }
}

/** Telemetry sink — reports one record per completed call (success or failure). */
export interface HarnessTelemetryRecord {
  path: string
  method: string
  correlationId: string
  status?: number
  attempts: number
  latencyMs: number
  outcome: 'ok' | 'error'
  errorCode?: HarnessErrorCode
}

type TelemetryReporter = (record: HarnessTelemetryRecord) => void

let telemetryReporter: TelemetryReporter | null = null

/** Register a process-wide telemetry sink. Defaults to no-op. */
export function setHarnessTelemetry(reporter: TelemetryReporter | null): void {
  telemetryReporter = reporter
}

function report(record: HarnessTelemetryRecord): void {
  try {
    telemetryReporter?.(record)
  } catch {
    // Telemetry must never break the request path.
  }
}

export interface HarnessRequestOptions {
  /** Gateway-relative path, e.g. "/v1/invoke" or "/v1/runs/abc/events". */
  path: string
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'
  /** JSON-serializable body. Ignored for GET/DELETE. */
  body?: unknown
  auth?: HarnessAuth
  /** Caller abort signal — composed with the internal timeout. */
  signal?: AbortSignal
  /** Per-request timeout. Defaults to 30s; pass 0 to disable (e.g. streaming). */
  timeoutMs?: number
  /** Retry policy. Pass `false` to disable, or a partial override. */
  retry?: Partial<RetryConfig> | false
  /**
   * Allow retrying a non-GET request. Only set when the upstream operation is
   * safe to repeat (idempotent). Defaults to false for write methods.
   */
  idempotent?: boolean
  /** Extra headers. `Authorization` and the correlation id are set for you. */
  headers?: Record<string, string>
  /** `Accept` header — set to `text/event-stream` for SSE; skips JSON assumptions. */
  accept?: string
  /** Reuse an inbound correlation id instead of generating one. */
  correlationId?: string
}

async function resolveBearer(auth: HarnessAuth | undefined, correlationId: string, attempts: number): Promise<string> {
  try {
    if (auth?.cookieHeader) {
      return await getModelPlaneTokenFromCookie(auth.cookieHeader)
    }
    if (auth?.internalClaims) {
      return await getModelPlaneTokenInternal(auth.internalClaims)
    }
    return (
      process.env.MODEL_GATEWAY_BEARER ??
      process.env.INTERNAL_API_KEY ??
      process.env.INTERNAL_SERVICE_SECRET ??
      'dev-bypass'
    )
  } catch (error: unknown) {
    throw new HarnessError({
      code: 'auth',
      message: `failed to mint Model Plane token: ${error instanceof Error ? error.message : String(error)}`,
      correlationId,
      attempts,
      retryable: false,
      cause: error,
    })
  }
}

/** Parse a `Retry-After` header (delta-seconds or HTTP date) into ms, if present. */
function retryAfterMs(response: Response): number | null {
  const raw = response.headers.get('retry-after')
  if (!raw) return null
  const seconds = Number(raw)
  if (Number.isFinite(seconds)) return Math.max(0, seconds * 1_000)
  const date = Date.parse(raw)
  if (Number.isFinite(date)) return Math.max(0, date - Date.now())
  return null
}

function backoffMs(attempt: number, cfg: RetryConfig): number {
  const exp = cfg.baseDelayMs * 2 ** (attempt - 1)
  const capped = Math.min(exp, cfg.maxDelayMs)
  // Full jitter — spreads retries so a recovering upstream isn't thundered.
  return Math.random() * capped
}

function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(new HarnessError({ code: 'aborted', message: 'aborted', correlationId: '', attempts: 0 }))
      return
    }
    const timer = setTimeout(resolve, ms)
    signal?.addEventListener(
      'abort',
      () => {
        clearTimeout(timer)
        reject(new HarnessError({ code: 'aborted', message: 'aborted', correlationId: '', attempts: 0 }))
      },
      { once: true },
    )
  })
}

/**
 * Fetch the gateway with auth, correlation id, timeout, retry, and telemetry.
 * Returns the raw `Response` (so streaming callers can read `body`). The
 * response may be non-2xx — `harnessJson` is the place that throws on status.
 */
export async function harnessFetch(opts: HarnessRequestOptions): Promise<Response> {
  const method = opts.method ?? 'POST'
  const correlationId = opts.correlationId ?? randomUUID()
  const timeoutMs = opts.timeoutMs ?? 30_000
  const retryCfg: RetryConfig | null =
    opts.retry === false ? null : { ...DEFAULT_RETRY, ...(opts.retry ?? {}) }
  const writeMethod = method !== 'GET'
  const canRetryMethod = !writeMethod || opts.idempotent === true
  const url = `${modelGatewayUrl()}${opts.path}`
  const start = Date.now()

  const maxAttempts = retryCfg && canRetryMethod ? retryCfg.maxAttempts : 1
  let lastError: HarnessError | null = null

  for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
    const bearer = await resolveBearer(opts.auth, correlationId, attempt)

    const controller = new AbortController()
    const timer = timeoutMs > 0 ? setTimeout(() => controller.abort(), timeoutMs) : null
    if (opts.signal) {
      if (opts.signal.aborted) controller.abort()
      else opts.signal.addEventListener('abort', () => controller.abort(), { once: true })
    }

    const headers: Record<string, string> = {
      Authorization: `Bearer ${bearer}`,
      [CORRELATION_HEADER]: correlationId,
      ...(opts.accept ? { Accept: opts.accept } : {}),
      ...(writeMethod && opts.body !== undefined ? { 'Content-Type': 'application/json' } : {}),
      ...(opts.headers ?? {}),
    }

    try {
      const response = await fetch(url, {
        method,
        headers,
        body: writeMethod && opts.body !== undefined ? JSON.stringify(opts.body) : undefined,
        signal: controller.signal,
      })

      if (RETRYABLE_STATUS.has(response.status) && attempt < maxAttempts) {
        const wait = retryAfterMs(response) ?? backoffMs(attempt, retryCfg as RetryConfig)
        lastError = new HarnessError({
          code: 'http',
          message: `gateway ${response.status} on ${opts.path}`,
          correlationId,
          attempts: attempt,
          status: response.status,
          retryable: true,
        })
        await sleep(wait, opts.signal)
        continue
      }

      report({
        path: opts.path,
        method,
        correlationId,
        status: response.status,
        attempts: attempt,
        latencyMs: Date.now() - start,
        outcome: response.ok ? 'ok' : 'error',
        errorCode: response.ok ? undefined : 'http',
      })
      return response
    } catch (error: unknown) {
      const aborted = error instanceof DOMException && error.name === 'AbortError'
      const causedByCaller = opts.signal?.aborted ?? false
      const code: HarnessErrorCode = aborted ? (causedByCaller ? 'aborted' : 'timeout') : 'network'
      lastError = new HarnessError({
        code,
        message: aborted
          ? causedByCaller
            ? `request aborted by caller: ${opts.path}`
            : `request timed out after ${timeoutMs}ms: ${opts.path}`
          : `network error calling ${opts.path}: ${error instanceof Error ? error.message : String(error)}`,
        correlationId,
        attempts: attempt,
        retryable: code === 'network' || code === 'timeout',
        cause: error,
      })

      // Caller-initiated aborts and exhausted attempts are terminal.
      if (code === 'aborted' || attempt >= maxAttempts || !lastError.retryable) {
        break
      }
      await sleep(backoffMs(attempt, retryCfg as RetryConfig), opts.signal)
    } finally {
      if (timer) clearTimeout(timer)
    }
  }

  const err =
    lastError ??
    new HarnessError({ code: 'network', message: `request failed: ${opts.path}`, correlationId, attempts: maxAttempts })
  report({
    path: opts.path,
    method,
    correlationId,
    status: err.status,
    attempts: err.attempts,
    latencyMs: Date.now() - start,
    outcome: 'error',
    errorCode: err.code,
  })
  throw err
}

/**
 * Unary JSON call. Throws {@link HarnessError} on non-2xx (with the upstream
 * error message when the body is JSON or text).
 */
export async function harnessJson<T>(opts: HarnessRequestOptions): Promise<T> {
  const response = await harnessFetch(opts)
  if (!response.ok) {
    let detail = `${response.status} ${response.statusText}`
    try {
      const text = await response.text()
      if (text) {
        try {
          const parsed = JSON.parse(text) as { error?: string; message?: string }
          detail = parsed.error ?? parsed.message ?? text.slice(0, 300)
        } catch {
          detail = text.slice(0, 300)
        }
      }
    } catch {
      // keep status-line detail
    }
    throw new HarnessError({
      code: 'http',
      message: `gateway ${response.status} on ${opts.path}: ${detail}`,
      correlationId: response.headers.get(CORRELATION_HEADER) ?? opts.correlationId ?? '',
      attempts: 1,
      status: response.status,
      retryable: RETRYABLE_STATUS.has(response.status),
    })
  }
  if (response.status === 204) return {} as T
  return (await response.json()) as T
}
