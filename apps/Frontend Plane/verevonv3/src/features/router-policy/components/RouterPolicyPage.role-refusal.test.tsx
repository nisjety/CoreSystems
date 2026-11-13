// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import RouterPolicyPage from '@/features/router-policy/components/RouterPolicyPage'
import type { RouterPolicyDocument } from '@/features/router-policy/lib/router-policy-model'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// W3-EP-13-S5-a, fetch level: nothing above fetch is replaced. inference-core refuses a role table with
// {error:{code:'invalid_role_table', details:{issues:[...]}}}; the transport keeps details whole and the page
// marks the row the issue names. This pins the production read path end to end (http.ts extractError ->
// ApiError.details -> roleIssuesFrom), not a test-built ApiError.

const CATALOGUE = {
  role_routing: true,
  models: [
    { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5', provider: 'anthropic', modality: 'chat' },
    { id: 'claude-opus-4-8', name: 'Claude Opus 4.8', provider: 'anthropic', modality: 'chat' },
    { id: 'gpt-4o-mini', name: 'GPT-4o mini', provider: 'openai', modality: 'chat' },
  ],
}

function policy(): RouterPolicyDocument {
  const row = { simple: 'gpt-4o-mini', moderate: 'gpt-4o-mini', complex: 'claude-sonnet-5-5' }
  return {
    enabled: true,
    budget_cap_usd: 50,
    constrained_fraction: 0.8,
    cheap_fallback: 'gpt-4o-mini',
    complexity: {
      large_total_chars: 4000,
      large_total_chars_score: 2,
      medium_total_chars: 1200,
      medium_total_chars_score: 1,
      long_user_turn_chars: 800,
      long_user_turn_score: 1,
      code_fence_score: 1,
      keyword_score: 1,
      tool_use_score: 2,
      deep_conversation_turns: 12,
      deep_conversation_score: 1,
      moderate_threshold: 1,
      complex_threshold: 3,
      keywords: ['analyze'],
    },
    table: { budget: { ...row }, balance: { ...row }, genius: { ...row } },
    roles: { title: { candidates: ['claude-opus-4-8'] } },
    version: 7,
    updated_by: 'user_1',
    updated_at: '2026-06-16T10:00:00Z',
  } as RouterPolicyDocument
}

function json(status: number, body: unknown) {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

function stubGateway(saveRefusal: unknown) {
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    if (url.includes('/api/v1/models')) return json(200, CATALOGUE)
    if (url.includes('/api/v1/router-policy') && (init?.method ?? 'GET') === 'GET') return json(200, { data: policy() })
    if (url.includes('/api/v1/router-policy')) return json(400, saveRefusal)
    return json(404, { error: { code: 'not_found', message: 'unexpected' } })
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function signIn() {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'admin' })
  flush()
}

function rowOf(heading: string): HTMLElement {
  const row = screen.getByText(heading).closest('.verevon-router-policy__mode')
  if (!(row instanceof HTMLElement)) throw new Error(`no row for ${heading}`)
  return row
}

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('RouterPolicyPage invalid_role_table through the transport', () => {
  it('marks the row the refusal names, from the body inference-core sends', async () => {
    signIn()
    const fetchMock = stubGateway({
      error: {
        code: 'invalid_role_table',
        message: 'the role table does not fit the chat catalogue',
        details: { issues: [{ reason: 'reasoning_only', purpose: 'title', model: 'claude-opus-4-8' }] },
      },
    })

    render(() => <RouterPolicyPage />)
    fireEvent.click(await screen.findByRole('button', { name: 'Save router policy' }))
    flush()

    const alert = await within(rowOf('Samtaletittel')).findByRole('alert')
    expect(alert.textContent).toContain('«claude-opus-4-8» kan ikke svare uten resonnering')
    expect(rowOf('Forskningsplan').querySelector('[role="alert"]')).toBeNull()
    expect(fetchMock.mock.calls.some(([, init]) => (init as RequestInit | undefined)?.method === 'PUT')).toBe(true)
  })

  it('marks no row when the refusal carries no issues it can read', async () => {
    signIn()
    stubGateway({
      error: { code: 'invalid_role_table', message: 'the role table does not fit the chat catalogue', details: {} },
    })

    render(() => <RouterPolicyPage />)
    fireEvent.click(await screen.findByRole('button', { name: 'Save router policy' }))
    flush()

    await new Promise((resolve) => setTimeout(resolve, 20))
    flush()
    expect(rowOf('Samtaletittel').querySelector('[role="alert"]')).toBeNull()
  })
})
