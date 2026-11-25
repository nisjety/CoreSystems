// @vitest-environment jsdom

/**
 * WS18-S4 (F-f06-16): on a project thread the answer row offers none of the turn actions that replay or rewrite a turn
 * (regenerate, continue, edit, branch, rewind, fork, rerun and feedback). The controller refuses them too; this file
 * covers the controls, and that an ordinary thread (no `turnActionsAllowed` prop, or `true`) still shows them.
 */

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AnswerRegion, AssistantMessage, UserMessage } from './ChatMessages'
import { ForkAction } from './ForkAction'
import type { ChatTurn } from './chat-types'

const { forkChatThread } = vi.hoisted(() => ({ forkChatThread: vi.fn() }))
vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return { ...actual, forkChatThread }
})

type AssistantProps = Parameters<typeof AssistantMessage>[0]
type UserProps = Parameters<typeof UserMessage>[0]

/** A finished assistant answer that was stopped with partial text, so every turn action has a reason to show. */
const stoppedAnswer = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'asst-1',
  role: 'assistant',
  content: 'Status er grønn',
  createdAt: '2026-11-24T08:00:00.000Z',
  streaming: false,
  status: 'stopped',
  requestId: 'req-1',
  tools: [],
  attachments: [],
  ...overrides,
})

const question = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'user-1',
  role: 'user',
  content: 'Hva er status?',
  createdAt: '2026-11-24T07:59:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  ...overrides,
})

/** Every required `AssistantMessage` prop, with stubs for the rest. A rewind is offered, so its control has something to show. */
const assistantProps = (message: ChatTurn, extra: Partial<AssistantProps> = {}): AssistantProps => ({
  copied: false,
  message,
  threadId: 'thread-project',
  onBranch: vi.fn(),
  onCopy: vi.fn(),
  onFeedback: vi.fn(async () => true),
  onRegenerate: vi.fn(),
  onContinue: vi.fn(),
  onForked: vi.fn(),
  rewindPlan: { hiddenTurns: 1, hidden: [], expectedLeafMessageId: 'user-2' },
  onRewind: vi.fn(),
  onApprovalDecision: vi.fn(),
  onApprovePlan: vi.fn(),
  onViewSteps: vi.fn(),
  ...extra,
})

const userProps = (message: ChatTurn, extra: Partial<UserProps> = {}): UserProps => ({
  copied: false,
  message,
  onBranch: vi.fn(),
  onCopy: vi.fn(),
  onEdit: vi.fn(),
  ...extra,
})

/** Opens the overflow menu of a message, where the branch entry lives. */
function openOverflowMenu() {
  fireEvent.click(screen.getByRole('button', { name: /Flere handlinger|More actions/ }))
  flush()
}

/** The answer row's turn actions, by the label each control carries. */
const ANSWER_ACTIONS = [
  /^Good response$/,
  /^Bad response$/,
  /Generer på nytt/,
  /^Fortsett$/,
  /Gå tilbake hit/,
  /Forgren herfra/,
]

afterEach(() => {
  cleanup()
  forkChatThread.mockReset()
  vi.unstubAllEnvs()
})

describe('AssistantMessage on a project thread', () => {
  it('shows none of the answer-row turn actions and no branch entry', async () => {
    vi.stubEnv('VITE_CHAT_THREAD_FORK', 'true')
    render(() => <AssistantMessage {...assistantProps(stoppedAnswer(), { turnActionsAllowed: false })} />)

    for (const label of ANSWER_ACTIONS) {
      expect(screen.queryByRole('button', { name: label }), String(label)).toBeNull()
    }
    // The menu keeps its other entries (read aloud, export); only the branch entry is withdrawn.
    openOverflowMenu()
    expect(screen.getByText(/Les høyt|Read aloud/)).toBeTruthy()
    expect(screen.queryByText('Fortsett i ny chat')).toBeNull()
  })

  it('offers every answer-row turn action on an ordinary thread, unchanged', async () => {
    vi.stubEnv('VITE_CHAT_THREAD_FORK', 'true')
    render(() => <AssistantMessage {...assistantProps(stoppedAnswer())} />)

    for (const label of ANSWER_ACTIONS) {
      expect(screen.getByRole('button', { name: label }), String(label)).toBeTruthy()
    }
    openOverflowMenu()
    expect(screen.getByText('Fortsett i ny chat')).toBeTruthy()
  })

  it('keeps the same controls when the prop is set to true', async () => {
    render(() => <AssistantMessage {...assistantProps(stoppedAnswer(), { turnActionsAllowed: true })} />)

    expect(screen.getByRole('button', { name: /^Good response$/ })).toBeTruthy()
    expect(screen.getByRole('button', { name: /Generer på nytt/ })).toBeTruthy()
  })
})

