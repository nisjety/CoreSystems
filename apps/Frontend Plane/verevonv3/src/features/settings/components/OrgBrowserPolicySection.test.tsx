// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  OrgBrowserPolicySection,
  normalizeRule,
  policyProblem,
} from '@/features/settings/components/OrgBrowserPolicySection'
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

const POLICY = {
  organization_id: 'org_1',
  revision: 4,
  mode: 'open',
  rules: [],
  updated_by: '',
  updated_at: null,
}

type Call = { url: string; method: string; body: unknown }

function stubGateway(answer: (call: Call) => Response | undefined): Call[] {
  const calls: Call[] = []
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const call: Call = {
      url: String(input),
      method: init?.method ?? 'GET',
      body: init?.body ? JSON.parse(String(init.body)) : undefined,
    }
    calls.push(call)
    return answer(call) ?? jsonResponse(POLICY)
  }))
  return calls
}

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
  vi.unstubAllEnvs()
})

describe('policy draft rules', () => {
  const tr = (no: string) => no

  it('refuses a host that is not a host name and a note over the limit', () => {
    expect(policyProblem([{ host: 'shop.example.com', rule: 'act' }], tr)).toBeNull()
    expect(policyProblem([{ host: 'bad host!', rule: 'act' }], tr)).toContain('er ikke et domenenavn')
    expect(policyProblem([{ host: '*.example.com', rule: 'read' }], tr)).toContain('er ikke et domenenavn')
    expect(policyProblem([{ host: 'a.example', rule: 'read', note: 'x'.repeat(201) }], tr)).toContain('lengre')
  })

  it('saves a trimmed host and a note only when one was written', () => {
    expect(normalizeRule({ host: ' shop.example.com ', rule: 'act', note: '  ' })).toEqual({
      host: 'shop.example.com',
      rule: 'act',
    })
  })
})

describe('OrgBrowserPolicySection', () => {
  it('is absent from a member, and reads no policy', () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    const calls = stubGateway(() => undefined)
    signIn('member')
    render(() => <OrgBrowserPolicySection />)
    flush()
    expect(screen.queryByTestId('org-browser-policy-section')).toBeNull()
    expect(calls).toHaveLength(0)
  })

  it('saves the draft from the revision the admin read', async () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    const calls = stubGateway((call) =>
      call.method === 'PUT'
        ? jsonResponse({ ...POLICY, revision: 5, rules: [{ host: 'shop.example.com', rule: 'act' }] })
        : undefined,
    )
    signIn('admin')
    render(() => <OrgBrowserPolicySection />)
    await waitFor(() => expect(screen.getByRole('button', { name: /Legg til nettside/ })).toBeTruthy())
    click(screen.getByRole('button', { name: /Legg til nettside/ }))
    type(document.getElementById('org-browser-policy-host-0') as HTMLElement, 'shop.example.com')
    change(document.getElementById('org-browser-policy-rule-0') as HTMLElement, 'act')
    click(screen.getByRole('button', { name: /Lagre nettleserpolicy/ }))
    await waitFor(() => expect(calls.some((call) => call.method === 'PUT')).toBe(true))
    const put = calls.find((call) => call.method === 'PUT')
    expect(put?.url.endsWith('/api/v1/orgs/org_1/browser-sites')).toBe(true)
    expect(put?.body).toEqual({
      expected_revision: 4,
      mode: 'open',
      rules: [{ host: 'shop.example.com', rule: 'act' }],
    })
  })

  it('refuses a malformed host before anything is sent', async () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    const calls = stubGateway(() => undefined)
    signIn('owner')
    render(() => <OrgBrowserPolicySection />)
    await waitFor(() => expect(screen.getByRole('button', { name: /Legg til nettside/ })).toBeTruthy())
    click(screen.getByRole('button', { name: /Legg til nettside/ }))
    type(document.getElementById('org-browser-policy-host-0') as HTMLElement, 'bad host!')
    click(screen.getByRole('button', { name: /Lagre nettleserpolicy/ }))
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('er ikke et domenenavn'))
    expect(calls.some((call) => call.method === 'PUT')).toBe(false)
  })

  it('says a stale draft was not saved when another administrator changed the policy', async () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    stubGateway((call) =>
      call.method === 'PUT'
        ? jsonResponse({ error: 'the browser site policy changed since it was read', code: 'revision_conflict' }, 409)
        : undefined,
    )
    signIn('admin')
    render(() => <OrgBrowserPolicySection />)
    await waitFor(() => expect(screen.getByRole('button', { name: /Legg til nettside/ })).toBeTruthy())
    click(screen.getByRole('button', { name: /Legg til nettside/ }))
    type(document.getElementById('org-browser-policy-host-0') as HTMLElement, 'shop.example.com')
    click(screen.getByRole('button', { name: /Lagre nettleserpolicy/ }))
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('utkastet ditt er ikke lagret'))
  })
})
