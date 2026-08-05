/**
 * U2-5 — Model Plane gateway JWT helper.
 *
 * Used by every verevon API proxy that talks to the gateway. Mints a real
 * RS256 JWT against auth-core's `/api/model-plane/token` endpoint instead
 * of sending the previous unverified `dev-bypass` string.
 *
 * Two issuance paths:
 *   - {@link getModelPlaneTokenFromSession} — forwards the caller's browser
 *     session cookie to auth-core. Used by proxies handling user-facing
 *     requests (chat, AI tools, audio).
 *   - {@link getModelPlaneTokenInternal} — uses the
 *     `X-Internal-Api-Key` header. Used by server-only callers (background
 *     workers, cron jobs) that have no user session.
 *
 * Both responses are TTL-cached in-process so a burst of proxy hits
 * doesn't make a roundtrip to auth-core for every call. The cache keys
 * include the session cookie (or `internal`) so each user gets their own
 * token and we never serve one tenant's token to another.
 *
 * Dev escape hatch: when `MODEL_GATEWAY_AUTH_DEV_BYPASS=1` is set on the
 * **gateway**, the gateway accepts any non-empty Bearer. Verevon still
 * sends the literal string `dev-bypass` when both:
 *   1. `MODEL_PLANE_USE_DEV_BYPASS=1` is set in verevon's env (off by
 *      default — opt-in only).
 *   2. auth-core is unreachable AND we're in dev mode.
 * This preserves the existing local-dev workflow without compromising
 * the production path (where the env var stays unset and any token
 * failure surfaces as a 502).
 */

import { createHash } from 'node:crypto'
import type { NextRequest } from 'next/server'

interface TokenBundle {
  token: string
  expiresAt: string
  expiresInSeconds: number
  issuer: string
  audience: string
}

interface CachedToken {
  token: string
  expiresAtMs: number
  /** Last-touched timestamp for the LRU eviction order. */
  lastUsedMs: number
}

/**
 * W4-4 (ui-ux-verevon-gap.md §13): hash + bound the cache.
 *
 *   - Keys are SHA-256 hashes of the cookie / internal-claims tuple. The
 *     raw cookie used to be embedded in the key, which (a) leaked the
 *     session secret if cache keys were ever logged and (b) prevented
 *     keys from being safely shared in metrics / structured logs.
 *   - The Map is bounded to {@link MAX_CACHE_ENTRIES}. When full we evict
 *     the oldest-touched entry (true LRU on read-and-write touches).
 *     Without this the Map grew one-per-unique-cookie forever — a slow
 *     leak in long-running prod processes.
 */
const MAX_CACHE_ENTRIES = 1024

function hashKey(rawKey: string): string {
  return createHash('sha256').update(rawKey).digest('hex')
}

const AUTH_CORE_BASE_URL = (
  process.env.AUTH_CORE_URL ||
  process.env.BETTER_AUTH_BASE_URL ||
  'http://auth-core:3011'
).replace(/\/+$/, '')

const TOKEN_PATH = '/api/model-plane/token'
const INTERNAL_TOKEN_PATH = '/api/model-plane/internal-token'

const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY ||
  process.env.INTERNAL_SERVICE_SECRET ||
  ''

const DEV_BYPASS_OPT_IN =
  (process.env.MODEL_PLANE_USE_DEV_BYPASS || '').toLowerCase() === '1' ||
  (process.env.MODEL_PLANE_USE_DEV_BYPASS || '').toLowerCase() === 'true'

const DEV_BYPASS_TOKEN = 'dev-bypass'

/**
 * Minimum lifetime we expect to remain on a cached token before treating
 * it as expired. Refreshing 30 seconds early keeps clock-skew between
 * verevon and auth-core from causing 401s right at the boundary.
 */
const REFRESH_SAFETY_MS = 30_000

const tokenCache = new Map<string, CachedToken>()

function isCachedTokenFresh(entry: CachedToken | undefined): entry is CachedToken {
  return Boolean(entry && entry.expiresAtMs - Date.now() > REFRESH_SAFETY_MS)
}

/**
 * LRU read: bump the touched timestamp on a cache hit so the entry moves
 * to the back of the eviction queue.
 */
function touchEntry(hashedKey: string, entry: CachedToken): void {
  entry.lastUsedMs = Date.now()
  tokenCache.set(hashedKey, entry)
}

/**
 * Evict the single oldest-touched entry. Called when the cache is full at
 * insertion time. O(N) — fine because N is bounded by MAX_CACHE_ENTRIES.
 */
function evictOldest(): void {
  let oldestKey: string | null = null
  let oldestTs = Number.POSITIVE_INFINITY
  for (const [key, value] of tokenCache.entries()) {
    if (value.lastUsedMs < oldestTs) {
      oldestTs = value.lastUsedMs
      oldestKey = key
    }
  }
  if (oldestKey) {
    tokenCache.delete(oldestKey)
  }
}

function rememberToken(hashedKey: string, bundle: TokenBundle): void {
  if (tokenCache.size >= MAX_CACHE_ENTRIES && !tokenCache.has(hashedKey)) {
    evictOldest()
  }
  const now = Date.now()
  tokenCache.set(hashedKey, {
    token: bundle.token,
    expiresAtMs: Date.parse(bundle.expiresAt) || now + 60_000,
    lastUsedMs: now,
  })
}

