// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/api/search-client', () => ({
  runWebSearch: vi.fn(),
  loadSearchSuggestions: vi.fn().mockResolvedValue({ suggestions: [] }),
}))

import { runWebSearch } from '@/shared/api/search-client'
import { BrowserChrome } from './BrowserChrome'
import { attachBrowserSession, browserSessionFromPreview } from './browser-session'
import type { ScrapePreview } from './knowledge-preview'
import type { BrowserNavigationFailure } from '@/features/browser/session/browser-navigation-error'

const search = vi.mocked(runWebSearch)

const page: ScrapePreview = {
  blocks: [],
  charCount: 0,
  description: '',
  markdown: '',
  source: 'extract',
  title: 'VG',
  url: 'https://vg.no/',
}

function session(url: string) {
  return browserSessionFromPreview(
    attachBrowserSession(page, {
      observation: { run_id: 'run-1', step: 1, title: 'VG', url },
      session: {
        capabilities: ['navigate'],
        control: { mode: 'agent_control' },
        id: 'run-1',
        profile: { scope: 'run_scoped', storage: 'isolated' },
        renderMode: 'chromium',
        status: 'live',
        tabs: [],
        title: 'VG',
        url,
        viewport: { width: 1280, height: 800 },
      },
    }),
  )
}

const refusal: BrowserNavigationFailure = {
  url: 'https://other.example/a',
  reason: 'outside_scope',
  sentence: 'Denne økten er begrenset til domenene i kjøringen.',
}

beforeEach(() => {
  search.mockReset()
  search.mockResolvedValue({
    results: [{ url: 'https://other.example/a', title: 'Annen side', snippet: '' }],
  })
})

async function searchAndClickResult(container: HTMLElement) {
  const input = container.querySelector<HTMLInputElement>('input[aria-label="Nettleseradresse"]') as HTMLInputElement
  fireEvent.input(input, { target: { value: 'norske nyheter' } })
  // Solid 2 applies the signal write in a microtask; a person's Enter comes later than that.
  await new Promise((resolve) => setTimeout(resolve, 0))
  fireEvent.keyDown(input, { key: 'Enter' })
  const result = await screen.findByRole('button', { name: 'Annen side' })
  fireEvent.click(result)
}

describe('BrowserChrome search results and the run-domain limit (D2-046)', () => {
  it('keeps the results open on a refusal outside the run domains and names both hosts', async () => {
    const onAction = vi.fn()
    const [error, setError] = createSignal<BrowserNavigationFailure | null>(null)
    const view = render(() => (
      <BrowserChrome
        session={session('https://vg.no/')}
        searchEnabled
        browserNavigationError={error()}
        onBrowserAction={onAction}
      />
    ))
    await searchAndClickResult(view.container)
    expect(onAction).toHaveBeenCalledWith({ type: 'navigate', url: 'https://other.example/a' })
    expect(view.container.querySelector('.browser-search-results')).not.toBeNull()

    setError(refusal)
    await waitFor(() => {
      const notice = view.container.querySelector('.browser-search-results [data-reason="outside_scope"]')
      expect(notice?.textContent).toContain('begrenset til')
      expect(notice?.textContent).toContain('kan ikke åpne other.example')
    })
    expect(view.container.querySelector('.browser-search-results')).not.toBeNull()
  })

  it('closes the results when the navigation goes through', async () => {
    const [url, setUrl] = createSignal('https://vg.no/')
    const view = render(() => <BrowserChrome session={session(url())} searchEnabled onBrowserAction={vi.fn()} />)
    await searchAndClickResult(view.container)
    expect(view.container.querySelector('.browser-search-results')).not.toBeNull()
    setUrl('https://other.example/a')
    await waitFor(() => expect(view.container.querySelector('.browser-search-results')).toBeNull())
  })

  it('closes the results when the navigation fails for another reason', async () => {
    const [error, setError] = createSignal<BrowserNavigationFailure | null>(null)
    const view = render(() => (
      <BrowserChrome
        session={session('https://vg.no/')}
        searchEnabled
        browserNavigationError={error()}
        onBrowserAction={vi.fn()}
      />
    ))
    await searchAndClickResult(view.container)
    setError({ ...refusal, reason: 'unreachable', sentence: 'Adressen finnes ikke.' })
    await waitFor(() => expect(view.container.querySelector('.browser-search-results')).toBeNull())
  })
})
