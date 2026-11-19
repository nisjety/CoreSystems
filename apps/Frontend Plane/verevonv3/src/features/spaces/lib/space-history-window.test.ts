import { describe, expect, it } from 'vitest'

import { ApiError } from '@/shared/api/api-error'
import { ERROR_COPY } from '@/shared/i18n/errors'

import {
  formatHistoryDate,
  isMemberWindowConflict,
  localDayErrorMessage,
  localDayValue,
  memberHistoryErrorMessage,
  memberWindowConflictMessage,
  memberWindowLabel,
  ownWindowDetail,
  ownWindowHidden,
  ownWindowSignature,
  startOfLocalDay,
  todayDayValue,
} from './space-history-window'

// The reader's language as the helpers take it: a chooser and a locale, as `useI18n()` gives them.
const no = { tr: (norwegian: string) => norwegian, locale: () => 'no' as const }
const en = { tr: (_norwegian: string, english: string) => english, locale: () => 'en' as const }

// A fixed "now" in the middle of the day, in the machine's own zone, so every assertion holds in any time zone.
const NOW = new Date(2026, 10, 19, 12, 30, 0, 0)

/** The parts of an instant in the machine's local zone. */
function localParts(instant: string) {
  const at = new Date(instant)
  return [at.getFullYear(), at.getMonth() + 1, at.getDate(), at.getHours(), at.getMinutes(), at.getSeconds(), at.getMilliseconds()]
}

describe('startOfLocalDay (D18)', () => {
  it('turns a picked day into the instant that local day starts, as an RFC 3339 UTC string', () => {
    const result = startOfLocalDay('2026-11-12', NOW)
    expect(result.ok).toBe(true)
    if (!result.ok) return
    // Start of 12 November, local: midnight, whatever the zone.
    expect(localParts(result.instant)).toEqual([2026, 11, 12, 0, 0, 0, 0])
    expect(result.instant).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.000Z$/)
    expect(Number.isFinite(Date.parse(result.instant))).toBe(true)
  })

  it('allows today, whose start has already passed, and refuses tomorrow', () => {
    expect(startOfLocalDay('2026-11-19', NOW).ok).toBe(true)
    expect(startOfLocalDay('2026-11-20', NOW)).toEqual({ ok: false, reason: 'future' })
    expect(startOfLocalDay('2027-01-01', NOW)).toEqual({ ok: false, reason: 'future' })
  })

  it('refuses a day at the first instant of tomorrow, but not a second before it', () => {
    const justBefore = new Date(2026, 10, 19, 23, 59, 59, 999)
    expect(startOfLocalDay('2026-11-19', justBefore).ok).toBe(true)
    expect(startOfLocalDay('2026-11-20', justBefore)).toEqual({ ok: false, reason: 'future' })
  })

  it('refuses nothing, and anything that is not a day, without a request', () => {
    expect(startOfLocalDay('', NOW)).toEqual({ ok: false, reason: 'empty' })
    expect(startOfLocalDay('   ', NOW)).toEqual({ ok: false, reason: 'empty' })
    for (const value of ['yesterday', '2026-11', '2026-11-1', '12.11.2026', '2026/11/12', '2026-11-12T00:00:00Z', '-2026-11-12']) {
      expect(startOfLocalDay(value, NOW), value).toEqual({ ok: false, reason: 'invalid' })
    }
  })

  it('refuses a day that does not exist instead of rolling it over to another one', () => {
    for (const value of ['2026-02-31', '2026-02-29', '2026-04-31', '2026-13-01', '2026-00-10', '2026-11-00', '2026-11-31']) {
      expect(startOfLocalDay(value, NOW), value).toEqual({ ok: false, reason: 'invalid' })
    }
    // A leap day that does exist.
    expect(startOfLocalDay('2024-02-29', NOW).ok).toBe(true)
  })

  it('does not read a two-digit year as 19xx', () => {
    expect(startOfLocalDay('0099-01-01', NOW)).toEqual({ ok: false, reason: 'invalid' })
  })
})

