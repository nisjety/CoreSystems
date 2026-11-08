import { describe, expect, it } from 'vitest'

import type { SpaceThread } from '@/shared/api/spaces-client'
import { translateApiError } from '@/shared/i18n/errors'

import {
  ACTIVE_RUN_STATUSES,
  AWAITING_APPROVAL_RUN_STATUS,
  LIVE_RUN_STATUSES,
  TURN_GROUP_WINDOW_MS,
  continuesTurnGroup,
  formatRelativeWhen,
  formatTurnTime,
  threadStatus,
} from './space-thread-presentation'

const no = (norwegian: string) => norwegian
const en = (_norwegian: string, english: string) => english

const thread = (latest_run_status?: string): SpaceThread =>
  ({
    thread_id: 'thread-1',
    space_id: 'room-1',
    owner_subject_id: 'someone',
    latest_run_status,
  }) as unknown as SpaceThread

// WS09-S1: Session Core derives the honest phase, so a live turn reads `running` (not `queued` for
// its whole life) and the room shows the matching word. The labels are the room's one vocabulary.
describe('threadStatus run phase labels', () => {
  it('words a running run as working and a queued run as queued', () => {
    expect(threadStatus(thread('running'), no)).toBe('Arbeider')
    expect(threadStatus(thread('running'), en)).toBe('Working')
    expect(threadStatus(thread('queued'), no)).toBe('I kø')
    expect(threadStatus(thread('queued'), en)).toBe('Queued')
  })

  it('keeps running and queued apart from the approval and terminal words', () => {
    const words = ['running', 'queued', 'awaiting_approval', 'completed', 'failed', 'cancelled'].map((status) =>
      threadStatus(thread(status), en),
    )
    expect(new Set(words).size).toBe(words.length)
    expect(words).toEqual(['Working', 'Queued', 'Needs approval', 'Completed', 'Failed', 'Stopped'])
  })

  it('words a thread with no run as open and humanizes an unknown token', () => {
    expect(threadStatus(thread(undefined), en)).toBe('Conversation open')
    expect(threadStatus(thread('other'), en)).toBe('Other')
    expect(threadStatus(thread('some_new_state'), en)).toBe('Some New State')
  })

  it('treats running and queued as live and an approval wait as active but not live', () => {
    expect([...LIVE_RUN_STATUSES].sort()).toEqual(['queued', 'running'])
    for (const status of ['queued', 'running', AWAITING_APPROVAL_RUN_STATUS]) {
      expect(ACTIVE_RUN_STATUSES.has(status)).toBe(true)
    }
    expect(LIVE_RUN_STATUSES.has(AWAITING_APPROVAL_RUN_STATUS)).toBe(false)
    expect(ACTIVE_RUN_STATUSES.has('completed')).toBe(false)
  })
})

// WS09-S1: the typed refusal of a second live run on a thread has its own words in both locales.
describe('thread_run_active copy', () => {
  it('has Norwegian and English text that is not the generic fallback', () => {
    const generic = translateApiError({ code: 'a_code_nobody_registered' }, en)
    const english = translateApiError({ code: 'thread_run_active' }, en)
    const norwegian = translateApiError({ code: 'thread_run_active' }, no)
    expect(english).not.toBe(generic)
    expect(english).toMatch(/already working/i)
    expect(norwegian).toMatch(/jobber allerede/i)
    expect(english).not.toBe(norwegian)
  })
})

describe('formatRelativeWhen (reply summary)', () => {
  const now = Date.parse('2026-10-28T12:00:00Z')
  const ago = (seconds: number) => new Date(now - seconds * 1000).toISOString()

  it('says "now" inside a minute, then minutes, then hours, then days', () => {
    expect(formatRelativeWhen(ago(10), now)).toMatch(/nå/i)
    expect(formatRelativeWhen(ago(4 * 60), now)).toMatch(/4\s*min/)
    expect(formatRelativeWhen(ago(3 * 3600), now)).toMatch(/3\s*t/)
    expect(formatRelativeWhen(ago(3 * 86_400), now)).toMatch(/3\s*d/)
  })

  it('uses Norwegian words for yesterday and the day before', () => {
    expect(formatRelativeWhen(ago(86_400 + 3600), now)).toBe('i går')
    expect(formatRelativeWhen(ago(2 * 86_400), now)).toBe('i forgårs')
  })

  it('reads a time ahead of the browser clock as now, not as the future', () => {
    expect(formatRelativeWhen(ago(-3600), now)).toMatch(/nå/i)
  })

  it('falls back to the plain date from a week on', () => {
    expect(formatRelativeWhen(ago(10 * 86_400), now)).toMatch(/18\.\s*okt/)
  })

  it('reads an absent or unreadable time as empty, never as a wrong one', () => {
    expect(formatRelativeWhen(undefined, now)).toBe('')
    expect(formatRelativeWhen('', now)).toBe('')
    expect(formatRelativeWhen('not a date', now)).toBe('')
  })
})

