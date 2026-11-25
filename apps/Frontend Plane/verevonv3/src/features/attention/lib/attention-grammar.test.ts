import { describe, expect, it } from 'vitest'

import {
  ATTENTION_ITEM_KINDS,
  type AttentionItem,
  type AttentionItemKind,
} from '@/shared/api/spaces-attention-items-client'

import {
  ago,
  ATTENTION_LANES,
  customSnooze,
  customSnoozeBounds,
  headlineText,
  itemSentence,
  laneOfKind,
  localDateTimeValue,
  MAX_SNOOZE_MS,
  MIN_SNOOZE_MS,
  orderItems,
} from './attention-grammar'

// WS11-S7: presentation only. The gateway already decided which rows the viewer may see.
const en = (_no: string, english: string) => english
const no = (norwegian: string, _en: string) => norwegian

function item(over: Partial<AttentionItem> & { kind: AttentionItemKind }): AttentionItem {
  return {
    itemId: `item-${over.kind}`,
    spaceRef: 'room-1',
    spaceName: 'Prosjektrommet',
    threadId: 'thread-1',
    messageId: null,
    runId: null,
    actor: { kind: 'user', subjectId: 'subject-1', agentRef: null },
    count: 1,
    state: 'open',
    snoozedUntil: null,
    createdAt: 1,
    updatedAt: 1,
    headline: null,
    headlineState: 'ok',
    ...over,
  }
}

describe('laneOfKind', () => {
  it.each([
    ['mention', 'mentions'],
    ['reply', 'threads'],
    ['agent_done', 'agents'],
    ['agent_failed', 'agents'],
    ['invite', 'invites'],
    ['reminder', 'reminders'],
  ] as const)('puts a %s row in the %s lane', (kind, lane) => {
    expect(laneOfKind(kind)).toBe(lane)
  })

  it('maps every row kind to a lane the inbox draws', () => {
    for (const kind of ATTENTION_ITEM_KINDS) expect(ATTENTION_LANES).toContain(laneOfKind(kind))
  })
})

describe('orderItems', () => {
  it('puts a failed run first, then mentions, replies and finished runs, newest first within a rank', () => {
    const ordered = orderItems([
      item({ itemId: 'done-new', kind: 'agent_done', updatedAt: 900 }),
      item({ itemId: 'reply-old', kind: 'reply', updatedAt: 100 }),
      item({ itemId: 'mention-old', kind: 'mention', updatedAt: 200 }),
      item({ itemId: 'failed-old', kind: 'agent_failed', updatedAt: 50 }),
      item({ itemId: 'mention-new', kind: 'mention', updatedAt: 300 }),
      item({ itemId: 'failed-new', kind: 'agent_failed', updatedAt: 60 }),
    ]).map((row) => row.itemId)
    expect(ordered).toEqual(['failed-new', 'failed-old', 'mention-new', 'mention-old', 'reply-old', 'done-new'])
  })

  it('returns a new array and leaves its input in place', () => {
    const input = [item({ itemId: 'a', kind: 'reply', updatedAt: 1 }), item({ itemId: 'b', kind: 'mention', updatedAt: 2 })]
    const ordered = orderItems(input)
    expect(ordered).not.toBe(input)
    expect(input.map((row) => row.itemId)).toEqual(['a', 'b'])
  })
})

describe('itemSentence', () => {
  it.each([
    [item({ kind: 'mention' }), 'Someone mentioned you in Prosjektrommet'],
    [item({ kind: 'mention', count: 3 }), 'Someone mentioned you in Prosjektrommet (3)'],
    [item({ kind: 'mention', actor: { kind: 'agent', subjectId: null, agentRef: 'a' } }), 'An agent mentioned you in Prosjektrommet'],
    [item({ kind: 'reply', count: 2 }), 'New replies in a thread you follow in Prosjektrommet (2)'],
    [item({ kind: 'invite' }), 'You were invited to Prosjektrommet'],
    [item({ kind: 'agent_done', count: 2 }), 'Your agent finished in Prosjektrommet (2)'],
    [item({ kind: 'agent_failed' }), 'Your agent failed in Prosjektrommet'],
    [item({ kind: 'reminder', count: 4 }), 'Reminder in Prosjektrommet (4)'],
    [item({ kind: 'invite', spaceName: null }), 'You were invited to a room'],
  ])('reads one sentence per row: %#', (row, sentence) => {
    expect(itemSentence(row, en)).toBe(sentence)
  })

  it('says the same in Norwegian when the viewer reads Norwegian', () => {
    expect(itemSentence(item({ kind: 'invite' }), no)).toBe('Du er invitert til Prosjektrommet')
    expect(itemSentence(item({ kind: 'agent_failed', count: 2 }), no)).toBe('Agenten din feilet i Prosjektrommet (2)')
  })
})

