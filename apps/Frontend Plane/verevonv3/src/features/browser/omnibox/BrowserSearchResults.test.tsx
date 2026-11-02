// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/api/search-client', () => ({ runWebSearch: vi.fn() }))

import { runWebSearch } from '@/shared/api/search-client'
import { BrowserSearchResults, safeSearchResults, SEARCH_RESULTS_MAX } from './BrowserSearchResults'

const search = vi.mocked(runWebSearch)

beforeEach(() => {
  search.mockReset()
})

afterEach(() => vi.restoreAllMocks())

describe('safeSearchResults', () => {
  it('keeps http(s) links only, without credentials, and caps the list', () => {
    const kept = safeSearchResults([
      { url: 'https://user:pw@www.vg.no/nyheter', title: 'VG' },
      { url: 'javascript:alert(1)', title: 'bad' },
      { url: 'ftp://files.example/', title: 'ftp' },
      { url: 'https://example.com/', title: '' },
    ])
    expect(kept.map((result) => result.url)).toEqual(['https://www.vg.no/nyheter', 'https://example.com/'])
    expect(kept[0]).toMatchObject({ title: 'VG', host: 'vg.no' })
    expect(kept[1]?.title).toBe('example.com')

    const many = Array.from({ length: SEARCH_RESULTS_MAX + 5 }, (_, index) => ({ url: `https://site${index}.example/` }))
    expect(safeSearchResults(many)).toHaveLength(SEARCH_RESULTS_MAX)
  })
})

describe('BrowserSearchResults', () => {
  it('shows the results and opens one through the navigation callback', async () => {
    search.mockResolvedValue({ results: [{ url: 'https://www.vg.no/nyheter', title: 'VG nyheter', snippet: 'Siste saker' }] })
    const onOpen = vi.fn()
    render(() => <BrowserSearchResults query="norge" zdr={false} onOpen={onOpen} onClose={vi.fn()} />)

    expect(screen.getByRole('status').textContent).toContain('Søker')
    const title = await screen.findByRole('button', { name: 'VG nyheter' })
    expect(search).toHaveBeenCalledWith({ query: 'norge', limit: SEARCH_RESULTS_MAX }, expect.any(AbortSignal))
    expect(screen.getByText('Siste saker')).toBeTruthy()

    fireEvent.click(title)
    expect(onOpen).toHaveBeenCalledWith('https://www.vg.no/nyheter')
  })

  it('shows the empty and error states', async () => {
    search.mockResolvedValueOnce({ results: [] })
    const { unmount } = render(() => <BrowserSearchResults query="ingenting" zdr={false} onOpen={vi.fn()} onClose={vi.fn()} />)
    await waitFor(() => expect(screen.getByText('Ingen treff for «ingenting».')).toBeTruthy())
    unmount()

    search.mockRejectedValueOnce(new Error('upstream down'))
    render(() => <BrowserSearchResults query="feil" zdr={false} onOpen={vi.fn()} onClose={vi.fn()} />)
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('Søket feilet'))
  })

  it('states the zero-data-retention note and still searches', async () => {
    search.mockResolvedValue({ results: [] })
    render(() => <BrowserSearchResults query="laks" zdr onOpen={vi.fn()} onClose={vi.fn()} />)
    expect(screen.getByRole('note').textContent).toContain('Zero data retention')
    await waitFor(() => expect(search).toHaveBeenCalledTimes(1))
  })

  it('names the host the session is limited to, and the refused host, when a result was refused', async () => {
    search.mockResolvedValue({ results: [{ url: 'https://other.example/a', title: 'Annen side' }] })
    render(() => (
      <BrowserSearchResults
        query="norge"
        zdr={false}
        limitedToHost="vg.no"
        refusedHost="other.example"
        onOpen={vi.fn()}
        onClose={vi.fn()}
      />
    ))
    await screen.findByRole('button', { name: 'Annen side' })
    const notice = screen.getByRole('alert')
    expect(notice.getAttribute('data-reason')).toBe('outside_scope')
    expect(notice.textContent).toContain('begrenset til vg.no')
    expect(notice.textContent).toContain('kan ikke åpne other.example')
  })

  it('shows the limit without a refused host when none is known, and no notice without a limit', async () => {
    search.mockResolvedValue({ results: [] })
    const { unmount } = render(() => (
      <BrowserSearchResults query="a" zdr={false} limitedToHost="vg.no" onOpen={vi.fn()} onClose={vi.fn()} />
    ))
    await waitFor(() => expect(screen.getByText('Ingen treff for «a».')).toBeTruthy())
    expect(screen.getByRole('alert').textContent).not.toContain('kan ikke åpne')
    unmount()

    render(() => <BrowserSearchResults query="b" zdr={false} onOpen={vi.fn()} onClose={vi.fn()} />)
    await waitFor(() => expect(screen.getByText('Ingen treff for «b».')).toBeTruthy())
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('closes from its own button', () => {
    search.mockResolvedValue({ results: [] })
    const onClose = vi.fn()
    render(() => <BrowserSearchResults query="laks" zdr={false} onOpen={vi.fn()} onClose={onClose} />)
    fireEvent.click(screen.getByRole('button', { name: 'Lukk søkeresultater' }))
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})
