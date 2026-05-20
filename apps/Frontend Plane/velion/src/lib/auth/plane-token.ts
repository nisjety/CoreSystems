/**
 * Phase A · A1.4 — generic plane-token minter.
 *
 * Generalises the existing `lib/model-plane/auth-token.ts` machinery so
 * every cross-plane fetch in velion mints an audience-scoped JWT from
 * auth-core instead of forwarding the raw browser cookie or trusting an
 * `X-Org-ID` header. Backstops the Wave 3 multi-tenant trust contract:
 *
 *   1. Velion server-side route handler calls `mintPlaneToken({ audience,
 *      request })` (session path) or `mintPlaneTokenInternal({ audience,
 *      ... })` (worker path).
 *   2. auth-core verifies the Better Auth session (or `INTERNAL_API_KEY`)
 *      and returns an RS256 JWT carrying `{ sub, org_id, roles, scopes,
 *      aud: <audience> }` claims with a 60–300s expiry.
 *   3. The receiving plane validates the JWT against
 *      `auth-core/api/convex-auth/jwks` and derives `org_id` from the
 *      verified claim — never from a client-supplied header.
 *
 * Tokens are TTL-cached in-process with a SHA-256 hashed key
 * (cookie ⊕ audience for session-path, claims tuple ⊕ audience for the
 * internal path). The cache is bounded at {@link MAX_CACHE_ENTRIES} with
 * LRU-on-touch eviction so a long-running velion process can't grow the
 * map unbounded.
 *
 * Dev escape hatch: when {@link DEV_BYPASS_OPT_IN} is true AND auth-core
 * is unreachable, the literal `dev-bypass` token is returned. Each plane
 * gateway has its own `*_AUTH_DEV_BYPASS=1` env to accept it. Production
 * must leave both opt-ins unset.
 */

import { createHash } from 'node:crypto'
import type { NextRequest } from 'next/server'

/**
 * Audiences understood by auth-core. Each maps to a `/api/<audience>/token`
 * (session path) and `/api/<audience>/internal-token` (worker path) on
 * auth-core. When extending, mirror the entry on the auth-core side AND
 * the receiving plane's JWT verifier.
 */
export type PlaneAudience =
  | 'model-plane'
  | 'data-plane'
  | 'quarry'
  | 'ingestion'
  | 'control-plane'
  | 'application-plane'

/** Lifetime returned by auth-core. */
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
  /** Last-touched timestamp for LRU eviction. */
  lastUsedMs: number
}

const MAX_CACHE_ENTRIES = 2048

const AUTH_CORE_BASE_URL = (
  process.env.AUTH_CORE_URL ||
  process.env.BETTER_AUTH_BASE_URL ||
  'http://auth-core:3011'
).replace(/\/+$/, '')

const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY ||
  process.env.INTERNAL_SERVICE_SECRET ||
  ''

const DEV_BYPASS_OPT_IN =
  (process.env.PLANE_TOKEN_DEV_BYPASS || process.env.MODEL_PLANE_USE_DEV_BYPASS || '')
    .toLowerCase() === '1' ||
  (process.env.PLANE_TOKEN_DEV_BYPASS || process.env.MODEL_PLANE_USE_DEV_BYPASS || '')
    .toLowerCase() === 'true'

const DEV_BYPASS_TOKEN = 'dev-bypass'

/**
 * Refresh-safety window: we treat a cached token as expired this far
 * before its real expiry, so clock-skew between velion and the plane
 * gateway can't cause a 401 right at the boundary.
 */
const REFRESH_SAFETY_MS = 30_000

const tokenCache = new Map<string, CachedToken>()

function hashKey(rawKey: string): string {
  return createHash('sha256').update(rawKey).digest('hex')
}

function isCachedTokenFresh(entry: CachedToken | undefined): entry is CachedToken {
  return Boolean(entry && entry.expiresAtMs - Date.now() > REFRESH_SAFETY_MS)
}

function touchEntry(hashedKey: string, entry: CachedToken): void {
  entry.lastUsedMs = Date.now()
  tokenCache.set(hashedKey, entry)
}

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

function devBypassFallback(audience: PlaneAudience): string {
  if (!DEV_BYPASS_OPT_IN) {
    throw new PlaneTokenError(
      `${audience}: token unavailable and PLANE_TOKEN_DEV_BYPASS is not set`,
      audience,
    )
  }
  return DEV_BYPASS_TOKEN
}

export class PlaneTokenError extends Error {
  readonly audience: PlaneAudience
  constructor(message: string, audience: PlaneAudience) {
    super(message)
    this.name = 'PlaneTokenError'
    this.audience = audience
  }
}

/**
 * URL paths on auth-core. The path layout mirrors the existing
 * `/api/model-plane/{token,internal-token}` controller — add a sibling
 * controller per audience under the same conventions.
 */
function sessionTokenUrl(audience: PlaneAudience): string {
  return `${AUTH_CORE_BASE_URL}/api/${audience}/token`
}

function internalTokenUrl(audience: PlaneAudience): string {
  return `${AUTH_CORE_BASE_URL}/api/${audience}/internal-token`
}

/**
 * Mint (or reuse cached) plane-scoped JWT from the caller's Better Auth
 * session cookie. The cookie header is forwarded verbatim to auth-core so
 * user identity resolves end-to-end.
 *
 * Cache key incorporates BOTH the cookie hash AND the audience, so a
 * single user-session yields distinct entries per audience and we never
 * accidentally send a model-plane-scoped token to data-plane.
 *
 * @throws PlaneTokenError when auth-core returns non-2xx and the
 *   dev-bypass escape hatch is not opted into. Callers should let the
 *   error surface as a 502 — falling back silently to a bogus token would
 *   mask the underlying outage.
 */
