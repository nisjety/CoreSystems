import { describe, expect, it } from 'vitest'

import {
  ATTENTION_ITEM_KINDS,
  type AttentionItem,
  type AttentionItemKind,
} from '@/shared/api/spaces-attention-items-client'

import { ATTENTION_LANES, headlineText, itemSentence, laneOfKind, orderItems } from './attention-grammar'

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
