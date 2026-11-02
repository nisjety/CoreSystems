// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { SsoPolicyPanel } from '@/features/settings/components/SsoPolicyPanel'
import { I18nProvider } from '@/shared/i18n'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// WS05-S7: the single sign-on policy panel. fetch is stubbed; every request is recorded so the tests
// can assert what was sent, that a member sends nothing, and that the client secret never lingers.

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

const PROVIDER = {
  provider_id: 'acme-oidc',
  domain: 'example.com',
  issuer: 'https://idp.example.com',
  type: 'oidc',
  domain_verified: true,
}

function policy(overrides: Record<string, unknown> = {}) {
  return {
    mode: 'report',
    revision: 3,
    domains: ['example.com'],
    break_glass_user_ids: [],
    providers: [PROVIDER],
    can_enforce: { ok: false, reasons: ['no_provider', 'sso_not_proven'] },
    ...overrides,
  }
}

const MEMBERS = {
  members: [
    { userId: 'owner_1', role: 'owner', name: 'Olga Owner' },
    { userId: 'owner_2', role: 'owner', name: 'Otto Owner' },
    { userId: 'member_1', role: 'member', name: 'Mia Member' },
  ],
}

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

/** The standard happy-path server: policy, members, and 200s for writes. */
function server(extra?: Handler): Handler {
  return (url, init) => {
    const custom = extra?.(url, init)
    if (custom) return custom
    if (url.endsWith('/members')) return json({ data: MEMBERS })
    if (url.endsWith('/sso/policy')) return json({ data: policy() })
    return json({ data: {} })
  }
}

function renderPanel() {
  return render(() => (
    <I18nProvider>
      <SsoPolicyPanel />
    </I18nProvider>
  ))
}

