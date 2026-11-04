import { describe, expect, it } from 'vitest'
import { citationFromUnknown, normalizeCitation, transcriptTurnToChatTurn, upsertCitation } from './chat-normalizers'

// WS23 S6: a connected-account citation is origin `connector`. Before this origin was accepted the
// SPA dropped it silently, so the Kilder panel never showed a connected item the answer relied on.

describe('connector citations', () => {
  it('keeps the connector origin and the provider grouping on a live citation event', () => {
    expect(normalizeCitation({
      id: 'conn-0123456789abcdef',
      title: 'Invoice 42',
      url: 'https://outlook.office.com/mail/id/m1',
      snippet: 'Ada · 2026-10-28',
      sourceGroupId: 'connector:microsoft',
      sourceGroupLabel: 'Microsoft 365',
      origin: 'connector',
    })).toEqual({
      id: 'conn-0123456789abcdef',
      title: 'Invoice 42',
      url: 'https://outlook.office.com/mail/id/m1',
      snippet: 'Ada · 2026-10-28',
      sourceGroupId: 'connector:microsoft',
      sourceGroupLabel: 'Microsoft 365',
      origin: 'connector',
    })
  })

  it('keeps the connector origin on a stored citation read back from history', () => {
    expect(citationFromUnknown({
      id: 'conn-0123456789abcdef',
      title: 'Invoice 42',
      url: 'https://outlook.office.com/mail/id/m1',
      snippet: 'Ada · 2026-10-28',
      source_group_id: 'connector:microsoft',
      origin: 'connector',
    }, 0)).toMatchObject({ origin: 'connector', sourceGroupId: 'connector:microsoft' })
  })

  it('still refuses an origin the SPA does not know on the history path too', () => {
    expect(citationFromUnknown({
      id: 'x',
      title: 'Y',
      url: 'https://example.com/source',
      snippet: '',
      origin: 'mailbox',
    }, 0)).not.toHaveProperty('origin')
  })

  it('carries the web and knowledge origins on the history path as it does on the live path', () => {
    expect(citationFromUnknown({ id: 'w', title: 'W', url: 'https://example.com/w', snippet: '', origin: 'web' }, 0))
      .toMatchObject({ origin: 'web' })
    expect(citationFromUnknown({ id: 'k', title: 'K', url: 'https://intranet/k', snippet: '', origin: 'knowledge' }, 0))
      .toMatchObject({ origin: 'knowledge' })
  })
})

// The server's citation number is what a `[n]` marker binds to. A citation without one must not carry an
// explicit `number: undefined`, or the merge in `upsertCitation` would clear the number an earlier frame set.
describe('server citation numbers', () => {
  it('carries the number the server assigned on a live citation event', () => {
    expect(normalizeCitation({
      id: 'kb-1',
      title: 'Refund policy',
      url: 'https://intranet/refunds',
      snippet: '',
      number: 3,
      origin: 'knowledge',
    })).toMatchObject({ id: 'kb-1', number: 3 })
  })

  it('leaves the number off a citation that carries none', () => {
    expect(normalizeCitation({ id: 'w', title: 'W', url: 'https://example.com/w', snippet: '' }))
      .not.toHaveProperty('number')
  })

  it('keeps an earlier number when a later frame for the same source carries none', () => {
    const first = normalizeCitation({ id: 'c3', title: 'Third', url: 'https://example.com/3', snippet: '', number: 3 })
    const later = normalizeCitation({ id: 'c3', title: 'Third', url: 'https://example.com/3', snippet: '' })
    expect(first && later && upsertCitation([first], later)).toMatchObject([{ id: 'c3', number: 3 }])
  })

  it('keeps the number on a citation read back from a stored turn', () => {
    const turn = transcriptTurnToChatTurn({
      id: 'turn-1',
      role: 'assistant',
      content: 'The claim [3].',
      createdAt: '',
      citations: [{ id: 'c3', title: 'Third', url: 'https://example.com/3', snippet: '', number: 3 }],
    })
    expect(turn.citations).toEqual([expect.objectContaining({ id: 'c3', number: 3 })])
  })
})