describe('the day helpers (D18)', () => {
  it('gives today as the latest day a picker offers, with zero padding', () => {
    expect(todayDayValue(new Date(2026, 0, 5, 9, 0, 0))).toBe('2026-01-05')
    expect(todayDayValue(NOW)).toBe('2026-11-19')
  })

  it('names the local day of an instant, and nothing for what is not one', () => {
    const noon = new Date(2026, 10, 12, 12, 0, 0).toISOString()
    expect(localDayValue(noon)).toBe('2026-11-12')
    // The local midnight a picker sends comes back as the same picked day.
    const picked = startOfLocalDay('2026-11-12', NOW)
    expect(picked.ok && localDayValue(picked.instant)).toBe('2026-11-12')
    for (const value of [null, undefined, '', 'not a time']) expect(localDayValue(value), String(value)).toBe('')
  })

  it('formats a date in the reader’s language, with the year, and nothing for a bad instant', () => {
    const instant = new Date(2026, 10, 12, 12, 0, 0).toISOString()
    expect(formatHistoryDate(instant, 'en')).toBe('Nov 12, 2026')
    expect(formatHistoryDate(instant, 'no')).toMatch(/12\.? nov\.? 2026/i)
    for (const value of [null, undefined, '', 'soon']) expect(formatHistoryDate(value, 'en'), String(value)).toBe('')
  })

  it('words a refused day in both languages', () => {
    expect(localDayErrorMessage('empty', no)).toBe('Velg en dato.')
    expect(localDayErrorMessage('empty', en)).toBe('Pick a date.')
    expect(localDayErrorMessage('future', no)).toBe('Velg i dag eller en tidligere dato.')
    expect(localDayErrorMessage('future', en)).toBe('Pick today or an earlier date.')
    expect(localDayErrorMessage('invalid', no)).toBe('Datoen er ikke gyldig.')
    expect(localDayErrorMessage('invalid', en)).toBe('That date is not valid.')
  })
})

describe('the window in words (D18)', () => {
  const instant = new Date(2026, 10, 12, 12, 0, 0).toISOString()
  const window = (over: Record<string, unknown>) => ({ source: 'member', mode: 'full', sinceAt: null, revision: 1, ...over }) as never

  it('labels each mode, with the date for since_date', () => {
    expect(memberWindowLabel(window({ mode: 'full' }), en)).toBe('Full history')
    expect(memberWindowLabel(window({ mode: 'full' }), no)).toBe('All historikk')
    expect(memberWindowLabel(window({ mode: 'since_join' }), en)).toBe('Since they joined')
    expect(memberWindowLabel(window({ mode: 'since_join' }), no)).toBe('Siden de ble med')
    expect(memberWindowLabel(window({ mode: 'since_date', sinceAt: instant }), en)).toBe('From Nov 12, 2026')
    expect(memberWindowLabel(window({ mode: 'since_date', sinceAt: instant }), no)).toMatch(/^Fra 12\.? nov\.? 2026/i)
    // A since_date with no instant, and a mode this build does not know, never invent a date.
    expect(memberWindowLabel(window({ mode: 'since_date', sinceAt: null }), en)).toBe('From a date')
    expect(memberWindowLabel(window({ mode: 'since_message' }), en)).toBe('Unknown setting')
  })

  it('says what the caller can still read only for a date or a join, and never more than Control said', () => {
    const own = (over: Record<string, unknown>) => ({ source: 'member', mode: 'full', sinceAt: null, hidden: true, ...over }) as never
    expect(ownWindowDetail(own({ mode: 'since_date', sinceAt: instant }), en)).toBe('You see messages from Nov 12, 2026.')
    expect(ownWindowDetail(own({ mode: 'since_date', sinceAt: instant }), no)).toMatch(/^Du ser meldinger fra 12\.? nov\.? 2026\.$/i)
    expect(ownWindowDetail(own({ mode: 'since_join' }), en)).toBe('You see messages from when you joined.')
    expect(ownWindowDetail(own({ mode: 'since_join' }), no)).toBe('Du ser meldinger fra du ble med.')
    expect(ownWindowDetail(own({ mode: 'since_date', sinceAt: null }), en)).toBeUndefined()
    expect(ownWindowDetail(own({ mode: 'full' }), en)).toBeUndefined()
    expect(ownWindowDetail(own({ mode: 'since_message' }), en)).toBeUndefined()
  })

  it('counts a window as hiding only on an explicit hidden true', () => {
    expect(ownWindowHidden({ source: 'member', mode: 'since_join', sinceAt: null, hidden: true })).toBe(true)
    expect(ownWindowHidden({ source: 'member', mode: 'since_join', sinceAt: null, hidden: false })).toBe(false)
    expect(ownWindowHidden(undefined)).toBe(false)
  })

  it('has one signature per floor, and changes it when who decides, the mode, the instant or the hiding changes', () => {
    const base = { source: 'member', mode: 'since_date', sinceAt: instant, hidden: true } as never
    const signature = ownWindowSignature(base)
    expect(ownWindowSignature({ ...(base as object) } as never)).toBe(signature)
    expect(ownWindowSignature({ ...(base as object), source: 'room' } as never)).not.toBe(signature)
    expect(ownWindowSignature({ ...(base as object), mode: 'since_join' } as never)).not.toBe(signature)
    expect(ownWindowSignature({ ...(base as object), sinceAt: null } as never)).not.toBe(signature)
    expect(ownWindowSignature({ ...(base as object), hidden: false } as never)).not.toBe(signature)
    expect(ownWindowSignature(undefined)).toBe('')
  })
})

