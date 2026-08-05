import { NextRequest, NextResponse } from 'next/server'

/**
 * Verevon edge middleware — closes verevon-gap.md G1.
 *
 * Replaces the dead `src/proxy.ts` (never wired) and the per-page
 * `getServerSession() + redirect()` boilerplate scattered across protected
 * pages. Sits in front of every matched route and enforces:
 *
 *  1. Protected paths require a *valid* session (not just a cookie name).
 *     We validate against auth-core `/api/v2/auth/getSession` once and cache
 *     the verdict in-process for 30s, keyed by SHA-256 of the cookie value
 *     (so we never log raw cookies).
 *
 *  2. Auth-only paths (e.g. /sign-up) bounce already-authenticated users
 *     to /dashboard.
 *
 *  3. /login is intentionally NOT protected so users with stale browser
 *     cookies can still reach the page; AuthPage redirects to /dashboard
 *     client-side when validation succeeds.
 *
 *  4. If auth-core is unreachable, we fail-open for protected paths but mark
 *     the response so per-page `getServerSession()` runs as the second line
 *     of defence. Hard-failing here would lock everyone out during outage.
 */

// G23 — accepted cookie-name set is split into "canonical" (Better Auth's
// current writer) vs "legacy" (older writers we still honour for backward
// compat). When the edge gate accepts a legacy name, we log a single
// `legacy_cookie_seen` warn line so 30 days of zero traffic on a name lets
// us drop it. Do not delete a row from `LEGACY_SESSION_COOKIES` without
// running the log query first.
const CANONICAL_SESSION_COOKIES = [
  'better-auth.session_token',
  'idknuten.sid',
  'idknuten.session_token',
] as const

const LEGACY_SESSION_COOKIES = [
  'auth_session',
  'session_token',
] as const

const SESSION_COOKIES = [
  ...CANONICAL_SESSION_COOKIES,
  ...LEGACY_SESSION_COOKIES,
] as const

const LEGACY_SET = new Set<string>(LEGACY_SESSION_COOKIES)

const SESSION_COOKIE_PATTERNS = [
  /^(?:__Secure-)?sid$/,
  /^(?:__Secure-)?sid_multi-/,
  /^(?:__Secure-)?session_token$/,
] as const

function logLegacyCookie(name: string): void {
  // Structured-log line so the telemetry sink + container-log ingestion can
  // count occurrences. Emitted at most once per request (the extract loop
  // returns on the first hit).
  // eslint-disable-next-line no-console -- structured log channel (G23)
  console.log(
    JSON.stringify({
      level: 'warn',
      msg: 'legacy_cookie_seen',
      cookie_name: name,
    }),
  )
}

const PROTECTED_PREFIXES = [
  '/agents',
  '/overview',
  '/dashboard',
  '/planner',
  '/tasks',
  '/profile',
  '/settings',
  '/workspace',
  '/onboarding',
] as const

const AUTH_ONLY_PATHS = ['/sign-up'] as const

const AUTH_SERVICE_URL = (
  process.env.AUTH_SERVICE_URL || 'http://auth-service:3011'
).replace(/\/+$/, '')

// In-process cache of validation verdicts. Keyed by sha256(cookieString) so we
// never store raw cookies in memory. Lifetime is 30s — short enough that
// logout invalidation is acceptable (sub-30s lag) and long enough to absorb
// the typical SPA navigation burst without hammering auth-core.
//
// G28 / G28-followup: cache the resolved `userId` + `userEmail` + `userName`
// alongside the `ok` flag so the edge gate can forward all three to
// protected pages via `x-verevon-user-{id,email,name}` request headers,
// eliminating the per-page `getServerSession()` round-trip that duplicated
// the work middleware already did. Profile fields are bounded-size (~256 B
// combined) and well under any sane header-limit (8 KB typical).
type CacheEntry = { ok: boolean; userId: string; userEmail: string; userName: string; expiresAt: number }
const VERDICT_CACHE = new Map<string, CacheEntry>()
const VERDICT_TTL_MS = 30_000
const MAX_CACHE_ENTRIES = 5_000

function pruneCache(now: number) {
  if (VERDICT_CACHE.size < MAX_CACHE_ENTRIES) return
  for (const [k, v] of VERDICT_CACHE) {
    if (v.expiresAt <= now) VERDICT_CACHE.delete(k)
  }
}

async function sha256Hex(input: string): Promise<string> {
  const data = new TextEncoder().encode(input)
  const digest = await crypto.subtle.digest('SHA-256', data)
  const bytes = new Uint8Array(digest)
  let out = ''
  for (let i = 0; i < bytes.length; i++) {
    out += bytes[i].toString(16).padStart(2, '0')
  }
  return out
}

function extractSessionCookieValue(request: NextRequest): string | null {
  for (const name of SESSION_COOKIES) {
    const v = request.cookies.get(name)?.value
    if (v) {
      if (LEGACY_SET.has(name)) logLegacyCookie(name)
      return `${name}=${v}`
    }
  }
  for (const c of request.cookies.getAll()) {
    if (SESSION_COOKIE_PATTERNS.some((p) => p.test(c.name))) {
      return `${c.name}=${c.value}`
    }
  }
  return null
}

interface ValidationResult {
  ok: boolean
  userId: string
  userEmail: string
  userName: string
}

