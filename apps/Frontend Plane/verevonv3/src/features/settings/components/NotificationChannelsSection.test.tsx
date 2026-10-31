// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { NotificationChannelsSection } from '@/features/settings/components/NotificationChannelsSection'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

const execution = {
  actionId: 'notifications.update_preference',
  runId: 'run-1',
  status: 'completed',
  auditId: 'audit-1',
  eventStream: '',
}

// Answers by method and path, the way the settings page calls the gateway.
function routedFetch(routes: Record<string, () => Response>) {
  return vi.fn(async (input: unknown, init?: RequestInit) => {
    const handler = routes[`${init?.method ?? 'GET'} ${String(input)}`]
    return handler ? handler() : jsonResponse({}, 404)
  })
}

// jsdom has no push APIs and is not a secure context, so a test that means "a capable browser" says so.
function capableBrowser() {
  Object.defineProperty(window, 'isSecureContext', { value: true, configurable: true })
  vi.stubGlobal('navigator', { ...navigator, serviceWorker: {} })
  vi.stubGlobal('PushManager', class PushManager {})
  vi.stubGlobal('Notification', class Notification {})
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
  Object.defineProperty(window, 'isSecureContext', { value: false, configurable: true })
})

describe('NotificationChannelsSection', () => {
  it('renders the saved channel choices and keeps push unavailable while the server has it off', async () => {
    capableBrowser()
    vi.stubGlobal('fetch', routedFetch({
      'GET /api/v1/notifications/preferences': () =>
        jsonResponse({
          preferences: [{ event_type: 'modelplane.needs_approval', channel: 'in_app', enabled: false }],
        }),
      'GET /api/v1/push/config': () => jsonResponse({ enabled: false }),
    }))

    render(() => <NotificationChannelsSection />)

    const inApp = screen.getByRole('switch', { name: 'Godkjenning kreves · I Verevon' })
    await screen.findByText('Push er ikke slått på på serveren ennå.')
    await waitFor(() => expect(inApp.getAttribute('aria-checked')).toBe('false'))
    expect((screen.getByRole('switch', { name: 'Godkjenning kreves · Push' }) as HTMLInputElement).disabled).toBe(true)
    expect(screen.getByRole('switch', { name: 'Plan klar · I Verevon' }).getAttribute('aria-checked')).toBe('true')
  })

  it('saves a channel change through notifications.update_preference', async () => {
    const fetchMock = routedFetch({
      'GET /api/v1/notifications/preferences': () => jsonResponse({ preferences: [] }),
      'GET /api/v1/push/config': () => jsonResponse({ enabled: false }),
      'POST /api/v1/actions/execute': () => jsonResponse(execution),
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <NotificationChannelsSection />)
    fireEvent.click(await screen.findByRole('switch', { name: 'Godkjenning kreves · I Verevon' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([path]) => path === '/api/v1/actions/execute')).toBe(true))
    const call = fetchMock.mock.calls.find(([path]) => path === '/api/v1/actions/execute')
    const body = JSON.parse(String((call?.[1] as RequestInit).body))
    expect(body.actionId).toBe('notifications.update_preference')
    expect(body.input).toEqual({ eventType: 'modelplane.needs_approval', channel: 'in_app', enabled: false })
  })

  it('puts a refused change back and says so', async () => {
    vi.stubGlobal('fetch', routedFetch({
      'GET /api/v1/notifications/preferences': () => jsonResponse({ preferences: [] }),
      'GET /api/v1/push/config': () => jsonResponse({ enabled: false }),
      'POST /api/v1/actions/execute': () => jsonResponse({ error: { code: 'internal', message: 'no' } }, 500),
    }))

    render(() => <NotificationChannelsSection />)
    const inApp = await screen.findByRole('switch', { name: 'Godkjenning kreves · I Verevon' })
    fireEvent.click(inApp)

    expect(await screen.findByText('Kunne ikke lagre varselinnstillingen.')).toBeTruthy()
    await waitFor(() => expect(inApp.getAttribute('aria-checked')).toBe('true'))
  })
})
