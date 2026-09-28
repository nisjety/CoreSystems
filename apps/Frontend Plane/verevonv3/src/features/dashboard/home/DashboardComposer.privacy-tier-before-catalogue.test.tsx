// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ModelInfo } from '@/shared/api/chat-client'
import type { PrivacyTier } from '@/shared/api/privacy-tier'
import { CATALOGUE_TIER_WAIT_MS } from './submit-privacy-tier'

// A turn's `minPrivacyTier` comes from the /api/v1/models catalogue. A turn
// sent before that catalogue had loaded used to go out with no tier at all
// (seen in about 3 of 10 e2e runs). These pin the fix: the send waits for the
// catalogue (bounded), then uses the tier last stored for the model, then the
// strictest tier. It never leaves with no tier for a catalogue model, and a
// send after loading is unchanged.

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

const SELECTION_KEY = 'verevon.ai-model-selection.v1:org-1'
const euModel: ModelInfo = { id: 'm-eu', name: 'EU Model', provider: 'eu-host', privacyTier: 'eu_resident' }
const plainModel: ModelInfo = { id: 'm-plain', name: 'Plain Model', provider: 'plain-host' }

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

/** The selection a previous page load stored, with or without its tier. */
function storeSelection(privacyTier?: PrivacyTier) {
  window.localStorage.setItem(SELECTION_KEY, JSON.stringify({
    model: 'm-eu',
    label: 'EU Model',
    provider: 'eu-host',
    ...(privacyTier ? { privacyTier } : {}),
  }))
}

function storedSelection(): Record<string, unknown> {
  return JSON.parse(window.localStorage.getItem(SELECTION_KEY) ?? '{}') as Record<string, unknown>
}

function deferredCatalogue() {
  let resolve!: (models: ModelInfo[]) => void
  const promise = new Promise<ModelInfo[]>((done) => { resolve = done })
  return { promise, resolve }
}

async function typeDraft(text: string) {
  const input = await screen.findByRole('textbox')
  fireEvent.input(input, { target: { value: text } })
  flush()
  return input
}

function submitForm(input: HTMLElement) {
  const form = input.closest('form')
  if (!form) throw new Error('composer form not found')
  fireEvent.submit(form)
  flush()
}

beforeEach(() => {
  mockListSubscriptions.mockResolvedValue([])
})

afterEach(() => {
  vi.useRealTimers()
  submitted.length = 0
  vi.clearAllMocks()
  window.localStorage.clear()
})

describe('a send before the catalogue has loaded', () => {
  it('waits for the catalogue and carries the model’s own tier', async () => {
    storeSelection()
    const catalogue = deferredCatalogue()
    mockListModels.mockReturnValue(catalogue.promise)
    renderComposer()

    const input = await typeDraft('Hei')
    submitForm(input)
    // Held, not sent without a tier; the composer shows it is busy.
    await Promise.resolve()
    expect(submitted).toHaveLength(0)
    expect(document.querySelector('[aria-busy="true"]')).toBeTruthy()

    catalogue.resolve([euModel])
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toMatchObject({ model: 'm-eu', minPrivacyTier: 'eu_resident', text: 'Hei' })
    expect(mockListModels).toHaveBeenCalledTimes(1)
  })

  it('sends the last-known tier once the bounded wait runs out', async () => {
    storeSelection('zdr_contractual')
    mockListModels.mockReturnValue(new Promise(() => {}))
    renderComposer()
    const input = await typeDraft('Hei')

    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    submitForm(input)
    await vi.advanceTimersByTimeAsync(CATALOGUE_TIER_WAIT_MS - 1)
    expect(submitted).toHaveLength(0)
    await vi.advanceTimersByTimeAsync(1)
    vi.useRealTimers()

    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toMatchObject({ model: 'm-eu', minPrivacyTier: 'zdr_contractual' })
  })

  it('fails closed to the strictest tier when nothing is known about the model', async () => {
    storeSelection()
    mockListModels.mockReturnValue(new Promise(() => {}))
    renderComposer()
    const input = await typeDraft('Hei')

    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    submitForm(input)
    await vi.advanceTimersByTimeAsync(CATALOGUE_TIER_WAIT_MS)
    vi.useRealTimers()

    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toMatchObject({ model: 'm-eu', minPrivacyTier: 'sovereign' })
  })

  it('fails closed at once, without the wait, when the catalogue request failed', async () => {
    storeSelection()
    mockListModels.mockRejectedValue(new Error('Catalog unavailable'))
    renderComposer()
    const input = await typeDraft('Hei')
    submitForm(input)

    // waitFor gives up after 1 s, well inside the 3 s bound: no wait happened.
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toMatchObject({ model: 'm-eu', minPrivacyTier: 'sovereign' })
  })

  it('uses the last-known tier when the catalogue request failed', async () => {
    storeSelection('global')
    mockListModels.mockRejectedValue(new Error('Catalog unavailable'))
    renderComposer()
    const input = await typeDraft('Hei')
    submitForm(input)

    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toMatchObject({ model: 'm-eu', minPrivacyTier: 'global' })
  })

  it('sends an intent mode at once and with no tier, as before', async () => {
    // Default selection: Verevon Balance, resolved server-side.
    mockListModels.mockReturnValue(new Promise(() => {}))
    renderComposer()
    const input = await typeDraft('Hei')
    submitForm(input)

    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).not.toHaveProperty('minPrivacyTier')
  })
})

describe('a send after the catalogue has loaded', () => {
  it('is unchanged: the listed tier, or no key for a model that claims none', async () => {
    storeSelection()
    mockListModels.mockResolvedValue([euModel, plainModel])
    renderComposer()
    const trigger = await screen.findByRole('button', { name: /velg ai-modell/i })
    await waitFor(() => expect(trigger.textContent).toContain('EU Model'))

    const input = await typeDraft('Hei')
    submitForm(input)
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]?.minPrivacyTier).toBe('eu_resident')

    fireEvent.click(trigger)
    fireEvent.click(await screen.findByText('Plain Model'))
    await typeDraft('Hei igjen')
    submitForm(input)
    await waitFor(() => expect(submitted).toHaveLength(2))
    expect(submitted[1]).toMatchObject({ model: 'm-plain', text: 'Hei igjen' })
    expect(submitted[1]).not.toHaveProperty('minPrivacyTier')
    expect(mockListModels).toHaveBeenCalledTimes(1)
  })

  it('stores the listed tier as the last-known one for the next page load', async () => {
    storeSelection()
    mockListModels.mockResolvedValue([euModel, plainModel])
    renderComposer()

    await waitFor(() => expect(storedSelection().privacyTier).toBe('eu_resident'))
    expect(storedSelection()).toMatchObject({ model: 'm-eu', provider: 'eu-host' })

    // Picking a model stores its tier too, "claims none" included.
    fireEvent.click(await screen.findByRole('button', { name: /velg ai-modell/i }))
    fireEvent.click(await screen.findByText('Plain Model'))
    expect(storedSelection()).toMatchObject({ model: 'm-plain', privacyTier: 'unspecified' })
  })
})
