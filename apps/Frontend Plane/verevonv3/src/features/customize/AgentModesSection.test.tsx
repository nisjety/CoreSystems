// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  AgentModesSection,
  __resetOrgAgentPolicyAvailabilityForTests,
  agentModesWithinCap,
  loadEffectiveAgentModeCap,
  orgAgentPolicyAvailable,
  parseServerIds,
} from '@/features/customize/AgentModesSection'
import { I18nProvider } from '@/shared/i18n'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function signIn(role: 'admin' | 'member') {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role })
  flush()
}

// Solid batches writes until a flush, so each interaction is settled before the next.
const change = (element: HTMLElement, value: string) => { fireEvent.change(element, { target: { value } }); flush() }
const type = (element: HTMLElement, value: string) => { fireEvent.input(element, { target: { value } }); flush() }
const click = (element: HTMLElement) => { fireEvent.click(element); flush() }

const STORED = {
  org_id: 'org_1',
  revision: 7,
  digest: 'd1',
  settings: {
    mode_cap: 'auto',
    rung_cap: 'danger_full_access',
    lockdown: false,
    'ext.mcp_servers': { mode: 'any', ids: [] },
    mcp_tool_approval: 'trust_annotations',
  },
}

const EFFECTIVE = {
  org_id: 'org_1',
  revision: 7,
  digest: 'd1',
  settings: [
    { key: 'mode_cap', kind: 'enum', status: 'enforced', value: 'auto', source: 'platform_default', fail_closed_value: 'lockdown' },
    { key: 'ext.skills', kind: 'allow_list', status: 'reserved', value: { mode: 'any', ids: [] }, source: 'platform_default', fail_closed_value: { mode: 'none', ids: [] } },
  ],
}

type Call = { method: string; path: string; body: unknown }
type Handler = (init?: RequestInit) => Response

/** Answers each `METHOD /path` from `routes`; an unknown route is a 404, as the gateway answers an absent one. */
function stubGateway(routes: Record<string, Handler>): Call[] {
  const calls: Call[] = []
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(String(input), 'http://localhost')
    const method = init?.method ?? 'GET'
    calls.push({ method, path: url.pathname, body: init?.body ? JSON.parse(String(init.body)) : undefined })
    const handler = routes[`${method} ${url.pathname}`]
    return handler ? handler(init) : jsonResponse({ error: { code: 'not_found', message: 'no route' } }, 404)
  }))
  return calls
}

const policyRoutes = (put: Handler = () => jsonResponse({ ...STORED, revision: 8 })): Record<string, Handler> => ({
  'GET /api/v1/org/agent-policy': () => jsonResponse(STORED),
  'GET /api/v1/org/agent-policy/effective': () => jsonResponse(EFFECTIVE),
  'PUT /api/v1/org/agent-policy': put,
})

beforeEach(() => {
  window.localStorage.setItem('verevon.locale', 'en')
  __resetOrgAgentPolicyAvailabilityForTests()
})

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('agentModesWithinCap', () => {
  it('admits the cap and every stricter mode, and no looser one', () => {
    expect(agentModesWithinCap('read_only')).toEqual(['lockdown', 'read_only'])
    expect(agentModesWithinCap('lockdown')).toEqual(['lockdown'])
    expect(agentModesWithinCap('auto')).toEqual(['lockdown', 'read_only', 'ask_first', 'auto'])
  })
})

describe('parseServerIds', () => {
  it('splits on lines and commas, trims, and drops blanks and duplicates', () => {
    expect(parseServerIds(' crm-a,\nmail:b\n\n crm-a ')).toEqual(['crm-a', 'mail:b'])
  })
})

