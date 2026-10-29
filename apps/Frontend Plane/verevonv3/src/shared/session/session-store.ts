import { createStore } from 'solid-js/store'
import { getCurrentSession, probeAuthSession, type AuthUser, type OnboardingStatus } from '../api/auth-client'
import { ApiError } from '../api/http'
import { clearSupportChatThreads } from '../chat/support-chat-thread'

export type SessionStatus = 'idle' | 'loading' | 'authenticated' | 'unauthenticated'

export type SessionOrg = {
  id: string
  name: string
  role: string
}

export type SessionState = {
  status: SessionStatus
  user: AuthUser | null
  activeOrg: SessionOrg | null
  permissions: string[]
  onboardingStatus: OnboardingStatus | null
}

const [session, setSession] = createStore<SessionState>({
  status: 'idle',
  user: null,
  activeOrg: null,
  permissions: [],
  onboardingStatus: null,
})

export function getSession(): SessionState {
  return session
}

/**
 * Did the server actually tell us this person is signed out?
 *
 * Only a 401 `unauthorized` is an answer. A timeout, a 5xx, or the gateway's
 * `session_verification_unavailable` all mean the question went unanswered, and
 * an unanswered question must never end a session — treating one as a sign-out
 * is what used to bounce a signed-in user to the login screen every few minutes.
 */
function isDefinitiveSignOut(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401 && error.code === 'unauthorized'
}

export async function loadSession(options?: { disableAuthCookieCache?: boolean }): Promise<void> {
  if (session.status === 'loading') return
  const previousStatus = session.status
  setSession('status', 'loading')

  try {
    // Probe the nullable auth endpoint first: it returns 200 + null when logged
    // out, so the sign-in screen never fires a 401 against the session-gated
    // snapshot route (which the browser would log as a console error). Only
    // fetch the rich snapshot once we know a session exists.
    //
    // The strict probe is deliberate: the forgiving `getAuthSession` reports an
    // unreachable backend as `null`, which is indistinguishable here from a real
    // sign-out and would clear a valid session.
    const auth = await probeAuthSession({
      disableCookieCache: options?.disableAuthCookieCache,
    })
    if (!auth?.user) {
      clearSession()
      return
    }

    const data = await getCurrentSession()
    if (session.user?.id && session.user.id !== data.user.id) clearSupportChatThreads()
    const onboardingStatus =
      session.user?.id === data.user.id &&
      session.activeOrg?.id === data.org?.id &&
      session.onboardingStatus === 'COMPLETED'
        ? 'COMPLETED'
        : data.onboardingStatus ?? null
    setSession({
      status: 'authenticated',
      user: data.user,
      activeOrg: data.org,
      permissions: data.permissions ?? [],
      onboardingStatus,
    })
  } catch (error) {
    if (isDefinitiveSignOut(error)) {
      clearSession()
      return
    }
    // Verification was unavailable, so the session's real state is unknown.
    // Keep an already-established session rather than ending it on a failure
    // that says nothing about the user. Only a first load — where there is no
    // session to preserve — falls back to the sign-in screen.
    if (previousStatus === 'authenticated') {
      setSession('status', 'authenticated')
      return
    }
    clearSession()
  }
}

export function setSessionUser(user: AuthUser): void {
  if (session.user?.id && session.user.id !== user.id) clearSupportChatThreads()
  setSession({ status: 'authenticated', user, activeOrg: null, permissions: [], onboardingStatus: null })
}

export function markSessionOnboardingComplete(activeOrg?: SessionOrg | null): void {
  if (!session.user) return
  setSession({
    status: 'authenticated',
    user: session.user,
    activeOrg: activeOrg === undefined ? session.activeOrg : activeOrg,
    permissions: [...session.permissions],
    onboardingStatus: 'COMPLETED',
  })
}

export function clearSession(): void {
  clearSupportChatThreads()
  setSession({ status: 'unauthenticated', user: null, activeOrg: null, permissions: [], onboardingStatus: null })
}

export function isOnboardingComplete(): boolean {
  return session.onboardingStatus === 'COMPLETED'
}