describe('headlineText', () => {
  it('shows a released title as it is', () => {
    expect(headlineText({ headline: 'Budsjett 2027', headlineState: 'ok', threadId: 'thread-1' }, en)).toBe('Budsjett 2027')
  })

  it('says why a title is missing for a hidden or restricted thread', () => {
    expect(headlineText({ headline: null, headlineState: 'hidden', threadId: 'thread-1' }, en)).toBe('Preview hidden')
    expect(headlineText({ headline: null, headlineState: 'restricted', threadId: 'thread-1' }, en)).toBe('Not available to you')
    expect(headlineText({ headline: null, headlineState: 'hidden', threadId: 'thread-1' }, no)).toBe('Forhåndsvisning er skjult')
  })

  it('draws nothing for an ok row without a released title, or for a row without a thread', () => {
    expect(headlineText({ headline: null, headlineState: 'ok', threadId: 'thread-1' }, en)).toBeNull()
    expect(headlineText({ headline: null, headlineState: 'restricted', threadId: null }, en)).toBeNull()
  })
})

describe('itemSentence with the roster name of the actor', () => {
  it('names a person from the roster instead of the fallback word', () => {
    expect(itemSentence(item({ kind: 'mention' }), en, 'Ola Nordmann')).toBe('Ola Nordmann mentioned you in Prosjektrommet')
    expect(itemSentence(item({ kind: 'mention', count: 2 }), no, 'Ola Nordmann')).toBe('Ola Nordmann nevnte deg i Prosjektrommet (2)')
  })

  it('names an agent from the roster instead of "An agent"', () => {
    const agent = item({ kind: 'mention', actor: { kind: 'agent', subjectId: null, agentRef: 'agent-1' } })
    expect(itemSentence(agent, en, 'Research bot')).toBe('Research bot mentioned you in Prosjektrommet')
  })

  it('keeps the fallback words when the roster did not name the actor', () => {
    expect(itemSentence(item({ kind: 'mention' }), en, undefined)).toBe('Someone mentioned you in Prosjektrommet')
  })

  it('leaves rows that name no one unchanged, whatever the roster says', () => {
    expect(itemSentence(item({ kind: 'agent_failed' }), en, 'Research bot')).toBe('Your agent failed in Prosjektrommet')
  })
})

describe('customSnooze', () => {
  // Local time, so the picker's value and the check agree in every timezone.
  const now = new Date(2026, 10, 18, 9, 0, 0).getTime()
  const at = (year: number, month: number, day: number, hour: number, minute: number) =>
    localDateTimeValue(new Date(year, month - 1, day, hour, minute).getTime())

  it('accepts a time between one minute and thirty days ahead, to the minute', () => {
    const result = customSnooze(at(2026, 11, 18, 11, 30), now)
    expect(result).toEqual({ ok: true, snoozeUntil: new Date(2026, 10, 18, 11, 30).getTime() })
  })

  it('refuses a time less than a minute ahead, and says so', () => {
    expect(customSnooze(at(2026, 11, 18, 9, 0), now)).toEqual({ ok: false, problem: 'too_soon' })
  })

  it('refuses a time more than thirty days ahead', () => {
    expect(customSnooze(at(2026, 12, 19, 9, 0), now)).toEqual({ ok: false, problem: 'too_late' })
  })

  it('refuses a value that is empty, malformed or not a real date', () => {
    expect(customSnooze('', now)).toEqual({ ok: false, problem: 'invalid' })
    expect(customSnooze('2026-11-18', now)).toEqual({ ok: false, problem: 'invalid' })
    expect(customSnooze('2026-02-31T10:00', now)).toEqual({ ok: false, problem: 'invalid' })
    expect(customSnooze('2026-11-18T25:00', now)).toEqual({ ok: false, problem: 'invalid' })
  })

  it('takes the window the server enforces: one minute and thirty days', () => {
    expect(MIN_SNOOZE_MS).toBe(60_000)
    expect(MAX_SNOOZE_MS).toBe(30 * 24 * 60 * 60 * 1000)
  })

  it('offers a picker range whose limits the check accepts', () => {
    const bounds = customSnoozeBounds(now)
    expect(customSnooze(bounds.min, now).ok).toBe(true)
    expect(customSnooze(bounds.max, now).ok).toBe(true)
    expect(customSnooze(bounds.initial, now).ok).toBe(true)
  })
})

describe('ago', () => {
  const now = Date.parse('2026-11-18T09:00:00Z')

  it('says how long ago a time was, in the viewer language, and draws nothing for an unreadable time', () => {
    expect(ago(now - 3 * 60 * 60 * 1000, 'en', now)).toBe('3 hours ago')
    expect(ago(null, 'en', now)).toBe('')
    expect(ago(0, 'en', now)).toBe('')
  })
})
