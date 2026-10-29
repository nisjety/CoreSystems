// @vitest-environment jsdom
import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { analysis, context } from '@/shared/api/visibility/__fixtures__/tools'
import { loadAnalysis, type Analysis } from '@/shared/api/visibility/tools-client'
import { loadWebsites, readWebsitePreference, type Website } from '@/shared/api/marketing/website-client'
import { VisibilityToolsPage } from './VisibilityToolsPage'
import { VisibilitySummary } from './VisibilitySummary'
vi.mock('@/shared/api/visibility/tools-client', () => ({ loadAnalysis: vi.fn() }))
vi.mock('@/shared/api/marketing/website-client', async original => ({ ...await original<typeof import('@/shared/api/marketing/website-client')>(), loadWebsites: vi.fn() }))
const website: Website = { id: context.websiteId, site: context.site, revision: context.websiteRevision, createdAt: '2026-10-01T00:00:00Z', updatedAt: '2026-10-01T00:00:00Z', ownership: { status: 'unverified' }, integrations: { searchConsole: { status: 'connected' }, ga4: { status: 'not_connected' }, clarity: { status: 'not_connected' }, pageSpeed: { status: 'available' } } }
beforeEach(() => { vi.mocked(loadAnalysis).mockReset(); vi.mocked(loadWebsites).mockResolvedValue([website]); vi.useFakeTimers({ toFake: ['Date'] }); vi.setSystemTime(new Date('2026-10-28T12:00:00Z')) })
afterEach(() => { cleanup(); vi.useRealTimers(); localStorage.clear() })
function mount() { render(() => <I18nProvider><Router><Route path="*all" component={() => <VisibilityToolsPage view="pages" canWrite />} /></Router></I18nProvider>) }
it('reloads saved evidence for a changed date range without starting collection', async () => {
  vi.mocked(loadAnalysis).mockResolvedValue(analysis()); mount()
  await screen.findByText('Measured homepage')
  fireEvent.change(screen.getByRole('combobox', { name: /Periode|Period/ }), { target: { value: 'Q3' } })
  await waitFor(() => expect(loadAnalysis).toHaveBeenLastCalledWith(context, 'Q3', expect.any(AbortSignal)))
})
it('clears and aborts old-site evidence before showing the new site', async () => {
  let oldResolve!: (v: Analysis) => void
  vi.mocked(loadWebsites).mockResolvedValue([website, { ...website, id: `web_${'2'.repeat(32)}`, site: 'https://other.no/' }])
  vi.mocked(loadAnalysis).mockImplementationOnce(() => new Promise(resolve => { oldResolve = resolve })).mockResolvedValue({ ...analysis(), pages: [] })
  mount()
  await waitFor(() => expect(loadAnalysis).toHaveBeenCalledTimes(1))
  const oldSignal = vi.mocked(loadAnalysis).mock.calls[0]![2]!
  fireEvent.change(screen.getByRole('combobox', { name: /Valgt nettsted|Selected website/ }), { target: { value: `web_${'2'.repeat(32)}` } })
  expect(oldSignal.aborted).toBe(true)
  expect(readWebsitePreference()).toBe(`web_${'2'.repeat(32)}`)
  oldResolve(analysis())
  await waitFor(() => expect(loadAnalysis).toHaveBeenCalledTimes(2))
  expect(screen.queryByText('Measured homepage')).toBeNull()
  expect(vi.mocked(loadAnalysis).mock.calls[1]![0].site).toBe('https://other.no/')
})
it('shows a compact real-data summary with links to all four deeper views', async () => {
  vi.mocked(loadAnalysis).mockResolvedValue(analysis())
  render(() => <I18nProvider><Router><Route path="*all" component={() => <VisibilitySummary context={context} quarter="Q4" />} /></Router></I18nProvider>)
  await screen.findByText(/Oppsummering av lagrede utvalg|Summary of saved samples/)
  expect(screen.getAllByRole('link')).toHaveLength(4)
  expect(screen.getByRole('link', { name: /Sider i utvalget|Sampled pages/ }).textContent).toContain('2')
  expect(screen.getByRole('link', { name: /Observerte lenker|Observed links/ }).textContent).toContain('1')
})

it('marks the primary website in the selector and the header, and only that one', async () => {
  vi.mocked(loadAnalysis).mockResolvedValue(analysis())
  vi.mocked(loadWebsites).mockResolvedValue([{ ...website, primary: true }, { ...website, id: `web_${'3'.repeat(32)}`, site: 'https://other.no/' }])
  mount()
  await screen.findByText('Measured homepage')
  expect(screen.getByText('Primær', { selector: 'span' })).toBeTruthy()
  const marked = screen.getAllByRole('option').filter(option => /Primær/.test(option.textContent ?? ''))
  expect(marked).toHaveLength(1)
  expect(marked[0]!.getAttribute('value')).toBe(context.websiteId)
})
