// D18: per-member room history. Pure helpers for the member list's history control and the timeline marker.
//
// A window only HIDES. Nothing here fetches, filters or reveals content: Control signs the floor and Session
// Core applies it on every read, so the page can only say what Control said. These helpers turn a calendar day
// into the instant Control takes, a window into words, and a refusal into a sentence.
import type { SpaceMemberHistoryWindow, SpaceOwnHistoryWindow } from '@/shared/api/spaces-client/member-history-window'
import { translateApiError } from '@/shared/i18n/errors'
import { localeIntl, type Locale } from '@/shared/i18n/locales'

/**
 * What these helpers need of the reader's language: the chooser, and the locale a date is written in. The page's
 * `useI18n()` value has both. They are passed as an object and called as `i18n.tr(no, en)`, the one shape the
 * UI copy gate reads as translated copy.
 */
export type HistoryI18n = {
  readonly tr: (noText: string, enText: string) => string
  readonly locale: () => Locale
}
type Words = Pick<HistoryI18n, 'tr'>

/** A calendar day taken from `<input type="date">`, as the instant Control takes, or why it is refused. */
export type LocalDayResult =
  | { readonly ok: true; readonly instant: string }
  | { readonly ok: false; readonly reason: 'empty' | 'invalid' | 'future' }

const DAY_VALUE = /^(\d{4})-(\d{2})-(\d{2})$/

/**
 * The instant a local calendar day starts, as an RFC 3339 UTC string (`2026-11-18T23:00:00.000Z` for 19 Nov in
 * Oslo). A day that does not exist (`2026-02-31`), text that is not a day, and a day whose start is after `now`
 * are refused; Control refuses a future instant as well (422 `member_history_window_invalid`, field `since_at`),
 * so this only saves the round trip. Today is allowed: its start is already past.
 */
export function startOfLocalDay(value: string, now: Date = new Date()): LocalDayResult {
  const text = value.trim()
  if (text === '') return { ok: false, reason: 'empty' }
  const parts = DAY_VALUE.exec(text)
  if (!parts) return { ok: false, reason: 'invalid' }
  const year = Number(parts[1])
  const month = Number(parts[2])
  const day = Number(parts[3])
  const start = new Date(year, month - 1, day, 0, 0, 0, 0)
  // `new Date(2026, 1, 31)` rolls over to 3 March, and a two-digit year is read as 19xx: neither is that day.
  if (start.getFullYear() !== year || start.getMonth() !== month - 1 || start.getDate() !== day) {
    return { ok: false, reason: 'invalid' }
  }
  if (start.getTime() > now.getTime()) return { ok: false, reason: 'future' }
  return { ok: true, instant: start.toISOString() }
}

/** The sentence for a day the picker or {@link startOfLocalDay} refused. */
export function localDayErrorMessage(reason: 'empty' | 'invalid' | 'future', i18n: Words): string {
  if (reason === 'empty') return i18n.tr('Velg en dato.', 'Pick a date.')
  if (reason === 'future') return i18n.tr('Velg i dag eller en tidligere dato.', 'Pick today or an earlier date.')
  return i18n.tr('Datoen er ikke gyldig.', 'That date is not valid.')
}

function dayValueOf(at: Date): string {
  const pad = (value: number, width = 2) => String(value).padStart(width, '0')
  return `${pad(at.getFullYear(), 4)}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}`
}

/** Today's local day as an `<input type="date">` value: the latest day the picker offers (`max`). */
export function todayDayValue(now: Date = new Date()): string {
  return dayValueOf(now)
}

/** The local calendar day an instant falls on, as an `<input type="date">` value; empty for anything that is not an instant. */
export function localDayValue(instant: string | null | undefined): string {
  if (!instant) return ''
  const at = new Date(instant)
  return Number.isFinite(at.getTime()) ? dayValueOf(at) : ''
}

/** An instant as a date for the reader ("12. nov. 2026" / "Nov 12, 2026"); empty for anything that is not an instant. */
export function formatHistoryDate(instant: string | null | undefined, locale: Locale): string {
  if (!instant) return ''
  const at = new Date(instant)
  if (!Number.isFinite(at.getTime())) return ''
  return at.toLocaleDateString(localeIntl(locale), { day: 'numeric', month: 'short', year: 'numeric' })
}

/** One member's window in words, for the admin's current-value line. A mode this build does not know says so. */
export function memberWindowLabel(window: SpaceMemberHistoryWindow, i18n: HistoryI18n): string {
  if (window.mode === 'full') return i18n.tr('All historikk', 'Full history')
  if (window.mode === 'since_join') return i18n.tr('Siden de ble med', 'Since they joined')
  if (window.mode === 'since_date') {
    const date = formatHistoryDate(window.sinceAt, i18n.locale())
    return date ? i18n.tr(`Fra ${date}`, `From ${date}`) : i18n.tr('Fra en dato', 'From a date')
  }
  return i18n.tr('Ukjent innstilling', 'Unknown setting')
}

