// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/api/search-client', () => ({
  runWebSearch: vi.fn(),
  loadSearchSuggestions: vi.fn().mockResolvedValue({ suggestions: [] }),
}))

import { runWebSearch } from '@/shared/api/search-client'
import { AssistantBrowserContext } from '@/features/core/assistant-agent/assistant-browser-context'
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
  it('hands the active tab and its initial epoch to the assistant from personal Nett', async () => {
    const prepareContinuation = vi.fn()
    const url = 'https://vg.no/section'
    const view = browserSessionFromPreview(attachBrowserSession(page, {
      session: {
        capabilities: ['navigate'], id: 'run-1', renderMode: 'chromium', status: 'live',
        control: { mode: 'human_takeover' },
        profile: { scope: 'run_scoped', storage: 'isolated' },
        tabs: [{ tabId: 'person-tab', active: true, url, tabEpoch: 0, holder: 'human' }],
        title: 'VG', url, viewport: { width: 1280, height: 800 },
      },
    }))
    render(() => (
      <AssistantBrowserContext value={{ view: () => null, stop: vi.fn(), prepareContinuation }}>
        <BrowserChrome session={view} personalBrowsing allowAssistantContinuation />
      </AssistantBrowserContext>
    ))
    fireEvent.click(await screen.findByRole('button', { name: 'La Verevon fortsette i denne fanen' }))
    expect(prepareContinuation).toHaveBeenCalledWith({
      sessionId: 'run-1', tabId: 'person-tab', origin: 'https://vg.no', tabEpoch: 0,
    })
  })

  it('shows the attempted address instead of Chromium’s error document in the omnibox', async () => {
    const url = 'https://example.org/missing'
    render(() => <BrowserChrome session={session('chrome-error://chromewebdata/')} browserNavigationError={{
      url, reason: 'http_response', sentence: 'Nettstedet svarte med HTTP 404.',
    }} />)
    const input = screen.getByRole<HTMLInputElement>('combobox', { name: 'Nettleseradresse' })
    await waitFor(() => expect(input.value).toBe(url))
  })

  it('keeps an unfinished address while focused and reflects the observed page on blur', async () => {
    const [url, setUrl] = createSignal('https://vg.no/')
    const view = render(() => <BrowserChrome session={session(url())} searchEnabled onBrowserAction={vi.fn()} />)
    const input = view.container.querySelector<HTMLInputElement>('input[aria-label="Nettleseradresse"]')!
    await waitFor(() => expect(input.value).toBe('https://vg.no/'))
    input.focus()
    fireEvent.input(input, { target: { value: 'unfinished search' } })
    setUrl('https://vg.no/news')
    await waitFor(() => expect(input.value).toBe('unfinished search'))
    input.blur()
    await waitFor(() => expect(input.value).toBe('https://vg.no/news'))
  })

  it('updates the address when an unfocused browser navigates', async () => {
    const [url, setUrl] = createSignal('https://vg.no/')
    const view = render(() => <BrowserChrome session={session(url())} searchEnabled onBrowserAction={vi.fn()} />)
    const input = view.container.querySelector<HTMLInputElement>('input[aria-label="Nettleseradresse"]')!
    await waitFor(() => expect(input.value).toBe('https://vg.no/'))
    setUrl('https://vg.no/news')
    await waitFor(() => expect(input.value).toBe('https://vg.no/news'))
  })

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


describe('personal browser controls', () => {
  it('opens the page tools from the compact menu', async () => {
    const view = render(() => <BrowserChrome personalBrowsing session={session('https://vg.no/')} onBrowserAction={vi.fn()} />)
    expect(view.container.querySelector('.knowledge-browser-chrome--personal')).not.toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Flere valg' }))
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Sideverktøy' }))
    await waitFor(() => expect(view.container.querySelector('.knowledge-browser-devtools')).not.toBeNull())
  })
})
