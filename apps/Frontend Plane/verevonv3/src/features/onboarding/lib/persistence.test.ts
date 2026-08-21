// @vitest-environment jsdom

import { createRoot } from 'solid-js'
import { createStore } from 'solid-js'
import type { StoreSetter } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createOnboardingPersistence } from '@/features/onboarding/lib/persistence'
import { createInitialOnboardingState } from '@/features/onboarding/lib/state'
import type { OnboardingState } from '@/features/onboarding/lib/model'

function createLocalStorageMock(): Storage {
  const entries = new Map<string, string>()

  return {
    clear: vi.fn(() => entries.clear()),
    getItem: vi.fn((key: string) => entries.get(key) ?? null),
    key: vi.fn((index: number) => Array.from(entries.keys())[index] ?? null),
    get length() {
      return entries.size
    },
    removeItem: vi.fn((key: string) => entries.delete(key)),
    setItem: vi.fn((key: string, value: string) => entries.set(key, value)),
  }
}

describe('createOnboardingPersistence', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      value: createLocalStorageMock(),
    })
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
    window.localStorage.clear()
  })

  it('debounces local persistence and trims noisy crawl snippets', async () => {
    vi.useFakeTimers()
    const setItem = vi.spyOn(window.localStorage, 'setItem')
    let disposeRoot: VoidFunction = () => undefined
    // Solid v2 forbids writing to a store from inside the owned scope that
    // created it (REACTIVE_WRITE_IN_OWNED_SCOPE) — capture the setter and
    // call it after `createRoot` has returned, once that scope is no longer
    // the active reactive context.
    let setState!: StoreSetter<OnboardingState>

    createRoot((dispose) => {
      disposeRoot = dispose
      const [state, setStateInner] = createStore(createInitialOnboardingState())
      setState = setStateInner
      createOnboardingPersistence({
        actor: { userId: 'test-user' },
        hydratedFromServer: () => false,
        localDebounceMs: 50,
        state,
        storageKey: 'verevonv3.test.onboarding',
      })
    })

    setState((s) => {
      s.website.url = 'https://example.com'
    })
    setState((s) => {
      s.website.snippets = Array.from({ length: 8 }, (_, index) => ({
        id: String(index),
        kind: 'text',
        title: `Snippet ${index}`,
        url: `https://example.com/${index}`,
      }))
    })

    await Promise.resolve()
    vi.advanceTimersByTime(49)
    expect(setItem).not.toHaveBeenCalled()

    vi.advanceTimersByTime(1)
    expect(setItem).toHaveBeenCalledTimes(1)
    const call = setItem.mock.calls[0]
    expect(call).toBeDefined()
    const payload = call?.[1] ?? ''
    const persisted = JSON.parse(String(payload)) as ReturnType<typeof createInitialOnboardingState>
    expect(persisted.website.url).toBe('https://example.com')
    expect(persisted.website.snippets).toHaveLength(6)

    disposeRoot()
  })
})
