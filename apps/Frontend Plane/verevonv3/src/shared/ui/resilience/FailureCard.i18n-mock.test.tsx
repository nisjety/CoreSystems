// @vitest-environment jsdom

import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

// Many feature tests mock the i18n barrel with only useI18n. A root card under such a mock must still render, so
// FailureCard takes readInitialLocale from i18n/locales, not from the barrel (WS07-S1 review nit).
vi.mock('@/shared/i18n', () => ({
  useI18n: () => ({ tr: (noText: string) => noText }),
}))
vi.mock('@/shared/build/version-check', () => ({ checkClientVersion: vi.fn(() => Promise.resolve()) }))

import { localeStorageKey } from '@/shared/i18n/locales'
import { FailureCard } from './FailureCard'

afterEach(() => {
  cleanup()
  window.localStorage.removeItem(localeStorageKey)
})

describe('FailureCard under a useI18n-only barrel mock', () => {
  it('a root card still renders and reads the stored locale', () => {
    window.localStorage.setItem(localeStorageKey, 'en')
    render(() => <FailureCard error={new Error('boom')} region="root" onRetry={() => undefined} />)
    expect(screen.getByRole('heading', { name: 'Something went wrong here' })).toBeTruthy()
  })
})
