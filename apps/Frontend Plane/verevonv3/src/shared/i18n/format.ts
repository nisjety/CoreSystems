import { localeIntl, type Locale } from './locales'

/**
 * Pure date and time formatters for the viewer's locale and, when the server
 * says the viewer chose them, their date format, time format and time zone.
 *
 * A preference is passed in ONLY when the server marks it explicit (a stored
 * `MM/DD/YYYY` default must never override a Norwegian reader's locale), so
 * this module treats every field of `FormatPrefs` that is present as chosen.
 * Unparseable input comes back as the raw value rather than a wrong date.
 */

export type FormatPrefs = {
  /** IANA zone, e.g. `Europe/Oslo`. An invalid or absent zone means the browser zone. */
  timeZone?: string
  dateFormat?: 'MM/DD/YYYY' | 'DD/MM/YYYY' | 'YYYY-MM-DD'
  timeFormat?: '12h' | '24h'
}

/** What a presentation helper needs to format for one viewer. */
export type SpaceFormatter = { locale: Locale; prefs?: FormatPrefs }

export function isValidTimeZone(zone: string): boolean {
  if (!zone || !zone.trim()) return false
  try {
    new Intl.DateTimeFormat(undefined, { timeZone: zone })
    return true
  } catch {
    return false
  }
}

function zoneOption(prefs?: FormatPrefs): { timeZone?: string } {
  const zone = prefs?.timeZone?.trim()
  return zone && isValidTimeZone(zone) ? { timeZone: zone } : {}
}

function parse(value: string): Date | undefined {
  const at = new Date(value)
  return Number.isFinite(at.getTime()) ? at : undefined
}

/** The numeric date in the chosen pattern, built from parts so the order is exact. */
function numericDate(at: Date, pattern: NonNullable<FormatPrefs['dateFormat']>, prefs?: FormatPrefs): string {
  const parts = new Intl.DateTimeFormat('en-US', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    ...zoneOption(prefs),
  }).formatToParts(at)
  const get = (type: string) => parts.find((part) => part.type === type)?.value ?? ''
  const y = get('year')
  const m = get('month')
  const d = get('day')
  if (pattern === 'YYYY-MM-DD') return `${y}-${m}-${d}`
  if (pattern === 'DD/MM/YYYY') return `${d}/${m}/${y}`
  return `${m}/${d}/${y}`
}

/** A day, for the room timeline: "6. sep." / "Sep 6", or the chosen numeric pattern. */
export function formatDay(value: string, locale: Locale, prefs?: FormatPrefs): string {
  try {
    const at = parse(value)
    if (!at) return value
    if (prefs?.dateFormat) return numericDate(at, prefs.dateFormat, prefs)
    return at.toLocaleDateString(localeIntl(locale), { day: 'numeric', month: 'short', ...zoneOption(prefs) })
  } catch {
    return value
  }
}

/** A moment, for the activity feed: "06.09.2026, 14:32" / "9/6/26, 2:32 PM". Empty for an absent value. */
export function formatMoment(value: string | undefined, locale: Locale, prefs?: FormatPrefs): string {
  if (!value) return ''
  try {
    const at = parse(value)
    if (!at) return value
    const hour12 = prefs?.timeFormat ? prefs.timeFormat === '12h' : undefined
    const hourOption = hour12 === undefined ? {} : { hour12 }
    if (prefs?.dateFormat) {
      const time = at.toLocaleTimeString(localeIntl(locale), {
        hour: '2-digit',
        minute: '2-digit',
        ...hourOption,
        ...zoneOption(prefs),
      })
      return `${numericDate(at, prefs.dateFormat, prefs)}, ${time}`
    }
    return at.toLocaleString(localeIntl(locale), {
      dateStyle: 'short',
      timeStyle: 'short',
      ...hourOption,
      ...zoneOption(prefs),
    })
  } catch {
    return value
  }
}

/** A full calendar date for a day label, such as a history window's start: "12. nov. 2026", "Nov 12, 2026", or the chosen numeric pattern. */
export function formatFullDate(value: string, locale: Locale, prefs?: FormatPrefs): string {
  try {
    const at = parse(value)
    if (!at) return ''
    if (prefs?.dateFormat) return numericDate(at, prefs.dateFormat, prefs)
    return at.toLocaleDateString(localeIntl(locale), { day: 'numeric', month: 'short', year: 'numeric', ...zoneOption(prefs) })
  } catch {
    return ''
  }
}

/** The explicit time format and zone of a viewer, as `Intl` options (nothing when none was chosen). */
function turnTimeOptions(prefs?: FormatPrefs): { hour12?: boolean; timeZone?: string } {
  const zone = prefs?.timeZone?.trim()
  const timeZone = zone && isValidTimeZone(zone) ? zone : undefined
  return {
    ...(prefs?.timeFormat ? { hour12: prefs.timeFormat === '12h' } : {}),
    ...(timeZone ? { timeZone } : {}),
  }
}

/**
 * A turn's time, for the header beside the author: "14:32" in the reader's local time, with the full
 * date and time for the tooltip and the machine-readable `datetime`. Undefined when the server sent no
 * time (or one that is not a time): a row then renders without one rather than with an invented one.
 * Moved here from the Space presentation module (WS16-S1/S3); the default is Norwegian with no preferences.
 */
export function formatTurnTime(
  value: string | undefined,
  fmt: SpaceFormatter = { locale: 'no' },
): { iso: string; short: string; full: string } | undefined {
  if (!value) return undefined
  const at = new Date(value)
  if (!Number.isFinite(at.getTime())) return undefined
  return {
    iso: at.toISOString(),
    short: at.toLocaleTimeString(localeIntl(fmt.locale), { hour: '2-digit', minute: '2-digit', ...turnTimeOptions(fmt.prefs) }),
    full: at.toLocaleString(localeIntl(fmt.locale), { dateStyle: 'long', timeStyle: 'short', ...turnTimeOptions(fmt.prefs) }),
  }
}
