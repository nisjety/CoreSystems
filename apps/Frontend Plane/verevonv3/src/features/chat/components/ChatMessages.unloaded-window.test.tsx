// @vitest-environment jsdom

/**
 * F-f01-25: a turn outside the loaded window keeps its text, and its actions wait for the earlier pages. Those actions
 * render disabled, with the reason as their title. A turn inside the window is unchanged.
 */

import { fireEvent, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { describe, expect, it, vi } from 'vitest'
import type { RewindPlan } from '@/features/chat/lib/chat-versions-server'
import { MessageBlock } from './ChatMessages'
import type { ChatTurn } from './chat-types'

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({
    status: 'authenticated',
    user: { id: 'user-1', role: 'member' },
    activeOrg: { id: 'org-1', name: 'Test', role: 'member' },
    permissions: [],
  }),
}))

vi.mock('@/shared/api/skills-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/skills-client')>()
  return { ...actual, memberSkillAuthoring: vi.fn(async () => false) }
})

const WAIT_TITLE = 'Last inn eldre meldinger først'
const REGENERATE_LABEL = 'Generer på nytt — beholder dette svaret som versjon 1'

/** A rewind offer on the answer, as a versioned thread gives it (WS17 S9). */
const PLAN: RewindPlan = {
  hiddenTurns: 1,
  hidden: [{ id: 'srv-q2', role: 'user', excerpt: 'Og i sjøen?' }],
  expectedLeafMessageId: 'srv-q2',
}

const answer = (): ChatTurn => ({
  id: 'srv-a1',
  role: 'assistant',
  content: 'Lakselus er en parasitt.',
  createdAt: '2026-11-07T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
})

const question = (): ChatTurn => ({
  id: 'srv-q1',
  role: 'user',
  content: 'Hva er lakselus?',
  createdAt: '2026-11-07T07:59:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
})

function renderTurn(
  message: ChatTurn,
  unloadedTurnIds?: ReadonlySet<string>,
  extra: Partial<Parameters<typeof MessageBlock>[0]> = {},
) {
  return render(() => (
    <MessageBlock
      copied={false}
      message={message}
      onBranch={vi.fn()}
      onCopy={vi.fn()}
      onEdit={vi.fn()}
      onFeedback={vi.fn(async () => true)}
      onRegenerate={vi.fn()}
      onApprovalDecision={vi.fn()}
      onApprovePlan={vi.fn()}
      onViewSteps={vi.fn()}
      threadId="thread-w"
      unloadedTurnIds={unloadedTurnIds}
      {...extra}
    />
  ))
}

/** Opens the turn's "More actions" menu and returns its branch item ("Fortsett i ny chat"). */
function branchItem(container: HTMLElement): HTMLButtonElement {
  fireEvent.click(buttonNamed(container, 'Flere handlinger'))
  flush()
  const found = Array.from(container.querySelectorAll<HTMLButtonElement>('.verevon-chat-menu__panel button'))
    .find((button) => button.textContent?.includes('Fortsett i ny chat'))
  if (!found) throw new Error('No branch item in the menu')
  return found
}

/** The control a label names, as a button element, so its disabled state can be read. */
function buttonNamed(container: HTMLElement, label: string): HTMLButtonElement {
  const found = container.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)
  if (!found) throw new Error(`No control labelled ${label}`)
  return found
}

