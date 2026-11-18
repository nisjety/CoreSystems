import { describe, expect, it } from 'vitest'
import { formatDay, formatMoment, isValidTimeZone } from '@/shared/i18n/format'

// WS16-S1: the pure date and time formatters. Every case pins an instant and an explicit zone, so the
// expected text does not depend on the machine the suite runs on.

const AT = '2026-09-06T12:32:00Z' // 14:32 in Europe/Oslo (CEST), 08:32 in America/New_York (EDT)
const OSLO = { timeZone: 'Europe/Oslo' }

describe('isValidTimeZone', () => {
  it('accepts an IANA zone and rejects an unknown, empty or blank one', () => {
    expect(isValidTimeZone('Europe/Oslo')).toBe(true)
    expect(isValidTimeZone('UTC')).toBe(true)
    expect(isValidTimeZone('Not/AZone')).toBe(false)
    expect(isValidTimeZone('')).toBe(false)
    expect(isValidTimeZone('   ')).toBe(false)
  })
})

describe('formatDay', () => {
  it('writes the day in the viewer locale when no date format is chosen', () => {
    expect(formatDay(AT, 'en', OSLO)).toBe('Sep 6')
    expect(formatDay(AT, 'no', OSLO)).toBe('6. sep.')
  })

  it.each([
    ['DD/MM/YYYY', '06/09/2026'],
    ['MM/DD/YYYY', '09/06/2026'],
    ['YYYY-MM-DD', '2026-09-06'],
  ] as const)('uses the explicit %s pattern whatever the locale', (pattern, expected) => {
    expect(formatDay(AT, 'no', { ...OSLO, dateFormat: pattern })).toBe(expected)
    expect(formatDay(AT, 'en', { ...OSLO, dateFormat: pattern })).toBe(expected)
  })

  it('takes the day in the chosen zone', () => {
    // 23:30 UTC on the 5th is already the 6th in Oslo.
    expect(formatDay('2026-09-05T23:30:00Z', 'en', { ...OSLO, dateFormat: 'YYYY-MM-DD' })).toBe('2026-09-06')
    expect(formatDay('2026-09-05T23:30:00Z', 'en', { timeZone: 'UTC', dateFormat: 'YYYY-MM-DD' })).toBe('2026-09-05')
  })

  it('returns the raw value when the input does not parse', () => {
    expect(formatDay('not a date', 'no', OSLO)).toBe('not a date')
    expect(formatDay('not a date', 'en', { dateFormat: 'DD/MM/YYYY' })).toBe('not a date')
  })
})

describe('formatMoment', () => {
  it('writes the date and time in the viewer locale when no preference is chosen', () => {
    expect(formatMoment(AT, 'no', OSLO)).toBe('06.09.2026, 14:32')
    // en-US puts a narrow no-break space before PM on newer ICU, so match the space loosely.
    expect(formatMoment(AT, 'en', OSLO)).toMatch(/^9\/6\/26, 2:32\s?PM$/)
  })

  it.each([
    ['24h', /^06\/09\/2026, 14:32$/],
    ['12h', /^06\/09\/2026, 0?2:32\s?PM$/],
  ] as const)('follows the chosen %s clock', (timeFormat, expected) => {
    expect(formatMoment(AT, 'en', { ...OSLO, dateFormat: 'DD/MM/YYYY', timeFormat })).toMatch(expected)
  })

  it('pairs the explicit date pattern with the time', () => {
    expect(formatMoment(AT, 'no', { ...OSLO, dateFormat: 'YYYY-MM-DD', timeFormat: '24h' })).toBe('2026-09-06, 14:32')
  })

  it('returns the raw value when the input does not parse, and nothing for an absent one', () => {
    expect(formatMoment('not a date', 'no', OSLO)).toBe('not a date')
    expect(formatMoment(undefined, 'en')).toBe('')
    expect(formatMoment('', 'no')).toBe('')
  })

  it('falls back to the browser zone when the chosen zone is not valid', () => {
    expect(formatMoment(AT, 'en', { timeZone: 'Not/AZone', timeFormat: '24h' }))
      .toBe(formatMoment(AT, 'en', { timeFormat: '24h' }))
    expect(formatDay(AT, 'en', { timeZone: 'Not/AZone' })).toBe(formatDay(AT, 'en'))
  })
})
