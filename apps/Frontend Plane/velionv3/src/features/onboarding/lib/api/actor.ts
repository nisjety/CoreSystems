import type { ActionActor } from '@/features/onboarding/lib/api/contracts'

const allowDevActorHeaders = import.meta.env.DEV || import.meta.env.VITE_ALLOW_DEV_ACTOR_HEADERS === 'true'

export function getBrowserActor(): ActionActor {
  const storageKey = 'velionv3.onboarding.actor'
  const fallback = {
    userId: 'velion-v3-local-user',
    userEmail: 'local@velion.dev',
    userName: 'Velion Local',
  }

  if (typeof window === 'undefined') return fallback

  try {
    const raw = window.localStorage.getItem(storageKey)
    if (raw) return { ...fallback, ...(JSON.parse(raw) as Partial<ActionActor>) }
    window.localStorage.setItem(storageKey, JSON.stringify(fallback))
  } catch {
    return fallback
  }

  return fallback
}

export function withDevActor<TBody extends Record<string, unknown>>(body: TBody, actor?: ActionActor) {
  return allowDevActorHeaders && actor ? { actor, ...body } : body
}