describe('turns outside the loaded window (F-f01-25)', () => {
  it('keeps the text of a turn outside the window, with its regenerate disabled and titled with the reason', () => {
    const { container, getByText, unmount } = renderTurn(answer(), new Set(['srv-a1']))

    expect(getByText('Lakselus er en parasitt.')).toBeTruthy()
    const regenerate = buttonNamed(container, REGENERATE_LABEL)
    expect(regenerate.disabled).toBe(true)
    expect(regenerate.getAttribute('title')).toBe(WAIT_TITLE)
    unmount()
  })

  it('disables the edit and the branch of a question outside the window, and keeps its copy', () => {
    const { container, unmount } = renderTurn(question(), new Set(['srv-q1']))

    const edit = buttonNamed(container, 'Rediger')
    expect(edit.disabled).toBe(true)
    expect(edit.getAttribute('title')).toBe(WAIT_TITLE)
    expect(buttonNamed(container, 'Copy').disabled).toBe(false)
    unmount()
  })

  it('disables the continue of a stopped answer outside the window, titled with the reason', () => {
    const stopped: ChatTurn = { ...answer(), status: 'stopped' }
    const { container, unmount } = renderTurn(stopped, new Set(['srv-a1']), { onContinue: vi.fn() })

    const resume = buttonNamed(container, 'Fortsett')
    expect(resume.disabled).toBe(true)
    expect(resume.getAttribute('title')).toBe(WAIT_TITLE)
    unmount()
  })

  it('disables the rerun of an effectful answer outside the window, titled with the reason', () => {
    const effectful: ChatTurn = { ...answer(), effectClass: 'effectful' }
    const { container, unmount } = renderTurn(effectful, new Set(['srv-a1']), { onRerunAsNewTurn: vi.fn() })

    const rerun = buttonNamed(container, 'Kjør som ny tur')
    expect(rerun.disabled).toBe(true)
    expect(rerun.getAttribute('title')).toBe(WAIT_TITLE)
    unmount()
  })

  it('disables the rewind of an answer outside the window, titled with the reason', () => {
    const { container, unmount } = renderTurn(answer(), new Set(['srv-a1']), { rewindPlan: PLAN, onRewind: vi.fn() })

    const rewind = buttonNamed(container, 'Gå tilbake hit')
    expect(rewind.disabled).toBe(true)
    expect(rewind.getAttribute('title')).toBe(WAIT_TITLE)
    unmount()
  })

  it('disables the branch menu item of an answer and of a question outside the window, titled with the reason', () => {
    const onAnswer = renderTurn(answer(), new Set(['srv-a1']))
    const answerBranch = branchItem(onAnswer.container)
    expect(answerBranch.disabled).toBe(true)
    expect(answerBranch.getAttribute('title')).toBe(WAIT_TITLE)
    onAnswer.unmount()

    const onQuestion = renderTurn(question(), new Set(['srv-q1']))
    const questionBranch = branchItem(onQuestion.container)
    expect(questionBranch.disabled).toBe(true)
    expect(questionBranch.getAttribute('title')).toBe(WAIT_TITLE)
    onQuestion.unmount()
  })

  it('leaves the continue, rerun, rewind and branch of a turn inside the window enabled', () => {
    const stopped = renderTurn(
      { ...answer(), status: 'stopped' },
      new Set(['older']),
      { onContinue: vi.fn(), rewindPlan: PLAN, onRewind: vi.fn() },
    )
    expect(buttonNamed(stopped.container, 'Fortsett').disabled).toBe(false)
    expect(buttonNamed(stopped.container, 'Gå tilbake hit').disabled).toBe(false)
    expect(branchItem(stopped.container).disabled).toBe(false)
    stopped.unmount()

    const effectful = renderTurn({ ...answer(), effectClass: 'effectful' }, new Set(['older']), { onRerunAsNewTurn: vi.fn() })
    expect(buttonNamed(effectful.container, 'Kjør som ny tur').disabled).toBe(false)
    effectful.unmount()
  })

  it('leaves the actions of a turn inside the window enabled, with their own labels as titles', () => {
    const { container, unmount } = renderTurn(answer(), new Set(['some-older-turn']))

    const regenerate = buttonNamed(container, REGENERATE_LABEL)
    expect(regenerate.disabled).toBe(false)
    expect(regenerate.getAttribute('title')).toBe(REGENERATE_LABEL)
    unmount()
  })

  it('leaves every action enabled when no window is set', () => {
    const { container, unmount } = renderTurn(question())

    expect(buttonNamed(container, 'Rediger').disabled).toBe(false)
    expect(buttonNamed(container, 'Rediger').getAttribute('title')).toBe('Rediger')
    unmount()
  })
})
