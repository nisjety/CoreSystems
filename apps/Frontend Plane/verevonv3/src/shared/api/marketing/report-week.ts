const DAY = 86_400_000
const dayTimestamp = (value: string) => /^\d{4}-\d\d-\d\d$/.test(value) ? Date.parse(`${value}T00:00:00Z`) : NaN
export function isUTCWeekStart(value: unknown): value is string {
  if (typeof value !== 'string') return false
  const time = dayTimestamp(value)
  return Number.isFinite(time) && new Date(time).toISOString().slice(0, 10) === value && new Date(time).getUTCDay() === 1
}
export function currentUTCWeek(now = new Date()): string {
  if (!Number.isFinite(now.getTime())) throw new Error('Invalid report clock')
  const day = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate())
  return new Date(day - (now.getUTCDay() + 6) % 7 * DAY).toISOString().slice(0, 10)
}
export function shiftUTCWeek(value: string, offset: number): string {
  if (!isUTCWeekStart(value) || !Number.isInteger(offset) || Math.abs(offset) !== 1) throw new Error('Invalid week navigation')
  return new Date(dayTimestamp(value) + offset * 7 * DAY).toISOString().slice(0, 10)
}
