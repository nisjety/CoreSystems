import { describe, expect, it } from 'vitest'

import type { SpaceThread } from '@/shared/api/spaces-client'
import { translateApiError } from '@/shared/i18n/errors'

import {
  ACTIVE_RUN_STATUSES,
  AWAITING_APPROVAL_RUN_STATUS,
  LIVE_RUN_STATUSES,
  formatRelativeWhen,
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
