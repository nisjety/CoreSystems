// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ModelEntitlementMeta, ModelInfo } from '@/shared/api/chat-client'

// The organisation's model policy (WS10-S6) rides beside the catalogue. The
// composer reconciles to it: a pinned model the policy no longer lists is replaced
// with a notice, and the response-effort select stops at the policy's ceiling.

const { mockListModels, mockMeta } = vi.hoisted(() => ({
  mockListModels: vi.fn(),
  mockMeta: vi.fn(),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: mockListModels,
    getModelEntitlementMeta: mockMeta,
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'member' } }),
}))

import { DashboardComposer } from './DashboardComposer'

const STORAGE_KEY = 'verevon.ai-model-selection.v1:org-1'
const submitted: Array<Record<string, unknown>> = []

const model = (id: string, overrides: Partial<ModelInfo> = {}): ModelInfo => ({
  id,
  name: id,
  provider: 'openai',
  modality: 'chat',
  ...overrides,
})

const meta = (overrides: Partial<ModelEntitlementMeta> = {}): ModelEntitlementMeta => ({
  state: 'role',
  enforced: true,
  revision: 3,
  restricted: true,
  hiddenCount: 1,
  ...overrides,
})

function ComposerPage() {
  const [message, setMessage] = createSignal('')
  return (
    <DashboardComposer
      appearance="chat"
      message={message()}
      onMessageChange={setMessage}
      onSubmit={(payload) => {
        submitted.push(payload as unknown as Record<string, unknown>)
      }}
    />
  )
}

function renderComposer(models: ModelInfo[], entitlement: ModelEntitlementMeta | undefined) {
  mockListModels.mockResolvedValue(models)
  mockMeta.mockReturnValue(entitlement)
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: ComposerPage }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

function rememberSelection(id: string, label: string, provider = 'openai') {
  window.localStorage.setItem(STORAGE_KEY, JSON.stringify({ model: id, label, provider }))
}

function effortSelect(): HTMLSelectElement {
  return screen.getByRole('combobox', { name: /svarinnsats/i }) as HTMLSelectElement
}

function optionValues(): string[] {
  return Array.from(effortSelect().options).map((option) => option.value)
}

async function send(text: string) {
  fireEvent.input(screen.getByRole('textbox'), { target: { value: text } })
  flush()
  const form = screen.getByRole('textbox').closest('form')
  if (!form) throw new Error('composer form not found')
  fireEvent.submit(form)
  await waitFor(() => expect(submitted.length).toBeGreaterThan(0))
}

beforeEach(() => {
  window.localStorage.clear()
  window.sessionStorage.clear()
})

afterEach(() => {
  submitted.length = 0
  vi.clearAllMocks()
})

describe('composer reconciles to the organisation model policy', () => {
  it('replaces a pinned model the policy no longer lists, and says so', async () => {
    rememberSelection('gpt-big', 'GPT Big')
    renderComposer([model('gpt-mini')], meta())

    expect(await screen.findByText(/Organisasjonen din tillater ikke lenger gpt-big/)).toBeTruthy()
    await send('Hei')
    expect(submitted[0]?.model).toBe('verevon-balance')
    expect(JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? '{}').model).toBe('verevon-balance')
  })

  it('moves to the policy default when the catalogue lists it', async () => {
    rememberSelection('gpt-big', 'GPT Big')
    renderComposer([model('gpt-mini')], meta({ defaultModel: 'gpt-mini' }))

    expect(await screen.findByText(/Organisasjonen din tillater ikke lenger gpt-big/)).toBeTruthy()
    await send('Hei')
    expect(submitted[0]?.model).toBe('gpt-mini')
  })

  it('keeps a selection the catalogue still lists, with no notice', async () => {
    rememberSelection('gpt-mini', 'gpt-mini')
    renderComposer([model('gpt-mini')], meta())

    await screen.findByRole('button', { name: /velg ai-modell/i })
    await send('Hei')
    expect(submitted[0]?.model).toBe('gpt-mini')
    expect(screen.queryByText(/tillater ikke lenger/)).toBeNull()
  })

  it('leaves the selection alone when no policy applies', async () => {
    rememberSelection('gpt-big', 'GPT Big')
    renderComposer([model('gpt-mini')], undefined)

    await screen.findByRole('button', { name: /velg ai-modell/i })
    await send('Hei')
    expect(submitted[0]?.model).toBe('gpt-big')
    expect(screen.queryByText(/tillater ikke lenger/)).toBeNull()
  })

  it('leaves the selection alone in shadow mode, where nothing is hidden', async () => {
    rememberSelection('gpt-big', 'GPT Big')
    renderComposer([model('gpt-mini')], meta({ enforced: false }))

    await screen.findByRole('button', { name: /velg ai-modell/i })
    await send('Hei')
    expect(submitted[0]?.model).toBe('gpt-big')
  })

  it('does not reset on an empty catalogue, which is an outage', async () => {
    rememberSelection('gpt-big', 'GPT Big')
    renderComposer([], meta())

    await screen.findByRole('button', { name: /velg ai-modell/i })
    await send('Hei')
    expect(submitted[0]?.model).toBe('gpt-big')
  })
})

