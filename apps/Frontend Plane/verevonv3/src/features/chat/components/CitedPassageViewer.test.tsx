// @vitest-environment jsdom

import { cleanup, fireEvent, render, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flush } from 'solid-js'
import * as chatClient from '@/shared/api/chat-client'
import { CitedPassageViewer, citedPassageOf, textFragmentHref } from './CitedPassageViewer'
import { isSafeLinkHref } from '../lib/safe-href'
import type { Citation } from './chat-types'

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return { ...actual, fetchCitedChunk: vi.fn() }
})

afterEach(() => {
  cleanup()
  vi.mocked(chatClient.fetchCitedChunk).mockReset()
})

/** A citation as the stream or a stored turn carries it, passage keys in the wire shape. */
function citation(overrides: Record<string, unknown> = {}): Citation {
  return {
    id: 'doc-1',
    title: 'Refusjonsrutine',
    url: 'notion',
    snippet: 'Refusjon innen 30 dager.',
    origin: 'knowledge',
    quote: 'Refusjon gis innen 30 dager etter levering.',
    locator: { document_id: 'doc-1', chunk_index: 2 },
    ...overrides,
  } as Citation
}

describe('citedPassageOf', () => {
  it('reads the quote and the locator in the wire shape', () => {
    expect(citedPassageOf(citation())).toEqual({
      quote: 'Refusjon gis innen 30 dager etter levering.',
      locator: { document_id: 'doc-1', chunk_index: 2 },
    })
  })

  it('has no passage when the server sent neither half, as with the switch off', () => {
    expect(citedPassageOf(citation({ quote: undefined, locator: undefined }))).toBeNull()
  })

  it('drops a malformed locator rather than guessing a chunk', () => {
    expect(citedPassageOf(citation({ quote: undefined, locator: { document_id: 'doc-1', chunk_index: '2' } }))).toBeNull()
  })
})

describe('textFragmentHref', () => {
  it('opens the live page at the first eight words of the quote, and the link stays safe', () => {
    const href = textFragmentHref('https://example.no/side', 'en to tre fire fem seks sju åtte ni ti')
    expect(href).toBe(`https://example.no/side#:~:text=${encodeURIComponent('en to tre fire fem seks sju åtte')}`)
    expect(isSafeLinkHref(href ?? '')).toBe(true)
  })

  it('escapes the fragment separators so a quote cannot split the directive', () => {
    const href = textFragmentHref('https://example.no/side#del', 'pris-liste, 2026 & mer')
    expect(href).toBe('https://example.no/side#del:~:text=pris%2Dliste%2C%202026%20%26%20mer')
  })

  it('refuses a javascript: page and any other unsafe address', () => {
    expect(textFragmentHref('javascript:alert(1)', 'tekst')).toBeNull()
    expect(textFragmentHref('data:text/html,hei', 'tekst')).toBeNull()
  })
})

describe('CitedPassageViewer', () => {
  it('shows a knowledge quote and reads the chunk the citation names on "Vis mer"', async () => {
    vi.mocked(chatClient.fetchCitedChunk).mockResolvedValue({ status: 'ok', text: 'Hele avsnittet om refusjon.' })
    const { container, getByText } = render(() => <CitedPassageViewer citation={citation()} onClose={() => {}} />)
    expect(container.querySelector('.verevon-chat-cited-passage__quote')?.textContent).toBe(
      'Refusjon gis innen 30 dager etter levering.',
    )
    // A knowledge source is never opened as a web page.
    expect(container.querySelector('.verevon-chat-cited-passage__open')).toBeNull()
    fireEvent.click(getByText('Vis mer'))
    await waitFor(() => expect(container.querySelector('.verevon-chat-cited-passage__chunk')?.textContent).toBe(
      'Hele avsnittet om refusjon.',
    ))
    expect(chatClient.fetchCitedChunk).toHaveBeenCalledWith({ document_id: 'doc-1', chunk_index: 2 })
  })

  it('says the viewer has no access when Data Plane refuses the chunk read', async () => {
    vi.mocked(chatClient.fetchCitedChunk).mockResolvedValue({ status: 'denied' })
    const { container, getByText } = render(() => <CitedPassageViewer citation={citation()} onClose={() => {}} />)
    fireEvent.click(getByText('Vis mer'))
    await waitFor(() => expect(container.textContent).toContain('Du har ikke tilgang til dette dokumentet'))
    expect(container.querySelector('.verevon-chat-cited-passage__chunk')).toBeNull()
  })

  it('opens a web source at its quote through a safe text-fragment link', () => {
    const web = citation({
      id: 'dr-1',
      url: 'https://example.no/side',
      origin: 'web',
      quote: 'Fristen er tretti dager.',
      locator: undefined,
    })
    const { container } = render(() => <CitedPassageViewer citation={web} onClose={() => {}} />)
    const link = container.querySelector<HTMLAnchorElement>('.verevon-chat-cited-passage__open')
    expect(link?.getAttribute('href')).toBe(
      `https://example.no/side#:~:text=${encodeURIComponent('Fristen er tretti dager.')}`,
    )
    expect(link?.getAttribute('rel')).toBe('noopener noreferrer')
    expect(container.textContent).not.toContain('Vis mer')
  })

  it('never links a javascript: source, but still shows its quote', () => {
    const hostile = citation({ url: 'javascript:alert(1)', origin: 'web', locator: undefined })
    const { container } = render(() => <CitedPassageViewer citation={hostile} onClose={() => {}} />)
    expect(container.querySelector('a')).toBeNull()
    expect(container.querySelector('.verevon-chat-cited-passage__quote')).not.toBeNull()
  })

  it('shows a connector source as its quote only', () => {
    const connector = citation({ origin: 'connector', url: 'https://outlook.office.com/mail/1', locator: undefined })
    const { container } = render(() => <CitedPassageViewer citation={connector} onClose={() => {}} />)
    expect(container.querySelector('a')).toBeNull()
    expect(container.textContent).not.toContain('Vis mer')
  })

  it('closes on the close button and on Escape', () => {
    const onClose = vi.fn()
    const { getByLabelText } = render(() => <CitedPassageViewer citation={citation()} onClose={onClose} />)
    // The Escape listener is attached by an effect after mount.
    flush()
    fireEvent.click(getByLabelText('Lukk'))
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(onClose).toHaveBeenCalledTimes(2)
  })
})
