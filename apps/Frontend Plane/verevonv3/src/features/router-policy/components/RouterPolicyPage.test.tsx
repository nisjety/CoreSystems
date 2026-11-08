// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import RouterPolicyPage from '@/features/router-policy/components/RouterPolicyPage'
import type { RouterPolicyDocument } from '@/features/router-policy/lib/router-policy-model'
import { ApiError } from '@/shared/api/api-error'
import { getRouterPolicy, updateRouterPolicy } from '@/shared/api/router-policy-client'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// The client is replaced at its boundary. The page's contract is what it loads, what it sends back, and what
// it does with a refusal. A role table refusal is built here the way inference-core sends it: its issues
// under `details.issues`.
vi.mock('@/shared/api/router-policy-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/router-policy-client')>()),
  getRouterPolicy: vi.fn(),
  updateRouterPolicy: vi.fn(),
}))

const CATALOGUE_MODELS = [
  { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5', provider: 'anthropic', modality: 'chat', context_window_tokens: 200000 },
  { id: 'claude-opus-4-8', name: 'Claude Opus 4.8', provider: 'anthropic', modality: 'chat' },
  { id: 'claude-haiku-4-5', name: 'Claude Haiku 4.5', provider: 'anthropic', modality: 'chat' },
  { id: 'gpt-4o-mini', name: 'GPT-4o mini', provider: 'openai', modality: 'chat' },
  { id: 'text-embedding-3-small', name: 'Embedding small', provider: 'openai', modality: 'embedding' },
]

function stubCatalogue(options: { roleRouting?: boolean } = {}) {
  const body = { models: CATALOGUE_MODELS, ...(options.roleRouting ? { role_routing: true } : {}) }
  vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status: 200,
  })))
}

function policy(overrides: Partial<RouterPolicyDocument> = {}): RouterPolicyDocument {
  const row = { simple: 'gpt-4o-mini', moderate: 'gpt-4o-mini', complex: 'claude-sonnet-5-5' }
  return {
    enabled: true,
    budget_cap_usd: 50,
    constrained_fraction: 0.8,
    cheap_fallback: 'gpt-4o-mini',
    tool_fallback_ladder: ['claude-sonnet-4-6', 'gpt-4o-mini'],
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
      tool_use_floors_complex: true,
    },
    table: { budget: { ...row }, balance: { ...row }, genius: { ...row } },
    version: 7,
    updated_by: 'user_1',
    updated_at: '2026-06-16T10:00:00Z',
    ...overrides,
  }
}

function roleTableRefusal(issues: readonly Record<string, unknown>[]): ApiError {
  return new ApiError('the role table does not fit the chat catalogue', 400, 'invalid_role_table', {
    details: { issues },
  })
}

/** The document the page handed to the client on its last save. */
function sentDocument(): RouterPolicyDocument {
  const call = vi.mocked(updateRouterPolicy).mock.calls.at(-1)
  if (!call) throw new Error('the page did not save')
  return call[1] as RouterPolicyDocument
}

// Solid batches writes until a flush, so each interaction is settled before the next.
const click = (element: HTMLElement) => { fireEvent.click(element); flush() }
const type = (element: HTMLElement, value: string) => { fireEvent.input(element, { target: { value } }); flush() }
const change = (element: HTMLElement, value: string) => { fireEvent.change(element, { target: { value } }); flush() }

function signIn() {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'admin' })
  flush()
}

/** The routing-table row a side task's heading sits in. */
function rowOf(heading: string): HTMLElement {
  const row = screen.getByText(heading).closest('.verevon-router-policy__mode')
  if (!(row instanceof HTMLElement)) throw new Error(`no row for ${heading}`)
  return row
}

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
  vi.resetAllMocks()
})

