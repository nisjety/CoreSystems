// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { SearchComparisonPanel } from './SearchComparisonPanel.tsx'

const websiteId = 'web_' + '1'.repeat(32)
const metrics = { key: 'triodelab web', clicks: 3, impressions: 300, ctr: 0.01, position: 7 }
const response = (path: string, site = 'https://triodelab.no/') => {
  const query = new URL(path, 'http://localhost').searchParams
  return new Response(JSON.stringify({ data: {
    site, source: 'google_search_console', status: 'connected', dimension: query.get('dimension'),
    currentPeriod: { startDate: query.get('currentStart'), endDate: query.get('currentEnd') },
    previousPeriod: { startDate: query.get('previousStart'), endDate: query.get('previousEnd') },
    rowLimit: 100, rowsAtLimit: false, comparableRows: 1, unmatchedRows: 1,
    rows: [{ key: metrics.key, state: 'compared', current: metrics, previous: { ...metrics, clicks: 6, impressions: 600 }, clicksChange: -3, impressionsChange: -300, positionChange: 0, clicksChangePercent: -50 },
      { key: 'unmatched', state: 'not_comparable', previous: { ...metrics, key: 'unmatched' } }],
  } }))
}
const mount = () => render(() => <I18nProvider><SearchComparisonPanel site="https://triodelab.no/" websiteId={websiteId} revision={1} /></I18nProvider>)

describe('Search comparison workflow', () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); window.localStorage.clear() })

  it('only requests an analysis when triggered and renders actual deltas with coverage limits', async () => {
    const fetchMock = vi.fn(async (path: string) => response(path))
    vi.stubGlobal('fetch', fetchMock)
    mount()
    expect(fetchMock).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: /Kjør analyse|Run analysis/ }))
    await screen.findByRole('cell', { name: metrics.key })
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(screen.getByTestId('comparison-current-period').textContent).toMatch(/2026/)
    expect(screen.getByRole('cell', { name: /^(−3|-3)$/ })).toBeTruthy()
    expect(screen.queryByRole('cell', { name: 'unmatched' })).toBeNull()
    expect(screen.getByText(/anonyme søk|anonymized queries/i)).toBeTruthy()
    fireEvent.click(screen.getByRole('checkbox', { name: /Vis alle|Show all/ }))
    expect(screen.getByRole('cell', { name: 'unmatched' })).toBeTruthy()
    expect(screen.getByText(/Kan ikke sammenlignes|Not comparable/)).toBeTruthy()
  })

  it('switches to page decline using adjacent 90 day windows', async () => {
    const fetchMock = vi.fn(async (path: string) => response(path))
    vi.stubGlobal('fetch', fetchMock)
    mount()
    fireEvent.change(screen.getByLabelText(/Analysetype|Analysis type/), { target: { value: 'decay' } })
    fireEvent.click(screen.getByRole('button', { name: /Kjør analyse|Run analysis/ }))
    await screen.findByRole('cell', { name: metrics.key })
    const query = new URL(fetchMock.mock.calls[0]![0], 'http://localhost').searchParams
    expect(query.get('dimension')).toBe('page')
    expect(Date.parse(query.get('currentEnd')!) - Date.parse(query.get('currentStart')!)).toBe(89 * 86400000)
    expect(screen.getByText(/både klikk og visninger|both clicks and impressions/i)).toBeTruthy()
  })

  it('retries an unavailable request without displaying invented observations', async () => {
    const fetchMock = vi.fn().mockRejectedValueOnce(new Error('offline')).mockImplementation(async (path: string) => response(path))
    vi.stubGlobal('fetch', fetchMock)
    mount()
    fireEvent.click(screen.getByRole('button', { name: /Kjør analyse|Run analysis/ }))
    await screen.findByRole('alert')
    expect(screen.queryByRole('table')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /Kjør analyse|Run analysis/ }))
    await screen.findByRole('table')
  })

  it('clears previous website evidence and cancels stale requests on a website change', async () => {
    let finish: ((value: Response) => void) | undefined
    const fetchMock = vi.fn().mockImplementationOnce(async (path: string) => response(path))
      .mockImplementation(() => new Promise<Response>((resolve) => { finish = resolve }))
    vi.stubGlobal('fetch', fetchMock)
    const [site, setSite] = createSignal('https://triodelab.no/')
    render(() => <I18nProvider><SearchComparisonPanel site={site()} websiteId={websiteId} revision={1} /></I18nProvider>)
    fireEvent.click(screen.getByRole('button', { name: /Kjør analyse|Run analysis/ }))
    await screen.findByRole('cell', { name: metrics.key })
    setSite('https://agenci.no/')
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    expect(screen.queryByRole('cell')).toBeNull()
    setSite('https://example.com/')
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(3))
    expect(fetchMock.mock.calls[1]?.[1]?.signal.aborted).toBe(true)
    finish?.(response(String(fetchMock.mock.calls[2]?.[0]), 'https://triodelab.no/'))
    await screen.findByRole('alert')
    expect(screen.queryByRole('table')).toBeNull()
  })
})
