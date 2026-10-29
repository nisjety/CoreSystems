// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { MarketingPulse } from './MarketingPulse.tsx'
import type { Website } from '../../../shared/api/marketing/website-client.ts'
import { sourceKeys } from '@/shared/api/marketing/preparation-client'
vi.mock('./SearchIntelligence.tsx', () => ({ SearchIntelligence: (props: { site: string; websiteId: string; revision: number }) => <span data-testid="registry-search-context">{props.websiteId}:{props.site}:{props.revision}</span> }))
vi.mock('./PageSpeedPanel.tsx', () => ({ PageSpeedPanel: (props: { site: string; websiteId: string; revision: number }) => <span data-testid="registry-pagespeed-context">{props.websiteId}:{props.site}:{props.revision}</span> }))
vi.mock('./PublicSignalsPanel', () => ({ PublicSignalsPanel: (props: { site: string; websiteId: string; revision: number }) => <span data-testid="registry-public-signals-context">{props.websiteId}:{props.site}:{props.revision}</span> }))
vi.mock('./WebVitalsPanel', () => ({ WebVitalsPanel: (props: { site: string; websiteId: string; revision: number }) => <span data-testid="registry-web-vitals-context">{props.websiteId}:{props.site}:{props.revision}</span> }))
vi.mock('./SiteSourcesPanel', () => ({ CookiebotPanel: (props: { site: string; websiteId: string; revision: number }) => <span data-testid="registry-cookiebot-context">{props.websiteId}:{props.site}:{props.revision}</span>, AlgoliaPanel: (props: { site: string; websiteId: string; revision: number }) => <span data-testid="registry-algolia-context">{props.websiteId}:{props.site}:{props.revision}</span> }))
vi.mock('./GA4Panel.tsx', () => ({ GA4Panel: (props: { site: string; websiteId: string; revision: number }) => <span data-testid="registry-ga4-context">{props.websiteId}:{props.site}:{props.revision}</span> }))
vi.mock('./ClarityPanel.tsx', () => ({ ClarityPanel: (props: { site: string; websiteId: string; revision: number }) => <span data-testid="registry-clarity-context">{props.websiteId}:{props.site}:{props.revision}</span> }))
vi.mock('./AnalyticsSyncPanel', () => ({ AnalyticsSyncPanel: () => null }))
const first: Website = { id: 'web_' + '1'.repeat(32), site: 'https://aquatiq.com/', revision: 1, createdAt: '2026-10-21T00:00:00Z', updatedAt: '2026-10-21T00:00:00Z', ownership: { status: 'google_owner' }, integrations: { searchConsole: { status: 'connected', property: 'sc-domain:aquatiq.com' }, ga4: { status: 'requires_consent' }, clarity: { status: 'not_connected' }, pageSpeed: { status: 'available' } } }
const json = (data: unknown) => new Response(JSON.stringify({ data }))
describe('backend-owned website context', () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); localStorage.clear() })
  it('does not treat a legacy browser list as backend authority or request data without a registered site', async () => {
    localStorage.setItem('studio-pulse-site-stack-v1', JSON.stringify({ sites: ['https://aquatiq.com/'], activeSite: 'https://aquatiq.com/' }))
    const fetcher = vi.fn().mockResolvedValue(json({ websites: [] })); vi.stubGlobal('fetch', fetcher)
    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)
    await waitFor(() => expect(screen.getByText(/add a website to get started|legg til et nettsted for å starte/i)).toBeTruthy())
    expect(fetcher).toHaveBeenCalledTimes(1)
    expect(screen.queryByTestId('registry-search-context')).toBeNull()
    expect(screen.queryByText('aquatiq.com')).toBeNull()
  })
  it('changing a registered URL resets primary results and updates both other panels with the same revision', async () => {
    const next: Website = { ...first, site: 'https://example.com/', revision: 2, ownership: { status: 'unverified' }, integrations: { ...first.integrations, searchConsole: { status: 'not_found' } } }
    const fetcher = vi.fn(async (url: string, init?: RequestInit) => {
      if (url === '/api/v1/visibility/websites') return json({ websites: [first] })
      if (init?.method === 'PATCH') return json(next)
      const site = new URL(url, 'http://localhost').searchParams.get('site')
      if (url.includes('/sources?')) return json({ websiteId:first.id, site, websiteRevision:site===first.site?1:2, source:'marketing_source_catalog', items:sourceKeys.map(key=>({key,implemented:true,mode:'public_check',status:key==='search_console'&&site===first.site?'connected':['quarry','pagespeed'].includes(key)?'available':'not_connected',refresh:'manual'})) })
      return json({ site, source: 'google_search_console', status: site === first.site ? 'connected' : 'unavailable', ...(site === first.site ? {} : { reason: 'property_unverified' }), period: { startDate: '2026-10-01', endDate: '2026-10-20' }, totals: site === first.site ? { clicks: 12, impressions: 100 } : null, days: [] })
    })
    vi.stubGlobal('fetch', fetcher)
    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)
    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('12'))
    expect(fetcher.mock.calls[1]?.[0]).toContain('websiteId=' + first.id)
    fireEvent.click(screen.getByRole('button', { name: /websites|nettsider/i }))
    fireEvent.click(screen.getByRole('button', { name: /edit url|endre url/i }))
    fireEvent.input(screen.getByLabelText(/website url|nettadresse/i), { target: { value: 'example.com' } })
    fireEvent.click(screen.getByRole('button', { name: /save url|lagre nettadresse/i }))
    await waitFor(() => expect(screen.getByTestId('registry-pagespeed-context').textContent).toBe(`${next.id}:${next.site}:2`))
    expect(screen.queryByTestId('registry-search-context')).toBeNull()
    expect(screen.getByTestId('registry-pagespeed-context').textContent).toBe(`${next.id}:${next.site}:2`)
    expect(screen.getByTestId('registry-public-signals-context').textContent).toBe(`${next.id}:${next.site}:2`)
    for (const provider of ['web-vitals','cookiebot','algolia','ga4','clarity']) expect(screen.queryByTestId(`registry-${provider}-context`)).toBeNull()
    expect(screen.getByTestId('pulse-primary-value').textContent).toBe('—')
    expect(fetcher.mock.calls.find((call) => call[1]?.method === 'PATCH')?.[1]?.body).toBe(JSON.stringify({ site: next.site, revision: 1 }))
    expect(localStorage.getItem('studio-pulse-active-website-id-v1')).toBe(first.id)
    expect(localStorage.getItem('studio-pulse-site-stack-v1')).toBeNull()
  })
  it('picks up completed onboarding mappings on foreground without remounting unchanged reports', async () => {
    let current = first
    const fetcher=vi.fn(async (url:string) => {
      if (url==='/api/v1/visibility/websites') return json({websites:[current]})
      if (url.includes('/sources?')) return json({websiteId:current.id,site:current.site,websiteRevision:current.revision,source:'marketing_source_catalog',items:sourceKeys.map(key=>({key,implemented:true,mode:'public_check',status:key==='search_console'?'connected':'not_connected',refresh:'manual'}))})
      return json({site:current.site,source:'google_search_console',status:'connected',period:{startDate:'2026-10-01',endDate:'2026-10-20'},totals:{clicks:12,impressions:100},days:[]})
    })
    vi.stubGlobal('fetch',fetcher)
    render(()=><I18nProvider><MarketingPulse/></I18nProvider>)
    await screen.findByTestId('registry-search-context')
    current={...first,revision:2,updatedAt:'2026-10-25T12:00:00Z'}
    fireEvent(window,new Event('focus'))
    await waitFor(()=>expect(screen.getByTestId('registry-search-context').textContent).toBe(`${first.id}:${first.site}:2`))
    const reads=fetcher.mock.calls.filter(([url])=>url.includes('/pulse?')).length
    fireEvent(window,new Event('focus'))
    await waitFor(()=>expect(fetcher.mock.calls.filter(([url])=>url==='/api/v1/visibility/websites')).toHaveLength(3))
    expect(fetcher.mock.calls.filter(([url])=>url.includes('/pulse?'))).toHaveLength(reads)
  })
})
