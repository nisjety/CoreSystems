// @vitest-environment jsdom

import { fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { OnboardingTopbar } from '@/features/onboarding/components/shared/OnboardingTopbar'
import { onboardingSteps } from '@/features/onboarding/lib/model'
import { I18nProvider, localeStorageKey } from '@/shared/i18n'

describe('OnboardingTopbar', () => {
  beforeEach(() => {
    installMemoryStorage()
    window.localStorage.removeItem(localeStorageKey)
  })

  it('shows the current progress and routes step selection through one shared chrome component', () => {
    const onBack = vi.fn()
    const onSelectStep = vi.fn()

    render(() => (
      <OnboardingTopbar
        steps={onboardingSteps}
        currentStep="organization"
        currentStepIndex={2}
        visibleStepNumber={3}
        onBack={onBack}
        onSelectStep={onSelectStep}
      />
    ))

    expect(screen.getByText('Steg 3 av 6')).toBeTruthy()

    fireEvent.click(screen.getByLabelText('Steg 1'))
    expect(onSelectStep).toHaveBeenCalledWith('website')

    fireEvent.click(screen.getByRole('button', { name: 'Tilbake' }))
    expect(onBack).toHaveBeenCalledTimes(1)
  })

  it('toggles the shared onboarding language state', () => {
    render(() => (
      <I18nProvider>
        <OnboardingTopbar
          steps={onboardingSteps}
          currentStep="organization"
          currentStepIndex={2}
          visibleStepNumber={3}
          onBack={vi.fn()}
          onSelectStep={vi.fn()}
        />
      </I18nProvider>
    ))

    fireEvent.click(screen.getByRole('button', { name: 'Bytt språk til English' }))
    flush()

    expect(screen.getByText('Step 3 of 6')).toBeTruthy()
    expect(screen.getByLabelText('Step 1')).toBeTruthy()
    expect(window.localStorage.getItem(localeStorageKey)).toBe('en')
  })
})

function installMemoryStorage() {
  const values = new Map<string, string>()
  Object.defineProperty(window, 'localStorage', {
    configurable: true,
    value: {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => {
        values.set(key, value)
      },
      removeItem: (key: string) => {
        values.delete(key)
      },
      clear: () => values.clear(),
      key: (index: number) => Array.from(values.keys())[index] ?? null,
      get length() {
        return values.size
      },
    } satisfies Storage,
  })
}
