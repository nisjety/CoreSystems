// WS16-S2: the client normalises the contract-v2 preference fields. Anything
// the server could not have sent (an unknown enum value, a non-boolean) is
// dropped rather than passed on, so callers can trust the types.

import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('./http', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./http')>()
  return { ...actual, requestJson: vi.fn() }
})

import { requestJson } from './http'
import { getPreferences, updatePreferences } from './settings-client'

const mockedRequest = vi.mocked(requestJson)

afterEach(() => {
  vi.clearAllMocks()
})

describe('getPreferences (contract v2)', () => {
  it('parses dateFormat, timeFormat, compactMode and explicit', async () => {
    mockedRequest.mockResolvedValue({
      preferences: {
        theme: 'dark',
        language: 'nb-NO',
        timezone: 'Europe/Oslo',
        dateFormat: 'DD/MM/YYYY',
        timeFormat: '24h',
        compactMode: true,
        explicit: { language: true, timezone: true, dateFormat: true, timeFormat: false, compactMode: true },
        notifications: { email: true, push: false },
      },
    })
    const prefs = await getPreferences()
    expect(prefs.dateFormat).toBe('DD/MM/YYYY')
    expect(prefs.timeFormat).toBe('24h')
    expect(prefs.compactMode).toBe(true)
    expect(prefs.timezone).toBe('Europe/Oslo')
    expect(prefs.explicit).toEqual({ language: true, timezone: true, dateFormat: true, timeFormat: false, compactMode: true })
    expect(prefs.notifications).toEqual({ email: true, push: false })
  })

  it('drops invalid values instead of passing them through', async () => {
    mockedRequest.mockResolvedValue({
      preferences: { dateFormat: 'auto', timeFormat: '13h', compactMode: 'yes', timezone: '', explicit: { dateFormat: 'true', language: 1 } },
    })
    const prefs = await getPreferences()
    expect(prefs.dateFormat).toBeUndefined()
    expect(prefs.timeFormat).toBeUndefined()
    expect(prefs.compactMode).toBeUndefined()
    expect(prefs.timezone).toBeUndefined()
    // Only a strict boolean true counts as explicit.
    expect(prefs.explicit).toEqual({ language: false, timezone: false, dateFormat: false, timeFormat: false, compactMode: false })
  })

  it('leaves explicit undefined when the server did not send it and omits timezone for a new user', async () => {
    mockedRequest.mockResolvedValue({ preferences: { theme: 'light', dateFormat: 'MM/DD/YYYY', timeFormat: '12h', compactMode: false } })
    const prefs = await getPreferences()
    expect(prefs.explicit).toBeUndefined()
    expect(prefs.timezone).toBeUndefined()
    expect(prefs.compactMode).toBe(false)
  })
})

describe('updatePreferences (contract v2)', () => {
  it('PATCHes the v2 fields including auto and re-reads the preferences', async () => {
    mockedRequest.mockResolvedValueOnce({ message: 'ok' })
    mockedRequest.mockResolvedValueOnce({ preferences: { dateFormat: 'YYYY-MM-DD', timeFormat: '12h' } })
    const result = await updatePreferences({ dateFormat: 'auto', timeFormat: '24h', compactMode: true })
    const [path, init] = mockedRequest.mock.calls[0] as [string, { method: string; body: string }]
    expect(path).toBe('/api/v1/preferences')
    expect(init.method).toBe('PATCH')
    expect(JSON.parse(init.body)).toEqual({ dateFormat: 'auto', timeFormat: '24h', compactMode: true })
    expect(result.dateFormat).toBe('YYYY-MM-DD')
  })
})