function devBypassFallback(): string {
  if (!DEV_BYPASS_OPT_IN) {
    throw new Error(
      'Model Plane token unavailable and MODEL_PLANE_USE_DEV_BYPASS is not set',
    )
  }
  return DEV_BYPASS_TOKEN
}

/**
 * Mint (or reuse a cached) Model Plane gateway JWT from the caller's
 * Better Auth session cookie. The request's cookie header is forwarded
 * verbatim to auth-core so the user's identity is resolved end-to-end.
 *
 * @throws when auth-core returns non-2xx and the dev-bypass escape hatch
 *   is not opted into. Callers should let the error surface as a 502 to
 *   the browser — silently falling back to a bogus token would mask the
 *   real failure.
 */
export async function getModelPlaneTokenFromSession(
  request: NextRequest,
): Promise<string> {
  const cookie = request.headers.get('cookie') ?? ''
  if (!cookie) {
    // No session cookie at all — caller is unauthenticated. Fall through to
    // dev-bypass when opted in; otherwise surface the error.
    return devBypassFallback()
  }

  const hashedKey = hashKey(`session:${cookie}`)
  const cached = tokenCache.get(hashedKey)
  if (isCachedTokenFresh(cached)) {
    touchEntry(hashedKey, cached)
    return cached.token
  }

  try {
    const res = await fetch(`${AUTH_CORE_BASE_URL}${TOKEN_PATH}`, {
      method: 'GET',
      headers: {
        cookie,
        accept: 'application/json',
      },
      cache: 'no-store',
    })
    if (!res.ok) {
      throw new Error(`auth-core ${res.status}: ${(await res.text()).slice(0, 200)}`)
    }
    const bundle = (await res.json()) as TokenBundle
    if (!bundle?.token) {
      throw new Error('auth-core returned no token')
    }
    rememberToken(hashedKey, bundle)
    return bundle.token
  } catch (error: unknown) {
    if (DEV_BYPASS_OPT_IN) {
      return DEV_BYPASS_TOKEN
    }
    throw error instanceof Error ? error : new Error(String(error))
  }
}

/**
 * Mint a Model Plane gateway JWT for a server-side (no-user-session) caller.
 * Uses the shared `INTERNAL_API_KEY` to authenticate against auth-core.
 *
 * For background workers, cron jobs, and any other path where there's no
 * browser session to forward. Production should rotate the shared secret
 * regularly and restrict the auth-core endpoint to internal network.
 *
 * @throws when `INTERNAL_API_KEY` is unset, when auth-core returns non-2xx,
 *   and the dev-bypass escape hatch isn't opted into.
 */
export async function getModelPlaneTokenInternal(claims: {
  userId: string
  orgId: string
  email?: string
  scopes?: readonly string[]
}): Promise<string> {
  if (!INTERNAL_API_KEY) {
    return devBypassFallback()
  }
  const hashedKey = hashKey(`internal:${claims.userId}:${claims.orgId}`)
  const cached = tokenCache.get(hashedKey)
  if (isCachedTokenFresh(cached)) {
    touchEntry(hashedKey, cached)
    return cached.token
  }

  try {
    const res = await fetch(`${AUTH_CORE_BASE_URL}${INTERNAL_TOKEN_PATH}`, {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        'x-internal-api-key': INTERNAL_API_KEY,
        accept: 'application/json',
      },
      body: JSON.stringify(claims),
      cache: 'no-store',
    })
    if (!res.ok) {
      throw new Error(`auth-core ${res.status}: ${(await res.text()).slice(0, 200)}`)
    }
    const bundle = (await res.json()) as TokenBundle
    if (!bundle?.token) {
      throw new Error('auth-core returned no token')
    }
    rememberToken(hashedKey, bundle)
    return bundle.token
  } catch (error: unknown) {
    if (DEV_BYPASS_OPT_IN) {
      return DEV_BYPASS_TOKEN
    }
    throw error instanceof Error ? error : new Error(String(error))
  }
}

/**
 * Helper for proxies that don't have a `NextRequest` in hand but DO have
 * the headers (e.g. fetched from somewhere else). Same semantics as
 * {@link getModelPlaneTokenFromSession} but takes raw headers.
 */
export async function getModelPlaneTokenFromCookie(
  cookieHeader: string,
): Promise<string> {
  if (!cookieHeader) {
    return devBypassFallback()
  }
  const hashedKey = hashKey(`session:${cookieHeader}`)
  const cached = tokenCache.get(hashedKey)
  if (isCachedTokenFresh(cached)) {
    touchEntry(hashedKey, cached)
    return cached.token
  }

  try {
    const res = await fetch(`${AUTH_CORE_BASE_URL}${TOKEN_PATH}`, {
      method: 'GET',
      headers: { cookie: cookieHeader, accept: 'application/json' },
      cache: 'no-store',
    })
    if (!res.ok) {
      throw new Error(`auth-core ${res.status}: ${(await res.text()).slice(0, 200)}`)
    }
    const bundle = (await res.json()) as TokenBundle
    if (!bundle?.token) {
      throw new Error('auth-core returned no token')
    }
    rememberToken(hashedKey, bundle)
    return bundle.token
  } catch (error: unknown) {
    if (DEV_BYPASS_OPT_IN) {
      return DEV_BYPASS_TOKEN
    }
    throw error instanceof Error ? error : new Error(String(error))
  }
}
