/**
 * Client failure classification (WS07-S1, F-t1-024).
 *
 * Pure: takes whatever a boundary caught and names the one class of failure the
 * user can act on. The classes drive the failure card copy and which buttons it
 * offers. Nothing here reads or keeps the error message beyond matching it, and
 * `buildFailureDetails` (what "Copy details" puts on the clipboard) never
 * includes the message, a URL query, an id or any content.
 */

export type ClientErrorClass =
  | 'stale_deploy'
  | 'session_expired'
  | 'gateway_unreachable'
  | 'render_error'

export type ClientRegion = 'root' | 'navbar' | 'sidebar' | 'outlet' | 'space-tab' | 'panel'

/** Messages a browser gives a dynamic import (or CSS preload) that 404s after a deploy. */
const STALE_DEPLOY_PATTERNS: readonly RegExp[] = [
  /failed to fetch dynamically imported module/i,
  /error loading dynamically imported module/i,
  /importing a module script failed/i,
  /unable to preload css/i,
]

/** `fetch` rejections: Chromium, Firefox and Safari word the same network failure differently. */
const NETWORK_FAILURE_PATTERNS: readonly RegExp[] = [
  /^failed to fetch$/i,
  /networkerror when attempting to fetch resource/i,
  /^load failed$/i,
]

const GATEWAY_STATUSES: readonly number[] = [502, 503, 504]

interface ApiErrorLike {
  readonly name?: unknown
  readonly status?: unknown
  readonly code?: unknown
}

function messageOf(error: unknown): string {
  if (typeof error === 'string') return error
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message?: unknown }).message
    if (typeof message === 'string') return message
  }
  return ''
}

/**
 * `ApiError` is recognised by shape, not by import: this module is loaded by
 * the root boundary, which must work even when the API layer is what broke.
 */
function asApiError(error: unknown): { status: number; code: string | null } | null {
  if (!error || typeof error !== 'object') return null
  const candidate = error as ApiErrorLike
  if (candidate.name !== 'ApiError' || typeof candidate.status !== 'number') return null
  return {
    status: candidate.status,
    code: typeof candidate.code === 'string' ? candidate.code : null,
  }
}

export function classifyClientError(error: unknown): ClientErrorClass {
  const message = messageOf(error)
  if (STALE_DEPLOY_PATTERNS.some((pattern) => pattern.test(message))) return 'stale_deploy'

  const api = asApiError(error)
  if (api) {
    if (api.status === 401 || api.code === 'unauthenticated') return 'session_expired'
    if (GATEWAY_STATUSES.includes(api.status)) return 'gateway_unreachable'
    return 'render_error'
  }

  const isTypeError = error instanceof TypeError || (error as { name?: unknown } | null)?.name === 'TypeError'
  if (isTypeError && NETWORK_FAILURE_PATTERNS.some((pattern) => pattern.test(message.trim()))) {
    return 'gateway_unreachable'
  }
  return 'render_error'
}

/** First path segment only (`/spaces/abc/def` becomes `spaces`): the route class, never an id. */
export function routeClassOf(pathname: string): string {
  const segment = pathname.split('/').find((part) => part.length > 0) ?? ''
  // A segment with anything other than a plain slug could be an id or a token.
  return /^[a-z][a-z0-9-]{0,31}$/i.test(segment) ? segment.toLowerCase() : segment ? 'other' : 'root'
}

declare const __VEREVON_BUILD_ID__: string | undefined

/** The build id Vite injects (WS07-S2 adds the `define`); `dev` until then. */
export function clientBuildId(): string {
  return typeof __VEREVON_BUILD_ID__ === 'string' && __VEREVON_BUILD_ID__ ? __VEREVON_BUILD_ID__ : 'dev'
}

export interface FailureDetails {
  readonly class: ClientErrorClass
  readonly build_id: string
  readonly region: ClientRegion
  readonly at: string
  readonly route_class: string
}

export function buildFailureDetails(input: {
  errorClass: ClientErrorClass
  region: ClientRegion
  pathname?: string
  now?: Date
}): FailureDetails {
  const pathname = input.pathname ?? (typeof window !== 'undefined' ? window.location.pathname : '/')
  return {
    class: input.errorClass,
    build_id: clientBuildId(),
    region: input.region,
    at: (input.now ?? new Date()).toISOString(),
    route_class: routeClassOf(pathname),
  }
}
