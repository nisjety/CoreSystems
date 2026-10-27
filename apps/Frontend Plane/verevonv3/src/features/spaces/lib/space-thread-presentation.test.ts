import { describe, expect, it } from 'vitest'

import type { SpaceThread } from '@/shared/api/spaces-client'
import { translateApiError } from '@/shared/i18n/errors'

import {
  ACTIVE_RUN_STATUSES,
  AWAITING_APPROVAL_RUN_STATUS,
  LIVE_RUN_STATUSES,
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