export async function mintPlaneToken(opts: {
  audience: PlaneAudience
  request: NextRequest
}): Promise<string> {
  const { audience, request } = opts
  const cookie = request.headers.get('cookie') ?? ''
  if (!cookie) {
    return devBypassFallback(audience)
  }

  const hashedKey = hashKey(`session:${audience}:${cookie}`)
  const cached = tokenCache.get(hashedKey)
  if (isCachedTokenFresh(cached)) {
    touchEntry(hashedKey, cached)
    return cached.token
  }

  try {
    const res = await fetch(sessionTokenUrl(audience), {
      method: 'GET',
      headers: { cookie, accept: 'application/json' },
      cache: 'no-store',
    })
    if (!res.ok) {
      throw new PlaneTokenError(
        `auth-core ${res.status}: ${(await res.text()).slice(0, 200)}`,
        audience,
      )
    }
    const bundle = (await res.json()) as TokenBundle
    if (!bundle?.token) {
      throw new PlaneTokenError('auth-core returned no token', audience)
    }
    rememberToken(hashedKey, bundle)
    return bundle.token
  } catch (error: unknown) {
    if (DEV_BYPASS_OPT_IN) {
      return DEV_BYPASS_TOKEN
    }
    if (error instanceof PlaneTokenError) {
      throw error
    }
    throw new PlaneTokenError(
      error instanceof Error ? error.message : String(error),
      audience,
    )
  }
}

/**
 * Mint a plane-scoped JWT for a server-side caller that has no browser
 * session (background workers, cron jobs, post-deploy hooks). Uses the
 * shared `INTERNAL_API_KEY` to authenticate against auth-core; auth-core
 * derives the token claims from the supplied identity payload.
 *
 * Production: restrict the internal-token endpoint to the
 * `inter-plane-bus` network and rotate the shared secret regularly.
 */
export async function mintPlaneTokenInternal(opts: {
  audience: PlaneAudience
  userId: string
  orgId: string
  email?: string
  scopes?: readonly string[]
}): Promise<string> {
  const { audience, userId, orgId, email, scopes } = opts
  if (!INTERNAL_API_KEY) {
    return devBypassFallback(audience)
  }
  const scopeKey = (scopes ?? []).slice().sort().join(',')
  const hashedKey = hashKey(`internal:${audience}:${userId}:${orgId}:${scopeKey}`)
  const cached = tokenCache.get(hashedKey)
  if (isCachedTokenFresh(cached)) {
    touchEntry(hashedKey, cached)
    return cached.token
  }

  try {
    const res = await fetch(internalTokenUrl(audience), {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        'x-internal-api-key': INTERNAL_API_KEY,
        accept: 'application/json',
      },
      body: JSON.stringify({ userId, orgId, email, scopes }),
      cache: 'no-store',
    })
    if (!res.ok) {
      throw new PlaneTokenError(
        `auth-core ${res.status}: ${(await res.text()).slice(0, 200)}`,
        audience,
      )
    }
    const bundle = (await res.json()) as TokenBundle
    if (!bundle?.token) {
      throw new PlaneTokenError('auth-core returned no token', audience)
    }
    rememberToken(hashedKey, bundle)
    return bundle.token
  } catch (error: unknown) {
    if (DEV_BYPASS_OPT_IN) {
      return DEV_BYPASS_TOKEN
    }
    if (error instanceof PlaneTokenError) {
      throw error
    }
    throw new PlaneTokenError(
      error instanceof Error ? error.message : String(error),
      audience,
    )
  }
}

/**
 * Cookie-string variant of {@link mintPlaneToken}, for the handful of
 * call sites that have a raw cookie header in hand (e.g. helpers in
 * `lib/affine/auth-bridge.ts`) instead of a `NextRequest`. Same caching
 * semantics — cache key derives from `(audience, cookie)`.
 */
export async function mintPlaneTokenFromCookie(opts: {
  audience: PlaneAudience
  cookieHeader: string
}): Promise<string> {
  const { audience, cookieHeader } = opts
  if (!cookieHeader) {
    return devBypassFallback(audience)
  }
  const hashedKey = hashKey(`session:${audience}:${cookieHeader}`)
  const cached = tokenCache.get(hashedKey)
  if (isCachedTokenFresh(cached)) {
    touchEntry(hashedKey, cached)
    return cached.token
  }

  try {
    const res = await fetch(sessionTokenUrl(audience), {
      method: 'GET',
      headers: { cookie: cookieHeader, accept: 'application/json' },
      cache: 'no-store',
    })
    if (!res.ok) {
      throw new PlaneTokenError(
        `auth-core ${res.status}: ${(await res.text()).slice(0, 200)}`,
        audience,
      )
    }
    const bundle = (await res.json()) as TokenBundle
    if (!bundle?.token) {
      throw new PlaneTokenError('auth-core returned no token', audience)
    }
    rememberToken(hashedKey, bundle)
    return bundle.token
  } catch (error: unknown) {
    if (DEV_BYPASS_OPT_IN) {
      return DEV_BYPASS_TOKEN
    }
    if (error instanceof PlaneTokenError) {
      throw error
    }
    throw new PlaneTokenError(
      error instanceof Error ? error.message : String(error),
      audience,
    )
  }
}

/**
 * For tests + admin tools — flush the in-process cache. Not exported on
 * the package barrel by intent; import via the file path.
 */
export function __resetPlaneTokenCacheForTests(): void {
  tokenCache.clear()
}
