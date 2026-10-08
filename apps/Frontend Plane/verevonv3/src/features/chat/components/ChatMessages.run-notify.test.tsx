// @vitest-environment jsdom

/**
 * "Varsle meg når ferdig" in the waiting state (chat gap-closure plan, task D9).
 *
 * A plain turn on a slow route (the ChatGPT subscription waits 60 to 95 seconds
 * for a first token) is the case this control is for: the reader can leave and
 * be told when the run finishes. The control rides the existing run-watcher
 * endpoints through `runs-client`, mocked here.
 *
 * What is pinned: when it appears (after the ten-second threshold, only with a
 * run id, never on a temporary thread), that it toggles through the watcher
 * endpoints and reports the state it really has, that a failed call is said out
 * loud rather than reverted silently, that a rebuilt indicator neither forgets
 * the state nor asks the server again, and that a waiting state without the new
 * prop renders exactly as it did before.
 */

import { fireEvent, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const hoisted = vi.hoisted(() => ({
  watchRun: vi.fn(),
  unwatchRun: vi.fn(),
  getRunWatchStatus: vi.fn(),
}))

vi.mock('@/shared/api/runs-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/runs-client')>()
  return {
    ...actual,
    watchRun: hoisted.watchRun,
    unwatchRun: hoisted.unwatchRun,
    getRunWatchStatus: hoisted.getRunWatchStatus,
  }
})

import { AnswerRegion, AssistantMessage, ThinkingDots } from './ChatMessages'
import { __resetRunNotifyForTests } from './RunNotifyButton'
import { deriveConversationNodes } from '@/shared/chat-nodes'
import type { ChatTurn } from './chat-types'

const SINCE = '2026-09-15T08:00:00.000Z'

/** Let the awaited status and toggle calls settle, then commit Solid's writes. */
async function settle() {
  for (let step = 0; step < 6; step += 1) await Promise.resolve()
  flush()
}

const notifyButton = (container: HTMLElement) =>
  container.querySelector<HTMLButtonElement>('button.verevon-run-watch-toggle')

/** Wait as long as the indicator has been waiting, from the turn's own start. */
function waitedFor(seconds: number) {
  vi.useFakeTimers()
  vi.setSystemTime(new Date(Date.parse(SINCE) + seconds * 1000))
}

beforeEach(() => {
  __resetRunNotifyForTests()
  hoisted.watchRun.mockReset().mockResolvedValue({ watching: true })
  hoisted.unwatchRun.mockReset().mockResolvedValue(undefined)
  hoisted.getRunWatchStatus.mockReset().mockResolvedValue({ watching: false })
})

afterEach(() => {
  vi.useRealTimers()
})

