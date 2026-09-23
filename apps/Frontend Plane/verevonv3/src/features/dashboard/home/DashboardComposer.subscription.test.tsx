// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

const { mockListModels, mockListSubscriptions } = vi.hoisted(() => ({
  mockListModels: vi.fn(),
  mockListSubscriptions: vi.fn(),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: mockListModels,
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>()
  return { ...actual, listChatGptSubscriptions: mockListSubscriptions }
})

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'admin' } }),
}))

import { DashboardComposer } from './DashboardComposer'

const submitted: Array<Record<string, unknown>> = []

function ComposerPage() {
  const [message, setMessage] = createSignal('')
  return (
    <DashboardComposer
      message={message()}
      onMessageChange={setMessage}
      onSubmit={(payload) => {
        submitted.push(payload as unknown as Record<string, unknown>)
      }}
    />
  )
}

function renderComposer() {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: ComposerPage }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

afterEach(() => {
  submitted.length = 0
  vi.clearAllMocks()
  window.localStorage.removeItem('verevon.ai-model-selection.v1:org-1')
})

// F-10 (CHAT_PARITY_AUDIT_2026-09-15.md §3.1): chat-client.ts's
// `buildChatWireBody` silently forces browse_web/generate_image/plan_mode/
// deep_research to `false` and drops every skill pick once
// `provider === 'openai-codex-subscription'` — but before this fix the
// composer's Søk/Bilde/Utfør/Dyp research controls stayed fully enabled and
// gave no indication anything was inert. This pins the composer-side mirror
// of that same condition.
function ComposerPageWithModeControls(props: { initiallyEnabled?: boolean }) {
  const [message, setMessage] = createSignal('')
  const [browseWeb, setBrowseWeb] = createSignal(props.initiallyEnabled ?? false)
  const [imageMode, setImageMode] = createSignal(props.initiallyEnabled ?? false)
  const [planMode, setPlanMode] = createSignal(props.initiallyEnabled ?? false)
  return (
    <DashboardComposer
      appearance="chat"
      browseWeb={browseWeb()}
      imageMode={imageMode()}
      message={message()}
      onBrowseWebChange={setBrowseWeb}
      onImageModeChange={setImageMode}
      onMessageChange={setMessage}
      onPlanModeChange={setPlanMode}
      onSubmit={(payload) => {
        submitted.push(payload as unknown as Record<string, unknown>)
      }}
      planMode={planMode()}
    />
  )
}

function renderComposerWithModeControls(initiallyEnabled = false) {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: () => <ComposerPageWithModeControls initiallyEnabled={initiallyEnabled} /> }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

describe('subscription-backed model disables capabilities it silently drops', () => {
  it('leaves Søk/Bilde/Utfør/Dyp research enabled for a non-subscription model', async () => {
    mockListModels.mockResolvedValue([])
    mockListSubscriptions.mockResolvedValue([])
    renderComposerWithModeControls()

    // Default selection is Verevon Balance (not subscription-backed).
    expect((await screen.findByRole('button', { name: /søk på nett/i }) as HTMLButtonElement).disabled).toBe(false)
    expect((screen.getByRole('button', { name: /dyp research/i }) as HTMLButtonElement).disabled).toBe(false)
    expect((screen.getByRole('button', { name: /generer bilde/i }) as HTMLButtonElement).disabled).toBe(false)
    expect((screen.getByRole('button', { name: /^utfør$/i }) as HTMLButtonElement).disabled).toBe(false)
  })

  it('disables Søk/Bilde/Utfør/Dyp research once a subscription-backed model is selected', async () => {
    mockListModels.mockResolvedValue([
      {
        id: 'gpt-5.6-terra',
        name: 'GPT 5.6 Terra Subscription',
        provider: 'openai-codex-subscription',
      },
    ])
    mockListSubscriptions.mockResolvedValue([{
      id: 'conn-subscription-1',
      providerKey: 'openai-codex-subscription',
      status: 'active',
    }])
    renderComposerWithModeControls()

    fireEvent.click(await screen.findByRole('button', { name: /choose ai model|velg ai-modell/i }))
    fireEvent.click(screen.getByText(/choose a model yourself|velg modell selv/i))
    const choice = await screen.findByRole('button', { name: /gpt 5\.6 terra subscription/i })
    fireEvent.click(choice)
    flush()

    expect((screen.getByRole('button', { name: /søk på nett/i }) as HTMLButtonElement).disabled).toBe(true)
    expect((screen.getByRole('button', { name: /dyp research/i }) as HTMLButtonElement).disabled).toBe(true)
    expect((screen.getByRole('button', { name: /generer bilde/i }) as HTMLButtonElement).disabled).toBe(true)
    expect((screen.getByRole('button', { name: /^utfør$/i }) as HTMLButtonElement).disabled).toBe(true)

    // Each disabled control must also say why — a bare disabled state repeats
    // F-10's original sin of an unlabeled no-op, just inverted.
    expect(screen.getByRole('button', { name: /søk på nett/i }).title).toMatch(
      /ikke tilgjengelig med denne modellen/i,
    )
  })
})

