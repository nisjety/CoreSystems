// @vitest-environment jsdom

import { cleanup, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it } from 'vitest'
import { I18nProvider, subscribeLocaleChange, useI18n, type LocaleChangeEvent } from '@/shared/i18n'

// WS16-S3: the locale events the preferences bootstrap listens to. Only the person's own toggle or
// Settings choice is a 'user' event, and only those are written through to the server. A server value
// the UI adopts is a 'server' event and is never written back.

type I18n = ReturnType<typeof useI18n>

let i18n: I18n | undefined
const unsubscribes: Array<() => void> = []

function renderProvider(): I18n {
  function Probe() {
    i18n = useI18n()
    return null
  }
  render(() => (
    <I18nProvider>
      <Probe />
    </I18nProvider>
  ))
  return i18n as I18n
}

function record(events: LocaleChangeEvent[]) {
  const unsubscribe = subscribeLocaleChange((event) => events.push(event))
  unsubscribes.push(unsubscribe)
  return events
}

afterEach(() => {
  for (const unsubscribe of unsubscribes.splice(0)) unsubscribe()
  cleanup()
  i18n = undefined
})

describe('locale change events', () => {
  it('does not report an adopted server value as a user choice', () => {
    const current = renderProvider()
    const events = record([])

    current.applyServerLocale('en')
    flush()

    expect(current.locale()).toBe('en')
    expect(events).toEqual([{ locale: 'en', source: 'server' }])
    expect(events.some((event) => event.source === 'user')).toBe(false)
  })

  it('reports setLocale and toggleLocale as one user event each', () => {
    const current = renderProvider()
    const events = record([])

    current.setLocale('en')
    flush()
    current.toggleLocale()
    flush()

    expect(events).toEqual([
      { locale: 'en', source: 'user' },
      { locale: 'no', source: 'user' },
    ])
  })

  it('isolates a listener that throws: the others still hear the change and the toggle still works', () => {
    const current = renderProvider()
    const bad = subscribeLocaleChange(() => {
      throw new Error('observer failed')
    })
    unsubscribes.push(bad)
    const events = record([])

    expect(() => current.setLocale('en')).not.toThrow()
    flush()

    expect(current.locale()).toBe('en')
    expect(events).toEqual([{ locale: 'en', source: 'user' }])
  })
})
