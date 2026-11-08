// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { DomainsPanel } from '@/features/settings/components/DomainsPanel'
import { I18nProvider } from '@/shared/i18n'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// WS05-S6: the verified-domains panel. fetch is stubbed; every request is recorded so the tests can
// assert what was sent (and that a member sends nothing).

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function signIn(role: string) {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role })
  flush()
}

const CHALLENGE = {
  type: 'dns_txt',
  name: '_verevon-challenge.example.com',
  value: 'verevon-site-verification=tokentoken0123',
  expires_at: '2026-12-01T00:00:00Z',
}

const PENDING = { domain: 'example.com', status: 'pending', auto_invite_enabled: false, challenge: CHALLENGE }

type Handler = (url: string, init: RequestInit | undefined) => Response | Promise<Response>

function stubFetch(handler: Handler) {
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => handler(String(input), init))
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function calls(fetchMock: ReturnType<typeof vi.fn>) {
  return fetchMock.mock.calls.map((call) => ({
    url: String(call[0]),
    method: String((call[1] as RequestInit | undefined)?.method ?? 'GET').toUpperCase(),
    body: (call[1] as RequestInit | undefined)?.body as string | undefined,
  }))
}

function renderPanel() {
  return render(() => (
    <I18nProvider>
      <DomainsPanel />
    </I18nProvider>
  ))
}

