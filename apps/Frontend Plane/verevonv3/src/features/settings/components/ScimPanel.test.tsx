// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ScimPanel } from '@/features/settings/components/ScimPanel'
import { I18nProvider } from '@/shared/i18n'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// WS05-S8: the SCIM provisioning token panel. fetch is stubbed; every request is recorded so the tests
// can assert what was sent, and that a member sends nothing. The bearer token is a fake literal. It
// must appear only in the one-time box after a create, never in the token list.

const BASE = '/api/v1/orgs/org_1/scim'
const BEARER = 'scim_bearer_xxxxxxxxxxxxxxxxxxxxxxxxxxxx'
const NOT_ENABLED = /SCIM-provisjonering er ikke aktivert for denne organisasjonen\.|SCIM provisioning is not enabled for this organization\./
const ADD_TOKEN = /opprett token|create token/i

const STATUS = { enabled: true, verified_domains: 1, active_tokens: 1, max_tokens: 3, managed_users: 2, active_users: 2 }
const TOKEN = {
  id: 'tok_1',
  name: 'Entra ID',
  prefix: 'scimabcd',
  created_by: 'user_1',
  created_at: '2026-10-01T10:00:00Z',
  expires_at: '2099-06-01T10:00:00Z',
}

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

type Handler = (url: string, init: RequestInit | undefined) => Response | Promise<Response> | undefined

function stubFetch(handler: Handler) {
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => (
    (await handler(String(input), init)) ?? new Response(null, { status: 404 })
  ))
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

/** The happy path: the status, one active token, and 200s for everything else. */
function server(extra?: Handler): Handler {
  return (url, init) => {
    const custom = extra?.(url, init)
    if (custom) return custom
    if (url === `${BASE}/status`) return json({ data: STATUS })
    if (url === `${BASE}/tokens` && !init?.method) return json({ data: { tokens: [TOKEN] } })
    return json({ data: {} })
  }
}

function renderPanel() {
  return render(() => (
    <I18nProvider>
      <ScimPanel />
    </I18nProvider>
  ))
}

function fillAndSubmitName(name: string) {
  fireEvent.click(screen.getByRole('button', { name: ADD_TOKEN }))
  flush()
  fireEvent.input(document.getElementById('scim-token-name') as HTMLInputElement, { target: { value: name } })
  flush()
  fireEvent.submit(screen.getByTestId('scim-add-form'))
  flush()
}

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('ScimPanel', () => {
  it.each(['member', 'viewer'])('a %s sees the plain note and sends no request', async (role) => {
    signIn(role)
    const fetchMock = stubFetch(server())
    renderPanel()

    expect(await screen.findByText(NOT_ENABLED)).toBeTruthy()
    expect(screen.queryByTestId('scim-tokens')).toBeNull()
    expect(screen.queryByTestId('scim-status')).toBeNull()
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('an admin sees the status and the tokens, with the prefix and never a token value', async () => {
    signIn('admin')
    const fetchMock = stubFetch(server())
    renderPanel()

    expect(await screen.findByText('Entra ID (scimabcd…)')).toBeTruthy()
    expect(screen.getByTestId('scim-status').textContent).toBe('1 av 3 tokens aktive. 2 aktive SCIM-brukere.')
    expect(screen.getByText('Aktivt')).toBeTruthy()
    expect(screen.getByTestId('scim-revoke-tok_1')).toBeTruthy()
    expect(calls(fetchMock).map((call) => call.url).sort()).toEqual([`${BASE}/status`, `${BASE}/tokens`])
  })

  it('shows a created token once, with the SCIM address, and clears it when dismissed', async () => {
    signIn('admin')
    const fetchMock = stubFetch(server((url, init) => (
      url === `${BASE}/tokens` && init?.method === 'POST'
        ? json({ data: { token: { ...TOKEN, id: 'tok_2', name: 'Okta', prefix: 'scimwxyz' }, bearer_token: BEARER } }, 201)
        : undefined
    )))
    renderPanel()
    await screen.findByText('Entra ID (scimabcd…)')

    fillAndSubmitName('Okta')

    const issued = await screen.findByTestId('scim-token-issued')
    expect(issued.textContent).toContain(BEARER)
    expect(issued.textContent).toContain('/api/v1/scim/v2')
    const post = calls(fetchMock).find((call) => call.method === 'POST')!
    expect(post.url).toBe(`${BASE}/tokens`)
    expect(JSON.parse(post.body ?? '{}')).toEqual({ name: 'Okta', expires_in_days: 90 })

    fireEvent.click(screen.getByRole('button', { name: /jeg har kopiert det|i have copied it/i }))
    flush()
    expect(screen.queryByTestId('scim-token-issued')).toBeNull()
    expect(document.body.textContent).not.toContain(BEARER)
  })

  it.each([
    [
      'no_verified_domain',
      422,
      /Verifiser minst ett domene før du oppretter et SCIM-token|Verify at least one domain before creating a SCIM token/,
    ],
    [
      'scim_token_limit',
      409,
      /opptil 3 aktive SCIM-tokens|up to 3 active SCIM tokens/,
    ],
  ] as const)('a create refused with %s shows its own copy and issues no token', async (code: string, status: number, copy: RegExp) => {
    signIn('admin')
    stubFetch(server((url, init) => (
      url === `${BASE}/tokens` && init?.method === 'POST'
        ? json({ error: { code, message: 'refused', details: {} } }, status)
        : undefined
    )))
    renderPanel()
    await screen.findByText('Entra ID (scimabcd…)')

    fillAndSubmitName('Okta')

    expect((await screen.findByRole('alert')).textContent).toMatch(copy)
    expect(screen.queryByTestId('scim-token-issued')).toBeNull()
  })

  it.each([['feature_disabled'], ['not_found']])('a %s answer falls back to the plain note and offers no controls', async (code: string) => {
    signIn('owner')
    stubFetch((url) => (
      url === `${BASE}/status` || url === `${BASE}/tokens`
        ? json({ error: { code, message: 'off' } }, 404)
        : undefined
    ))
    renderPanel()

    expect(await screen.findByText(NOT_ENABLED)).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: ADD_TOKEN }))
    flush()
    expect(screen.queryByTestId('scim-add-form')).toBeNull()
    expect(screen.queryByTestId('scim-tokens')).toBeNull()
  })
})
