import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatSearchHit } from '@/shared/api/chat-client'
import { ChatSearchResults, nextSearchIndex, searchOptionId, snippetRuns } from './ChatSearchResults'

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en }) }))

const older: ChatSearchHit = {
  threadId: 'thread-old',
  messageId: 'message-4',
  role: 'assistant',
  snippet: 'The invoice was paid on Tuesday',
  createdAt: '2026-08-02T09:00:00Z',
  threadTitle: 'Billing questions',
  archived: false,
}
const archived: ChatSearchHit = {
  threadId: 'thread-arch',
  messageId: 'message-9',
  role: 'user',
  snippet: '<img src=x onerror="window.hacked=true">invoice',
  createdAt: '2026-06-01T09:00:00Z',
  threadTitle: '',
  archived: true,
}

afterEach(cleanup)

describe('snippetRuns', () => {
  it('marks only the text between the markers and drops the markers themselves', () => {
    expect(snippetRuns('a match b')).toEqual([
      { text: 'a ', matched: false },
      { text: 'match', matched: true },
      { text: ' b', matched: false },
    ])
  })

  it('keeps markup-looking content as plain text, never as an element', () => {
    expect(snippetRuns('<b>x</b>')).toEqual([{ text: '<b>x</b>', matched: true }])
  })

  it('returns no runs for an empty snippet', () => {
    expect(snippetRuns('')).toEqual([])
  })
})

describe('nextSearchIndex', () => {
  it('moves down and wraps from the last hit to the first', () => {
    expect(nextSearchIndex(-1, 'ArrowDown', 3)).toBe(0)
    expect(nextSearchIndex(0, 'ArrowDown', 3)).toBe(1)
    expect(nextSearchIndex(2, 'ArrowDown', 3)).toBe(0)
  })

  it('moves up and wraps from the first hit to the last', () => {
    expect(nextSearchIndex(-1, 'ArrowUp', 3)).toBe(2)
    expect(nextSearchIndex(2, 'ArrowUp', 3)).toBe(1)
    expect(nextSearchIndex(0, 'ArrowUp', 3)).toBe(2)
  })

  it('jumps to the ends with Home and End, and ignores other keys and empty lists', () => {
    expect(nextSearchIndex(1, 'Home', 3)).toBe(0)
    expect(nextSearchIndex(0, 'End', 3)).toBe(2)
    expect(nextSearchIndex(0, 'a', 3)).toBeNull()
    expect(nextSearchIndex(-1, 'ArrowDown', 0)).toBeNull()
  })
})

describe('ChatSearchResults', () => {
  it('lists each hit as an option, with the matched words highlighted and the markup kept as text', () => {
    const { container } = render(() => (
      <ChatSearchResults idPrefix="search" hits={[older, archived]} status="ready" activeIndex={-1} onOpen={() => {}} onActiveChange={() => {}} />
    ))
    flush()
    expect(screen.getByRole('listbox', { name: 'Matches in your chats' })).toBeTruthy()
    const options = screen.getAllByRole('option')
    expect(options).toHaveLength(2)
    expect(options[0]!.id).toBe(searchOptionId('search', 0))
    expect(options[1]!.id).toBe(searchOptionId('search', 1))
    expect(container.querySelectorAll('mark').length).toBe(2)
    expect(container.querySelector('mark')!.textContent).toBe('invoice')
    expect(container.querySelector('img')).toBeNull()
    expect(screen.getByText('<img src=x onerror="window.hacked=true">')).toBeTruthy()
    expect(screen.getByText('Untitled chat', { exact: false })).toBeTruthy()
    expect(screen.getByText(/Archived/)).toBeTruthy()
    expect(screen.getByText('2 matches in your chats')).toBeTruthy()
  })

  it('opens the hit that was clicked, and reports hovered options as the active one', () => {
    const onOpen = vi.fn()
    const onActiveChange = vi.fn()
    render(() => (
      <ChatSearchResults idPrefix="search" hits={[older, archived]} status="ready" activeIndex={0} onOpen={onOpen} onActiveChange={onActiveChange} />
    ))
    flush()
    const options = screen.getAllByRole('option')
    expect(options[0]!.getAttribute('aria-selected')).toBe('true')
    expect(options[1]!.getAttribute('aria-selected')).toBe('false')
    fireEvent.mouseEnter(options[1]!)
    expect(onActiveChange).toHaveBeenCalledWith(1)
    fireEvent.click(options[1]!)
    expect(onOpen).toHaveBeenCalledWith(archived)
  })

  it('says no matches were found, without a listbox', () => {
    render(() => (
      <ChatSearchResults idPrefix="search" hits={[]} status="ready" activeIndex={-1} onOpen={() => {}} onActiveChange={() => {}} />
    ))
    flush()
    expect(screen.getByText('No matches in your chats.')).toBeTruthy()
    expect(screen.queryByRole('listbox')).toBeNull()
  })

  it('tells the user search is unavailable and offers no hits, so the local list stands alone', () => {
    render(() => (
      <ChatSearchResults idPrefix="search" hits={[]} status="unavailable" activeIndex={-1} onOpen={() => {}} onActiveChange={() => {}} />
    ))
    flush()
    expect(screen.getByText(/Search is unavailable/)).toBeTruthy()
    expect(screen.queryByRole('option')).toBeNull()
  })
})
