import { createSignal } from 'solid-js'

/**
 * Content-free diagnostics for a problem report (`VITE_REPORT_PROBLEM`). Two pieces of state live here. Both are in
 * memory only, so a reload drops them, and neither is sent until the person ticks that section of the report.
 *
 *   * The last few failed gateway requests: the `x-request-id` the gateway returned, the status, the machine error
 *     code, when, and the route with its ids masked. No request or response body and no query string is kept, so
 *     nothing a person typed or read can reach a report through this ring.
 *   * The chat run the person has open, and whether that chat keeps content. A temporary chat, or an organization
 *     with zero retention, offers no run id, so a report never names a run that kept nothing.
 */

/** How many failed requests the ring keeps. */
export const RECENT_FAILURE_LIMIT = 5

/** One failed gateway request, as the ring keeps it. */
export interface FailedRequest {
  /** The `x-request-id` the gateway returned, or null when the response carried none the gateway would issue. */
  readonly requestId: string | null
  /** The HTTP status, or 0 when the response named none. */
  readonly status: number
  /** The machine error code, or null. Only a short code alphabet is kept, never a message. */
  readonly code: string | null
  /** When the failure was recorded, ISO-8601. */
  readonly at: string
  /** The request path with every id-like segment replaced by `:id`, or null when the response named no URL. */
  readonly route: string | null
}

/** The chat run a report may name. `retainsContent` is false for a temporary chat or a zero-retention organization. */
export interface SupportRunContext {
  readonly runId: string
  readonly retainsContent: boolean
}

/** The gateway's own rule for `x-request-id`: 1 to 128 bytes of `[A-Za-z0-9._:-]`. */
const REQUEST_ID_PATTERN = /^[A-Za-z0-9._:-]{1,128}$/
const ERROR_CODE_PATTERN = /^[A-Za-z0-9_.-]{1,64}$/
const ROUTE_LIMIT = 200

const [failures, setFailures] = createSignal<readonly FailedRequest[]>([])
const [runContext, setRunContext] = createSignal<SupportRunContext | null>(null)

/** The failed requests in the ring, oldest first. Read reactively. */
export const recentFailedRequests = failures

/** The chat run the person has open, or null when there is none. Read reactively. */
export const supportRunContext = runContext

/** Whether this build offers the problem report mode in the feedback widget: the strict literal `true`. Default off. */
export function reportProblemEnabled(): boolean {
  return import.meta.env.VITE_REPORT_PROBLEM === 'true'
}

/** Publish the chat run on screen, or clear it with null. The chat page calls this while it is mounted. */
export function setSupportRunContext(context: SupportRunContext | null): void {
  setRunContext(context)
}

/**
 * Record one failed response. `apiErrorFromResponse` calls this for every failed gateway call, with the response it
 * already holds. It never throws: a diagnostic must not change the error the caller receives.
 */
export function recordFailedResponse(
  response: Partial<Pick<Response, 'status' | 'headers' | 'url'>>,
  code: string | null,
): void {
  try {
    const failure: FailedRequest = {
      requestId: requestIdOf(response.headers),
      status: typeof response.status === 'number' ? response.status : 0,
      code: code !== null && ERROR_CODE_PATTERN.test(code) ? code : null,
      at: new Date().toISOString(),
      route: routeOf(response.url),
    }
    setFailures((previous) => [...previous, failure].slice(-RECENT_FAILURE_LIMIT))
  } catch {
    // A diagnostic must never break the request that failed.
  }
}

/** The request ids of the recent failures that carry one, oldest first. A report's "recent error ids" section sends these. */
export function recentErrorRequestIds(): string[] {
  return failures().flatMap((failure) => (failure.requestId === null ? [] : [failure.requestId]))
}

function requestIdOf(headers: Headers | undefined): string | null {
  const value = headers?.get('x-request-id')?.trim() ?? ''
  return REQUEST_ID_PATTERN.test(value) ? value : null
}

function routeOf(url: string | undefined): string | null {
  if (!url) return null
  let pathname: string
  try {
    pathname = new URL(url, 'http://localhost').pathname
  } catch {
    return null
  }
  return pathname
    .split('/')
    .map((segment) => (isIdSegment(segment) ? ':id' : segment))
    .join('/')
    .slice(0, ROUTE_LIMIT)
}

/**
 * A path segment that names one record rather than the route. Numbers, UUIDs and prefixed ids all carry a digit, and
 * a long opaque token is masked whatever it holds. `v1` is an API version, not an id.
 */
function isIdSegment(segment: string): boolean {
  if (/^v\d+$/i.test(segment)) return false
  return /\d/.test(segment) || segment.length > 24
}
