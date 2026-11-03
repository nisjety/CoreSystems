// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ChangeWebhooksPanel } from '@/features/ingestions/components/ChangeWebhooksPanel'

const ADDRESS_PLACEHOLDER = 'https://hooks.example.com/verevon'
const SECRET = `whsec_${'f'.repeat(64)}`
const NEW_SECRET = `whsec_${'e'.repeat(64)}`

const subscription = {
  id: 'whk_1',
  url: 'https://hooks.example.com/q',
  events: ['change_detected'],
  active: true,
  description: 'prod',
  secretHint: 'whsec_...ab12',
  createdAt: '2026-10-31T10:00:00Z',
  lastDelivery: null,
}

type Call = { method: string; url: string; body: Record<string, unknown> | undefined }

/** A fake gateway: one subscription in the list, the org's ZDR posture, and the webhook answers. */
function stubApi(options: { zdr?: boolean } = {}): Call[] {
  const calls: Call[] = []
  const json = (body: unknown, status = 200) =>
    new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json' }, status })
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      const method = init?.method ?? 'GET'
      calls.push({ method, url, body: init?.body ? (JSON.parse(String(init.body)) as Record<string, unknown>) : undefined })
      if (url.endsWith('/api/v1/orgs/org_1')) {
        return json({ data: { metadata: { interactiveRetention: { zdr: options.zdr === true } } } })
      }
      if (url.endsWith('/api/ingestions/webhooks') && method === 'GET') {
        return json({ data: { items: [subscription], nextCursor: null } })
      }
      if (url.endsWith('/api/ingestions/webhooks') && method === 'POST') {
        return json({ data: { ...subscription, secret: SECRET } }, 201)
      }
      if (url.endsWith('/rotate-secret') && method === 'POST') {
        return json({ data: { id: 'whk_1', secret: NEW_SECRET, secretHint: 'whsec_...cd34' } })
      }
      if (url.endsWith('/api/ingestions/webhooks/whk_1/deliveries?limit=25')) {
        return json({ data: { items: [], nextCursor: null } })
      }
      return json({ error: { code: 'not_found', message: `Unhandled ${method} ${url}` } }, 404)
    }),
  )
  return calls
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('ChangeWebhooksPanel', () => {
  it('shows a created secret once, in memory, and never writes it to storage', async () => {
    stubApi()
    const setItem = vi.spyOn(Storage.prototype, 'setItem')
    render(() => <ChangeWebhooksPanel orgId="org_1" />)

    const input = await screen.findByPlaceholderText(ADDRESS_PLACEHOLDER)
    fireEvent.input(input, { target: { value: 'https://hooks.example.com/new' } })
    flush()
    fireEvent.submit(input.closest('form')!)
    flush()

    expect(await screen.findByText(SECRET)).toBeTruthy()
    expect(screen.getByText(/Verevon viser den bare én gang/)).toBeTruthy()
    for (const call of setItem.mock.calls) {
      expect(String(call[1])).not.toContain('whsec_')
    }

    fireEvent.click(screen.getByRole('button', { name: 'Jeg har lagret den' }))
    await waitFor(() => expect(screen.queryByText(SECRET)).toBeNull())
  })

  it('hides the create form for a Zero Data Retention organization and keeps the list', async () => {
    stubApi({ zdr: true })
    render(() => <ChangeWebhooksPanel orgId="org_1" />)

    await waitFor(() => expect(screen.queryByPlaceholderText(ADDRESS_PLACEHOLDER)).toBeNull())
    expect(screen.getByText('https://hooks.example.com/q')).toBeTruthy()
    expect(screen.getByText(/Webhooks er ikke tilgjengelig under dataskjerming/)).toBeTruthy()
  })

  it('asks once more before it rotates a secret, and then shows the new one once', async () => {
    const calls = stubApi()
    render(() => <ChangeWebhooksPanel orgId="org_1" />)

    fireEvent.click(await screen.findByRole('button', { name: 'Roter hemmelighet' }))
    flush()
    expect(calls.some((call) => call.url.endsWith('/rotate-secret'))).toBe(false)

    fireEvent.click(await screen.findByRole('button', { name: 'Bekreft ny hemmelighet' }))
    expect(await screen.findByText(NEW_SECRET)).toBeTruthy()
    expect(calls.filter((call) => call.url.endsWith('/rotate-secret'))).toHaveLength(1)
    expect(screen.getByText(/Den gamle hemmeligheten slutter å virke/)).toBeTruthy()
  })
})
