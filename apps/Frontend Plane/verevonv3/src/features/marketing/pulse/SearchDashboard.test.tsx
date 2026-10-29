// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { SearchDashboard } from './SearchDashboard'

const websiteId = `web_${'1'.repeat(32)}`
const row = { key: 'site design', clicks: 2, impressions: 200, ctr: .01, position: 5 }
const search = (site: string) => ({ site, source: 'google_search_console', status: 'connected', period: { startDate: '2026-10-01', endDate: '2026-10-24' }, queries: [row], pages: [{ ...row, key: `${site}services/` }], opportunities: [{ ...row, type: 'title_meta' }], rowLimit: 100, queriesAtLimit: false, pagesAtLimit: false })
const response = (data: unknown) => new Response(JSON.stringify({ data }))
const waiting = (site: string) => ({ websiteId, websiteRevision: 1, site, source: 'marketing_trends_auto', status: 'waiting_for_search_console' })
const scrollDescriptor = Object.getOwnPropertyDescriptor(Element.prototype, 'scrollIntoView')

describe('unified search evidence', () => {
  afterEach(() => {
    cleanup(); vi.unstubAllGlobals(); window.localStorage.clear()
    if (scrollDescriptor) Object.defineProperty(Element.prototype, 'scrollIntoView', scrollDescriptor)
    else Reflect.deleteProperty(Element.prototype, 'scrollIntoView')
  })
  it('shares one search read between summary and table, and opens the requested evidence tab', async () => {
    const fetcher = vi.fn(async (url: string) => response(url.includes('/trends') ? waiting('https://triodelab.no/') : search('https://triodelab.no/')))
    vi.stubGlobal('fetch', fetcher)
    Object.defineProperty(Element.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() })
    render(() => <I18nProvider><SearchDashboard site="https://triodelab.no/" websiteId={websiteId} revision={1} quarter="Q4"/></I18nProvider>)
    await screen.findByRole('cell', { name: row.key })
    expect(screen.getByTestId('search-query-count').textContent).toBe('1')
    expect(screen.getByTestId('search-page-count').textContent).toBe('1')
    expect(screen.getByTestId('search-opportunity-count').textContent).toBe('1')
    expect(screen.getByTestId('search-report-disclosure').hasAttribute('open')).toBe(false)
    expect(screen.getByTestId('trends-report-disclosure').hasAttribute('open')).toBe(false)
    expect(screen.getByTestId('ads-report-disclosure').hasAttribute('open')).toBe(false)
    fireEvent.click(screen.getByRole('button', { name: /Se sidene|Explore pages/ }))
    expect(screen.getByTestId('search-report-disclosure').hasAttribute('open')).toBe(true)
    await screen.findByRole('cell', { name: 'https://triodelab.no/services/' })
    fireEvent.click(screen.getByRole('button', { name: /Vurder mulighetene|Review opportunities/ }))
    await screen.findByRole('button', { name: /Legg i oppgavelisten|Add to backlog/ })
    fireEvent.click(screen.getByRole('button', { name: /Se annonsestatus|View ad status/ }))
    expect(screen.getByTestId('ads-report-disclosure').hasAttribute('open')).toBe(true)
    expect(screen.getByTestId('ads-report-disclosure').textContent).toMatch(/ikke implementert|not implemented/)
    expect(fetcher.mock.calls.filter(([url]) => url.includes('/search?'))).toHaveLength(1)
    expect(fetcher.mock.calls.some(([url]) => url.includes('/comparison'))).toBe(false)
  })
  it('clears summary counts immediately when the website changes, before a new response arrives', async () => {
    const fetcher = vi.fn((url: string) => url.includes('/trends') ? Promise.resolve(response(waiting(new URL(url, 'http://localhost').searchParams.get('site')!))) : url.includes('agenci') ? new Promise<Response>(() => {}) : Promise.resolve(response(search('https://triodelab.no/'))))
    vi.stubGlobal('fetch', fetcher)
    const [site, setSite] = createSignal('https://triodelab.no/')
    render(() => <I18nProvider><SearchDashboard site={site()} websiteId={websiteId} revision={1} quarter="Q4"/></I18nProvider>)
    await screen.findByRole('cell', { name: row.key })
    setSite('https://agenci.no/')
    await waitFor(() => expect(screen.getByTestId('search-query-count').textContent).toBe('—'))
    expect(screen.getByTestId('search-opportunity-count').textContent).toBe('—')
    expect(screen.queryByRole('cell', { name: row.key })).toBeNull()
  })
})
