import { describe, expect, it } from 'vitest'
import { currentUTCWeek, isUTCWeekStart, shiftUTCWeek } from './report-week'

describe('UTC report week navigation', () => {
  it('uses Monday UTC, not local time or daylight-saving transitions', () => {
    expect(currentUTCWeek(new Date('2026-10-25T23:59:00Z'))).toBe('2026-10-19')
    expect(currentUTCWeek(new Date('2026-10-26T00:00:00Z'))).toBe('2026-10-26')
    expect(shiftUTCWeek('2026-10-19', 1)).toBe('2026-10-26')
    expect(shiftUTCWeek('2026-03-30', -1)).toBe('2026-03-23')
  })
  it('rejects non-Mondays, impossible dates and arbitrary navigation offsets', () => {
    for (const input of ['2026-10-20', '2026-02-30', '2026-1-05', 'invalid']) expect(isUTCWeekStart(input)).toBe(false)
    expect(isUTCWeekStart('2026-10-19')).toBe(true)
    expect(() => shiftUTCWeek('2026-10-20', -1)).toThrow()
    expect(() => shiftUTCWeek('2026-10-19', 0.5)).toThrow()
  })
})