describe('formatTurnTime', () => {
  // Built from local components so the expectation holds in any time zone.
  const local = new Date(2026, 9, 31, 14, 32, 5)

  it('gives a short HH:MM, the full date and time, and the ISO instant', () => {
    const shown = formatTurnTime(local.toISOString())
    expect(shown?.short).toBe('14:32')
    expect(shown?.iso).toBe(local.toISOString())
    expect(shown?.full).toMatch(/2026/)
    expect(shown?.full).toMatch(/14:32/)
  })

  it('shows nothing, rather than an invented time, when there is none or it is unreadable', () => {
    expect(formatTurnTime(undefined)).toBeUndefined()
    expect(formatTurnTime('')).toBeUndefined()
    expect(formatTurnTime('not a time')).toBeUndefined()
  })
})

describe('continuesTurnGroup', () => {
  const at = (minutes: number, seconds = 0) => new Date(Date.UTC(2026, 9, 31, 9, minutes, seconds)).toISOString()
  const human = (id: string | undefined, createdAt: string | undefined) => ({
    role: 'user' as const,
    authorSubjectId: id,
    createdAt,
  })
  const agent = (name: string | undefined, createdAt: string | undefined) => ({
    role: 'assistant' as const,
    agentName: name,
    createdAt,
  })

  it('groups the same member within five minutes, and not past it', () => {
    expect(continuesTurnGroup(human('u1', at(0)), human('u1', at(2)), 'Verevon')).toBe(true)
    expect(continuesTurnGroup(human('u1', at(0)), human('u1', at(5)), 'Verevon')).toBe(true)
    expect(continuesTurnGroup(human('u1', at(0)), human('u1', at(5, 1)), 'Verevon')).toBe(false)
    expect(TURN_GROUP_WINDOW_MS).toBe(300_000)
  })

  it('starts a new group for a different member or an unattributed turn', () => {
    expect(continuesTurnGroup(human('u1', at(0)), human('u2', at(1)), 'Verevon')).toBe(false)
    expect(continuesTurnGroup(human(undefined, at(0)), human(undefined, at(1)), 'Verevon')).toBe(false)
    expect(continuesTurnGroup(human('u1', at(0)), human(undefined, at(1)), 'Verevon')).toBe(false)
  })

  it('groups the same agent persona, using the room name for an unrecorded one', () => {
    expect(continuesTurnGroup(agent('Lagerhjelpen', at(0)), agent('Lagerhjelpen', at(1)), 'Verevon')).toBe(true)
    expect(continuesTurnGroup(agent(undefined, at(0)), agent(undefined, at(1)), 'Verevon')).toBe(true)
    expect(continuesTurnGroup(agent(undefined, at(0)), agent('Verevon', at(1)), 'Verevon')).toBe(true)
    expect(continuesTurnGroup(agent('Lagerhjelpen', at(0)), agent('Innkjøp', at(1)), 'Verevon')).toBe(false)
  })

  it('never groups an agent with a member', () => {
    expect(continuesTurnGroup(human('u1', at(0)), agent('Verevon', at(1)), 'Verevon')).toBe(false)
    expect(continuesTurnGroup(agent('Verevon', at(0)), human('u1', at(1)), 'Verevon')).toBe(false)
  })

  it('does not group what it cannot prove: a missing or backwards time, or the first turn', () => {
    expect(continuesTurnGroup(undefined, human('u1', at(1)), 'Verevon')).toBe(false)
    expect(continuesTurnGroup(human('u1', undefined), human('u1', at(1)), 'Verevon')).toBe(false)
    expect(continuesTurnGroup(human('u1', at(0)), human('u1', undefined), 'Verevon')).toBe(false)
    expect(continuesTurnGroup(human('u1', at(3)), human('u1', at(1)), 'Verevon')).toBe(false)
  })
})

// The room reads both spellings of a stop and a timed-out run the way the sidebar does (the shared
// thread-status vocabulary), and keeps a raw token for a state it does not name.
describe('threadStatus shared run-status vocabulary', () => {
  it('reads the second spelling of a stop as Stopped and a timed-out run as Failed', () => {
    expect(threadStatus(thread('canceled'), en)).toBe('Stopped')
    expect(threadStatus(thread('timed_out'), en)).toBe('Failed')
  })

  it('keeps a raw token for a state the room does not name', () => {
    expect(threadStatus(thread('paused'), en)).toBe('Paused')
    expect(threadStatus(thread('blocked'), en)).toBe('Blocked')
  })
})