const BASE = '/api/v1/orgs/org_1/sso'

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('SsoPolicyPanel', () => {
  it.each(['member', 'viewer'])('a %s sees the plain note and sends no request', async (role) => {
    signIn(role)
    const fetchMock = stubFetch(server())
    renderPanel()

    expect(await screen.findByText(/ikke konfigurert for denne organisasjonen|not configured for this organization/)).toBeTruthy()
    expect(screen.queryByTestId('sso-providers')).toBeNull()
    expect(screen.queryByTestId('sso-policy-save')).toBeNull()
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('feature_disabled falls back to the plain note and offers no controls', async () => {
    signIn('owner')
    stubFetch((url) => {
      if (url.endsWith('/members')) return json({ data: MEMBERS })
      return json({ error: { code: 'feature_disabled', message: 'Identity administration is not enabled.' } }, 404)
    })
    renderPanel()

    expect(await screen.findByText(/ikke konfigurert for denne organisasjonen|not configured for this organization/)).toBeTruthy()
    expect(screen.queryByTestId('sso-policy-save')).toBeNull()
    expect(screen.queryByRole('alert')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /legg til leverandør|add provider/i }))
    flush()
    expect(screen.queryByTestId('sso-provider-form')).toBeNull()
  })

  it('an admin sees the providers and the current policy', async () => {
    signIn('admin')
    const fetchMock = stubFetch(server())
    renderPanel()

    expect(await screen.findByText('acme-oidc')).toBeTruthy()
    expect(screen.getByText(/example\.com · https:\/\/idp\.example\.com/)).toBeTruthy()
    expect((document.getElementById('sso-policy-mode') as HTMLSelectElement).value).toBe('report')
    // Report mode lists no blockers: they matter only once enforce is picked.
    expect(screen.queryByTestId('sso-enforce-blockers')).toBeNull()
    expect(calls(fetchMock).map((call) => call.url).sort()).toEqual([`${BASE}/policy`, '/api/v1/orgs/org_1/members'].sort())
  })

  it('picking enforce lists every blocker from can_enforce', async () => {
    signIn('owner')
    stubFetch(server((url) =>
      url.endsWith('/sso/policy')
        ? json({ data: policy({ can_enforce: { ok: false, reasons: ['no_verified_domain', 'no_provider', 'sso_not_proven', 'break_glass_invalid', 'break_glass_requires_2fa'] } }) })
        : undefined,
    ))
    renderPanel()
    await screen.findByText('acme-oidc')

    fireEvent.change(document.getElementById('sso-policy-mode') as HTMLSelectElement, { target: { value: 'enforce' } })
    flush()

    const blockers = await screen.findByTestId('sso-enforce-blockers')
    for (const copy of [
      /Verifiser et selskapsdomene|Verify a company domain/,
      /identitetsleverandør|identity provider on a verified domain/,
      /Logg inn med SSO selv|Sign in with SSO yourself/,
      /minst én eier|at least one owner/,
      /tofaktorautentisering|two-factor authentication/,
    ]) {
      expect(blockers.textContent).toMatch(copy)
    }
    expect(blockers.querySelectorAll('li')).toHaveLength(5)
  })

  it('no blockers are shown when enforce is possible', async () => {
    signIn('owner')
    stubFetch(server((url) => (url.endsWith('/sso/policy') ? json({ data: policy({ can_enforce: { ok: true, reasons: [] } }) }) : undefined)))
    renderPanel()
    await screen.findByText('acme-oidc')
    fireEvent.change(document.getElementById('sso-policy-mode') as HTMLSelectElement, { target: { value: 'enforce' } })
    flush()
    expect(screen.queryByTestId('sso-enforce-blockers')).toBeNull()
  })

  it('saves the mode, the chosen break-glass owners and the policy revision it loaded', async () => {
    signIn('owner')
    const fetchMock = stubFetch(server())
    renderPanel()
    await screen.findByText('acme-oidc')

    const group = await screen.findByTestId('sso-break-glass')
    await waitFor(() => expect(group.querySelectorAll('input[type="checkbox"]').length).toBe(2))
    // Only owners are offered.
    expect(group.textContent).toContain('Olga Owner')
    expect(group.textContent).not.toContain('Mia Member')
    const boxes = [...group.querySelectorAll('input[type="checkbox"]')] as HTMLInputElement[]
    for (const box of boxes) {
      fireEvent.click(box)
    }
    flush()
    fireEvent.change(document.getElementById('sso-policy-mode') as HTMLSelectElement, { target: { value: 'off' } })
    flush()
    fireEvent.click(screen.getByTestId('sso-policy-save'))
    flush()

    await waitFor(() => expect(calls(fetchMock).some((call) => call.method === 'PUT')).toBe(true))
    const put = calls(fetchMock).find((call) => call.method === 'PUT')!
    expect(put.url).toBe(`${BASE}/policy`)
    expect(JSON.parse(put.body ?? '{}')).toEqual({
      mode: 'off',
      break_glass_user_ids: ['owner_1', 'owner_2'],
      expected_revision: 3,
    })
    expect(await screen.findByText(/SSO-policyen er lagret|SSO policy was saved/)).toBeTruthy()
  })

  it('at most two break-glass owners can be ticked', async () => {
    signIn('owner')
    stubFetch(server((url) =>
      url.endsWith('/members')
        ? json({ data: { members: [1, 2, 3].map((n) => ({ userId: `owner_${n}`, role: 'owner', name: `Owner ${n}` })) } })
        : undefined,
    ))
    renderPanel()
    await screen.findByText('acme-oidc')
    const group = await screen.findByTestId('sso-break-glass')
    await waitFor(() => expect(group.querySelectorAll('input[type="checkbox"]').length).toBe(3))
    const boxes = [...group.querySelectorAll('input[type="checkbox"]')] as HTMLInputElement[]
    for (const box of boxes) {
      fireEvent.click(box)
      flush()
    }
    expect(boxes.filter((box) => box.checked)).toHaveLength(2)
  })

  it.each([
    [409, 'policy_revision_conflict', /endret av en annen|changed by someone else/],
    [422, 'sso_not_proven', /Logg inn med SSO selv|Sign in with SSO yourself/],
    [422, 'break_glass_requires_2fa', /tofaktorautentisering|two-factor authentication/],
    [503, 'identity_unavailable', /ikke tilgjengelig|not available/],
  ])('a %s %s refusal on save shows its own copy', async (status, code, copy) => {
    signIn('owner')
    stubFetch(server((url, init) =>
      init?.method === 'PUT' && url.endsWith('/sso/policy')
        ? json({ error: { code, message: 'refused', details: {} } }, status as number)
        : undefined,
    ))
    renderPanel()
    await screen.findByText('acme-oidc')
    fireEvent.click(screen.getByTestId('sso-policy-save'))
    flush()
    expect((await screen.findByRole('alert')).textContent).toMatch(copy as RegExp)
  })

  it('clears the client secret after a provider is added and never renders it', async () => {
    signIn('admin')
    const secret = 'client-secret-ssssssssssssssss'
    const fetchMock = stubFetch(server((url, init) =>
      init?.method === 'POST' && url.endsWith('/sso/providers')
        ? json({ data: { ...PROVIDER, provider_id: 'new-idp' } }, 201)
        : undefined,
    ))
    renderPanel()
    await screen.findByText('acme-oidc')

    const open = () => {
      fireEvent.click(screen.getByRole('button', { name: /legg til leverandør|add provider/i }))
      flush()
    }
    open()
    const type = (id: string, value: string) => {
      fireEvent.input(document.getElementById(id) as HTMLInputElement, { target: { value } })
      flush()
    }
    type('sso-provider-id', 'new-idp')
    type('sso-provider-domain', 'example.com')
    type('sso-provider-issuer', 'https://idp.example.com')
    type('sso-provider-client-id', 'client-1')
    type('sso-provider-client-secret', secret)
    fireEvent.submit(screen.getByTestId('sso-provider-form'))
    flush()

    await waitFor(() => expect(calls(fetchMock).some((call) => call.method === 'POST')).toBe(true))
    const post = calls(fetchMock).find((call) => call.method === 'POST')!
    expect(post.url).toBe(`${BASE}/providers`)
    expect(JSON.parse(post.body ?? '{}')).toEqual({
      provider_id: 'new-idp',
      domain: 'example.com',
      type: 'oidc',
      issuer: 'https://idp.example.com',
      client_id: 'client-1',
      client_secret: secret,
    })

    // The form closes and the draft is reset: reopening shows every field, the secret included, empty.
    await waitFor(() => expect(screen.queryByTestId('sso-provider-form')).toBeNull())
    expect(await screen.findByText(/Leverandøren er lagt til|provider was added/)).toBeTruthy()
    expect(document.body.textContent).not.toContain(secret)
    open()
    for (const id of ['sso-provider-id', 'sso-provider-client-id', 'sso-provider-client-secret']) {
      expect((document.getElementById(id) as HTMLInputElement).value, id).toBe('')
    }
    expect(document.body.innerHTML).not.toContain(secret)
  })

  it('keeps the form open when the provider is refused and shows the reason', async () => {
    signIn('admin')
    stubFetch(server((url, init) =>
      init?.method === 'POST' && url.endsWith('/sso/providers')
        ? json({ error: { code: 'invalid_issuer', message: 'bad issuer', details: {} } }, 400)
        : undefined,
    ))
    renderPanel()
    await screen.findByText('acme-oidc')
    fireEvent.click(screen.getByRole('button', { name: /legg til leverandør|add provider/i }))
    flush()
    fireEvent.input(document.getElementById('sso-provider-issuer') as HTMLInputElement, { target: { value: 'http://idp.example.com' } })
    flush()
    fireEvent.submit(screen.getByTestId('sso-provider-form'))
    flush()
    expect((await screen.findByRole('alert')).textContent).toMatch(/https-adresse|https address/)
    expect(screen.getByTestId('sso-provider-form')).toBeTruthy()
  })

  it('removes a provider and explains a refusal for the last one under enforce', async () => {
    signIn('owner')
    const fetchMock = stubFetch(server((url, init) =>
      init?.method === 'DELETE'
        ? json({ error: { code: 'last_provider_enforced', message: 'last', details: {} } }, 409)
        : undefined,
    ))
    renderPanel()
    fireEvent.click(await screen.findByTestId('sso-provider-remove-acme-oidc'))
    flush()
    expect((await screen.findByRole('alert')).textContent).toMatch(/siste leverandøren|last provider/)
    expect(calls(fetchMock).find((call) => call.method === 'DELETE')).toEqual({
      url: `${BASE}/providers/acme-oidc`,
      method: 'DELETE',
      body: undefined,
    })
    expect(screen.getByTestId('sso-provider-remove-acme-oidc')).toBeTruthy()
  })
})
