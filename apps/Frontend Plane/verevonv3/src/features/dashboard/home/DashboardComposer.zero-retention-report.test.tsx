// @vitest-environment jsdom

// Home keeps an unsent draft in storage only while the next turn is not
// zero-retention (Temporary chat). It cannot see the composer's own state, so
// the composer reports it: once when it appears, on every change, and
// `undefined` when it goes away, which Home reads as "not known, write nothing".

import { createRouter, memoryHistory } from '@solidjs/router'
import { render } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

vi.mock('@/shared/api/chat-actions-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-actions-client')>()
  return { ...actual, loadSpecializedActions: vi.fn().mockResolvedValue(actual.BUILTIN_ACTIONS) }
})

import { DashboardComposer } from './DashboardComposer'

function renderComposer(report: (zeroRetention: boolean | undefined) => void, options: { reporting?: boolean } = {}) {
  const [temporaryChat, setTemporaryChat] = createSignal(false)
  const [locked, setLocked] = createSignal(false)
  function ComposerPage() {
    const [message, setMessage] = createSignal('')
    return (
      <DashboardComposer
        message={message()}
        onMessageChange={setMessage}
        onSubmit={() => undefined}
        onZeroRetentionChange={options.reporting === false ? undefined : report}
        temporaryChat={temporaryChat()}
        temporaryChatLocked={locked()}
      />
    )
  }
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: ComposerPage }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  const view = render(() => <TestRouter>{(routed) => <>{routed.children}</>}</TestRouter>)
  return {
    ...view,
    setTemporaryChat: (value: boolean) => { setTemporaryChat(value); flush() },
    setLocked: (value: boolean) => { setLocked(value); flush() },
  }
}

describe('DashboardComposer reports zero retention to its parent', () => {
  it('reports an ordinary turn when it appears', () => {
    const report = vi.fn()
    renderComposer(report)
    expect(report).toHaveBeenCalledTimes(1)
    expect(report).toHaveBeenLastCalledWith(false)
  })

  it('reports Temporary chat as it is switched on and off', () => {
    const report = vi.fn()
    const view = renderComposer(report)
    view.setTemporaryChat(true)
    expect(report).toHaveBeenLastCalledWith(true)
    view.setTemporaryChat(false)
    expect(report).toHaveBeenLastCalledWith(false)
    expect(report.mock.calls.map(([value]) => value)).toEqual([false, true, false])
  })

  it('reports a thread that is locked as temporary', () => {
    const report = vi.fn()
    const view = renderComposer(report)
    view.setLocked(true)
    expect(report).toHaveBeenLastCalledWith(true)
  })

  it('reports that it is gone, so the parent no longer treats the state as known', () => {
    const report = vi.fn()
    const view = renderComposer(report)
    view.unmount()
    expect(report).toHaveBeenLastCalledWith(undefined)
  })

  it('needs no listener', () => {
    const view = renderComposer(vi.fn(), { reporting: false })
    view.setTemporaryChat(true)
    expect(() => view.unmount()).not.toThrow()
  })
})
