import { createRoot } from 'solid-js'
import { describe, expect, it } from 'vitest'
import { ChatMarkdown } from './ChatMessages'
import type { Citation } from './chat-types'

// The server numbers each source it sends, and the numbering can have gaps: a source that was not read takes
// no number. A `[n]` marker in the answer names the source the server numbered `n`, never the source that
// happens to sit at array position `n`.

function renderAnswer(content: string, citations: Citation[]): HTMLElement {
  return createRoot((dispose) => {
    const element = ChatMarkdown({ content, citations }) as unknown as HTMLElement
    dispose()
    return element
  })
}

function source(id: string, url: string, extra: Partial<Citation> = {}): Citation {
  return { id, title: id, url, snippet: '', ...extra }
}

/** Each source a chip lists, in document order: the marker it answers to and the link it opens. */
function chipRows(answer: HTMLElement): Array<{ index: string | null; href: string | null }> {
  return Array.from(answer.querySelectorAll('.verevon-chat-citation-chip__source'), (row) => ({
    index: row.querySelector('.verevon-chat-citation-chip__index')?.textContent ?? null,
    href: row.getAttribute('href'),
  }))
}

describe('citation markers bind by the server number', () => {
  it('binds [3] to the source numbered 3 when the numbering has a gap', () => {
    const answer = renderAnswer('First source [1], third source [3].', [
      source('c1', 'https://first.example/1', { number: 1 }),
      source('c3', 'https://third.example/3', { number: 3 }),
    ])

    expect(chipRows(answer)).toEqual([
      { index: '1', href: 'https://first.example/1' },
      { index: '3', href: 'https://third.example/3' },
    ])
    expect(answer.textContent).not.toContain('[3]')
  })

  it('leaves a marker plain when no source carries its number', () => {
    const answer = renderAnswer('The second claim [2] has no source.', [
      source('c1', 'https://first.example/1', { number: 1 }),
      source('c3', 'https://third.example/3', { number: 3 }),
    ])

    expect(answer.querySelectorAll('.verevon-chat-citation-chip')).toHaveLength(0)
    expect(answer.textContent).toContain('[2]')
  })

  it('never binds an unread source that carries no number', () => {
    const answer = renderAnswer('The read source [1].', [
      source('dr-unread-7', 'https://lead.example/7', { snippet: '[not read: lead]' }),
      source('c1', 'https://read.example/1', { number: 1 }),
    ])

    expect(chipRows(answer)).toEqual([{ index: '1', href: 'https://read.example/1' }])
  })

  it('binds a mixed knowledge and web turn by number, a list marker included', () => {
    const answer = renderAnswer('Internal policy [1, 2] and a web source [4].', [
      source('kb-1', 'https://intranet.example/policy', {
        number: 1,
        origin: 'knowledge',
        sourceGroupId: 'kb:sharepoint',
        sourceGroupLabel: 'Microsoft 365',
      }),
      source('kb-2', 'https://intranet.example/appendix', {
        number: 2,
        origin: 'knowledge',
        sourceGroupId: 'kb:sharepoint',
        sourceGroupLabel: 'Microsoft 365',
      }),
      source('web-4', 'https://news.example/story', { number: 4, origin: 'web' }),
    ])

    expect(chipRows(answer)).toEqual([
      { index: '1', href: 'https://intranet.example/policy' },
      { index: '2', href: 'https://intranet.example/appendix' },
      { index: '4', href: 'https://news.example/story' },
    ])
  })

  it('falls back to array position when no source in the turn carries a number', () => {
    const answer = renderAnswer('First [1] and second [2].', [
      source('a', 'https://first.example/a'),
      source('b', 'https://second.example/b'),
    ])

    expect(chipRows(answer)).toEqual([
      { index: '1', href: 'https://first.example/a' },
      { index: '2', href: 'https://second.example/b' },
    ])
  })

  it('does not fall back to position once one source in the turn carries a number', () => {
    const answer = renderAnswer('The claim [1].', [
      source('a', 'https://first.example/a', { number: 2 }),
      source('b', 'https://second.example/b'),
    ])

    expect(answer.querySelectorAll('.verevon-chat-citation-chip')).toHaveLength(0)
    expect(answer.textContent).toContain('[1]')
  })
})