describe('"Varsle meg" beside the elapsed counter', () => {
  it('is absent before the ten-second threshold, even with a run id', async () => {
    waitedFor(9)
    const { container, unmount } = render(() => <ThinkingDots since={SINCE} runId="run-1" />)
    await settle()
    expect(notifyButton(container)).toBeNull()
    expect(container.querySelector('.verevon-chat-thinking__elapsed')).toBeNull()
    // Not even a status read before the control exists: an ordinary wait costs nothing.
    expect(hoisted.getRunWatchStatus).not.toHaveBeenCalled()
    unmount()
  })

  it('appears once the wait crosses the threshold and the turn has a run id', async () => {
    waitedFor(12)
    const { container, unmount } = render(() => <ThinkingDots since={SINCE} runId="run-1" />)
    await settle()
    const button = notifyButton(container)
    expect(button).not.toBeNull()
    expect(button?.textContent).toContain('Varsle meg når ferdig')
    expect(button?.getAttribute('aria-pressed')).toBe('false')
    expect(container.querySelector('.verevon-chat-thinking__elapsed')?.textContent).toBe('12 s')
    unmount()
  })

  it('appears when the counter is crossed while the indicator is already on screen', async () => {
    waitedFor(8)
    const { container, unmount } = render(() => <ThinkingDots since={SINCE} runId="run-1" />)
    await settle()
    expect(notifyButton(container)).toBeNull()
    vi.advanceTimersByTime(3_000)
    await settle()
    expect(notifyButton(container)).not.toBeNull()
    unmount()
  })

  it('is absent without a run id, however long the wait', async () => {
    waitedFor(95)
    const { container, unmount } = render(() => <ThinkingDots since={SINCE} />)
    await settle()
    expect(container.querySelector('.verevon-chat-thinking__elapsed')?.textContent).toBe('1 min 35 s')
    expect(notifyButton(container)).toBeNull()
    expect(hoisted.getRunWatchStatus).not.toHaveBeenCalled()
    unmount()
  })

  it('renders exactly as before when the new prop is not given', async () => {
    waitedFor(45)
    const { container, unmount } = render(() => <ThinkingDots since={SINCE} />)
    await settle()
    expect(container.textContent).toBe('Tenker45 s')
    expect(container.querySelectorAll('button')).toHaveLength(0)
    unmount()
  })

  it('shows the state the server already holds for this run', async () => {
    hoisted.getRunWatchStatus.mockResolvedValue({ watching: true })
    waitedFor(30)
    const { container, unmount } = render(() => <ThinkingDots since={SINCE} runId="run-1" />)
    await settle()
    expect(hoisted.getRunWatchStatus).toHaveBeenCalledWith('run-1')
    expect(notifyButton(container)?.getAttribute('aria-pressed')).toBe('true')
    expect(notifyButton(container)?.textContent).toContain('Du varsles når ferdig')
    unmount()
  })

  it('turns the watch on and off through the watcher endpoints and shows each state', async () => {
    waitedFor(30)
    const { container, unmount } = render(() => <ThinkingDots since={SINCE} runId="run-1" />)
    await settle()

    fireEvent.click(notifyButton(container)!)
    await settle()
    expect(hoisted.watchRun).toHaveBeenCalledTimes(1)
    expect(hoisted.watchRun.mock.calls[0]?.[0]).toBe('run-1')
    expect(hoisted.unwatchRun).not.toHaveBeenCalled()
    expect(notifyButton(container)?.getAttribute('aria-pressed')).toBe('true')
    expect(notifyButton(container)?.textContent).toContain('Du varsles når ferdig')

    fireEvent.click(notifyButton(container)!)
    await settle()
    expect(hoisted.unwatchRun).toHaveBeenCalledTimes(1)
    expect(hoisted.unwatchRun.mock.calls[0]?.[0]).toBe('run-1')
    expect(hoisted.watchRun).toHaveBeenCalledTimes(1)
    expect(notifyButton(container)?.getAttribute('aria-pressed')).toBe('false')
    expect(notifyButton(container)?.textContent).toContain('Varsle meg når ferdig')
    unmount()
  })

  it('says so, and keeps the old state, when the watch could not be registered', async () => {
    hoisted.watchRun.mockRejectedValue(new Error('capability-core unavailable'))
    waitedFor(30)
    const { container, unmount } = render(() => <ThinkingDots since={SINCE} runId="run-1" />)
    await settle()

    fireEvent.click(notifyButton(container)!)
    await settle()
    expect(notifyButton(container)?.getAttribute('aria-pressed')).toBe('false')
    expect(container.querySelector('[role="alert"]')?.textContent).toContain('Kunne ikke slå på varsling')

    // The next attempt starts clean: the old complaint is not left standing.
    hoisted.watchRun.mockResolvedValue({ watching: true })
    fireEvent.click(notifyButton(container)!)
    await settle()
    expect(container.querySelector('[role="alert"]')).toBeNull()
    expect(notifyButton(container)?.getAttribute('aria-pressed')).toBe('true')
    unmount()
  })

  it('says so when the watch could not be cancelled', async () => {
    hoisted.getRunWatchStatus.mockResolvedValue({ watching: true })
    hoisted.unwatchRun.mockRejectedValue(new Error('capability-core unavailable'))
    waitedFor(30)
    const { container, unmount } = render(() => <ThinkingDots since={SINCE} runId="run-1" />)
    await settle()

    fireEvent.click(notifyButton(container)!)
    await settle()
    expect(notifyButton(container)?.getAttribute('aria-pressed')).toBe('true')
    expect(container.querySelector('[role="alert"]')?.textContent).toContain('Kunne ikke slå av varsling')
    unmount()
  })

  it('ignores a second click while the first is still in flight', async () => {
    let release: (value: { watching: boolean }) => void = () => {}
    hoisted.watchRun.mockReturnValue(new Promise((resolve) => { release = resolve }))
    waitedFor(30)
    const { container, unmount } = render(() => <ThinkingDots since={SINCE} runId="run-1" />)
    await settle()

    fireEvent.click(notifyButton(container)!)
    await settle()
    fireEvent.click(notifyButton(container)!)
    await settle()
    expect(hoisted.watchRun).toHaveBeenCalledTimes(1)
    expect(hoisted.unwatchRun).not.toHaveBeenCalled()

    release({ watching: true })
    await settle()
    expect(notifyButton(container)?.getAttribute('aria-pressed')).toBe('true')
    unmount()
  })

  /**
   * The transcript rebuilds this indicator on every derivation (see the note on
   * `ThinkingDots`). A control that started from "off" and asked the server
   * again each time would flicker and flood the endpoint on a streaming turn.
   */
  it('keeps its state, and asks the server once, when the indicator is rebuilt', async () => {
    waitedFor(30)
    const first = render(() => <ThinkingDots since={SINCE} runId="run-1" />)
    await settle()
    fireEvent.click(notifyButton(first.container)!)
    await settle()
    expect(notifyButton(first.container)?.getAttribute('aria-pressed')).toBe('true')
    first.unmount()

    const second = render(() => <ThinkingDots since={SINCE} runId="run-1" />)
    await settle()
    expect(notifyButton(second.container)?.getAttribute('aria-pressed')).toBe('true')
    expect(hoisted.getRunWatchStatus).toHaveBeenCalledTimes(1)
    second.unmount()
  })

  it('does not let a status read that lands after a click overwrite what the reader chose', async () => {
    let answer: (value: { watching: boolean }) => void = () => {}
    hoisted.getRunWatchStatus.mockReturnValue(new Promise((resolve) => { answer = resolve }))
    waitedFor(30)
    const { container, unmount } = render(() => <ThinkingDots since={SINCE} runId="run-1" />)
    await settle()
    expect(hoisted.getRunWatchStatus).toHaveBeenCalledTimes(1)

    // The reader presses the control while the first read is still on its way.
    fireEvent.click(notifyButton(container)!)
    await settle()
    expect(notifyButton(container)?.getAttribute('aria-pressed')).toBe('true')

    // The late read reports the state from before the click; it must not undo it.
    answer({ watching: false })
    await settle()
    expect(notifyButton(container)?.getAttribute('aria-pressed')).toBe('true')
    unmount()
  })

  it('keeps one run apart from another', async () => {
    waitedFor(30)
    const first = render(() => <ThinkingDots since={SINCE} runId="run-1" />)
    await settle()
    fireEvent.click(notifyButton(first.container)!)
    await settle()
    first.unmount()

    const other = render(() => <ThinkingDots since={SINCE} runId="run-2" />)
    await settle()
    expect(notifyButton(other.container)?.getAttribute('aria-pressed')).toBe('false')
    other.unmount()
  })

  it('treats an unreadable status as "not watching" rather than breaking the indicator', async () => {
    hoisted.getRunWatchStatus.mockRejectedValue(new Error('status unavailable'))
    waitedFor(30)
    const { container, unmount } = render(() => <ThinkingDots since={SINCE} runId="run-1" />)
    await settle()
    expect(notifyButton(container)?.getAttribute('aria-pressed')).toBe('false')
    expect(container.querySelector('[role="alert"]')).toBeNull()
    unmount()
  })
})