describe('composer model search under a policy notice', () => {
  it('keeps the notice while the search narrows the catalogue', async () => {
    rememberSelection('gpt-big', 'GPT Big')
    const many = ['gpt-mini', 'gpt-nano', 'gpt-large', 'gpt-vision', 'gpt-audio', 'gpt-turbo', 'gpt-omni'].map((id) => model(id))
    renderComposer(many, meta())

    expect(await screen.findByText(/Organisasjonen din tillater ikke lenger gpt-big/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /velg ai-modell/i }))
    fireEvent.input(await screen.findByRole('searchbox', { name: /søk etter modell/i }), { target: { value: 'claude' } })

    expect(await screen.findByText('Ingen modeller passer')).toBeTruthy()
    expect(screen.getByText(/Organisasjonen din tillater ikke lenger gpt-big/)).toBeTruthy()
  })
})

describe('composer effort select under a policy ceiling', () => {
  const ladder = ['low', 'medium', 'high', 'xhigh'] as const

  it('offers every profile when no ceiling applies', async () => {
    renderComposer([model('gpt-mini', { effortLevels: [...ladder] })], meta({ restricted: false }))
    await screen.findByRole('button', { name: /velg ai-modell/i })
    expect(optionValues()).toEqual(['quick', 'auto', 'deep'])
  })

  it('drops Grundig above a medium ceiling, and keeps Rask and Standard', async () => {
    renderComposer([model('gpt-mini', { effortLevels: [...ladder] })], meta({ maxEffort: 'medium' }))
    await screen.findByRole('button', { name: /velg ai-modell/i })
    await waitFor(() => expect(optionValues()).toEqual(['quick', 'auto']))
  })

  it('does not apply the ceiling in shadow mode', async () => {
    renderComposer(
      [model('gpt-mini', { effortLevels: [...ladder] })],
      meta({ enforced: false, maxEffort: 'low' }),
    )
    await screen.findByRole('button', { name: /velg ai-modell/i })
    expect(optionValues()).toEqual(['quick', 'auto', 'deep'])
  })

  it('withholds Maksimal above a high ceiling even for a model that lists xhigh', async () => {
    renderComposer([model('gpt-mini', { effortLevels: [...ladder] })], meta({ maxEffort: 'high' }))
    await screen.findByRole('button', { name: /velg ai-modell/i })
    await waitFor(() => expect(optionValues()).toEqual(['quick', 'auto', 'deep']))
  })

  it('offers Maksimal again when the ceiling is xhigh', async () => {
    renderComposer([model('gpt-mini', { effortLevels: [...ladder] })], meta({ maxEffort: 'xhigh' }))
    await screen.findByRole('button', { name: /velg ai-modell/i })
    await waitFor(() => expect(optionValues()).toEqual(['quick', 'auto', 'deep']))
    fireEvent.click(await screen.findByRole('button', { name: /velg ai-modell/i }))
    fireEvent.click(await screen.findByText('gpt-mini'))
    await waitFor(() => expect(optionValues()).toEqual(['quick', 'auto', 'deep', 'max']))
  })
})
