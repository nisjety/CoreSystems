// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { WebsiteManager } from './WebsiteManager.tsx'
import type { Website } from '../../../shared/api/marketing/website-client.ts'
import { createSignal } from 'solid-js'
// The child has its own network/permission suite; isolate this registry unit suite.
vi.mock('./WebsitePreparationPanel', () => ({ WebsitePreparationPanel: (props: { websiteId: string; site: string; revision: number; canManageIntegrations: boolean }) => <div data-testid="preparation-context" data-website={props.websiteId} data-site={props.site} data-revision={props.revision} data-can-manage={String(props.canManageIntegrations)} /> }))
const website: Website = {
  id: 'web_' + '1'.repeat(32), site: 'https://aquatiq.com/', revision: 1,
  createdAt: '2026-10-21T00:00:00Z', updatedAt: '2026-10-21T00:00:00Z',
  ownership: { status: 'unverified' }, integrations: { searchConsole: { status: 'connected', accessLevel: 'siteFullUser', property: 'sc-domain:aquatiq.com' }, ga4: { status: 'requires_consent' }, clarity: { status: 'not_connected' }, pageSpeed: { status: 'available' } },
}
describe('website manager', () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals() })
  it('lets nonadmins select and read websites without exposing integration write forms', () => {
    const onSelect = vi.fn()
    const fetcher = vi.fn(); vi.stubGlobal('fetch', fetcher)
    const owner: Website = { ...website, ownership: { status: 'google_owner' }, integrations: { ...website.integrations, searchConsole: { status: 'connected', property: 'sc-domain:aquatiq.com', accessLevel: 'siteOwner' } } }
    const { container } = render(() => <I18nProvider><WebsiteManager canManageIntegrations={false} websites={[owner]} selected={owner} onSelect={onSelect} onUpdated={() => undefined} onClose={() => undefined} /></I18nProvider>)
    fireEvent.click(screen.getByRole('button', { name: 'aquatiq.com' }))
    expect(onSelect).toHaveBeenCalledWith(owner.id)
    expect(screen.getByTestId('preparation-context').dataset).toMatchObject({ website: owner.id, site: owner.site, revision: '1', canManage: 'false' })
    expect(screen.getByTestId('website-searchConsole').textContent).toMatch(/connected|tilkoblet/i)
    expect(screen.getByTestId('ga4-current-reporting-host').textContent).toContain('aquatiq.com')
    expect(screen.getByText(/administrator.*integrasjon|administrator.*integration/i)).toBeTruthy()
    expect(container.querySelector('form')).toBeNull()
    expect(screen.queryByRole('button', { name: /connect Google|koble til Google/i })).toBeNull()
    expect(screen.queryByRole('button', { name: /edit URL|endre URL/i })).toBeNull()
    expect(fetcher).not.toHaveBeenCalled()
  })
  it('aborts a write and rejects stale form handlers when administrator access is revoked', async () => {
    const [allowed, setAllowed] = createSignal(true)
    const onUpdated = vi.fn()
    let resolve: ((value: Response) => void) | undefined
    const fetcher = vi.fn().mockImplementation(() => new Promise<Response>((done) => { resolve = done }))
    vi.stubGlobal('fetch', fetcher)
    const { container } = render(() => <I18nProvider><WebsiteManager canManageIntegrations={allowed()} websites={[]} onSelect={() => undefined} onUpdated={onUpdated} onClose={() => undefined} /></I18nProvider>)
    const form = container.querySelector('form')!
    fireEvent.input(screen.getByLabelText(/website url|nettadresse/i), { target: { value: 'aquatiq.com' } })
    fireEvent.submit(form)
    await waitFor(() => expect(fetcher).toHaveBeenCalledOnce())
    const signal = fetcher.mock.calls[0]?.[1]?.signal as AbortSignal
    setAllowed(false)
    await waitFor(() => expect(signal.aborted).toBe(true))
    fireEvent.submit(form)
    expect(fetcher).toHaveBeenCalledOnce()
    resolve?.(new Response(JSON.stringify({ data: website })))
    await new Promise((done) => setTimeout(done, 0))
    expect(onUpdated).not.toHaveBeenCalled()
    expect(container.querySelector('form')).toBeNull()
  })
  it('distinguishes reporting access from ownership and does not automatically start consent or provider creation', () => {
    const fetcher = vi.fn(); vi.stubGlobal('fetch', fetcher)
    render(() => <I18nProvider><WebsiteManager websites={[website]} selected={website} onSelect={() => undefined} onUpdated={() => undefined} onClose={() => undefined} /></I18nProvider>)
    expect(screen.queryByTestId('website-ownership')).toBeNull()
    expect(screen.getByTestId('website-searchConsole').textContent).toMatch(/connected|tilkoblet/i)
    expect(screen.getByTestId('website-ga4').textContent).toMatch(/consent|samtykke/i)
    expect(screen.queryByLabelText(/GA4 reporting host|GA4-rapporteringsvert/i)).toBeNull()
    expect(fetcher).not.toHaveBeenCalled()
  })
  it('lets only the current Google site owner select the website or exact www twin with a revision-bound command', async () => {
    const owner: Website = {
      ...website,
      ownership: { status: 'google_owner' },
      integrations: {
        ...website.integrations,
        searchConsole: { status: 'connected', property: 'sc-domain:aquatiq.com', accessLevel: 'siteOwner' },
        ga4: { status: 'connected', propertyId: '123', reportingSite: 'https://www.aquatiq.com/' },
      },
    }
    const mapped = { ...owner, revision: 2, integrations: { ...owner.integrations, ga4: { ...owner.integrations.ga4, reportingSite: owner.site } } }
    const onUpdated = vi.fn()
    const fetcher = vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: mapped })))
    vi.stubGlobal('fetch', fetcher)
    render(() => <I18nProvider><WebsiteManager websites={[owner]} selected={owner} onSelect={() => undefined} onUpdated={onUpdated} onClose={() => undefined} /></I18nProvider>)
    expect(screen.getByTestId('ga4-current-reporting-host').textContent).toContain('www.aquatiq.com')
    const select = screen.getByLabelText(/GA4 reporting host|GA4-rapporteringsvert/i) as HTMLSelectElement
    expect(Array.from(select.options).map((option) => option.value)).toEqual([owner.site, 'https://www.aquatiq.com/'])
    fireEvent.change(select, { target: { value: owner.site } })
    fireEvent.click(screen.getByRole('button', { name: /save GA4 reporting host|lagre GA4-rapporteringsvert/i }))
    await waitFor(() => expect(onUpdated).toHaveBeenCalledWith(mapped))
    expect(fetcher.mock.calls[0]).toMatchObject([`/api/v1/visibility/websites/${owner.id}/mappings`, { method: 'PATCH', body: JSON.stringify({ ga4ReportingSite: owner.site, revision: owner.revision }) }])
  })
  it('requires an exact Search Console domain-owner proof before rendering GA4 host controls', () => {
    const ownerWithAnotherDomain: Website = {
      ...website,
      ownership: { status: 'google_owner' },
      integrations: { ...website.integrations, searchConsole: { status: 'connected', property: 'sc-domain:other.example', accessLevel: 'siteOwner' } },
    }
    const fetcher = vi.fn(); vi.stubGlobal('fetch', fetcher)
    render(() => <I18nProvider><WebsiteManager websites={[ownerWithAnotherDomain]} selected={ownerWithAnotherDomain} onSelect={() => undefined} onUpdated={() => undefined} onClose={() => undefined} /></I18nProvider>)
    expect(screen.queryByLabelText(/GA4 reporting host|GA4-rapporteringsvert/i)).toBeNull()
    expect(fetcher).not.toHaveBeenCalled()
  })
  it('shows the compare-and-retry message when a GA4 reporting-host revision conflicts', async () => {
    const owner: Website = {
      ...website,
      ownership: { status: 'google_owner' },
      integrations: { ...website.integrations, searchConsole: { status: 'connected', property: 'sc-domain:aquatiq.com', accessLevel: 'siteOwner' }, ga4: {status:'connected',propertyId:'123'} },
    }
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('{}', { status: 409 })))
    render(() => <I18nProvider><WebsiteManager websites={[owner]} selected={owner} onSelect={() => undefined} onUpdated={() => undefined} onClose={() => undefined} /></I18nProvider>)
    fireEvent.click(screen.getByRole('button', { name: /save GA4 reporting host|lagre GA4-rapporteringsvert/i }))
    expect((await screen.findByRole('alert')).textContent).toMatch(/changed|endret/i)
  })
  it('ignores a late reporting-host result after website selection changes', async () => {
    const owner: Website = { ...website, ownership: { status: 'unverified' }, integrations: { ...website.integrations, searchConsole: { status: 'connected', property: 'sc-domain:aquatiq.com', accessLevel: 'siteFullUser' }, ga4: { status: 'connected', propertyId:'123' } } }
    const other: Website = { ...owner, id: 'web_' + '2'.repeat(32), site: 'https://example.com/' }
    let resolve: ((value: Response) => void) | undefined
    vi.stubGlobal('fetch', vi.fn().mockImplementation(() => new Promise<Response>((done) => { resolve = done })))
    const onUpdated = vi.fn()
    const [selected, setSelected] = createSignal(owner)
    render(() => <I18nProvider><WebsiteManager websites={[owner, other]} selected={selected()} onSelect={() => undefined} onUpdated={onUpdated} onClose={() => undefined} /></I18nProvider>)
    fireEvent.click(screen.getByRole('button', { name: /save GA4 reporting host|lagre GA4-rapporteringsvert/i }))
    await waitFor(() => expect(fetch).toHaveBeenCalledOnce())
    setSelected(other)
    resolve?.(new Response(JSON.stringify({ data: { ...owner, revision: 2 } })))
    await new Promise((done) => setTimeout(done, 0))
    expect(onUpdated).not.toHaveBeenCalled()
  })
  it('adds a URL to the backend rather than storing a website list in the browser', async () => {
    const onUpdated = vi.fn()
    const fetcher = vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: website })))
    vi.stubGlobal('fetch', fetcher)
    render(() => <I18nProvider><WebsiteManager websites={[]} onSelect={() => undefined} onUpdated={onUpdated} onClose={() => undefined} /></I18nProvider>)
    fireEvent.input(screen.getByLabelText(/website url|nettadresse/i), { target: { value: 'aquatiq.com' } })
    fireEvent.click(screen.getByRole('button', { name: /add website|legg til nettsted/i }))
    await waitFor(() => expect(onUpdated).toHaveBeenCalledWith(website))
    expect(fetcher.mock.calls[0]).toMatchObject(['/api/v1/visibility/websites', { method: 'POST', body: JSON.stringify({ site: website.site }) }])
    expect(localStorage.getItem('studio-pulse-site-stack-v1')).toBeNull()
  })
})
