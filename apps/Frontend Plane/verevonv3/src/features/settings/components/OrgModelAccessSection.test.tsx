// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { OrgModelAccessSection, parseModelEntries } from '@/features/settings/components/OrgModelAccessSection'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function signIn(role: 'owner' | 'admin' | 'member') {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role })
  flush()
}

// Solid batches writes until a flush, so each interaction is settled before the next.
const click = (element: HTMLElement) => { fireEvent.click(element); flush() }
const change = (element: HTMLElement, value: string) => { fireEvent.change(element, { target: { value } }); flush() }
const type = (element: HTMLElement, value: string) => { fireEvent.input(element, { target: { value } }); flush() }

const POLICY_SET = {
  organization_id: 'org_1',
  revision: 4,
  org_default: null,
  roles: {},
  updated_at: null,
  updated_by: '',
}

type Call = { url: string; method: string; body: unknown }

/** GET answers `state`; a PUT answers `put` (default: the stored set, bumped). */
function stubGateway(state: unknown, put: () => Response = () => jsonResponse({ ...POLICY_SET, revision: 5 })) {
  const calls: Call[] = []
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const method = init?.method ?? 'GET'
    calls.push({ url: String(input), method, body: init?.body ? JSON.parse(String(init.body)) : undefined })
    return method === 'PUT' ? put() : jsonResponse(state)
  }))
  return calls
}

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('parseModelEntries', () => {
  it('splits on lines and commas, trims, and drops blanks and duplicates', () => {
    expect(parseModelEntries(' gpt-4o,\nprovider:anthropic\n\n gpt-4o ')).toEqual(['gpt-4o', 'provider:anthropic'])
  })
})

describe('OrgModelAccessSection', () => {
  it('says nothing is editable when the gateway reports the switch off', async () => {
    signIn('admin')
    const calls = stubGateway({ enabled: false })

    render(() => <OrgModelAccessSection />)

    expect(await screen.findByText(/Modelltilgang er ikke slått på/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: /Lagre modelltilgang/ })).toBeNull()
    expect(calls).toEqual([{ url: '/api/v1/orgs/org_1/model-entitlement', method: 'GET', body: undefined }])
  })

  it('does not call the gateway for a member', () => {
    signIn('member')
    const calls = stubGateway({ enabled: true, ...POLICY_SET })

    render(() => <OrgModelAccessSection />)

    expect(screen.getByText(/Bare administratorer kan se eller/)).toBeTruthy()
    expect(calls).toEqual([])
  })

  it('shows the editor with the stored revision when the switch is on', async () => {
    signIn('owner')
    stubGateway({ enabled: true, ...POLICY_SET })

    render(() => <OrgModelAccessSection />)

    expect(await screen.findByText(/Revisjon 4/)).toBeTruthy()
    expect(screen.getByRole('button', { name: /Lagre modelltilgang/ })).toBeTruthy()
  })

  it('saves a listed org default from the revision it was read at', async () => {
    signIn('admin')
    const calls = stubGateway({ enabled: true, ...POLICY_SET })

    render(() => <OrgModelAccessSection />)
    click(await screen.findByRole('button', { name: /Opprett organisasjonsstandard/ }))
    change(await screen.findByLabelText('Tillatte modeller'), 'listed')
    type(await screen.findByLabelText(/Tillatelseliste/), 'gpt-4o\nprovider:anthropic\n')
    change(screen.getByLabelText('Høyeste resonneringsinnsats'), 'medium')
    click(screen.getByRole('button', { name: /Lagre modelltilgang/ }))

    await waitFor(() => expect(calls.some((call) => call.method === 'PUT')).toBe(true))
    expect(calls.find((call) => call.method === 'PUT')).toEqual({
      url: '/api/v1/orgs/org_1/model-entitlement',
      method: 'PUT',
      body: {
        expected_revision: 4,
        org_default: { allow_mode: 'listed', allow: ['gpt-4o', 'provider:anthropic'], max_effort: 'medium' },
        roles: {},
      },
    })
    expect(await screen.findByText('lagret')).toBeTruthy()
  })

  it('adds a role override that replaces the default for that role', async () => {
    signIn('admin')
    const calls = stubGateway({ enabled: true, ...POLICY_SET })

    render(() => <OrgModelAccessSection />)
    type(await screen.findByLabelText('Ny rolleoverstyring'), 'member')
    click(screen.getByRole('button', { name: /Legg til rolle/ }))
    type(await screen.findByLabelText(/Nektelisten/), 'gpt-5')
    click(screen.getByRole('button', { name: /Lagre modelltilgang/ }))

    await waitFor(() => expect(calls.some((call) => call.method === 'PUT')).toBe(true))
    expect((calls.find((call) => call.method === 'PUT')?.body as { roles: unknown }).roles).toEqual({
      member: { allow_mode: 'all', deny: ['gpt-5'] },
    })
  })

  it('refuses a listed policy with no models before it leaves the page', async () => {
    signIn('admin')
    const calls = stubGateway({ enabled: true, ...POLICY_SET })

    render(() => <OrgModelAccessSection />)
    click(await screen.findByRole('button', { name: /Opprett organisasjonsstandard/ }))
    change(await screen.findByLabelText('Tillatte modeller'), 'listed')
    click(screen.getByRole('button', { name: /Lagre modelltilgang/ }))

    expect((await screen.findByRole('alert')).textContent).toMatch(/minst én modell/)
    expect(calls.filter((call) => call.method === 'PUT')).toEqual([])
  })

  it('refuses a pattern entry, which org-core would refuse', async () => {
    signIn('admin')
    const calls = stubGateway({ enabled: true, ...POLICY_SET })

    render(() => <OrgModelAccessSection />)
    click(await screen.findByRole('button', { name: /Opprett organisasjonsstandard/ }))
    type(await screen.findByLabelText(/Nektelisten/), 'gpt-*')
    click(screen.getByRole('button', { name: /Lagre modelltilgang/ }))

    expect((await screen.findByRole('alert')).textContent).toMatch(/Mønstre med \*/)
    expect(calls.filter((call) => call.method === 'PUT')).toEqual([])
  })

  it('explains a stale revision instead of overwriting', async () => {
    signIn('admin')
    stubGateway(
      { enabled: true, ...POLICY_SET },
      () => jsonResponse({ code: 'revision_conflict', error: 'changed', current_revision: 9 }, 409),
    )

    render(() => <OrgModelAccessSection />)
    click(await screen.findByRole('button', { name: /Opprett organisasjonsstandard/ }))
    click(screen.getByRole('button', { name: /Lagre modelltilgang/ }))

    expect((await screen.findByRole('alert')).textContent).toMatch(/endret av noen andre/)
  })

  it('names the ZDR guard when a policy would leave no attested model', async () => {
    signIn('admin')
    stubGateway(
      { enabled: true, ...POLICY_SET },
      () => jsonResponse({ error: { code: 'model_policy_leaves_no_zdr_model', message: 'x' } }, 409),
    )

    render(() => <OrgModelAccessSection />)
    click(await screen.findByRole('button', { name: /Opprett organisasjonsstandard/ }))
    click(screen.getByRole('button', { name: /Lagre modelltilgang/ }))

    expect((await screen.findByRole('alert')).textContent).toMatch(/nullbevaringsgodkjenning/)
  })
})
