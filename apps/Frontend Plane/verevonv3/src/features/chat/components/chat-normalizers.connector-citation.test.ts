import { describe, expect, it } from 'vitest'
import { citationFromUnknown, normalizeCitation } from './chat-normalizers'

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