const LIST_URL = '/api/v1/orgs/org_1/domains'

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('DomainsPanel', () => {
  it.each(['member', 'viewer'])('a %s sees the plain note and sends no request', async (role) => {
    signIn(role)
    const fetchMock = stubFetch(() => json({ data: { domains: [PENDING] } }))
    renderPanel()

    expect(await screen.findByText(/Ingen domener er verifisert|No domains have been verified/)).toBeTruthy()
    expect(screen.queryByTestId('domains-rows')).toBeNull()
    expect(screen.queryByTestId('domain-verify-example.com')).toBeNull()
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('an admin sees the pending challenge with the DNS name and value to publish', async () => {
    signIn('admin')
    const fetchMock = stubFetch(() => json({ data: { domains: [PENDING, { domain: 'done.example', status: 'verified', auto_invite_enabled: false, verified_at: '2026-10-01T00:00:00Z' }] } }))
    renderPanel()

    const challenge = await screen.findByTestId('domain-challenge-example.com')
    expect(challenge.textContent).toContain('_verevon-challenge.example.com')
    expect(challenge.textContent).toContain('verevon-site-verification=tokentoken0123')
    // A verified domain has no challenge and no verify button; both can be removed.
    expect(screen.queryByTestId('domain-challenge-done.example')).toBeNull()
    expect(screen.queryByTestId('domain-verify-done.example')).toBeNull()
    expect(screen.getByTestId('domain-verify-example.com')).toBeTruthy()
    expect(screen.getByTestId('domain-revoke-done.example')).toBeTruthy()
    expect(calls(fetchMock)).toEqual([{ url: LIST_URL, method: 'GET', body: undefined }])
  })

  it.each([
    ['txt_not_found', /Fant ikke TXT|TXT record was not found/],
    ['txt_mismatch', /verdien stemmer ikke|does not match/],
    ['dns_unavailable', /DNS svarte ikke|did not answer/],
    ['challenge_expired', /utløpt|expired/],
  ])('verify with reason %s shows its own copy', async (reason, copy) => {
    signIn('owner')
    const fetchMock = stubFetch((url, init) => {
      if (init?.method === 'POST' && url.endsWith('/verify')) {
        return json({ data: { domain: 'example.com', status: 'pending', checked_at: '2026-10-01T00:00:00Z', reason } })
      }
      return json({ data: { domains: [PENDING] } })
    })
    renderPanel()
    fireEvent.click(await screen.findByTestId('domain-verify-example.com'))
    flush()

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toMatch(copy)
    const verify = calls(fetchMock).find((call) => call.method === 'POST')
    expect(verify).toEqual({ url: `${LIST_URL}/example.com/verify`, method: 'POST', body: undefined })
  })

  it('a verified result shows the notice and reloads the list', async () => {
    signIn('admin')
    let verified = false
    const fetchMock = stubFetch((url, init) => {
      if (init?.method === 'POST' && url.endsWith('/verify')) {
        verified = true
        return json({ data: { domain: 'example.com', status: 'verified', checked_at: '2026-10-01T00:00:00Z' } })
      }
      return json({ data: { domains: [verified ? { domain: 'example.com', status: 'verified', auto_invite_enabled: false } : PENDING] } })
    })
    renderPanel()
    fireEvent.click(await screen.findByTestId('domain-verify-example.com'))
    flush()

    expect(await screen.findByText(/er verifisert|is verified/)).toBeTruthy()
    await waitFor(() => expect(screen.queryByTestId('domain-verify-example.com')).toBeNull())
    expect(calls(fetchMock).filter((call) => call.method === 'GET')).toHaveLength(2)
  })

  it('feature_disabled falls back to the plain note and offers no actions', async () => {
    signIn('owner')
    stubFetch(() => json({ error: { code: 'feature_disabled', message: 'Identity administration is not enabled.' } }, 404))
    renderPanel()

    expect(await screen.findByText(/Ingen domener er verifisert|No domains have been verified/)).toBeTruthy()
    expect(screen.queryByTestId('domains-rows')).toBeNull()
    expect(screen.queryByRole('alert')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /legg til domene|add domain/i }))
    flush()
    expect(screen.queryByTestId('domains-add-form')).toBeNull()
  })

  it('adds a domain by sending only the typed domain', async () => {
    signIn('admin')
    const fetchMock = stubFetch((_url, init) => {
      if (init?.method === 'POST') return json({ data: PENDING }, 201)
      return json({ data: { domains: [] } })
    })
    renderPanel()
    expect(await screen.findByTestId('domains-empty')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /legg til domene|add domain/i }))
    flush()
    const input = document.getElementById('org-domain-new') as HTMLInputElement
    fireEvent.input(input, { target: { value: '  example.com ' } })
    flush()
    fireEvent.submit(screen.getByTestId('domains-add-form'))
    flush()

    await waitFor(() => expect(calls(fetchMock).some((call) => call.method === 'POST')).toBe(true))
    const post = calls(fetchMock).find((call) => call.method === 'POST')!
    expect(post.url).toBe(LIST_URL)
    expect(JSON.parse(post.body ?? '{}')).toEqual({ domain: 'example.com' })
  })

  it.each([
    ['invalid_domain', { reason: 'free_mail' }, 400, /Gmail|Free mail/],
    ['invalid_domain', { reason: 'ip_literal' }, 400, /IP-adresse|IP address/],
    ['invalid_domain', { reason: 'something_new' }, 400, /ikke gyldig|not valid/],
    ['domain_verified_elsewhere', {}, 409, /allerede verifisert av en annen|already verified by another/],
    ['domain_limit_reached', { max: 10 }, 409, /10 domener|10 domains/],
  ])('an add refused with %s %o shows its own copy', async (code, details, status, copy) => {
    signIn('admin')
    stubFetch((_url, init) => {
      if (init?.method === 'POST') return json({ error: { code, message: 'refused', details } }, status as number)
      return json({ data: { domains: [] } })
    })
    renderPanel()
    expect(await screen.findByTestId('domains-empty')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /legg til domene|add domain/i }))
    flush()
    fireEvent.input(document.getElementById('org-domain-new') as HTMLInputElement, { target: { value: 'gmail.com' } })
    flush()
    fireEvent.submit(screen.getByTestId('domains-add-form'))
    flush()

    expect((await screen.findByRole('alert')).textContent).toMatch(copy as RegExp)
  })

  it('removing a domain that backs an enforced SSO policy explains why it stays', async () => {
    signIn('owner')
    const fetchMock = stubFetch((_url, init) => {
      if (init?.method === 'DELETE') return json({ error: { code: 'domain_in_use', message: 'in use', details: {} } }, 409)
      return json({ data: { domains: [{ domain: 'example.com', status: 'verified', auto_invite_enabled: false }] } })
    })
    renderPanel()
    fireEvent.click(await screen.findByTestId('domain-revoke-example.com'))
    flush()

    expect((await screen.findByRole('alert')).textContent).toMatch(/påkrevde single sign-on|enforced single sign-on/)
    expect(calls(fetchMock).find((call) => call.method === 'DELETE')).toEqual({ url: `${LIST_URL}/example.com`, method: 'DELETE', body: undefined })
    // The row is still listed.
    expect(screen.getByTestId('domain-revoke-example.com')).toBeTruthy()
  })
})
