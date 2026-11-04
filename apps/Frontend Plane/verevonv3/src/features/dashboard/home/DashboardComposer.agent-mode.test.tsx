// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AgentModeId, ModelInfo } from '@/shared/api/chat-client'

// The organisation's agent-mode cap (WS25-S11) limits the composer's picker to the cap and the modes stricter
// than it. A choice is sent on that turn only, and a choice the cap no longer admits is dropped.

const { mockListModels } = vi.hoisted(() => ({
  mockListModels: vi.fn(),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: mockListModels,
    getModelEntitlementMeta: vi.fn(() => undefined),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'member' } }),
}))

import { I18nProvider } from '@/shared/i18n'
import { DashboardComposer } from './DashboardComposer'

const submitted: Array<Record<string, unknown>> = []

const model = (id: string): ModelInfo => ({ id, name: id, provider: 'openai', modality: 'chat' })

function ParentComposer(props: { cap?: AgentModeId; initial?: AgentModeId }) {
  const [message, setMessage] = createSignal('')
  const [mode, setMode] = createSignal<AgentModeId | undefined>(props.initial)
  return (
    <DashboardComposer
      appearance="chat"
      message={message()}
      onMessageChange={setMessage}
      agentModeCap={props.cap}
      agentMode={mode()}
      onAgentModeChange={setMode}
      onSubmit={(payload) => {
        submitted.push(payload as unknown as Record<string, unknown>)
      }}
    />
  )
}

function renderComposer(props: { cap?: AgentModeId; initial?: AgentModeId }) {
  mockListModels.mockResolvedValue([model('gpt-mini')])
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: () => <I18nProvider><ParentComposer {...props} /></I18nProvider> }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(routeProps) => <>{routeProps.children}</>}</TestRouter>)
}

function picker(): HTMLSelectElement | null {
  return screen.queryByRole('combobox', { name: 'Agent mode' }) as HTMLSelectElement | null
}

function optionValues(select: HTMLSelectElement): string[] {
  return Array.from(select.options).map((option) => option.value)
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
  window.localStorage.setItem('verevon.locale', 'en')
  window.sessionStorage.clear()
})

afterEach(() => {
  submitted.length = 0
  vi.clearAllMocks()
})

describe('the composer agent-mode picker', () => {
  it('is not offered while the organisation has no cap to limit it by', async () => {
    renderComposer({})
    await waitFor(() => expect(mockListModels).toHaveBeenCalled())
    expect(picker()).toBeNull()
  })

  it('offers the cap and the modes stricter than it, and nothing looser', async () => {
    renderComposer({ cap: 'read_only' })
    await waitFor(() => expect(picker()).not.toBeNull())
    expect(optionValues(picker() as HTMLSelectElement)).toEqual(['lockdown', 'read_only'])
    expect((picker() as HTMLSelectElement).value).toBe('read_only')
  })

  it('sends the mode the person picked on that turn', async () => {
    renderComposer({ cap: 'read_only' })
    await waitFor(() => expect(picker()).not.toBeNull())
    fireEvent.change(picker() as HTMLSelectElement, { target: { value: 'lockdown' } })
    flush()

    await send('Hei')
    expect(submitted[0]?.agentMode).toBe('lockdown')
  })

  it('sends no agent mode when the person has not picked one, so the turn is unchanged', async () => {
    renderComposer({ cap: 'read_only' })
    await waitFor(() => expect(picker()).not.toBeNull())

    await send('Hei')
    expect('agentMode' in (submitted[0] ?? {})).toBe(false)
  })

  it('drops a choice the cap no longer admits, and does not send it', async () => {
    renderComposer({ cap: 'read_only', initial: 'auto' })
    await waitFor(() => expect(picker()).not.toBeNull())
    await waitFor(() => expect((picker() as HTMLSelectElement).value).toBe('read_only'))

    await send('Hei')
    expect('agentMode' in (submitted[0] ?? {})).toBe(false)
  })
})
