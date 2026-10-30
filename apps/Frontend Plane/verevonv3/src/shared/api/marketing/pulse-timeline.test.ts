import { describe, expect, it } from 'vitest'
import { buildPulseTimeline, formatPulseDate, pulsePeriodDates } from './pulse-timeline.ts'

describe('Visibility calendar timeline', () => {
  const period = { startDate: '2026-10-01', endDate: '2026-10-20' }

  it('labels the actual requested dates, not a hardcoded full year', () => {
    const timeline = buildPulseTimeline(period, ['2026-10-18', '2026-10-01'])
    expect(timeline?.ticks).toEqual([
      { date: '2026-10-01', position: 2.5 },
      { date: '2026-10-06', position: 27.5 },
      { date: '2026-10-11', position: 52.5 },
      { date: '2026-10-15', position: 72.5 },
      { date: '2026-10-20', position: 97.5 },
    ])
    expect(timeline?.lastAvailableDate).toBe('2026-10-18')
    expect(timeline?.availableRange).toEqual({ left: 0, width: 90 })
  })

  it('retains missing days as calendar slots and handles leap days in UTC', () => {
    expect(pulsePeriodDates({ startDate: '2024-02-28', endDate: '2024-03-01' })).toEqual([
      '2024-02-28', '2024-02-29', '2024-03-01',
    ])
    expect(pulsePeriodDates({ startDate: '2026-03-28', endDate: '2026-03-30' })).toHaveLength(3)
    expect(pulsePeriodDates(period)).toHaveLength(20)
  })

  it('does not claim data exists through the requested end or the refresh date', () => {
    expect(buildPulseTimeline(period, [])).toMatchObject({ lastAvailableDate: null, availableRange: null })
    expect(buildPulseTimeline(period, ['2026-09-30', '2026-10-21', '2026-02-30', 'invalid']))
      .toMatchObject({ lastAvailableDate: null, availableRange: null })
  })

  it('positions partial availability on the same daily grid as the bars', () => {
    expect(buildPulseTimeline(period, ['2026-10-05', '2026-10-10'])?.availableRange)
      .toEqual({ left: 20, width: 30 })
  })

  it('supports one-day and short periods without repeated ticks', () => {
    expect(buildPulseTimeline({ startDate: '2026-10-01', endDate: '2026-10-01' }, ['2026-10-01']))
      .toEqual({ ticks: [{ date: '2026-10-01', position: 50 }], lastAvailableDate: '2026-10-01', availableRange: { left: 0, width: 100 } })
    expect(buildPulseTimeline({ startDate: '2026-10-01', endDate: '2026-10-03' }, [])?.ticks.map((tick) => tick.date))
      .toEqual(['2026-10-01', '2026-10-02', '2026-10-03'])
  })

  it.each([
    { startDate: 'invalid', endDate: '2026-10-20' },
    { startDate: '2026-02-30', endDate: '2026-03-02' },
    { startDate: '2026-10-21', endDate: '2026-10-20' },
    { startDate: '2025-01-01', endDate: '2026-12-31' },
  ])('fails closed for invalid or unbounded calendar periods: %j', (invalid) => {
    expect(pulsePeriodDates(invalid)).toEqual([])
    expect(buildPulseTimeline(invalid, [])).toBeNull()
  })

  it('formats real dates in the selected language without local-time shifts', () => {
    expect(formatPulseDate('2026-10-01', 'en-US', true)).toBe('Oct 1, 2026')
    expect(formatPulseDate('2026-10-01', 'nb-NO', true)).toBe('1. okt. 2026')
    expect(formatPulseDate('2026-10-01', 'en-US')).toBe('Oct 1')
    expect(formatPulseDate('2026-02-30', 'en-US')).toBe('—')
  })
})
