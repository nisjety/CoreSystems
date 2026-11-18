// @vitest-environment jsdom

import { cleanup, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it } from 'vitest'
import { I18nProvider, useI18n } from '@/shared/i18n'
import type { SpaceFormatter } from '@/shared/i18n/format'
import { clearPreferences, receivePreferences } from '@/shared/preferences/preferences-store'
import { useFormat } from '@/shared/preferences/use-format'

// WS16-S3: useFormat() hands out getters, so a formatter read later still reflects the UI locale in use
// and the explicit preferences the store holds. Both are read inside a provider, as the app does.

const NOT_EXPLICIT = { language: false, timezone: false, dateFormat: false, timeFormat: false, compactMode: false }

function renderFormat() {
  let formatter: SpaceFormatter | undefined
  let i18n: ReturnType<typeof useI18n> | undefined
  function Probe() {
    formatter = useFormat()
    i18n = useI18n()
    return null
  }
  render(() => (
    <I18nProvider>
      <Probe />
    </I18nProvider>
  ))
  return {
    formatter: () => formatter as SpaceFormatter,
    i18n: () => i18n as ReturnType<typeof useI18n>,
  }
}

afterEach(() => {
  cleanup()
  clearPreferences()
})

describe('useFormat', () => {
  it('follows the UI locale that I18nProvider holds', () => {
    const view = renderFormat()
    expect(view.formatter().locale).toBe('no')
    view.i18n().setLocale('en')
    flush()
    expect(view.formatter().locale).toBe('en')
    view.i18n().toggleLocale()
    flush()
    expect(view.formatter().locale).toBe('no')
  })

  it('passes on only the explicit preferences the store holds, and follows the store as it changes', () => {
    const view = renderFormat()
    expect(view.formatter().prefs).toBeUndefined()

    receivePreferences({
      timezone: 'Europe/Oslo',
      dateFormat: 'DD/MM/YYYY',
      timeFormat: '24h',
      explicit: { ...NOT_EXPLICIT, dateFormat: true, timeFormat: true },
    })
    flush()
    expect(view.formatter().prefs).toEqual({ dateFormat: 'DD/MM/YYYY', timeFormat: '24h' })

    clearPreferences()
    flush()
    expect(view.formatter().prefs).toBeUndefined()
  })
})
