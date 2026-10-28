/**
 * Cron helpers for the schedule composer: a few presets and a plain-language
 * reading of the common five-field shapes. Anything the reader does not
 * recognise returns `null`, and the composer then shows the raw expression.
 */
export type CronPreset = { id: 'hourly' | 'daily' | 'weekdays' | 'weekly' | 'monthly'; cron: string }

export const cronPresets: CronPreset[] = [
  { id: 'hourly', cron: '0 * * * *' },
  { id: 'daily', cron: '0 7 * * *' },
  { id: 'weekdays', cron: '0 7 * * 1-5' },
  { id: 'weekly', cron: '0 7 * * 1' },
  { id: 'monthly', cron: '0 7 1 * *' },
]

type Tr = (no: string, en: string) => string

const WEEKDAYS: Array<[string, string]> = [
  ['søndag', 'Sunday'],
  ['mandag', 'Monday'],
  ['tirsdag', 'Tuesday'],
  ['onsdag', 'Wednesday'],
  ['torsdag', 'Thursday'],
  ['fredag', 'Friday'],
  ['lørdag', 'Saturday'],
]

function isNumber(field: string, min: number, max: number): boolean {
  if (!/^\d+$/.test(field)) return false
  const value = Number(field)
  return value >= min && value <= max
}

function clock(hour: string, minute: string): string {
  // Quarry reads schedule crons in UTC unless a timezone is set, and nothing here sets one.
  return `${hour.padStart(2, '0')}:${minute.padStart(2, '0')} UTC`
}

export function cronPresetLabel(id: CronPreset['id'], tr: Tr): string {
  switch (id) {
    case 'hourly':
      return tr('Hver time', 'Hourly')
    case 'daily':
      return tr('Daglig', 'Daily')
    case 'weekdays':
      return tr('Hverdager', 'Weekdays')
    case 'weekly':
      return tr('Ukentlig', 'Weekly')
    case 'monthly':
      return tr('Månedlig', 'Monthly')
  }
}

export function describeCron(expression: string, tr: Tr): string | null {
  const fields = expression.trim().split(/\s+/)
  if (fields.length !== 5) return null
  const [minute = '', hour = '', dayOfMonth = '', month = '', dayOfWeek = ''] = fields

  const everyMinutes = /^\*\/(\d+)$/.exec(minute)
  if (everyMinutes && hour === '*' && dayOfMonth === '*' && month === '*' && dayOfWeek === '*') {
    const step = everyMinutes[1] ?? ''
    return tr(`Hvert ${step}. minutt`, `Every ${step} minutes`)
  }
  if (!isNumber(minute, 0, 59) || month !== '*') return null

  if (hour === '*' && dayOfMonth === '*' && dayOfWeek === '*') {
    return minute === '0'
      ? tr('Hver time, på hel time', 'Every hour, on the hour')
      : tr(`Hver time, ${minute} min over`, `Every hour at ${minute} past`)
  }
  if (!isNumber(hour, 0, 23)) return null
  const at = clock(hour, minute)

  if (dayOfMonth === '*' && dayOfWeek === '*') return tr(`Hver dag kl. ${at}`, `Every day at ${at}`)
  if (dayOfMonth === '*' && dayOfWeek === '1-5') return tr(`Hverdager kl. ${at}`, `Weekdays at ${at}`)
  if (dayOfMonth === '*' && isNumber(dayOfWeek, 0, 7)) {
    const [no, en] = WEEKDAYS[Number(dayOfWeek) % 7] ?? ['', '']
    return tr(`Hver ${no} kl. ${at}`, `Every ${en} at ${at}`)
  }
  if (dayOfWeek === '*' && isNumber(dayOfMonth, 1, 31)) {
    return tr(`Den ${dayOfMonth}. hver måned kl. ${at}`, `Day ${dayOfMonth} of every month at ${at}`)
  }
  return null
}
