import { createStore, untrack } from 'solid-js'
import { getAuthSession, getCurrentSession, type AuthUser, type OnboardingStatus } from '../api/auth-client'
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

export async function loadSession(options?: { disableAuthCookieCache?: boolean }): Promise<void> {
  // Deliberate one-shot peek, not a reactive dependency: callers invoke this
  // from effect callbacks (e.g. AppShell), whose strict-read scope would
  // otherwise warn STRICT_READ_UNTRACKED on this synchronous store read.
  if (untrack(() => session.status) === 'loading') return
  setSession((s) => { s.status = 'loading' })

  try {
    // Probe the nullable auth endpoint first: it returns 200 + null when logged
    // out, so the sign-in screen never fires a 401 against the session-gated
    // snapshot route (which the browser would log as a console error). Only
    // fetch the rich snapshot once we know a session exists.
    const auth = await getAuthSession({
      disableCookieCache: options?.disableAuthCookieCache,
    })
    if (!auth?.user) {
      clearSupportChatThreads()
      setSession((s) => {
        s.status = 'unauthenticated'
        s.user = null
        s.activeOrg = null
        s.permissions = []
        s.onboardingStatus = null
      })
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
    setSession((s) => {
      s.status = 'authenticated'
      s.user = data.user
      s.activeOrg = data.org
      s.permissions = data.permissions ?? []
      s.onboardingStatus = onboardingStatus
    })
  } catch {
    clearSupportChatThreads()
    setSession((s) => {
      s.status = 'unauthenticated'
      s.user = null
      s.activeOrg = null
      s.permissions = []
      s.onboardingStatus = null
    })
  }
}

export function setSessionUser(user: AuthUser): void {
  if (session.user?.id && session.user.id !== user.id) clearSupportChatThreads()
  setSession((s) => {
    s.status = 'authenticated'
    s.user = user
    s.activeOrg = null
    s.permissions = []
    s.onboardingStatus = null
  })
}

export function markSessionOnboardingComplete(activeOrg?: SessionOrg | null): void {
  if (!session.user) return
  setSession((s) => {
    s.status = 'authenticated'
    s.activeOrg = activeOrg === undefined ? s.activeOrg : activeOrg
    s.onboardingStatus = 'COMPLETED'
  })
}

export function clearSession(): void {
  clearSupportChatThreads()
  setSession((s) => {
    s.status = 'unauthenticated'
    s.user = null
    s.activeOrg = null
    s.permissions = []
    s.onboardingStatus = null
  })
}

export function isOnboardingComplete(): boolean {
  return session.onboardingStatus === 'COMPLETED'
}
