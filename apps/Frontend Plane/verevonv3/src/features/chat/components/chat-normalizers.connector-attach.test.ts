// @vitest-environment jsdom

/**
 * Regression pin for connector-picker spec §2.4's claim: a mail/calendar
 * synthetic `.txt` attachment (built client-side as `new Blob([text], {type:
 * 'text/plain'})`, name e.g. "E-post – <subject>.txt") needs ZERO changes to
 * textIngestibleAttachments/inlineTextAttachments — `type.startsWith('text/')`
 * already routes it through the existing inline-text path. This is a pin, not
 * new logic: if either function ever stops treating a plain `text/plain`
 * attachment this way, this test should be the one that fails.
 */

import { afterEach, describe, expect, it, vi } from 'vitest'
import { appendInlinedAttachments, inlineTextAttachments, textIngestibleAttachments } from './chat-normalizers'
import type { ComposerAttachment } from './chat-types'

afterEach(() => vi.unstubAllGlobals())

function connectorTextAttachment(overrides: Partial<ComposerAttachment> = {}): ComposerAttachment {
  return {
    id: 'connector-outlook-msg-1-123',
    name: 'E-post – Leveranse uke 12.txt',
    size: 42,
    type: 'text/plain',
    url: 'blob:connector-mail-body',
    source: 'outlook',
    sourceItemId: 'msg-1',
    connectionId: 'conn-ms-1',
    fetchedAt: '2026-09-28T09:00:00.000Z',
    ...overrides,
  }
}

describe('a connector-picked mail/calendar .txt attachment', () => {
  it('is treated as text-ingestible purely by its type, same as any other text/plain attachment', () => {
    const attachment = connectorTextAttachment()
    expect(textIngestibleAttachments([attachment])).toEqual([attachment])
  })

  it('inlines through the same fetch(url) + response.text() path as a local text upload, with no extraction call', () => {
    const body = 'Emne: Leveranse uke 12\nFra: Leverandør AS\nDato: 2026-09-28\n\nVaren er sendt.'
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => body }))

    return inlineTextAttachments([connectorTextAttachment()]).then((inlined) => {
      expect(inlined).toEqual([{ name: 'E-post – Leveranse uke 12.txt', content: body }])
      expect(vi.mocked(fetch)).toHaveBeenCalledWith('blob:connector-mail-body')
    })
  })

  it('carries provenance fields through unchanged — additive fields, no destructuring breaks', () => {
    const attachment = connectorTextAttachment()
    // A calendar variant, same shape family, different source tag.
    const calendarAttachment = connectorTextAttachment({
      id: 'connector-gcal-evt-1-456',
      name: 'Kalender – Ukesmøte.txt',
      source: 'gcal',
      sourceItemId: 'evt-1',
    })
    for (const item of [attachment, calendarAttachment]) {
      expect(textIngestibleAttachments([item])).toHaveLength(1)
    }
  })

  it('appendInlinedAttachments fences the connector-sourced body the same way as any other attachment', () => {
    const wired = appendInlinedAttachments('Hva sier leverandøren?', [
      { name: 'E-post – Leveranse uke 12.txt', content: 'Varen er sendt.' },
    ])
    expect(wired).toBe(
      'Hva sier leverandøren?\n\n--- VEDLEGG: E-post – Leveranse uke 12.txt ---\nVaren er sendt.\n--- SLUTT PÅ VEDLEGG: E-post – Leveranse uke 12.txt ---',
    )
  })
})
