import { createStore } from 'solid-js/store'
import { getCurrentSession, type AuthUser, type OnboardingStatus } from '../api/auth-client'

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

export async function loadSession(): Promise<void> {
  if (session.status === 'loading') return
  setSession('status', 'loading')

  try {
    const data = await getCurrentSession()
    setSession({
      status: 'authenticated',
      user: data.user,
      activeOrg: data.org,
      permissions: data.permissions ?? [],
      onboardingStatus: data.onboardingStatus ?? null,
    })
  } catch {
    setSession({ status: 'unauthenticated', user: null, activeOrg: null, permissions: [], onboardingStatus: null })
  }
}

export function setSessionUser(user: AuthUser): void {
  setSession({ status: 'authenticated', user, activeOrg: null, permissions: [], onboardingStatus: null })
}

export function clearSession(): void {
  setSession({ status: 'unauthenticated', user: null, activeOrg: null, permissions: [], onboardingStatus: null })
}

export function isOnboardingComplete(): boolean {
  return session.onboardingStatus === 'COMPLETED'
}
