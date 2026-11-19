import { cleanup, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'

import { turnEntryCard, turnEntryFromWire } from '../../lib/entry-registry'
import type { LinkPreviewsEntry, TurnEntry } from '../../lib/entry-registry'
import { LinkPreviewCards } from './LinkPreviewCards'
import { UnknownEntryCard } from './UnknownEntryCard'

// WS15-S9: the text-only link previews of a post, and the neutral card for an entry this build cannot draw.
// The link opens only when the shared link rules accept it, and the host shown is read from that link.
// No I18nProvider wraps these renders, so the copy is the Norwegian default.

afterEach(() => cleanup())

function previewsOf(items: readonly Record<string, unknown>[]): TurnEntry {
  const entry = turnEntryFromWire({ kind: 'link_previews', version: 1, link_previews: items })
  if (!entry) throw new Error('expected an entry')
  return entry
}

function linkPreviewsOf(items: readonly Record<string, unknown>[]): LinkPreviewsEntry {
  const entry = previewsOf(items)
  if (entry.kind !== 'link_previews') throw new Error('expected link previews')
  return entry
}

describe('LinkPreviewCards (WS15-S9)', () => {
  it('links each preview to its captured target, with the host read from that link and not from the server', () => {
    const entry = linkPreviewsOf([
      {
        url: 'https://www.example.test/artikkel?ref=feed',
        final_url: 'https://www.example.test/artikkel',
        host: 'annet.example',
        title: 'Artikkel',
        description: 'En kort tekst',
        site_name: 'Example',
      },
    ])
    const { container } = render(() => <LinkPreviewCards entry={entry} />)
    const link = container.querySelector('a.verevon-room-link-preview') as HTMLAnchorElement
    expect(link.getAttribute('href')).toBe('https://www.example.test/artikkel')
    expect(link.querySelector('.verevon-room-link-preview__host')?.textContent).toContain('Example')
    expect(link.querySelector('.verevon-room-link-preview__domain')?.textContent).toBe('www.example.test')
    expect(link.querySelector('.verevon-room-link-preview__title')?.textContent).toBe('Artikkel')
    expect(link.querySelector('.verevon-room-link-preview__description')?.textContent).toBe('En kort tekst')
    expect(link.textContent).not.toContain('annet.example')
  })

  it('opens each link in a new tab with no opener, no referrer and no follow', () => {
    const entry = linkPreviewsOf([{ url: 'https://example.test/a', title: 'A' }])
    const { container } = render(() => <LinkPreviewCards entry={entry} />)
    const link = container.querySelector('a.verevon-room-link-preview') as HTMLAnchorElement
    expect(link.getAttribute('target')).toBe('_blank')
    expect(link.getAttribute('rel')).toBe('noopener noreferrer nofollow')
  })

  it('draws no image, and says the preview was captured when the post was sent', () => {
    const entry = linkPreviewsOf([{ url: 'https://example.test/a', title: 'A', description: 'B' }])
    const { container } = render(() => <LinkPreviewCards entry={entry} />)
    expect(container.querySelector('img')).toBeNull()
    expect(container.querySelector('.verevon-room-link-preview__note')?.textContent).toBe(
      'Forhåndsvisning hentet da innlegget ble sendt',
    )
  })

  it('draws the host alone when the site gives no name, and never a second host line', () => {
    const entry = linkPreviewsOf([{ url: 'https://ok.example.test/side', title: 'Ja', site_name: '' }])
    const { container } = render(() => <LinkPreviewCards entry={entry} />)
    expect(container.querySelector('.verevon-room-link-preview__host')?.textContent).toBe('ok.example.test')
    expect(container.querySelector('.verevon-room-link-preview__domain')).toBeNull()
  })

  it('draws only the safe previews of a mixed list, and never an unsafe link', () => {
    const entry = linkPreviewsOf([
      { url: 'javascript:alert(1)', title: 'Nei' },
      { url: 'https://ok.example.test/side', title: 'Ja' },
    ])
    const { container } = render(() => <LinkPreviewCards entry={entry} />)
    const links = container.querySelectorAll('a')
    expect(links).toHaveLength(1)
    expect(links[0]?.getAttribute('href')).toBe('https://ok.example.test/side')
    expect(container.querySelector('a[href^="javascript"]')).toBeNull()
  })

  it('is the card for its kind', () => {
    expect(turnEntryCard(previewsOf([{ url: 'https://example.test/a' }]))).toBe(LinkPreviewCards)
  })
})

describe('UnknownEntryCard (WS15-S9)', () => {
  it('says in neutral words that this version cannot show the item, and keeps the kind out of the text', () => {
    const unknown = turnEntryFromWire({ kind: 'poll', version: 3 }) as TurnEntry
    const { container } = render(() => <UnknownEntryCard entry={unknown} />)
    const card = container.querySelector('[data-entry-kind]') as HTMLElement
    expect(card.getAttribute('data-entry-kind')).toBe('poll')
    expect(card.getAttribute('role')).toBe('note')
    expect(card.textContent).toBe('Dette elementet støttes ikke i denne versjonen av Verevon.')
    expect(card.textContent).not.toContain('poll')
  })

  it('is the card for a kind nothing registered, and for a link list that did not parse', () => {
    const unknownKind = turnEntryFromWire({ kind: 'poll', version: 1 }) as TurnEntry
    const brokenPreviews = turnEntryFromWire({ kind: 'link_previews', version: 1, link_previews: 'ikke en liste' }) as TurnEntry
    expect(brokenPreviews.kind).toBe('unknown')
    expect(turnEntryCard(unknownKind)).toBe(UnknownEntryCard)
    expect(turnEntryCard(brokenPreviews)).toBe(UnknownEntryCard)
  })

  it('draws the neutral card, and no link, for a preview list whose every link was refused', () => {
    const entry = previewsOf([{ url: 'javascript:alert(1)', title: 'Farlig' }])
    expect(entry.kind).toBe('unknown')
    const { container } = render(() => <UnknownEntryCard entry={entry} />)
    expect(container.querySelector('a')).toBeNull()
    expect(container.querySelector('.verevon-room-entry--unknown')).not.toBeNull()
  })
})
