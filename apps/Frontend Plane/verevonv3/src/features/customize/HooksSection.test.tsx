// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { HooksSection, hookMatchesTool, hookVerdict, type HookRule } from '@/features/customize/HooksSection'
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

const rule = (overrides: Partial<HookRule> = {}): HookRule => ({
  id: 'hr_x',
  event: 'pre_tool',
  tool_matcher: '*',
  decision: 'deny',
  reason: 'reason',
  priority: 0,
  enabled: true,
  ...overrides,
})

const LISTING = {
  org_id: 'org_1',
  revision: 3,
  rules: [
    rule({ id: 'hr_1', tool_matcher: 'shell', reason: 'No shell in this organisation', priority: 10 }),
    rule({ id: 'hr_2', tool_matcher: 'mcp__crm__*', decision: 'ask', reason: 'Ask before CRM writes' }),
  ],
}

beforeEach(() => {
  window.localStorage.setItem('verevon.locale', 'en')
})

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('hookMatchesTool', () => {
  it('reads an empty matcher or * as any tool, a trailing * as a prefix, and anything else as a name', () => {
    expect(hookMatchesTool('', 'shell')).toBe(true)
    expect(hookMatchesTool('*', 'shell')).toBe(true)
    expect(hookMatchesTool('shell', 'shell')).toBe(true)
    expect(hookMatchesTool('shell', 'shell2')).toBe(false)
    expect(hookMatchesTool('mcp__crm__*', 'mcp__crm__write')).toBe(true)
    expect(hookMatchesTool('mcp__crm__*', 'mcp__mail__write')).toBe(false)
  })
})

describe('hookVerdict', () => {
  it('lets a deny beat an ask, and an ask beat an allow', () => {
    const rules = [rule({ id: 'a', decision: 'ask' }), rule({ id: 'b', decision: 'deny', tool_matcher: 'shell' })]
    expect(hookVerdict(rules, 'pre_tool', 'shell').verdict).toBe('deny')
    expect(hookVerdict([rule({ id: 'a', decision: 'ask' })], 'pre_tool', 'shell').verdict).toBe('ask')
  })

  it('allows a call that no enabled rule for its event matches', () => {
    const rules = [
      rule({ enabled: false }),
      rule({ event: 'post_tool' }),
      rule({ tool_matcher: 'mail' }),
    ]
    expect(hookVerdict(rules, 'pre_tool', 'shell')).toEqual({ verdict: 'allow', matching: [] })
  })
})

describe('HooksSection', () => {
  it('lists the rules for a member, with no way to change them', async () => {
    signIn('member')
    stubGateway({ 'GET /api/v1/org/hooks': () => jsonResponse(LISTING) })

    render(() => <I18nProvider><HooksSection /></I18nProvider>)

    expect(await screen.findByText('No shell in this organisation')).toBeTruthy()
    expect(screen.getByText('Ask before CRM writes')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Add rule' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Edit' })).toBeNull()
  })

  it('an admin adds a rule, and the write carries the rule in the gateway wire form', async () => {
    signIn('admin')
    const calls = stubGateway({
      'GET /api/v1/org/hooks': () => jsonResponse(LISTING),
      'POST /api/v1/org/hooks': () => jsonResponse({ ...rule({ id: 'hr_3' }), revision: 4 }),
    })

    render(() => <I18nProvider><HooksSection /></I18nProvider>)
    click(await screen.findByRole('button', { name: 'Add rule' }))
    // A field's accessible name includes its help text, so the fields are reached by id.
    type(document.getElementById('hook-tool') as HTMLInputElement, 'mail_send')
    type(document.getElementById('hook-reason') as HTMLInputElement, 'No sending mail')
    type(document.getElementById('hook-priority') as HTMLInputElement, '5')
    click(screen.getByRole('button', { name: 'Add rule' }))

    await waitFor(() => expect(calls.some((call) => call.method === 'POST')).toBe(true))
    expect(calls.find((call) => call.method === 'POST')?.body).toEqual({
      event: 'pre_tool',
      tool_matcher: 'mail_send',
      decision: 'deny',
      reason: 'No sending mail',
      priority: 5,
      enabled: true,
    })
  })

  it('offers only deny for a post-tool rule, since the tool has already acted', async () => {
    signIn('admin')
    stubGateway({ 'GET /api/v1/org/hooks': () => jsonResponse(LISTING) })

    render(() => <I18nProvider><HooksSection /></I18nProvider>)
    click(await screen.findByRole('button', { name: 'Add rule' }))
    change(screen.getByRole('combobox', { name: /When the rule applies/ }), 'post_tool')

    const decision = screen.getByRole('combobox', { name: /^Decision/ }) as HTMLSelectElement
    expect(Array.from(decision.options).map((option) => option.value)).toEqual(['deny'])
  })

  it('retires a rule with a delete to its id', async () => {
    signIn('admin')
    const calls = stubGateway({
      'GET /api/v1/org/hooks': () => jsonResponse(LISTING),
      'DELETE /api/v1/org/hooks/hr_1': () => jsonResponse({ id: 'hr_1', revoked: true, revision: 4 }),
    })

    render(() => <I18nProvider><HooksSection /></I18nProvider>)
    click((await screen.findAllByRole('button', { name: 'Retire' }))[0] as HTMLElement)

    await waitFor(() => expect(calls.some((call) => call.method === 'DELETE')).toBe(true))
    expect(calls.find((call) => call.method === 'DELETE')?.path).toBe('/api/v1/org/hooks/hr_1')
  })

  it('tests a tool name against the active rules and says what the engine would do', async () => {
    signIn('member')
    stubGateway({ 'GET /api/v1/org/hooks': () => jsonResponse(LISTING) })

    render(() => <I18nProvider><HooksSection /></I18nProvider>)
    await screen.findByText('No shell in this organisation')
    type(screen.getByRole('textbox', { name: 'Tool name' }), 'shell')
    expect(await screen.findByText('The tool is denied.')).toBeTruthy()

    type(screen.getByRole('textbox', { name: 'Tool name' }), 'web_search')
    expect(await screen.findByText('The tool is allowed: no rule matches.')).toBeTruthy()
  })
})