describe('"Varsle meg" reaching the indicator through the answer region', () => {
  const waitingTurn = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
    id: 'asst-1',
    role: 'assistant',
    content: '',
    createdAt: SINCE,
    status: 'waiting',
    streaming: true,
    tools: [],
    attachments: [],
    ...overrides,
  })

  const pendingAnswer = (turn: ChatTurn) => {
    const node = deriveConversationNodes(turn).find((candidate) => candidate.kind === 'answer')
    return (node as Extract<typeof node, { kind: 'answer' }>).answer
  }

  it('derives the run id onto the pending node only when the turn has one', () => {
    const withRun = pendingAnswer(waitingTurn({ runId: 'run-1' }))
    expect(withRun).toMatchObject({ state: 'pending', runId: 'run-1' })
    // The absent key, as for `activity`: a turn with no run derives the shape it always did.
    expect(Object.keys(pendingAnswer(waitingTurn()))).toEqual(['state', 'since'])
  })

  it('shows the control for a derived pending node that carries a run id', async () => {
    waitedFor(20)
    const { container, unmount } = render(() => (
      <AnswerRegion answer={pendingAnswer(waitingTurn({ runId: 'run-1' }))} onRegenerate={() => {}} />
    ))
    await settle()
    expect(notifyButton(container)).not.toBeNull()
    unmount()
  })

  it('hides the control for a temporary thread, whose run events are suppressed', async () => {
    waitedFor(20)
    const { container, unmount } = render(() => (
      <AnswerRegion
        answer={pendingAnswer(waitingTurn({ runId: 'run-1' }))}
        onRegenerate={() => {}}
        temporaryChat
      />
    ))
    await settle()
    expect(container.querySelector('.verevon-chat-thinking__elapsed')?.textContent).toBe('20 s')
    expect(notifyButton(container)).toBeNull()
    expect(hoisted.getRunWatchStatus).not.toHaveBeenCalled()
    unmount()
  })

  const messageProps = (message: ChatTurn, temporaryChat?: boolean) => ({
    copied: false,
    message,
    onBranch: vi.fn(),
    onCopy: vi.fn(),
    onFeedback: vi.fn(async () => true),
    onRegenerate: vi.fn(),
    onApprovalDecision: vi.fn(),
    onApprovePlan: vi.fn(),
    onViewSteps: vi.fn(),
    temporaryChat,
  })

  it('reaches the indicator from a waiting assistant turn that has a run id', async () => {
    waitedFor(25)
    const { container, unmount } = render(() => (
      <AssistantMessage {...messageProps(waitingTurn({ runId: 'run-1' }))} />
    ))
    await settle()
    expect(notifyButton(container)).not.toBeNull()
    unmount()
  })

  it('does not reach the indicator from a waiting turn with no run id yet', async () => {
    waitedFor(25)
    const { container, unmount } = render(() => <AssistantMessage {...messageProps(waitingTurn())} />)
    await settle()
    expect(container.querySelector('.verevon-chat-thinking__elapsed')).not.toBeNull()
    expect(notifyButton(container)).toBeNull()
    unmount()
  })

  it('is withheld on a temporary thread all the way from the assistant turn', async () => {
    waitedFor(25)
    const { container, unmount } = render(() => (
      <AssistantMessage {...messageProps(waitingTurn({ runId: 'run-1' }), true)} />
    ))
    await settle()
    expect(container.querySelector('.verevon-chat-thinking__elapsed')).not.toBeNull()
    expect(notifyButton(container)).toBeNull()
    unmount()
  })
})
