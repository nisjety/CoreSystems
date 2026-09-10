// @vitest-environment jsdom
//
// Settings → Integrations must offer the same recovery as the onboarding
// connect step when integration-core refuses a Microsoft sync with
// `409 no_sources_registered`: that is the normal state of a connection with
// no SharePoint/OneDrive library registered yet, not a fault, so the row
// offers the library picker instead of a generic "action failed".
import { createRouter, memoryHistory } from '@solidjs/router'
import { QueryClient, QueryClientProvider } from '@tanstack/solid-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import type { JSX } from '@solidjs/web'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { VerevonWorkspaceSettingsPage } from '@/features/settings/components/WorkspaceSettingsPage'

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-aquatiq', name: 'Aquatiq AS', role: 'owner' } }),
}))

// Labels are matched in both locales: the page renders under whichever locale
// the ambient i18n context resolves to.
const SYNC_BUTTON = /^(Synkroniser|Sync)$/
const ADD_LIBRARY_BUTTON = /^(Legg til bibliotek|Add library)$/
const PICKER_GROUP = /Velg SharePoint-bibliotek|Pick SharePoint library/
const SKIP_BUTTON = /Velg senere under Kunnskap|Choose later under Knowledge/

const microsoftProvider = {
  key: 'microsoft',
  label: 'Microsoft 365',
  category: 'productivity',
  configured: true,
  status: 'ready',
  directOAuthReady: true,
  capabilities: [
    { key: 'sharepoint.read', label: 'SharePoint and OneDrive read', direction: 'read' },
    { key: 'mail.read', label: 'Outlook read', direction: 'read' },
  ],
}

const microsoftConnection = {
  id: 'conn_f019f69a',
  providerKey: 'microsoft',
  providerLabel: 'Microsoft 365',
  displayName: 'Ima Fernandes Da Costa',
  providerEmail: 'ima.dacosta@aquatiq.com',
  status: 'active',
  capabilities: ['profile.read', 'sharepoint.read', 'mail.read'],
  scopes: ['Files.Read.All', 'Sites.Read.All', 'Mail.Read'],
  createdAt: '2026-09-04T01:00:00.000Z',
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

/** Fetch double for the integrations section plus the Knowledge SharePoint
 * routes the picker uses. `syncResponses` is consumed one call at a time so a
 * test can refuse the first sync and accept the retry. */
function stubIntegrationsFetch(options: { syncResponses: Array<() => Response> }) {
  const syncQueue = [...options.syncResponses]
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    const method = init?.method ?? 'GET'

    if (url.endsWith('/api/v1/integrations/providers')) {
      return json({ success: true, data: { providers: [microsoftProvider] } })
    }
    if (url.endsWith('/api/v1/integrations/connections')) {
      return json({ success: true, data: { connections: [microsoftConnection] } })
    }
    if (url.includes('/api/v1/integrations/connections/') && url.endsWith('/sync') && method === 'POST') {
      const next = syncQueue.shift()
      if (!next) throw new Error(`unexpected extra sync call: ${url}`)
      return next()
    }
    if (url.endsWith('/api/v1/knowledge/sharepoint/sites')) {
      return json({
        success: true,
        data: {
          count: 2,
          sites: [
            { id: 'site-1', name: 'aquatiq.sharepoint.com', display_name: 'Aquatiq AS', web_url: 'https://aquatiq.sharepoint.com' },
            { id: 'site-2', name: 'Support', web_url: 'https://aquatiq.sharepoint.com/sites/support' },
          ],
        },
      })
    }
    if (url.includes('/api/v1/knowledge/sharepoint/sites/') && url.endsWith('/drives')) {
      return json({
        success: true,
        data: {
          count: 1,
          drives: [{ id: 'drive-1', name: 'Dokumenter', drive_type: 'documentLibrary' }],
        },
      })
    }
    if (url.endsWith('/api/v1/knowledge/sharepoint') && method === 'POST') {
      return json({ success: true, data: { id: 'finspo-src-1', syncStarted: true } })
    }
    return json({})
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function renderIntegrationsSection() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const component = (): JSX.Element => <VerevonWorkspaceSettingsPage section="integrations" />
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component }],
    history: memoryHistory('/settings/integrations'),
    explicitLinks: true,
  })
  return render(() => (
    <QueryClientProvider client={queryClient}>
      <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
    </QueryClientProvider>
  ))
}