describe('connected subscription model picker', () => {
  it('waits for a slow connection lookup without claiming disconnection or losing the draft', async () => {
    window.localStorage.setItem('verevon.ai-model-selection.v1:org-1', JSON.stringify({
      model: 'gpt-5.6-terra', label: 'Terra', provider: 'openai-codex-subscription', subscriptionConnectionId: 'conn',
    }))
    mockListModels.mockResolvedValue([{ id: 'gpt-5.6-terra', name: 'Terra', provider: 'openai-codex-subscription' }])
    let resolveConnections!: (connections: Array<{ id: string; providerKey: string; status: string }>) => void
    mockListSubscriptions.mockReturnValue(new Promise(resolve => { resolveConnections = resolve }))
    renderComposerWithModeControls()
    const input = await screen.findByRole('textbox') as HTMLTextAreaElement
    fireEvent.input(input, { target: { value: 'Keep this draft while checking' } })
    flush()
    const submit = input.closest('form')!.querySelector('button[type="submit"]') as HTMLButtonElement
    expect(submit.disabled).toBe(true)
    expect(screen.getByText(/kontrollerer ChatGPT-abonnementet/i)).toBeTruthy()
    fireEvent.submit(input.closest('form')!)
    flush()
    expect(submitted).toHaveLength(0)
    expect(input.value).toBe('Keep this draft while checking')
    expect(screen.queryByText(/koble til.*integrasjoner|ikke lenger tilkoblet/i)).toBeNull()

    resolveConnections([{ id: 'conn', providerKey: 'openai-codex-subscription', status: 'active' }])
    await waitFor(() => expect(submit.disabled).toBe(false))
    expect(submitted).toHaveLength(0)
    fireEvent.click(submit)
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toMatchObject({ model: 'gpt-5.6-terra', provider: 'openai-codex-subscription', subscriptionConnectionId: 'conn' })
  })

  it('shows all plan models with the provider catalog and carries the selected connection route', async () => {
    mockListModels.mockResolvedValue([
      {
        id: 'gpt-6-astra',
        name: 'GPT 6 Astra Subscription',
        provider: 'openai-codex-subscription',
      },
      {
        id: 'gpt-5.6-luna',
        name: 'GPT 5.6 Luna Subscription',
        provider: 'openai-codex-subscription',
      },
    ])
    mockListSubscriptions.mockResolvedValue([{
      id: 'conn-subscription-1',
      providerKey: 'openai-codex-subscription',
      status: 'active',
    }])
    renderComposer()

    fireEvent.click(await screen.findByRole('button', { name: /choose ai model|velg ai-modell/i }))
    fireEvent.click(screen.getByText(/choose a model yourself|velg modell selv/i))
    expect(screen.getByRole('region', { name: /available ai models|tilgjengelige ai-modeller/i }).tabIndex).toBe(0)
    expect(await screen.findByText('GPT 6 Astra Subscription')).toBeTruthy()
    const choice = await screen.findByRole('button', { name: /gpt 5\.6 luna subscription/i })
    expect(choice.closest('details')).not.toBeNull()

    fireEvent.click(choice)
    const textbox = screen.getByRole('textbox')
    fireEvent.input(textbox, { target: { value: 'Use my subscription' } })
    flush()
    fireEvent.submit(textbox.closest('form')!)

    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toMatchObject({
      model: 'gpt-5.6-luna',
      provider: 'openai-codex-subscription',
      subscriptionConnectionId: 'conn-subscription-1',
    })
  })
})

