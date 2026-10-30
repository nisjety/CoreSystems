const DAY_MS = 86_400_000
type PulsePeriod = { startDate: string; endDate: string }

function dateTimestamp(date: string): number {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) return NaN
  const timestamp = Date.parse(`${date}T00:00:00Z`)
  return Number.isFinite(timestamp) && new Date(timestamp).toISOString().slice(0, 10) === date ? timestamp : NaN
}

/** A slot is a UTC calendar day, including days Google has not returned. */
export function pulsePeriodDates(period: PulsePeriod): string[] {
  const start = dateTimestamp(period.startDate)
  const end = dateTimestamp(period.endDate)
  const count = (end - start) / DAY_MS + 1
  if (!Number.isInteger(count) || count < 1 || count > 366) return []
  return Array.from({ length: count }, (_, index) => new Date(start + index * DAY_MS).toISOString().slice(0, 10))
}

export function buildPulseTimeline(period: PulsePeriod, observedDates: readonly string[]) {
  const dates = pulsePeriodDates(period)
  if (dates.length === 0) return null
  const tickCount = Math.min(5, dates.length)
  const ticks = Array.from({ length: tickCount }, (_, index) => {
    const slot = tickCount === 1 ? 0 : Math.round(index * (dates.length - 1) / (tickCount - 1))
    return { date: dates[slot]!, position: (slot + 0.5) * 100 / dates.length }
  })
  const observed = new Set(observedDates)
  const first = dates.findIndex((date) => observed.has(date))
  const last = dates.findLastIndex((date) => observed.has(date))
  return {
    ticks,
    lastAvailableDate: last < 0 ? null : dates[last]!,
    availableRange: first < 0 ? null : { left: first / dates.length * 100, width: (last - first + 1) / dates.length * 100 },
  }
}

export function formatPulseDate(date: string, locale: string, includeYear = false): string {
  const timestamp = dateTimestamp(date)
  if (!Number.isFinite(timestamp)) return '—'
  return new Intl.DateTimeFormat(locale, {
    day: 'numeric', month: 'short', ...(includeYear ? { year: 'numeric' } as const : {}), timeZone: 'UTC',
  }).format(timestamp)
}