const refusedSync = () => json({
  success: false,
  error: {
    code: 'no_sources_registered',
    message: 'No SharePoint or OneDrive library is registered for this organization yet.',
  },
}, 409)

const acceptedSync = () => json({
  success: true,
  data: { syncJob: { id: 'sync_1', connectionId: 'conn_f019f69a', status: 'waiting_provider' } },
}, 202)

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('Settings → Integrations library picker', () => {
  it('offers the picker on a refused Microsoft sync, registers the library, and retries the sync', async () => {
    const fetchMock = stubIntegrationsFetch({ syncResponses: [refusedSync, acceptedSync] })
    renderIntegrationsSection()

    const syncButton = await screen.findByRole('button', { name: SYNC_BUTTON })
    fireEvent.click(syncButton)

    // The refusal is not reported as a failure: it names the next step and
    // opens the picker on the affected row.
    const picker = await screen.findByRole('group', { name: PICKER_GROUP })
    expect(picker).toBeTruthy()
    expect(screen.getByRole('status').textContent).toMatch(/bibliotek|library/i)
    expect(screen.queryByText(/Integrasjonshandlingen mislyktes|Integration action failed/i)).toBeNull()

    // The first site's first document library is preselected, so registering
    // is a single click.
    const addLibrary = await screen.findByRole('button', { name: ADD_LIBRARY_BUTTON })
    await waitFor(() => expect(addLibrary.hasAttribute('disabled')).toBe(false))
    fireEvent.click(addLibrary)

    await waitFor(() => {
      const registerCall = fetchMock.mock.calls.find(([input, init]) =>
        String(input).endsWith('/api/v1/knowledge/sharepoint') && init?.method === 'POST')
      expect(registerCall).toBeTruthy()
      expect(JSON.parse(String(registerCall?.[1]?.body))).toMatchObject({
        kind: 'drive',
        siteId: 'site-1',
        driveId: 'drive-1',
        driveName: 'Dokumenter',
        driveType: 'documentLibrary',
      })
    })

    // Registering a library is what makes the refused sync acceptable, so it
    // is retried immediately rather than left for the user to click again.
    await waitFor(() => {
      const syncCalls = fetchMock.mock.calls.filter(([input, init]) =>
        String(input).includes('/api/v1/integrations/connections/') && String(input).endsWith('/sync') && init?.method === 'POST')
      expect(syncCalls).toHaveLength(2)
    })
    await waitFor(() => expect(screen.queryByRole('group', { name: PICKER_GROUP })).toBeNull())
  })

  it('lets the operator defer to Knowledge, which closes the picker without registering', async () => {
    const fetchMock = stubIntegrationsFetch({ syncResponses: [refusedSync] })
    renderIntegrationsSection()

    fireEvent.click(await screen.findByRole('button', { name: SYNC_BUTTON }))
    await screen.findByRole('group', { name: PICKER_GROUP })

    fireEvent.click(await screen.findByRole('button', { name: SKIP_BUTTON }))

    await waitFor(() => expect(screen.queryByRole('group', { name: PICKER_GROUP })).toBeNull())
    expect(screen.getByRole('status').textContent).toMatch(/Kunnskap|Knowledge/)
    expect(fetchMock.mock.calls.some(([input, init]) =>
      String(input).endsWith('/api/v1/knowledge/sharepoint') && init?.method === 'POST')).toBe(false)
  })

  // The allow-list is deliberately narrow: any other integration failure must
  // still read as a failure, with no picker offered.
  it('keeps reporting an unrelated sync failure as a failure', async () => {
    const failedSync = () => json({
      success: false,
      error: { code: 'integration_error', message: 'Sync could not be queued.' },
    }, 500)
    stubIntegrationsFetch({ syncResponses: [failedSync] })
    renderIntegrationsSection()

    fireEvent.click(await screen.findByRole('button', { name: SYNC_BUTTON }))

    await waitFor(() => expect(screen.getByRole('status').textContent).toMatch(/Sync could not be queued/i))
    expect(screen.queryByRole('group', { name: PICKER_GROUP })).toBeNull()
  })
})
