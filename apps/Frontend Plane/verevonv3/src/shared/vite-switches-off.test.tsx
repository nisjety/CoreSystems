// @vitest-environment jsdom

// Verification plan 3.4: each VITE_* switch added in this phase reads only the literal "true", and off it renders
// and sends nothing new. Covered here: VITE_SHELL_TOASTS, VITE_CHAT_CHANGED_FILES, VITE_CHAT_DOUBLE_ESCAPE_STOP and
// VITE_CHAT_SPEED_TIER (the composer's Fast chip). Each case has an "on" control, so an "off" pass cannot come from
// a fixture that never renders the feature at all.

import { createRouter, memoryHistory } from '@solidjs/router'
import { render, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: vi.fn().mockResolvedValue([
      { id: 'gpt-fast-1', name: 'GPT Fast 1', provider: 'openai', modality: 'chat', priorityTier: true },
    ]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})
vi.mock('@/shared/api/chat-actions-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-actions-client')>()
  return { ...actual, loadSpecializedActions: vi.fn().mockResolvedValue(actual.BUILTIN_ACTIONS) }
})
vi.mock('@/shared/ai/model-selection', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/ai/model-selection')>()
  return { ...actual, readAiModelSelection: () => ({ model: 'gpt-fast-1', provider: 'openai' }) }
})

import { ChangedFilesCard, chatChangedFilesEnabled } from '@/features/chat/components/ChangedFilesCard'
import { chatDoubleEscapeStopEnabled, createDoubleEscape } from '@/features/chat/lib/use-chat-shortcuts'
import { DashboardComposer } from '@/features/dashboard/home/DashboardComposer'
import { dismissToast, shellToasts, shellToastsEnabled, showToast } from '@/shared/ui/toast/toast-store'

const NOT_TRUE = [undefined, '', '1', 'TRUE', 'True', 'yes', 'on', ' true'] as const

function stub(name: string, value: string | undefined) {
  if (value !== undefined) vi.stubEnv(name, value)
}

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('VITE_SHELL_TOASTS', () => {
  afterEach(() => {
    for (const toast of shellToasts()) dismissToast(toast.id)
    flush()
    vi.unstubAllEnvs()
  })

  it.each(NOT_TRUE)('off for %j: showToast shows nothing and returns an empty id', (value) => {
    stub('VITE_SHELL_TOASTS', value)
    expect(shellToastsEnabled()).toBe(false)
    expect(showToast({ tone: 'info', message: 'Lagret' })).toBe('')
    flush()
    expect(shellToasts()).toEqual([])
  })

  it('on for the literal "true" (control)', () => {
    vi.stubEnv('VITE_SHELL_TOASTS', 'true')
    expect(showToast({ tone: 'info', message: 'Lagret' })).not.toBe('')
    flush()
    expect(shellToasts().length).toBe(1)
  })
})

describe('VITE_CHAT_CHANGED_FILES', () => {
  it.each(NOT_TRUE)('off for %j', (value) => {
    stub('VITE_CHAT_CHANGED_FILES', value)
    expect(chatChangedFilesEnabled()).toBe(false)
  })

  it('on for the literal "true", and the card itself renders nothing for an empty list', () => {
    vi.stubEnv('VITE_CHAT_CHANGED_FILES', 'true')
    expect(chatChangedFilesEnabled()).toBe(true)
    const { container, unmount } = render(() => <ChangedFilesCard files={[]} />)
    expect(container.textContent).toBe('')
    unmount()
  })
})

describe('VITE_CHAT_DOUBLE_ESCAPE_STOP', () => {
  const escape = { key: 'Escape', altKey: false, ctrlKey: false, metaKey: false, shiftKey: false }

  it.each(NOT_TRUE)('off for %j: two quick Escapes never stop the run', (value) => {
    stub('VITE_CHAT_DOUBLE_ESCAPE_STOP', value)
    expect(chatDoubleEscapeStopEnabled()).toBe(false)
    const feed = createDoubleEscape()
    expect(feed(escape, null, 1000)).toBeNull()
    expect(feed(escape, null, 1100)).toBeNull()
  })

  it('on for the literal "true": the second Escape inside the window stops the run (control)', () => {
    vi.stubEnv('VITE_CHAT_DOUBLE_ESCAPE_STOP', 'true')
    const feed = createDoubleEscape()
    expect(feed(escape, null, 1000)).toBeNull()
    expect(feed(escape, null, 1100)).toEqual({ type: 'stop-run' })
  })
})

describe('VITE_CHAT_SPEED_TIER (the composer Fast chip)', () => {
  function renderComposer() {
    function Page() {
      const [message, setMessage] = createSignal('')
      return (
        <DashboardComposer
          message={message()}
          onMessageChange={setMessage}
          onSubmit={() => undefined}
          speedTier="standard"
          onSpeedTierChange={() => undefined}
        />
      )
    }
    const TestRouter = createRouter({
      routes: [{ path: '/*all', component: Page }],
      history: memoryHistory(),
      explicitLinks: true,
    })
    return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
  }
  const FAST = /Fast: prioritert tjeneste|Fast: priority tier/

  async function settle() {
    for (let i = 0; i < 5; i++) {
      await new Promise((resolve) => setTimeout(resolve, 0))
      flush()
    }
  }

  it.each(NOT_TRUE)('off for %j: no Fast chip on a model that declares the priority tier', async (value) => {
    stub('VITE_CHAT_SPEED_TIER', value)
    const { unmount } = renderComposer()
    await settle()
    expect(screen.queryByRole('button', { name: FAST })).toBeNull()
    unmount()
  })

  it('on for the literal "true": the chip shows on that model (control)', async () => {
    vi.stubEnv('VITE_CHAT_SPEED_TIER', 'true')
    const { unmount } = renderComposer()
    expect(await screen.findByRole('button', { name: FAST })).toBeTruthy()
    unmount()
  })
})
