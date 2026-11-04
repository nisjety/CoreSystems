// @vitest-environment jsdom

/**
 * EP-11 S1: the "Save as skill" message action. An admin sees it on a saved answer; a member sees it only
 * while the member switch is on. A temporary (zero-retention) chat and an unsaved conversation never offer it.
 */

import { render, waitFor } from '@solidjs/testing-library'
import { describe, expect, it, vi } from 'vitest'
import { memberSkillAuthoring } from '@/shared/api/skills-client'
import { AssistantMessage } from './ChatMessages'
import type { ChatTurn } from './chat-types'

const mocks = vi.hoisted(() => ({
  role: 'member' as 'member' | 'owner',
  memberAuthoring: false,
}))

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({
    status: 'authenticated',
    user: { id: 'user-1', role: 'member' },
    activeOrg: { id: 'org-1', name: 'Test', role: mocks.role },
    permissions: [],
  }),
}))

vi.mock('@/shared/api/skills-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/skills-client')>()
  return { ...actual, memberSkillAuthoring: vi.fn(async () => mocks.memberAuthoring) }
})

const SAVE_LABEL = 'Lagre som ferdighet'

/** A finished assistant turn, saved in a thread unless a case says otherwise. */
const answer = (): ChatTurn => ({
  id: 'asst-1',
  role: 'assistant',
  content: 'Svaret er ferdig.',
  createdAt: '2026-09-17T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
})

function renderAnswer(options: { threadId?: string | null; temporaryChat?: boolean } = {}) {
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
      threadId={options.threadId === undefined ? 'thread-1' : options.threadId}
      temporaryChat={options.temporaryChat}
    />
  ))
}

describe('AssistantMessage — Save as skill', () => {
  it('save_as_skill_absent_on_zdr_thread', () => {
    mocks.role = 'owner'
    const { queryByLabelText, unmount } = renderAnswer({ temporaryChat: true })
    expect(queryByLabelText(SAVE_LABEL)).toBeNull()
    unmount()
  })

  it('offers save_as_skill to an admin on a saved answer', () => {
    mocks.role = 'owner'
    const { getByLabelText, unmount } = renderAnswer()
    expect(getByLabelText(SAVE_LABEL)).toBeTruthy()
    unmount()
  })

  it('hides save_as_skill on a conversation not yet saved', () => {
    mocks.role = 'owner'
    const { queryByLabelText, unmount } = renderAnswer({ threadId: null })
    expect(queryByLabelText(SAVE_LABEL)).toBeNull()
    unmount()
  })

  it('hides save_as_skill from a member while the member switch is off', async () => {
    mocks.role = 'member'
    mocks.memberAuthoring = false
    const { queryByLabelText, unmount } = renderAnswer()
    await waitFor(() => expect(vi.mocked(memberSkillAuthoring)).toHaveBeenCalled())
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(queryByLabelText(SAVE_LABEL)).toBeNull()
    unmount()
  })

  it('offers save_as_skill to a member while the member switch is on', async () => {
    mocks.role = 'member'
    mocks.memberAuthoring = true
    const { getByLabelText, unmount } = renderAnswer()
    await waitFor(() => expect(getByLabelText(SAVE_LABEL)).toBeTruthy())
    unmount()
  })
})
