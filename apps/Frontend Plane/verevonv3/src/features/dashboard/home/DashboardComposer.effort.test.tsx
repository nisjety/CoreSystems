// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ModelInfo } from '@/shared/api/chat-client'
import { consumePendingChatLaunch, writePendingChatLaunch } from '@/features/chat/lib/pending-chat-launch'

// The response-mode select of the chat composer follows the catalogue: three
// profiles for every model, plus Maksimal only while the selected model lists
// `xhigh`, sent as that canonical level.

const { mockListModels } = vi.hoisted(() => ({ mockListModels: vi.fn() }))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: mockListModels,
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

import { DashboardComposer } from './DashboardComposer'

function catalogModel(overrides: Partial<ModelInfo>): ModelInfo {
  return { id: 'm-1', name: 'Test Model', ...overrides }
}

const submitted: Array<Record<string, unknown>> = []

function ComposerPage(props: { roundTrip: boolean }) {
  const [message, setMessage] = createSignal('')
  return (
    <DashboardComposer
      appearance="chat"
      message={message()}
      onMessageChange={setMessage}
      onSubmit={async (payload) => {
        submitted.push(payload as unknown as Record<string, unknown>)
        if (props.roundTrip) await writePendingChatLaunch(payload)
      }}
    />
  )
}

function renderComposer(models: ModelInfo[], roundTrip = false) {
  mockListModels.mockResolvedValue(models)
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: () => <ComposerPage roundTrip={roundTrip} /> }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

async function selectModel(name: string) {
  fireEvent.click(await screen.findByRole('button', { name: /velg ai-modell/i }))
  await waitFor(() => expect(document.querySelector('.dashboard-composer-model-menu')).toBeTruthy())
  fireEvent.click(await screen.findByText(name))
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
  consumePendingChatLaunch()
  vi.clearAllMocks()
})

describe('composer response-mode select', () => {
  it('offers_three_profiles_by_default', async () => {
    renderComposer([catalogModel({ id: 'm-plain', name: 'Plain Model' })])
    await screen.findByRole('button', { name: /velg ai-modell/i })
    expect(optionValues()).toEqual(['quick', 'auto', 'deep'])

    // A model whose ladder stops below xhigh does not get Maksimal either.
    await selectModel('Plain Model')
    expect(optionValues()).toEqual(['quick', 'auto', 'deep'])
  })

  it('offers_maksimal_only_when_the_model_lists_xhigh', async () => {
    renderComposer([
      catalogModel({ id: 'm-high', name: 'High Model', effortLevels: ['low', 'medium', 'high'] }),
      catalogModel({ id: 'm-xhigh', name: 'Xhigh Model', effortLevels: ['low', 'medium', 'high', 'xhigh'] }),
    ])

    await selectModel('High Model')
    expect(optionValues()).toEqual(['quick', 'auto', 'deep'])

    await selectModel('Xhigh Model')
    await waitFor(() => expect(optionValues()).toEqual(['quick', 'auto', 'deep', 'max']))
    expect(Array.from(effortSelect().options).at(-1)?.textContent).toBe('Maksimal')

    fireEvent.change(effortSelect(), { target: { value: 'max' } })
    await send('Tenk grundig')
    expect(submitted[0]?.effort).toBe('xhigh')

    // Back on a model without the level: the pick falls back to Deep instead of
    // sending a level that model never listed.
    submitted.length = 0
    await selectModel('High Model')
    await waitFor(() => expect(optionValues()).toEqual(['quick', 'auto', 'deep']))
    expect(effortSelect().value).toBe('deep')
    await send('Og igjen')
    expect(submitted[0]?.effort).toBe('deep')
  })

  it('sends no effort for the standard pick', async () => {
    renderComposer([catalogModel({ id: 'm-xhigh', name: 'Xhigh Model', effortLevels: ['high', 'xhigh'] })])
    await selectModel('Xhigh Model')
    await send('Hei')
    expect(submitted[0]).not.toHaveProperty('effort')
  })

  it('launch_effort_survives_roundtrip', async () => {
    renderComposer([catalogModel({ id: 'm-xhigh', name: 'Xhigh Model', effortLevels: ['high', 'xhigh'] })], true)
    await selectModel('Xhigh Model')
    await waitFor(() => expect(optionValues()).toContain('max'))
    fireEvent.change(effortSelect(), { target: { value: 'max' } })
    await send('Tenk grundig')

    await waitFor(() => expect(consumePendingChatLaunch()).toMatchObject({ effort: 'xhigh' }))
  })
})
