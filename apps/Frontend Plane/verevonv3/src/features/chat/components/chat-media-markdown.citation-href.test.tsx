import { createRoot } from 'solid-js'
import { describe, expect, it } from 'vitest'
import { ChatMarkdown } from './ChatMessages'
import type { Citation } from './chat-types'

// A citation URL comes from the model or a search result. Rendering through the
// mounted markdown path covers both the inline marker and the chip it opens.
function renderCitedAnswer(url: string): HTMLElement {
  const citations: Citation[] = [{ id: 'c1', title: 'Kilde', url, snippet: 'Utdrag' }]
  return createRoot((dispose) => {
    const element = ChatMarkdown({ content: 'Svaret bygger på kilden [1].', citations }) as unknown as HTMLElement
    dispose()
    return element
  })
}

describe('citation chip hrefs', () => {
  it.each([
    'javascript:alert(1)',
    'JaVaScRiPt:alert(1)',
    'java\tscript:alert(1)',
    'vbscript:msgbox(1)',
    'data:text/html,<script>alert(1)</script>',
    'file:///etc/passwd',
  ])('renders the unsafe citation %j as inert text, never an anchor', (url) => {
    const answer = renderCitedAnswer(url)
    expect(answer.querySelectorAll('a, [href]')).toHaveLength(0)

    const row = answer.querySelector('.verevon-chat-citation-chip__source')
    expect(row?.tagName).toBe('SPAN')
    expect(row?.hasAttribute('href')).toBe(false)
    expect(row?.classList.contains('verevon-chat-link-blocked')).toBe(true)
    expect(row?.getAttribute('title')).toBeTruthy()
    // The inert row shows the same text the linked row shows for the same citation.
    const linked = renderCitedAnswer('https://example.com/kilde').querySelector('.verevon-chat-citation-chip__source')
    expect(row?.textContent).toContain('Kilde')
    expect(row?.textContent).toBe(linked?.textContent)
  })

  it('keeps an https citation as a real link to its own URL', () => {
    const row = renderCitedAnswer('https://example.com/kilde').querySelector('.verevon-chat-citation-chip__source')
    expect(row?.tagName).toBe('A')
    expect(row?.getAttribute('href')).toBe('https://example.com/kilde')
    expect(row?.getAttribute('target')).toBe('_blank')
    expect(row?.getAttribute('rel')).toBe('noopener noreferrer')
    expect(row?.classList.contains('verevon-chat-link-blocked')).toBe(false)
  })
})
