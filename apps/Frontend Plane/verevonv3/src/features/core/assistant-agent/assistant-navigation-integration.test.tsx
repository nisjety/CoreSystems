import { fireEvent, render, screen, waitFor, cleanup } from '@solidjs/testing-library'
import { createSignal, flush, onCleanup } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { InlineAssistantState } from '@/features/core/lib/inline-assistant'

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => ({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } }) }))

import { GlobalAssistant } from '@/features/core/components/GlobalAssistant'
import { AssistantNavigationCursor, createAssistantNavigationGuide } from '@/features/core/assistant-navigation/assistant-navigation'
import { createAssistantNavigationAgent, type AssistantNavigationAgentState } from './assistant-navigation-agent'

const empty: InlineAssistantState = { turns: [], status: 'idle', error: null, activity: null, threadId: null, needsFullChat: false }
const proposal = (mode: 'point' | 'navigate') => JSON.stringify({ type: 'assistant.navigation.guide', href: '/knowledge', mode })

beforeEach(() => {
  localStorage.clear(); sessionStorage.clear(); window.history.replaceState({}, '', '/')
  const nav = document.createElement('nav')
  nav.className = 'core-sidebar__mini-nav'
  nav.innerHTML = '<a href="/knowledge">Knowledge</a>'
  document.body.append(nav)
  nav.querySelector('a')!.getBoundingClientRect = () => ({ x: 10, y: 100, left: 10, top: 100, right: 50, bottom: 140, width: 40, height: 40, toJSON() {} })
})
afterEach(() => { cleanup(); document.querySelector('nav.core-sidebar__mini-nav')?.remove() })

function mount(infer: (prompt: string, signal: AbortSignal) => Promise<{ content: string }>) {
  const navigate = vi.fn((href: string) => window.history.pushState({}, '', href)), completed = vi.fn()
  let guide!: ReturnType<typeof createAssistantNavigationGuide>
  const view = render(() => {
    const [message, setMessage] = createSignal('')
    const [state, setState] = createSignal<AssistantNavigationAgentState>({ status: 'idle', error: null })
    guide = createAssistantNavigationGuide({ navigate, onComplete: completed, onOpenChat: vi.fn() })
    const agent = createAssistantNavigationAgent({ scope: () => 'user-a:org-a', onChange: setState,
      onAction: action => guide.startAction(action), dependencies: { infer } })
    onCleanup(() => { agent.dispose(); guide.dispose() })
    return <><AssistantNavigationCursor guide={guide} /><GlobalAssistant forceReveal message={message()} onMessageChange={setMessage}
      onBeforeSubmit={() => false} canHandleBeforeSubmit={() => false} onSubmit={agent.send} navigationAgent={state()}
      onStopNavigation={agent.stop} onClose={() => {}} onStop={() => {}} onOpenChat={() => {}} onNewChat={() => {}}
      sourceScope="workspace" onSourceScopeChange={() => {}} state={empty} /></>
  })
  return { ...view, navigate, completed, guide: () => guide }
}

describe('assistant task input and model navigation', () => {
  it.each(['point', 'navigate'] as const)('sends the request to the model and %s only after a completed valid proposal', async (mode) => {
    let release!: (value: { content: string }) => void
    const infer = vi.fn((_prompt: string, _signal: AbortSignal) => new Promise<{ content: string }>(resolve => { release = resolve }))
    const { guide, navigate } = mount(infer)
    fireEvent.input(screen.getByRole('textbox', { name: 'Task for Verevon' }), { target: { value: 'Find the knowledge page' } }); flush()
    fireEvent.click(screen.getByRole('button', { name: 'Start task' })); flush()
    expect(infer).toHaveBeenCalledOnce()
    expect(guide().destination()).toBeNull()
    release({ content: proposal(mode) }); flush()
    await waitFor(() => expect(guide().destination()?.href).toBe('/knowledge'))
    await waitFor(() => expect(guide().destination()).toBeNull(), { timeout: 3500 })
    if (mode === 'navigate') expect(navigate).toHaveBeenCalledExactlyOnceWith('/knowledge')
    else expect(navigate).not.toHaveBeenCalled()
  })

  it('keeps the draft and does not start a guide when the model fails', async () => {
    const infer = vi.fn(async () => { throw new Error('Model unavailable') })
    const { guide } = mount(infer)
    const input = screen.getByRole('textbox', { name: 'Task for Verevon' }) as HTMLTextAreaElement
    fireEvent.input(input, { target: { value: 'Find the knowledge page' } }); flush()
    fireEvent.click(screen.getByRole('button', { name: 'Start task' })); flush()
    await waitFor(() => expect(screen.getByText('Model unavailable')).toBeTruthy())
    expect(input.value).toBe('Find the knowledge page')
    expect(guide().destination()).toBeNull()
  })
})