describe('ForkAction', () => {
  it('does not fork when the turn actions are withheld', async () => {
    forkChatThread.mockResolvedValue({ threadId: 'thread-fork' })
    render(() => <ForkAction threadId="thread-project" messageId="asst-1" turnActionsAllowed={false} />)

    fireEvent.click(screen.getByRole('button', { name: /Forgren herfra|Fork from here/ }))

    expect(forkChatThread).not.toHaveBeenCalled()
  })

  it('forks as before when the turn actions are allowed', async () => {
    forkChatThread.mockResolvedValue({ threadId: 'thread-fork' })
    render(() => <ForkAction threadId="thread-source" messageId="asst-1" turnActionsAllowed={true} />)

    fireEvent.click(screen.getByRole('button', { name: /Forgren herfra|Fork from here/ }))

    expect(forkChatThread).toHaveBeenCalledWith('thread-source', { fromMessageId: 'asst-1' })
  })
})

describe('the rerun of an effectful answer on a project thread', () => {
  const effectfulAnswer = (): ChatTurn => ({ ...stoppedAnswer(), effectClass: 'effectful' })

  it('offers no run-as-a-new-turn control when the turn actions are withheld', () => {
    render(() => <AssistantMessage {...assistantProps(effectfulAnswer(), { turnActionsAllowed: false, onRerunAsNewTurn: vi.fn() })} />)

    expect(screen.getByText('Effekt registrert — kan ikke endres')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Kjør som ny tur' })).toBeNull()
  })

  it('offers the control on an ordinary thread, unchanged', () => {
    render(() => <AssistantMessage {...assistantProps(effectfulAnswer(), { onRerunAsNewTurn: vi.fn() })} />)

    expect(screen.getByRole('button', { name: 'Kjør som ny tur' })).toBeTruthy()
  })
})

describe('a failed answer on a project thread', () => {
  const failed = { state: 'failed' as const, message: 'Svaret ble ikke lagret.' }

  it('offers no retry when the turn actions are withheld', () => {
    const onRegenerate = vi.fn()
    const { container } = render(() => <AnswerRegion answer={failed} onRegenerate={onRegenerate} turnActionsAllowed={false} />)

    expect(container.textContent).toContain('Svaret ble ikke lagret.')
    expect(container.querySelector('.verevon-chat-error-notice button')).toBeNull()
    expect(screen.queryByRole('button', { name: /Prøv igjen/ })).toBeNull()
  })

  it('offers the retry when the prop is absent or true', () => {
    for (const extra of [{}, { turnActionsAllowed: true }]) {
      const onRegenerate = vi.fn()
      const { container, unmount } = render(() => <AnswerRegion answer={failed} onRegenerate={onRegenerate} {...extra} />)

      const retry = container.querySelector<HTMLButtonElement>('.verevon-chat-error-notice button')
      expect(retry?.textContent).toBe('Prøv igjen')
      fireEvent.click(retry!)
      expect(onRegenerate).toHaveBeenCalledTimes(1)
      unmount()
    }
  })

  it('offers no retry through the answer row of a project thread', () => {
    const errored: ChatTurn = { ...stoppedAnswer(), status: 'error', content: 'Svaret ble ikke lagret.' }
    render(() => <AssistantMessage {...assistantProps(errored, { turnActionsAllowed: false })} />)

    expect(screen.getByText('Svaret ble ikke lagret.')).toBeTruthy()
    expect(screen.queryByRole('button', { name: /Prøv igjen/ })).toBeNull()
  })
})

describe('UserMessage on a project thread', () => {
  it('shows no edit and no branch entry, and keeps copy', () => {
    render(() => <UserMessage {...userProps(question(), { turnActionsAllowed: false })} />)

    expect(screen.queryByRole('button', { name: 'Rediger' })).toBeNull()
    expect(screen.queryByRole('button', { name: /Flere handlinger|More actions/ })).toBeNull()
    expect(screen.getByRole('button', { name: 'Copy' })).toBeTruthy()
  })

  it('offers edit and branch on an ordinary thread, unchanged', () => {
    render(() => <UserMessage {...userProps(question())} />)

    expect(screen.getByRole('button', { name: 'Rediger' })).toBeTruthy()
    expect(screen.getByRole('button', { name: /Flere handlinger|More actions/ })).toBeTruthy()
  })
})