describe('memberHistoryErrorMessage (D18)', () => {
  const failure = (status: number, code: string | null, details: Record<string, unknown> | null = null) =>
    new ApiError('server text that must never be shown: postgres://leak', status, code, { details })

  it('words a 422 by the field Control refused', () => {
    const since = failure(422, 'member_history_window_invalid', { field: 'since_at' })
    expect(memberHistoryErrorMessage(since, no, 'change')).toBe('Velg i dag eller en tidligere dato.')
    expect(memberHistoryErrorMessage(since, en, 'change')).toBe('Pick today or an earlier date.')
    const mode = failure(422, 'member_history_window_invalid', { field: 'mode' })
    expect(memberHistoryErrorMessage(mode, no, 'change')).toBe('Velg all historikk, fra en dato eller siden de ble med.')
    expect(memberHistoryErrorMessage(mode, en, 'change')).toBe('Choose full history, from a date or since they joined.')
    // A field it does not name falls to the shared words of the code.
    const other = failure(422, 'member_history_window_invalid', { field: 'something_else' })
    expect(memberHistoryErrorMessage(other, en, 'change')).toBe(ERROR_COPY.member_history_window_invalid?.en)
    expect(memberHistoryErrorMessage(failure(422, 'member_history_window_invalid'), en, 'change')).toBe(
      ERROR_COPY.member_history_window_invalid?.en,
    )
  })

  it('says nobody was added when an invite that hides is switched off, and nothing was changed otherwise', () => {
    const off = failure(503, 'member_history_window_unavailable')
    expect(memberHistoryErrorMessage(off, en, 'invite')).toBe('Hiding earlier messages from new members is not turned on yet. Nobody was added.')
    expect(memberHistoryErrorMessage(off, no, 'invite')).toBe('Å skjule tidligere meldinger for nye medlemmer er ikke slått på ennå. Ingen er lagt til.')
    expect(memberHistoryErrorMessage(off, en, 'change')).toBe('Per-member history is not turned on yet. Nothing was changed.')
    expect(memberHistoryErrorMessage(off, no, 'change')).toBe('Historikk per medlem er ikke slått på ennå. Ingenting er endret.')
  })

  it('words the room admin outage for the operation it hit', () => {
    const outage = failure(503, 'room_admin_unavailable')
    expect(memberHistoryErrorMessage(outage, en, 'read')).toBe('The history setting could not be loaded right now. Try again shortly.')
    expect(memberHistoryErrorMessage(outage, en, 'change')).toBe(
      'The history setting could not be changed right now. Nothing was changed. Try again shortly.',
    )
    expect(memberHistoryErrorMessage(outage, en, 'invite')).toBe('That person could not be added right now. Nobody was added. Try again shortly.')
    expect(memberHistoryErrorMessage(outage, no, 'invite')).toBe('Personen kunne ikke legges til akkurat nå. Ingen er lagt til. Prøv igjen om litt.')
  })

  it('reads every other typed refusal through the shared copy, in both languages, never the server’s text', () => {
    for (const [status, code] of [
      [403, 'room_role_forbidden'],
      [403, 'room_delegation_required'],
      [404, 'room_not_found'],
      [404, 'member_not_found'],
      [409, 'room_archived'],
      [409, 'room_roster_derived'],
      [400, 'invalid_request'],
      [404, 'feature_disabled'],
      [502, 'upstream_unavailable'],
    ] as const) {
      const err = failure(status, code)
      const shared = ERROR_COPY[code]
      expect(shared, code).toBeDefined()
      expect(memberHistoryErrorMessage(err, no, 'change'), code).toBe(shared?.no)
      expect(memberHistoryErrorMessage(err, en, 'change'), code).toBe(shared?.en)
      expect(memberHistoryErrorMessage(err, en, 'change'), code).not.toMatch(/postgres|leak/)
    }
  })

  // `space_authority_unavailable` is also a Space denial reason, so the shared seam words it with the denial vocabulary
  // and a retry sentence; this only checks it is said in both languages and never leaks the server's text.
  it('words an authority outage in both languages without the server’s text', () => {
    const err = failure(503, 'space_authority_unavailable')
    const norwegian = memberHistoryErrorMessage(err, no, 'change')
    const english = memberHistoryErrorMessage(err, en, 'change')
    expect(norwegian).not.toBe(english)
    expect(english).toMatch(/cannot be confirmed|could not be confirmed/i)
    expect(`${norwegian} ${english}`).not.toMatch(/postgres|leak/)
  })

  it('answers a code with no copy of its own with the operation’s sentence, and a rate limit by its class', () => {
    const unknown = failure(500, 'a_code_nobody_registered')
    expect(memberHistoryErrorMessage(unknown, en, 'read')).toBe('The history setting could not be loaded. Try again shortly.')
    expect(memberHistoryErrorMessage(unknown, en, 'change')).toBe('The history setting could not be changed. Nothing was changed.')
    expect(memberHistoryErrorMessage(unknown, no, 'change')).toBe('Historikken kunne ikke endres. Ingenting er endret.')
    expect(memberHistoryErrorMessage(unknown, en, 'invite')).toBe('That person could not be added. Nothing was changed.')
    expect(memberHistoryErrorMessage(failure(429, 'rate_limited'), en, 'change')).toMatch(/too many requests/i)
    expect(memberHistoryErrorMessage(new Error('boom'), en, 'change')).toBe('The history setting could not be changed. Nothing was changed.')
  })

  it('recognises the conflict that means the window moved, and words it after the reload', () => {
    expect(isMemberWindowConflict(failure(409, 'room_member_changed', { current_revision: 4 }))).toBe(true)
    expect(isMemberWindowConflict(failure(409, 'room_archived'))).toBe(false)
    expect(isMemberWindowConflict(new Error('room_member_changed'))).toBe(false)
    expect(isMemberWindowConflict(null)).toBe(false)
    expect(memberWindowConflictMessage(en)).toBe('The setting was changed by someone else. We reloaded it; check the choice and save again.')
    expect(memberWindowConflictMessage(no)).toBe('Innstillingen ble endret av noen andre. Vi har lastet den på nytt; sjekk valget og lagre igjen.')
  })
})
