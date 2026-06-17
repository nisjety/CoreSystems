import 'server-only'
import { cookies } from 'next/headers'
import { getServerSession } from '@/components/auth/lib/auth-server'

const SESSION_COOKIE_NAMES = [
  'better-auth.session_token',
  'auth_session',
  'idknuten.sid',
  'idknuten.session_token',
  'session_token',
] as const

const SESSION_COOKIE_PATTERNS = [
  /^(?:__Secure-)?sid$/,
  /^(?:__Secure-)?sid_multi-/,
  /^(?:__Secure-)?session_token$/,
] as const

async function hasAnySessionCookie(): Promise<boolean> {
  const store = await cookies()
  for (const name of SESSION_COOKIE_NAMES) {
    if (store.get(name)?.value) return true
  }
  return store
    .getAll()
    .some(({ name }) => SESSION_COOKIE_PATTERNS.some((p) => p.test(name)))
}

/**
 * G30 v2: server-side resolution of the post-OAuth-callback routing decision.
 *
 * The client component (`AuthCallbackClient.tsx`) previously made 3 round
 * trips to velion API proxies as soon as the OAuth redirect landed. Better
 * Auth's secondary-storage write (Postgres + Redis) races those calls and
 * frequently produces transient 401s during the first 1-2 s after redirect.
 *
 * v1 (already shipped) wrapped the client call in 6×400 ms backoff so the
 * race was absorbed. v2 (this file) resolves the decision in the server
 * component before the client ever renders, eliminating the noisy network
 * dance entirely. The client receives the verdict as a prop.
 *
 * The legacy client path is preserved as a fallback for the rare case where
 * even the server-side backoff cannot resolve a session (e.g. cookie not
 * yet visible to the server in the very first nanoseconds of redirect).
 */

const USER_SERVICE_URL =
  (process.env.USER_SERVICE_URL || 'http://user-core:3012').replace(/\/+$/, '')

function getInternalApiKey(): string | null {
  const key = process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET
  return key && key.trim() ? key.trim() : null
}

export interface ServerOnboardingState {
  /** True iff the server resolved a Better Auth session. */
  hasSession: boolean
  /** The resolved user id, if any. Useful for analytics on the client. */
  userId: string | null
  /** The resolved onboarding decision. `null` means "fall back to client path". */
  needsOnboarding: boolean | null
  /**
   * onboardingStatus value as returned by user-core's `/me/session-context`,
   * if the server could reach it. Mirrors the contract in
   * `onboarding-service.ts:needsOnboarding`: `COMPLETED` → no wizard;
   * `CONNECTORS_PENDING` → wizard required.
   */
  onboardingStatus: string | null
}

const EMPTY: ServerOnboardingState = {
  hasSession: false,
  userId: null,
  needsOnboarding: null,
  onboardingStatus: null,
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/**
 * Polls `getServerSession()` up to `attempts` times with linear backoff.
 * Returns `null` if the session never resolves — caller decides whether to
 * fall back to a client-side path or short-circuit.
 */
async function waitForServerSession(
  attempts = 5,
  baseDelayMs = 150,
): Promise<Awaited<ReturnType<typeof getServerSession>>> {
  for (let i = 0; i < attempts; i += 1) {
    const session = await getServerSession()
    if (session?.user?.id) return session
    if (i < attempts - 1) {
      await sleep(baseDelayMs * (i + 1))
    }
  }
  return null
}

interface SessionContextResponse {
  userId?: string
  orgId?: string | null
  role?: string | null
  onboardingStatus?: string | null
  onboarding_complete?: boolean
}

async function fetchSessionContext(
  internalKey: string,
  userId: string,
  email: string | null,
): Promise<SessionContextResponse | null> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    'X-Internal-Api-Key': internalKey,
    'X-User-Id': userId,
  }
  if (email) headers['X-User-Email'] = email

  const ctrl = new AbortController()
  const timer = setTimeout(() => ctrl.abort(), 2_000)
  try {
    const response = await fetch(`${USER_SERVICE_URL}/api/v1/me/session-context`, {
      method: 'GET',
      headers,
      signal: ctrl.signal,
      cache: 'no-store',
    })
    if (!response.ok) return null
    return (await response.json()) as SessionContextResponse
  } catch {
    return null
  } finally {
    clearTimeout(timer)
  }
}

/**
 * Resolve everything the client needs to skip its own auth dance:
 *   1. Wait for the Better Auth session to settle (replication race).
 *   2. Call user-core `/me/session-context` with the verified user id.
 *   3. Map `onboardingStatus` to a boolean.
 *
 * Any failure here is non-fatal — the caller can pass the verdict to the
 * client as `needsOnboarding: null`, and the client will fall back to its
 * existing `waitForProfile()` + `waitForOnboardingCheck()` retry loop.
 */
export async function resolveOnboardingState(): Promise<ServerOnboardingState> {
  // Fast-fail: no session cookie at all means there's nothing to resolve.
  // This keeps cold/logged-out visits to /auth/callback responsive — the
  // retry loop below only runs when there's a plausible session in flight.
  if (!(await hasAnySessionCookie())) return EMPTY

  const session = await waitForServerSession()
  if (!session?.user?.id) return EMPTY

  const internalKey = getInternalApiKey()
  if (!internalKey) {
    // Env not wired — cannot call user-core internally. Defer to client.
    return {
      hasSession: true,
      userId: session.user.id,
      needsOnboarding: null,
      onboardingStatus: null,
    }
  }

  const ctx = await fetchSessionContext(
    internalKey,
    session.user.id,
    session.user.email ?? null,
  )

  if (!ctx) {
    return {
      hasSession: true,
      userId: session.user.id,
      needsOnboarding: null,
      onboardingStatus: null,
    }
  }

  // Mirror the contract documented in `onboarding-service.ts:needsOnboarding`.
  const completed =
    ctx.onboardingStatus === 'COMPLETED' || ctx.onboarding_complete === true
  const connectorsPending = ctx.onboardingStatus === 'CONNECTORS_PENDING'
  const hasOrg = !!ctx.orgId

  return {
    hasSession: true,
    userId: session.user.id,
    needsOnboarding: completed ? false : connectorsPending || !hasOrg ? true : null,
    onboardingStatus: ctx.onboardingStatus ?? null,
  }
}
