import { afterEach, describe, expect, it, vi } from 'vitest'

import { cancelSpaceReminder, createSpaceReminder, listPendingSpaceReminders, spaceReminderFrom } from './reminders'

type FetchCall = { url: string; init: RequestInit | undefined }

function stubFetch(status: number, body: unknown): FetchCall[] {
  const calls: FetchCall[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url: String(url), init })
      return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
    }),
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

const sentBody = (call: FetchCall | undefined) => JSON.parse(String(call?.init?.body)) as Record<string, unknown>

const row = (over: Record<string, unknown> = {}) => ({
  reminder_id: 'rem_1',
  space_ref: 'room',
  thread_id: 'thr_1',
  message_id: 'msg_1',
  remind_at: 1760000000000,
  state: 'pending',
  created_at: 1759990000000,
  ...over,
})

describe('spaceReminderFrom (WS11-S11)', () => {
  it('reads a well-formed row', () => {
    expect(spaceReminderFrom(row())).toEqual({
      reminderId: 'rem_1',
      spaceRef: 'room',
      threadId: 'thr_1',
      messageId: 'msg_1',
      remindAt: 1760000000000,
      createdAt: 1759990000000,
    })
  })

  it('drops a row with no object, no id, no space, no thread or no numeric time', () => {
    for (const raw of [
      null,
      'rem_1',
      5,
      row({ reminder_id: '' }),
      row({ reminder_id: undefined }),
      row({ space_ref: '  ' }),
      row({ thread_id: 7 }),
      row({ remind_at: '1760000000000' }),
      row({ remind_at: Number.NaN }),
    ]) {
      expect(spaceReminderFrom(raw)).toBeNull()
    }
  })

  it('reads an absent message id and created time as null', () => {
    expect(spaceReminderFrom(row({ message_id: '', created_at: undefined }))).toMatchObject({
      messageId: null,
      createdAt: null,
    })
  })
})

describe('createSpaceReminder (WS11-S11)', () => {
  it('POSTs the thread, the message, the whole-second time and a retry key', async () => {
    const calls = stubFetch(201, { data: { reminder_id: 'rem_1', remind_at: 1760000000000, deduplicated: false } })
    const created = await createSpaceReminder({
      spaceRef: 'space 1',
      threadId: 'thr_1',
      messageId: 'msg_1',
      remindAt: 1760000000000.7,
    })
    expect(calls[0]?.url.endsWith('/api/v1/spaces/space%201/reminders')).toBe(true)
    expect(calls[0]?.init?.method).toBe('POST')
    const body = sentBody(calls[0])
    expect(body).toMatchObject({ thread_id: 'thr_1', message_id: 'msg_1', remind_at: 1760000000000 })
    const key = String(body.idempotency_key)
    expect(key.startsWith('remind-')).toBe(true)
    expect(key.length).toBeGreaterThanOrEqual(16)
    expect(key.length).toBeLessThanOrEqual(64)
    expect(created).toEqual({ reminderId: 'rem_1', remindAt: 1760000000000, deduplicated: false })
  })

  it('omits message_id when none is given and reads the deduplicated flag', async () => {
    const calls = stubFetch(200, { data: { reminder_id: 'rem_1', remind_at: 1760000000000, deduplicated: true } })
    const created = await createSpaceReminder({ spaceRef: 'room', threadId: 'thr_1', remindAt: 1760000000000 })
    expect(sentBody(calls[0])).not.toHaveProperty('message_id')
    expect(created.deduplicated).toBe(true)
  })
})

describe('listPendingSpaceReminders (WS11-S11)', () => {
  it('GETs the pending reminders, soonest first, and drops malformed rows', async () => {
    const calls = stubFetch(200, {
      data: {
        reminders: [
          row({ reminder_id: 'late', remind_at: 300 }),
          { reminder_id: 'broken' },
          row({ reminder_id: 'soon', remind_at: 100 }),
          row({ reminder_id: 'mid', remind_at: 200 }),
        ],
      },
    })
    const reminders = await listPendingSpaceReminders()
    expect(calls[0]?.url.endsWith('/api/v1/spaces/reminders?state=pending')).toBe(true)
    expect(reminders.map((reminder) => reminder.reminderId)).toEqual(['soon', 'mid', 'late'])
  })
})

describe('cancelSpaceReminder (WS11-S11)', () => {
  it('DELETEs the encoded reminder path', async () => {
    const calls = stubFetch(200, { data: { reminder_id: 'rem/1', cancelled: true } })
    await cancelSpaceReminder('rem/1')
    expect(calls[0]?.url.endsWith('/api/v1/spaces/reminders/rem%2F1')).toBe(true)
    expect(calls[0]?.init?.method).toBe('DELETE')
  })
})