/** Whether the caller's own window says earlier messages are hidden. Anything else draws no marker. */
export function ownWindowHidden(window: SpaceOwnHistoryWindow | undefined): boolean {
  return window?.hidden === true
}

/**
 * What the caller can still read, for the marker's second half: from the chosen date, or from the join.
 * Nothing for any other window, so the marker never says more than Control did.
 */
export function ownWindowDetail(window: SpaceOwnHistoryWindow, i18n: HistoryI18n): string | undefined {
  if (window.mode === 'since_date') {
    const date = formatHistoryDate(window.sinceAt, i18n.locale())
    return date ? i18n.tr(`Du ser meldinger fra ${date}.`, `You see messages from ${date}.`) : undefined
  }
  if (window.mode === 'since_join') return i18n.tr('Du ser meldinger fra du ble med.', 'You see messages from when you joined.')
  return undefined
}

/**
 * One string that changes whenever the floor does (who decides it, its mode, its instant, whether it hides).
 * The page compares it between reads: a moved floor means what it already holds was read under the old one.
 */
export function ownWindowSignature(window: SpaceOwnHistoryWindow | undefined): string {
  return window ? `${window.source}|${window.mode}|${window.sinceAt ?? ''}|${window.hidden ? 'hidden' : 'open'}` : ''
}

/** What the person was doing when it failed: reading a window, changing one, or inviting with "don't share". */
export type MemberHistoryOperation = 'read' | 'change' | 'invite'

function errorCodeOf(err: unknown): string | null {
  const code = (err as { code?: unknown } | null)?.code
  return typeof code === 'string' ? code : null
}

function errorFieldOf(err: unknown): string | null {
  const details = (err as { details?: unknown } | null)?.details
  if (!details || typeof details !== 'object' || Array.isArray(details)) return null
  const field = (details as { field?: unknown }).field
  return typeof field === 'string' ? field : null
}

/** Whether the refusal says the window moved under the caller (409 `room_member_changed`): read it again, then say so. */
export function isMemberWindowConflict(err: unknown): boolean {
  return errorCodeOf(err) === 'room_member_changed'
}

/** The sentence once a conflict was answered by reading the window again; set AFTER the reload so it outlives it. */
export function memberWindowConflictMessage(i18n: Words): string {
  return i18n.tr(
    'Innstillingen ble endret av noen andre. Vi har lastet den på nytt; sjekk valget og lagre igjen.',
    'The setting was changed by someone else. We reloaded it; check the choice and save again.',
  )
}

/**
 * The sentence for a failed read, change or invite. A typed D18 code gets its own words (the 422 names the field it
 * refused; a switched-off invite says nobody was added); every other code gets the shared words of
 * shared/i18n/errors.ts, and a code with none gets the operation's own sentence. Never the server's text.
 */
export function memberHistoryErrorMessage(err: unknown, i18n: Words, operation: MemberHistoryOperation): string {
  const code = errorCodeOf(err)
  if (code === 'member_history_window_invalid') {
    const field = errorFieldOf(err)
    if (field === 'since_at') return i18n.tr('Velg i dag eller en tidligere dato.', 'Pick today or an earlier date.')
    if (field === 'mode') {
      return i18n.tr('Velg all historikk, fra en dato eller siden de ble med.', 'Choose full history, from a date or since they joined.')
    }
  }
  if (code === 'member_history_window_unavailable' && operation === 'invite') {
    return i18n.tr(
      'Å skjule tidligere meldinger for nye medlemmer er ikke slått på ennå. Ingen er lagt til.',
      'Hiding earlier messages from new members is not turned on yet. Nobody was added.',
    )
  }
  if (code === 'room_admin_unavailable') {
    if (operation === 'read') {
      return i18n.tr('Historikken kunne ikke hentes akkurat nå. Prøv igjen om litt.', 'The history setting could not be loaded right now. Try again shortly.')
    }
    return operation === 'invite'
      ? i18n.tr('Personen kunne ikke legges til akkurat nå. Ingen er lagt til. Prøv igjen om litt.', 'That person could not be added right now. Nobody was added. Try again shortly.')
      : i18n.tr(
          'Historikken kunne ikke endres akkurat nå. Ingenting er endret. Prøv igjen om litt.',
          'The history setting could not be changed right now. Nothing was changed. Try again shortly.',
        )
  }
  const own =
    operation === 'read'
      ? i18n.tr('Historikken kunne ikke hentes. Prøv igjen om litt.', 'The history setting could not be loaded. Try again shortly.')
      : operation === 'invite'
        ? i18n.tr('Personen kunne ikke legges til. Ingenting er endret.', 'That person could not be added. Nothing was changed.')
        : i18n.tr('Historikken kunne ikke endres. Ingenting er endret.', 'The history setting could not be changed. Nothing was changed.')
  // `translateApiError` reads the pair through `tr`, so a sentence already in the reader's language goes in as both.
  return translateApiError(err, i18n.tr, { no: own, en: own })
}