async function validateSession(cookieHeader: string): Promise<ValidationResult> {
  // Edge fetch with a tight timeout. AbortController is supported in Edge.
  const ctrl = new AbortController()
  const timer = setTimeout(() => ctrl.abort(), 1_500)
  try {
    // G30 v3: use Better Auth's native /api/auth/get-session (GET) instead of
    // the custom oRPC /api/v2/auth/getSession (POST). The custom wrapper
    // intermittently returns "not authenticated" for cookies Better Auth
    // itself accepts.
    const res = await fetch(`${AUTH_SERVICE_URL}/api/auth/get-session`, {
      method: 'GET',
      headers: {
        'Content-Type': 'application/json',
        Cookie: cookieHeader,
      },
      signal: ctrl.signal,
      cache: 'no-store',
    })
    const empty: ValidationResult = { ok: false, userId: '', userEmail: '', userName: '' }
    if (!res.ok) return empty
    const payload: unknown = await res.json().catch(() => null)
    if (!payload || typeof payload !== 'object') return empty
    const obj = payload as Record<string, unknown>
    if (obj.authenticated === false) return empty
    const inner = (obj.data && typeof obj.data === 'object' ? (obj.data as Record<string, unknown>) : obj)
    const user = inner.user as Record<string, unknown> | undefined
    if (user && typeof user.id === 'string' && user.id.length > 0) {
      return {
        ok: true,
        userId: user.id,
        userEmail: typeof user.email === 'string' ? user.email : '',
        userName: typeof user.name === 'string' ? user.name : '',
      }
    }
    return empty
  } catch {
    // Auth-core unreachable / timeout — fail open to avoid lockout. Per-page
    // `getServerSession()` will catch genuine session loss as a second gate.
    return { ok: true, userId: '', userEmail: '', userName: '' }
  } finally {
    clearTimeout(timer)
  }
}

async function isAuthenticated(request: NextRequest): Promise<ValidationResult> {
  const cookieHeader = extractSessionCookieValue(request)
  if (!cookieHeader) return { ok: false, userId: '', userEmail: '', userName: '' }

  const now = Date.now()
  const key = await sha256Hex(cookieHeader)
  const cached = VERDICT_CACHE.get(key)
  if (cached && cached.expiresAt > now) {
    return { ok: cached.ok, userId: cached.userId, userEmail: cached.userEmail, userName: cached.userName }
  }

  const result = await validateSession(cookieHeader)
  VERDICT_CACHE.set(key, { ...result, expiresAt: now + VERDICT_TTL_MS })
  pruneCache(now)
  return result
}

// G28 / G28-followup: headers protected pages read via `next/headers` to
// skip their own `getServerSession()` call. Trusted because the edge gate
// is the only writer (the matcher excludes anything that could be
// user-controlled reaching the page).
export const VEREVON_USER_ID_HEADER = 'x-verevon-user-id'
export const VEREVON_USER_EMAIL_HEADER = 'x-verevon-user-email'
export const VEREVON_USER_NAME_HEADER = 'x-verevon-user-name'

export async function proxy(request: NextRequest) {
  const { pathname } = request.nextUrl

  // E2E test bypass — never active in production.
  if (
    process.env.NODE_ENV !== 'production' &&
    request.headers.get('x-e2e-bypass') ===
      (process.env.E2E_BYPASS_SECRET ?? 'e2e-dev-bypass-secret')
  ) {
    return NextResponse.next()
  }

  const isProtected = PROTECTED_PREFIXES.some((p) => pathname.startsWith(p))
  const isAuthOnly = AUTH_ONLY_PATHS.some((p) => pathname.startsWith(p))
  if (!isProtected && !isAuthOnly) return NextResponse.next()

  const { ok: authenticated, userId, userEmail, userName } = await isAuthenticated(request)

  if (isAuthOnly && authenticated) {
    return NextResponse.redirect(new URL('/dashboard', request.url))
  }

  if (isProtected && !authenticated) {
    const loginUrl = new URL('/login', request.url)
    loginUrl.searchParams.set('redirect', `${pathname}${request.nextUrl.search}`)
    return NextResponse.redirect(loginUrl)
  }

  // G28 / G28-followup: stamp the resolved user identity onto the forwarded
  // request so protected pages can read it via `next/headers` instead of
  // re-running `getServerSession()`. `userId` is empty on the fail-open path
  // (auth-core unreachable); pages must treat absence as "fall through to
  // the legacy `getServerSession()` defence-in-depth check."
  if (isProtected && userId) {
    const forwarded = new Headers(request.headers)
    forwarded.set(VEREVON_USER_ID_HEADER, userId)
    if (userEmail) forwarded.set(VEREVON_USER_EMAIL_HEADER, userEmail)
    if (userName) forwarded.set(VEREVON_USER_NAME_HEADER, userName)
    return NextResponse.next({ request: { headers: forwarded } })
  }

  return NextResponse.next()
}

export const config = {
  matcher: [
    '/agents',
    '/agents/:path*',
    '/overview',
    '/overview/:path*',
    '/dashboard',
    '/dashboard/:path*',
    '/planner',
    '/planner/:path*',
    '/tasks',
    '/tasks/:path*',
    '/profile',
    '/profile/:path*',
    '/settings',
    '/settings/:path*',
    '/workspace',
    '/workspace/:path*',
    '/onboarding',
    '/onboarding/:path*',
    '/sign-up',
  ],
}