describe('RouterPolicyPage saves the whole document it loaded', () => {
  it('save_round_trips_tool_fallback_ladder: a save sends back a ladder the page does not show', async () => {
    signIn()
    stubCatalogue()
    vi.mocked(getRouterPolicy).mockResolvedValue(policy({ tool_fallback_ladder: ['claude-opus-4-8', 'gpt-4o-mini'] }))
    vi.mocked(updateRouterPolicy).mockImplementation(async (_orgId, document) => document)

    render(() => <RouterPolicyPage />)
    type(await screen.findByLabelText('Budget cap (USD)'), '7')
    click(screen.getByRole('button', { name: 'Save router policy' }))

    await waitFor(() => expect(updateRouterPolicy).toHaveBeenCalledTimes(1))
    expect(vi.mocked(updateRouterPolicy).mock.calls[0]?.[0]).toBe('org_1')
    expect(sentDocument().budget_cap_usd).toBe(7)
    expect(sentDocument().tool_fallback_ladder).toEqual(['claude-opus-4-8', 'gpt-4o-mini'])
  })

  it('save_round_trips_roles: a save sends back the role table and each effort unchanged', async () => {
    signIn()
    stubCatalogue({ roleRouting: true })
    vi.mocked(getRouterPolicy).mockResolvedValue(
      policy({ roles: { title: { candidates: ['claude-sonnet-5-5'], effort: 'minimal' } } }),
    )
    vi.mocked(updateRouterPolicy).mockImplementation(async (_orgId, document) => document)

    render(() => <RouterPolicyPage />)
    type(await screen.findByLabelText('Budget cap (USD)'), '9')
    click(screen.getByRole('button', { name: 'Save router policy' }))

    await waitFor(() => expect(updateRouterPolicy).toHaveBeenCalledTimes(1))
    expect(sentDocument().roles).toEqual({ title: { candidates: ['claude-sonnet-5-5'], effort: 'minimal' } })
  })

  it('does not save before the policy has loaded', async () => {
    signIn()
    stubCatalogue()
    vi.mocked(getRouterPolicy).mockRejectedValue(new ApiError('upstream failed', 502, 'upstream_error'))

    render(() => <RouterPolicyPage />)

    expect(await screen.findByText('Could not load the router policy from the gateway.')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Save router policy' })).toBeNull()
    expect(updateRouterPolicy).not.toHaveBeenCalled()
  })
})

describe('RouterPolicyPage role editor', () => {
  it('invalid_role_table_error_marks_the_row: the refusal lands on the purpose it names', async () => {
    signIn()
    stubCatalogue({ roleRouting: true })
    vi.mocked(getRouterPolicy).mockResolvedValue(policy({ roles: { title: { candidates: ['claude-opus-4-8'] } } }))
    vi.mocked(updateRouterPolicy).mockRejectedValueOnce(
      roleTableRefusal([{ reason: 'reasoning_only', purpose: 'title', model: 'claude-opus-4-8' }]),
    )

    render(() => <RouterPolicyPage />)
    click(await screen.findByRole('button', { name: 'Save router policy' }))

    const alert = await within(rowOf('Samtaletittel')).findByRole('alert')
    expect(alert.textContent).toContain('«claude-opus-4-8» kan ikke svare uten resonnering')
    expect(rowOf('Forskningsplan').querySelector('[role="alert"]')).toBeNull()
  })

  it('shows the stored roles read-only, with the ikke aktivert note, while role routing is off', async () => {
    signIn()
    stubCatalogue()
    vi.mocked(getRouterPolicy).mockResolvedValue(policy({ roles: { title: { candidates: ['claude-sonnet-5-5'] } } }))

    render(() => <RouterPolicyPage />)

    expect(await screen.findByText(/ikke aktivert/)).toBeTruthy()
    expect(screen.getByText('1. Claude Sonnet 5.5')).toBeTruthy()
    expect(document.getElementById('router-role-title-add')).toBeNull()
    expect(screen.queryByRole('button', { name: /^Fjern/ })).toBeNull()
  })

  it('adds only chat models and keeps the order the admin sets', async () => {
    signIn()
    stubCatalogue({ roleRouting: true })
    vi.mocked(getRouterPolicy).mockResolvedValue(policy({ roles: { follow_ups: { candidates: ['gpt-4o-mini'] } } }))
    vi.mocked(updateRouterPolicy).mockImplementation(async (_orgId, document) => document)

    render(() => <RouterPolicyPage />)
    await waitFor(() => expect(document.getElementById('router-role-follow_ups-add')).not.toBeNull())
    const select = document.getElementById('router-role-follow_ups-add') as HTMLSelectElement
    const offered = Array.from(select.options).map((option) => option.value)
    expect(offered).toContain('claude-sonnet-5-5')
    expect(offered).not.toContain('text-embedding-3-small')
    expect(offered).not.toContain('gpt-4o-mini')

    change(select, 'claude-sonnet-5-5')
    click(await screen.findByRole('button', { name: 'Flytt Claude Sonnet 5.5 opp' }))
    click(screen.getByRole('button', { name: 'Save router policy' }))

    await waitFor(() => expect(updateRouterPolicy).toHaveBeenCalledTimes(1))
    expect(sentDocument().roles?.follow_ups?.candidates).toEqual(['claude-sonnet-5-5', 'gpt-4o-mini'])
  })

  it('removing the last candidate drops the purpose and then the table, as the server writes it', async () => {
    signIn()
    stubCatalogue({ roleRouting: true })
    vi.mocked(getRouterPolicy).mockResolvedValue(policy({ roles: { title: { candidates: ['claude-sonnet-5-5'] } } }))
    vi.mocked(updateRouterPolicy).mockImplementation(async (_orgId, document) => document)

    render(() => <RouterPolicyPage />)
    click(await screen.findByRole('button', { name: 'Fjern Claude Sonnet 5.5' }))
    expect(within(rowOf('Samtaletittel')).getByText('Ingen modell valgt. Turens egen modell brukes.')).toBeTruthy()
    click(screen.getByRole('button', { name: 'Save router policy' }))

    await waitFor(() => expect(updateRouterPolicy).toHaveBeenCalledTimes(1))
    expect(sentDocument()).not.toHaveProperty('roles')
  })

  it('offers no add control once a role names four models', async () => {
    signIn()
    stubCatalogue({ roleRouting: true })
    vi.mocked(getRouterPolicy).mockResolvedValue(policy({
      roles: { title: { candidates: ['claude-sonnet-5-5', 'claude-opus-4-8', 'claude-haiku-4-5', 'gpt-4o-mini'] } },
    }))

    render(() => <RouterPolicyPage />)
    expect(await screen.findByRole('button', { name: 'Fjern Claude Haiku 4.5' })).toBeTruthy()

    expect(document.getElementById('router-role-title-add')).toBeNull()
  })
})
