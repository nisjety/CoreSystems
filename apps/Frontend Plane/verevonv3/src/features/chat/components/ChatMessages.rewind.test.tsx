// @vitest-environment jsdom

/**
 * WS17 S9 (F-t3-069): the rewind confirmation shows what the rewind would hide before anything moves, and an undo
 * offer stands under the answer a rewind landed on. The thread moves are tested in the controller.
 */

import { fireEvent, render, waitFor, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { describe, expect, it, vi } from 'vitest'
import type { HiddenTurn, RewindPlan } from '@/features/chat/lib/chat-versions-server'
import { AssistantMessage } from './ChatMessages'
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

const REWIND_LABEL = 'Gå tilbake hit'

const hidden: HiddenTurn[] = [
  { id: 'srv-q2', role: 'user', excerpt: 'Og i sjøen?' },
  { id: 'srv-a2', role: 'assistant', excerpt: 'Den lever der.' },
]

const plan: RewindPlan = { hiddenTurns: 2, hidden, expectedLeafMessageId: 'srv-a2' }

const answer = (): ChatTurn => ({
  id: 'srv-a1',
  role: 'assistant',
  content: 'Lakselus er en parasitt.',
  createdAt: '2026-11-07T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
})

function renderAnswer(props: Partial<Parameters<typeof AssistantMessage>[0]> = {}) {
  return render(() => (
    <AssistantMessage
      copied={false}
      message={answer()}
      onBranch={vi.fn()}
      onCopy={vi.fn()}
      onFeedback={vi.fn(async () => true)}
      onRegenerate={vi.fn()}
      onApprovalDecision={vi.fn()}
      onApprovePlan={vi.fn()}
      onViewSteps={vi.fn()}
      threadId="thread-v"
      {...props}
    />
  ))
}

describe('AssistantMessage — rewind with an undo (WS17 S9)', () => {
  it('offers a rewind only when the thread has one to offer', () => {
    const plain = renderAnswer()
    expect(plain.queryByLabelText(REWIND_LABEL)).toBeNull()
    plain.unmount()

    const versioned = renderAnswer({ rewindPlan: plan, onRewind: vi.fn() })
    expect(versioned.getByLabelText(REWIND_LABEL)).toBeTruthy()
    versioned.unmount()
  })

  it('lists the turns a rewind would hide before anything moves', () => {
    const onRewind = vi.fn()
    const { getByLabelText, getByRole, unmount } = renderAnswer({ rewindPlan: plan, onRewind })

    fireEvent.click(getByLabelText(REWIND_LABEL))
    flush()

    const dialog = getByRole('dialog')
    expect(within(dialog).getByText(/2 svar og meldinger etter dette punktet skjules/)).toBeTruthy()
    expect(within(dialog).getByText('Og i sjøen?')).toBeTruthy()
    expect(within(dialog).getByText('Den lever der.')).toBeTruthy()
    expect(onRewind).not.toHaveBeenCalled()
    unmount()
  })

  it('takes focus when it opens, and closes without rewinding on Escape or Cancel', async () => {
    const onRewind = vi.fn()
    const { getByLabelText, getByRole, queryByRole, unmount } = renderAnswer({ rewindPlan: plan, onRewind })

    fireEvent.click(getByLabelText(REWIND_LABEL))
    flush()
    const dialog = getByRole('dialog')
    await waitFor(() => expect(document.activeElement).toBe(dialog))
    fireEvent.keyDown(dialog, { key: 'Escape' })
    flush()
    expect(queryByRole('dialog')).toBeNull()

    fireEvent.click(getByLabelText(REWIND_LABEL))
    flush()
    fireEvent.click(within(getByRole('dialog')).getByRole('button', { name: 'Avbryt' }))
    flush()
    expect(queryByRole('dialog')).toBeNull()
    expect(onRewind).not.toHaveBeenCalled()
    unmount()
  })

  it('rewinds once the person confirms, and closes the confirmation', () => {
    const onRewind = vi.fn()
    const { getByLabelText, getByRole, queryByRole, unmount } = renderAnswer({ rewindPlan: plan, onRewind })

    fireEvent.click(getByLabelText(REWIND_LABEL))
    flush()
    fireEvent.click(within(getByRole('dialog')).getByRole('button', { name: REWIND_LABEL }))
    flush()

    expect(onRewind).toHaveBeenCalledTimes(1)
    expect(queryByRole('dialog')).toBeNull()
    unmount()
  })

  it('offers an undo under the answer a rewind landed on, naming what it brings back', () => {
    const onUndoRewind = vi.fn()
    const { container, getByRole, getByText, unmount } = renderAnswer({ rewindUndo: hidden, onUndoRewind })

    const offer = container.querySelector('.verevon-chat-rewind-undo')
    expect(offer).not.toBeNull()
    expect(getByRole('status').textContent).toContain('Angre gjenoppretter 2 meldinger etter dette punktet.')
    expect(getByText('Og i sjøen?')).toBeTruthy()

    fireEvent.click(within(getByRole('status')).getByRole('button', { name: 'Angre' }))
    expect(onUndoRewind).toHaveBeenCalledTimes(1)
    unmount()
  })

  it('shows no undo offer under an answer that no rewind landed on', () => {
    const { container, unmount } = renderAnswer({ rewindUndo: null })
    expect(container.querySelector('.verevon-chat-rewind-undo')).toBeNull()
    unmount()
  })
})
