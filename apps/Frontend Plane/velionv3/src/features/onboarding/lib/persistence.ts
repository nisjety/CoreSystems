import { createEffect, onCleanup, type Accessor } from 'solid-js'
import type { ActionActor } from '@/features/onboarding/lib/api'
import { saveOnboardingState } from '@/features/onboarding/lib/api'
import type { OnboardingState } from '@/features/onboarding/lib/model'
import { createPersistedOnboardingState } from '@/features/onboarding/lib/state'

type OnboardingPersistenceOptions = {
  actor: ActionActor
  hydratedFromServer: Accessor<boolean>
  localDebounceMs?: number
  remoteDebounceMs?: number
  state: OnboardingState
  storageKey: string
}

export function createOnboardingPersistence(options: OnboardingPersistenceOptions) {
  let localTimer: number | undefined
  let remoteTimer: number | undefined

  createEffect(() => {
    const snapshot = createPersistedOnboardingState(options.state)

    if (typeof window !== 'undefined') {
      window.clearTimeout(localTimer)
      localTimer = window.setTimeout(() => {
        window.localStorage.setItem(options.storageKey, JSON.stringify(snapshot))
      }, options.localDebounceMs ?? 120)
    }

    if (!options.hydratedFromServer()) return

    window.clearTimeout(remoteTimer)
    remoteTimer = window.setTimeout(() => {
      void saveOnboardingState({
        actor: options.actor,
        step: snapshot.step,
        state: snapshot,
      }).catch(() => undefined)
    }, options.remoteDebounceMs ?? 500)
  })

  onCleanup(() => {
    if (typeof window === 'undefined') return
    window.clearTimeout(localTimer)
    window.clearTimeout(remoteTimer)
  })
}