describe('the org agent policy API availability', () => {
  it('reads a 404 as the API being off, and caches that answer for the page', async () => {
    const calls = stubGateway({
      'GET /api/v1/org/agent-policy': () => jsonResponse({ error: { code: 'not_found', message: 'off' } }, 404),
    })

    expect(await orgAgentPolicyAvailable()).toBe(false)
    expect(await orgAgentPolicyAvailable()).toBe(false)
    expect(calls.filter((call) => call.method === 'GET')).toHaveLength(1)
  })

  it('does not cache a failure, so the next call asks again', async () => {
    let answer = 500
    const calls = stubGateway({
      'GET /api/v1/org/agent-policy': () => (answer === 500
        ? jsonResponse({ error: { code: 'internal', message: 'down' } }, 500)
        : jsonResponse(STORED)),
    })

    await expect(orgAgentPolicyAvailable()).rejects.toBeTruthy()
    answer = 200
    expect(await orgAgentPolicyAvailable()).toBe(true)
    expect(calls.filter((call) => call.method === 'GET')).toHaveLength(2)
  })

  it('loads the mode cap in force, and offers no cap where the API is off', async () => {
    stubGateway({
      'GET /api/v1/org/agent-policy': () => jsonResponse(STORED),
      'GET /api/v1/org/agent-policy/effective': () => jsonResponse({ ...EFFECTIVE, settings: [{ ...EFFECTIVE.settings[0], value: 'read_only' }] }),
    })
    expect(await loadEffectiveAgentModeCap()).toBe('read_only')
  })

  it('offers no mode cap where the policy API is off', async () => {
    stubGateway({})
    expect(await loadEffectiveAgentModeCap()).toBeUndefined()
  })
})

describe('AgentModesSection', () => {
  it('an admin changes the mode cap and saves the whole document at the revision it read', async () => {
    signIn('admin')
    const calls = stubGateway(policyRoutes())

    render(() => <I18nProvider><AgentModesSection /></I18nProvider>)
    change(await screen.findByRole('combobox', { name: /Highest agent mode/ }), 'read_only')
    click(await screen.findByRole('button', { name: 'Save agent policy' }))

    await waitFor(() => expect(calls.some((call) => call.method === 'PUT')).toBe(true))
    expect(calls.find((call) => call.method === 'PUT')?.body).toEqual({
      revision: 7,
      settings: {
        mode_cap: 'read_only',
        rung_cap: 'danger_full_access',
        lockdown: false,
        'ext.mcp_servers': { mode: 'any', ids: [] },
        mcp_tool_approval: 'trust_annotations',
      },
    })
  })

  it('a member sees the values read-only, with a locked badge and no save control', async () => {
    signIn('member')
    stubGateway(policyRoutes())

    render(() => <I18nProvider><AgentModesSection /></I18nProvider>)
    const select = await screen.findByRole('combobox', { name: /Highest agent mode/ })

    expect((select as HTMLSelectElement).disabled).toBe(true)
    expect(screen.getByText('Locked for members')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Save agent policy' })).toBeNull()
  })

  it('refuses a server id the gateway would reject, before anything is sent', async () => {
    signIn('admin')
    const calls = stubGateway(policyRoutes())

    render(() => <I18nProvider><AgentModesSection /></I18nProvider>)
    change(await screen.findByRole('combobox', { name: /MCP servers the agent may use/ }), 'approved_only')
    type(await screen.findByRole('textbox', { name: /Approved server IDs/ }), 'bad id!')
    click(screen.getByRole('button', { name: 'Save agent policy' }))

    expect(await screen.findByText(/is not a valid server ID/)).toBeTruthy()
    expect(calls.some((call) => call.method === 'PUT')).toBe(false)
  })

  it('says the policy changed when the save is refused at a stale revision', async () => {
    signIn('admin')
    stubGateway(policyRoutes(() => jsonResponse({ error: { code: 'stale_revision', message: 'stale' } }, 409)))

    render(() => <I18nProvider><AgentModesSection /></I18nProvider>)
    change(await screen.findByRole('combobox', { name: /Highest agent mode/ }), 'lockdown')
    click(screen.getByRole('button', { name: 'Save agent policy' }))

    expect(await screen.findByText(/changed after you opened it/)).toBeTruthy()
  })
})
