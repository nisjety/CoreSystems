import { describe, expect, it } from 'vitest'
import { localeStorageKey } from '@/shared/i18n/locales'
import {
  buildChatWireBody,
  clientTimeZone,
  clientUtcOffsetMinutes,
  type ChatInvokeRequest,
} from './chat-client'

// EP-8-S1: every turn tells the gateway the browser's own zone, offset and app language, so the model can
// answer with today's local date and in the language the person uses. These tests pin the wire fields: the
// zone and offset are on every turn, the offset is east positive, and the language is sent only when the
// turn names one.

const ask: ChatInvokeRequest = { content: 'Hva er datoen i morgen?', model: 'verevon-balance' }

describe('buildChatWireBody local clock and language', () => {
  it('sends the browser zone and the UTC offset on every turn', () => {
    const body = buildChatWireBody(ask)
    expect(body.client_timezone).toBe(Intl.DateTimeFormat().resolvedOptions().timeZone)
    expect(body.client_utc_offset_minutes).toBe(clientUtcOffsetMinutes(new Date()))
    expect(Number.isInteger(body.client_utc_offset_minutes)).toBe(true)
  })

  it('sends the app language the turn names, else the stored one, and none when neither is set', () => {
    window.localStorage.removeItem(localeStorageKey)
    expect(buildChatWireBody({ ...ask, locale: 'no' })).toMatchObject({ ui_locale: 'no' })
    expect(buildChatWireBody({ ...ask, locale: 'en' })).toMatchObject({ ui_locale: 'en' })
    expect(buildChatWireBody(ask)).not.toHaveProperty('ui_locale')
    try {
      window.localStorage.setItem(localeStorageKey, 'en')
      expect(buildChatWireBody(ask)).toMatchObject({ ui_locale: 'en' })
      expect(buildChatWireBody({ ...ask, locale: 'no' })).toMatchObject({ ui_locale: 'no' })
      window.localStorage.setItem(localeStorageKey, 'de')
      expect(buildChatWireBody(ask)).not.toHaveProperty('ui_locale')
    } finally {
      window.localStorage.removeItem(localeStorageKey)
    }
  })
})

describe('clientUtcOffsetMinutes', () => {
  it('is east positive, the sign of the offset from UTC', () => {
    // getTimezoneOffset reports minutes behind UTC, so UTC+01:00 reads -60 and becomes 60.
    expect(clientUtcOffsetMinutes({ getTimezoneOffset: () => -60 })).toBe(60)
    expect(clientUtcOffsetMinutes({ getTimezoneOffset: () => 300 })).toBe(-300)
  })

  it('reports UTC as zero, never negative zero', () => {
    expect(Object.is(clientUtcOffsetMinutes({ getTimezoneOffset: () => 0 }), 0)).toBe(true)
  })
})

describe('clientTimeZone', () => {
  it('is the zone name the runtime reports', () => {
    expect(clientTimeZone()).toBe(Intl.DateTimeFormat().resolvedOptions().timeZone)
  })
})