// F-08 (CHAT_PARITY_AUDIT_2026-09-15.md §3.1) asked whether the picker can
// strand a user. This was the one remaining way: the selection is persisted per
// org, but the connection that made it routable is not. Once it lapses the
// Subscription group vanishes from the catalog — so the picker shows no way
// back and no sign anything is wrong — while the F-10 mirror keeps four
// controls disabled and the payload keeps naming an unroutable model.
describe('a persisted subscription model whose connection has lapsed', () => {
  it('preserves Terra and blocks sending when its connection is unavailable', async () => {
    window.localStorage.setItem(
      'verevon.ai-model-selection.v1:org-1',
      JSON.stringify({
        model: 'gpt-5.6-terra',
        label: 'GPT 5.6 Terra Subscription',
        provider: 'openai-codex-subscription',
        subscriptionConnectionId: 'conn-subscription-1',
      }),
    )
    mockListModels.mockResolvedValue([
      {
        id: 'gpt-5.6-terra',
        name: 'GPT 5.6 Terra Subscription',
        provider: 'openai-codex-subscription',
      },
    ])
    // The connection is gone (revoked, expired, or reconnected elsewhere).
    mockListSubscriptions.mockResolvedValue([])
    renderComposerWithModeControls()

    expect(
      await screen.findByText(
        /abonnementet er ikke lenger tilkoblet|subscription is no longer connected/i,
      ),
    ).toBeTruthy()
    await waitFor(() =>
      expect(
        (screen.getByRole('button', { name: /søk på nett/i }) as HTMLButtonElement).disabled,
      ).toBe(true),
    )
    expect((screen.getByRole('button', { name: /^utfør$/i }) as HTMLButtonElement).disabled).toBe(true)

    expect(JSON.parse(window.localStorage.getItem('verevon.ai-model-selection.v1:org-1')!).model).toBe('gpt-5.6-terra')
    const input = screen.getByRole('textbox') as HTMLTextAreaElement
    fireEvent.input(input, { target: { value: 'Keep this draft' } })
    flush()
    fireEvent.submit(input.closest('form')!)
    await screen.findByText(/koble til chatgpt-abonnementet i integrasjoner før du sender|connect your chatgpt subscription in integrations before sending/i)
    expect(submitted).toEqual([])
    expect(input.value).toBe('Keep this draft')
  })
})

it('routes a persisted Terra selection while the model catalog is unavailable', async () => {
  window.localStorage.setItem('verevon.ai-model-selection.v1:org-1', JSON.stringify({
    model: 'gpt-5.6-terra', label: 'Terra', provider: 'openai-codex-subscription', subscriptionConnectionId: 'conn',
  }))
  mockListModels.mockRejectedValue(new Error('Catalog unavailable'))
  mockListSubscriptions.mockResolvedValue([{ id: 'conn', providerKey: 'openai-codex-subscription', status: 'active' }])
  renderComposerWithModeControls()
  const input = await screen.findByRole('textbox')
  await waitFor(() => expect(mockListSubscriptions).toHaveBeenCalled())
  flush()
  fireEvent.input(input, { target: { value: 'Use the selected subscription' } })
  flush()
  fireEvent.submit(input.closest('form')!)
  await waitFor(() => expect(submitted).toHaveLength(1))
  expect(submitted[0]).toMatchObject({ model: 'gpt-5.6-terra', provider: 'openai-codex-subscription', subscriptionConnectionId: 'conn' })
})


it('shows unavailable modes as inactive on a persisted Terra selection', async () => {
  window.localStorage.setItem('verevon.ai-model-selection.v1:org-1', JSON.stringify({model:'gpt-5.6-terra',label:'Terra',provider:'openai-codex-subscription',subscriptionConnectionId:'conn'}))
  mockListModels.mockResolvedValue([{id:'gpt-5.6-terra',name:'Terra',provider:'openai-codex-subscription'}])
  mockListSubscriptions.mockResolvedValue([{id:'conn',providerKey:'openai-codex-subscription',status:'active'}])
  renderComposerWithModeControls(true)
  const search = await screen.findByRole('button', {name:/søk på nett/i})
  expect(search.getAttribute('aria-pressed')).toBe('false')
  expect((search as HTMLButtonElement).disabled).toBe(true)
  expect(screen.getByRole('button', {name:/generer bilde/i}).getAttribute('aria-pressed')).toBe('false')
  expect(screen.getByRole('button', {name:/^utfør$/i}).getAttribute('aria-pressed')).toBe('false')
  expect(screen.getByRole('button', {name:/kontekst for dette svaret:/i}).getAttribute('aria-label')).not.toMatch(/nett/i)
})
