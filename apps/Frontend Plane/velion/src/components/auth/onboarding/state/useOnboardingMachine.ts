'use client'

/**
 * Phase 1 onboarding · state-machine hook.
 *
 * Single owner of the wizard state. The hook
 *
 *   1. Hydrates from `localStorage` on first mount (SSR-safe — defers
 *      the read until after the initial render so the server-rendered
 *      shell never disagrees with the client).
 *   2. Persists every transition back to `localStorage` synchronously.
 *   3. Exposes the canonical `goTo` / `next` / `back` API + a setter
 *      for every payload section.
 *
 * Errors during the localStorage round-trip are swallowed (Safari
 * private mode + Lockdown Mode both reject `setItem`) — the wizard
 * still works, the user just loses resume-on-refresh. We log a single
 * warn so this is greppable in browser console without spamming.
 */

import { useCallback, useEffect, useRef, useState } from 'react'

import {
  INITIAL_STATE,
  ONBOARDING_STEPS,
  STORAGE_KEY,
  type ConnectorPick,
  type OnboardingState,
  type OnboardingStep,
  type OrganizationPayload,
  type PlanRecommendation,
  type WebsitePayload,
} from './types'

function readStored(): OnboardingState | null {
  if (typeof window === 'undefined') return null
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as Partial<OnboardingState>
    // Defensive: drop the cache if step is missing or unknown so we
    // start fresh instead of trusting a half-written record.
    if (!parsed.step || !ONBOARDING_STEPS.includes(parsed.step)) {
      return null
    }
    return {
      ...INITIAL_STATE,
      ...parsed,
      connectors: Array.isArray(parsed.connectors) ? parsed.connectors : [],
    }
  } catch {
    return null
  }
}

function writeStored(state: OnboardingState): void {
  if (typeof window === 'undefined') return
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(state))
  } catch {
    if (typeof window !== 'undefined' && window.console) {
      window.console.warn(
        '[onboarding] localStorage write failed; resume-on-refresh disabled for this session',
      )
    }
  }
}

export interface OnboardingMachine {
  state: OnboardingState
  hydrated: boolean
  goTo: (step: OnboardingStep) => void
  next: () => void
  back: () => void
  setOrganization: (org: OrganizationPayload) => void
  setWebsite: (website: WebsitePayload) => void
  addConnector: (connector: ConnectorPick) => void
  removeConnector: (id: string) => void
  setRecommendation: (rec: PlanRecommendation) => void
  markIntroPlayed: () => void
  reset: () => void
}

export function useOnboardingMachine(): OnboardingMachine {
  // `hydrated=false` on first render so the right pane can show the
  // intro video even before localStorage is read — this matches the
  // user's "land on auth page → product video plays" expectation.
  const [state, setState] = useState<OnboardingState>(INITIAL_STATE)
  const [hydrated, setHydrated] = useState(false)
  const initialised = useRef(false)

  useEffect(() => {
    if (initialised.current) return
    initialised.current = true
    const stored = readStored()
    if (stored) {
      setState(stored)
    }
    setHydrated(true)
  }, [])

  const persist = useCallback((updater: (prev: OnboardingState) => OnboardingState) => {
    setState((prev) => {
      const next = updater(prev)
      writeStored(next)
      return next
    })
  }, [])

  const goTo = useCallback(
    (step: OnboardingStep) => {
      persist((prev) => ({ ...prev, step }))
    },
    [persist],
  )

  const next = useCallback(() => {
    persist((prev) => {
      const idx = ONBOARDING_STEPS.indexOf(prev.step)
      if (idx < 0 || idx >= ONBOARDING_STEPS.length - 1) return prev
      return { ...prev, step: ONBOARDING_STEPS[idx + 1] }
    })
  }, [persist])

  const back = useCallback(() => {
    persist((prev) => {
      const idx = ONBOARDING_STEPS.indexOf(prev.step)
      if (idx <= 0) return prev
      return { ...prev, step: ONBOARDING_STEPS[idx - 1] }
    })
  }, [persist])

  const setOrganization = useCallback(
    (org: OrganizationPayload) => {
      persist((prev) => ({ ...prev, organization: org }))
    },
    [persist],
  )

  const setWebsite = useCallback(
    (website: WebsitePayload) => {
      persist((prev) => ({ ...prev, website }))
    },
    [persist],
  )

  const addConnector = useCallback(
    (connector: ConnectorPick) => {
      persist((prev) => {
        if (prev.connectors.some((c) => c.id === connector.id)) {
          // Idempotent: refresh authedAt timestamp on re-add.
          return {
            ...prev,
            connectors: prev.connectors.map((c) =>
              c.id === connector.id ? { ...c, ...connector } : c,
            ),
          }
        }
        return { ...prev, connectors: [...prev.connectors, connector] }
      })
    },
    [persist],
  )

  const removeConnector = useCallback(
    (id: string) => {
      persist((prev) => ({
        ...prev,
        connectors: prev.connectors.filter((c) => c.id !== id),
      }))
    },
    [persist],
  )

  const setRecommendation = useCallback(
    (rec: PlanRecommendation) => {
      persist((prev) => ({ ...prev, recommendation: rec }))
    },
    [persist],
  )

  const markIntroPlayed = useCallback(() => {
    persist((prev) =>
      prev.introPlayed ? prev : { ...prev, introPlayed: true },
    )
  }, [persist])

  const reset = useCallback(() => {
    setState(INITIAL_STATE)
    if (typeof window !== 'undefined') {
      try {
        window.localStorage.removeItem(STORAGE_KEY)
      } catch {
        // Swallow — same Safari private-mode case as the writer.
      }
    }
  }, [])

  return {
    state,
    hydrated,
    goTo,
    next,
    back,
    setOrganization,
    setWebsite,
    addConnector,
    removeConnector,
    setRecommendation,
    markIntroPlayed,
    reset,
  }
}
