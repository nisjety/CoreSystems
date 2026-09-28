// @vitest-environment jsdom

/**
 * "Planlegg dette" (chat-wave2 "schedule" spec §2.1, §3) — the message-action
 * entry point into the schedule-from-chat flow. Lives in the assistant
 * turn's overflow menu, right after "Fortsett i ny chat"; mirrors the
 * `onContinue`-only-when-offered convention `ChatMessages.continue.test.tsx`
 * already pins for `onContinue`.
 */

import { fireEvent, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { describe, expect, it, vi } from 'vitest'
import { AssistantMessage } from './ChatMessages'
import type { ChatTurn } from './chat-types'

const baseTurn = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'asst-1',
  role: 'assistant',
  content: 'Svaret er ferdig.',
  createdAt: '2026-09-17T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  ...overrides,
})

const baseProps = (onScheduleThis: (() => void) | undefined) => ({
  copied: false,
  message: baseTurn(),
  onBranch: vi.fn(),
  onCopy: vi.fn(),
  onFeedback: vi.fn(async () => true),
  onRegenerate: vi.fn(),
  onScheduleThis,
  onApprovalDecision: vi.fn(),
  onApprovePlan: vi.fn(),
  onViewSteps: vi.fn(),
})

function openOverflowMenu(getByLabelText: (label: string) => HTMLElement) {
  fireEvent.click(getByLabelText('Flere handlinger'))
  flush()
}

describe('AssistantMessage — "Planlegg dette" (schedule-from-chat) menu item', () => {
  it('appears in the overflow menu and calls onScheduleThis on click', () => {
    const onScheduleThis = vi.fn()
    const { getByLabelText, getByText, unmount } = render(() => (
      <AssistantMessage {...baseProps(onScheduleThis)} />
    ))

    openOverflowMenu(getByLabelText)
    const item = getByText('Planlegg dette')
    expect(item).toBeTruthy()
    fireEvent.click(item)
    expect(onScheduleThis).toHaveBeenCalledTimes(1)
    unmount()
  })

  it('is absent when the surface offers no onScheduleThis handler', () => {
    const { getByLabelText, queryByText, unmount } = render(() => (
      <AssistantMessage {...baseProps(undefined)} />
    ))

    openOverflowMenu(getByLabelText)
    expect(queryByText('Planlegg dette')).toBeNull()
    unmount()
  })
})
