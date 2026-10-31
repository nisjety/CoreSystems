/** Explicit period context for a chat turn. It is added to the visible user text;
 * it does not claim to filter upstream data or change source permissions. */
export type ChatWorkPeriod =
  | { kind: 'none' }
  | { kind: 'this_month' | 'last_month' | 'last_quarter' | 'year_to_date' | 'rolling_12' }
  | { kind: 'custom'; from: string; to: string }

function localDate(year: number, month: number, day: number): string {
  return `${year}-${String(month + 1).padStart(2, '0')}-${String(day).padStart(2, '0')}`
}

function lastDay(year: number, month: number): string {
  const date = new Date(year, month + 1, 0)
  return localDate(date.getFullYear(), date.getMonth(), date.getDate())
}

export function resolveChatWorkPeriod(period: ChatWorkPeriod, now = new Date()): { from: string; to: string } | null {
  if (period.kind === 'none') return null
  if (period.kind === 'custom') {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(period.from) || !/^\d{4}-\d{2}-\d{2}$/.test(period.to) || period.from > period.to) return null
    return { from: period.from, to: period.to }
  }
  const year = now.getFullYear()
  const month = now.getMonth()
  const today = localDate(year, month, now.getDate())
  if (period.kind === 'this_month') return { from: localDate(year, month, 1), to: today }
  if (period.kind === 'last_month') {
    const previous = new Date(year, month - 1, 1)
    return { from: localDate(previous.getFullYear(), previous.getMonth(), 1), to: lastDay(previous.getFullYear(), previous.getMonth()) }
  }
  if (period.kind === 'last_quarter') {
    const quarterStart = Math.floor(month / 3) * 3 - 3
    const previous = new Date(year, quarterStart, 1)
    return { from: localDate(previous.getFullYear(), previous.getMonth(), 1), to: lastDay(previous.getFullYear(), previous.getMonth() + 2) }
  }
  if (period.kind === 'year_to_date') return { from: localDate(year, 0, 1), to: today }
  const start = new Date(year, month - 11, 1)
  return { from: localDate(start.getFullYear(), start.getMonth(), 1), to: today }
}

export function withChatWorkPeriod(text: string, period: ChatWorkPeriod, locale: 'no' | 'en', now = new Date()): string {
  const range = resolveChatWorkPeriod(period, now)
  if (!range) return text
  const note = locale === 'no'
    ? `Arbeidsperiode: ${range.from} til ${range.to}. Bruk denne perioden når datoer er relevante, og si fra hvis kildene ikke kan avgrenses til den.`
    : `Working period: ${range.from} to ${range.to}. Use this period when dates matter, and say if the sources cannot be limited to it.`
  return `${text.trim()}\n\n${note}`
}
