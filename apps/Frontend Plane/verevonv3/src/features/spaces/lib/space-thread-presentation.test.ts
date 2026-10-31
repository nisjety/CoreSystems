import { describe, expect, it } from 'vitest'
import type { SpaceThread } from '@/shared/api/spaces-client'
import { threadStatus } from './space-thread-presentation'

const english = (_no: string, en: string) => en
const norsk = (no: string) => no

function thread(latest_run_status?: string): SpaceThread {
  return { thread_id: 't1', space_id: 'room-1', latest_run_status } as SpaceThread
}

describe('threadStatus', () => {
  it('names each run state in English as the room always has', () => {
    expect(threadStatus(thread(), english)).toBe('Conversation open')
    expect(threadStatus(thread('awaiting_approval'), english)).toBe('Needs approval')
    expect(threadStatus(thread('running'), english)).toBe('Working')
    expect(threadStatus(thread('queued'), english)).toBe('Queued')
    expect(threadStatus(thread('completed'), english)).toBe('Completed')
    expect(threadStatus(thread('failed'), english)).toBe('Failed')
    expect(threadStatus(thread('cancelled'), english)).toBe('Stopped')
  })

  it('keeps the Norwegian wording, and a raw token for a state the room does not name', () => {
    expect(threadStatus(thread(), norsk)).toBe('Samtale åpen')
    expect(threadStatus(thread('awaiting_approval'), norsk)).toBe('Venter på godkjenning')
    expect(threadStatus(thread('running'), norsk)).toBe('Arbeider')
    expect(threadStatus(thread('queued'), norsk)).toBe('I kø')
    expect(threadStatus(thread('completed'), norsk)).toBe('Fullført')
    expect(threadStatus(thread('failed'), norsk)).toBe('Feilet')
    expect(threadStatus(thread('cancelled'), norsk)).toBe('Stoppet')
    expect(threadStatus(thread('paused'), english)).toBe('Paused')
    expect(threadStatus(thread('blocked'), english)).toBe('Blocked')
  })

  // The room now reads both spellings of a stop and a timed-out run the way the sidebar does.
  it('reads the second spelling of a stop as Stopped and a timed-out run as Failed', () => {
    expect(threadStatus(thread('canceled'), english)).toBe('Stopped')
    expect(threadStatus(thread('timed_out'), english)).toBe('Failed')
  })
})
