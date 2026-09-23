// @vitest-environment jsdom

/**
 * "Fortsett" (continue-after-stop; chat-parity §0.1). The controller-level
 * behaviour (seeding, the model-facing continuation instruction, the guard
 * against an empty or non-stopped turn) is covered in
 * `use-chat-controller.continue-generation.test.ts`; this file covers the
 * other half — the control itself only ever appears on the one turn shape it
 * is meant for.
 */

import { fireEvent, render } from '@solidjs/testing-library'
import { describe, expect, it, vi } from 'vitest'
import { AssistantMessage } from './ChatMessages'
import type { ChatTurn } from './chat-types'

/** A finished, non-stopped assistant turn — the baseline every case overrides. */
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

/**
 * Every required `AssistantMessage` prop, with no-op stubs for the rest.
 * `onContinue` has no default — an explicit `undefined` must actually reach
 * the component undefined, which a default parameter would silently paper
 * over (JS applies a default to an explicitly-passed `undefined` too).
 */
const baseProps = (message: ChatTurn, onContinue: (() => void) | undefined) => ({
  copied: false,
  message,
  onBranch: vi.fn(),
  onCopy: vi.fn(),
  onFeedback: vi.fn(async () => true),
  onRegenerate: vi.fn(),
  onContinue,
  onApprovalDecision: vi.fn(),
  onApprovePlan: vi.fn(),
  onViewSteps: vi.fn(),
})

describe('AssistantMessage — "Fortsett" (continue-after-stop) control', () => {
  it('renders "Fortsett" on a stopped turn that has partial content, and invokes onContinue on click', () => {
    const onContinue = vi.fn()
    const { getByLabelText, unmount } = render(() => (
      <AssistantMessage {...baseProps(baseTurn({ status: 'stopped', content: 'Norsk fiskeoppdrett startet på' }), onContinue)} />
    ))

    const button = getByLabelText('Fortsett')
    expect(button).toBeTruthy()
    fireEvent.click(button)
    expect(onContinue).toHaveBeenCalledTimes(1)
    unmount()
  })

  it('does NOT render "Fortsett" on a stopped turn with no content — nothing to continue from', () => {
    const { queryByLabelText, unmount } = render(() => (
      <AssistantMessage {...baseProps(baseTurn({ status: 'stopped', content: '' }), vi.fn())} />
    ))

    expect(queryByLabelText('Fortsett')).toBeNull()
    unmount()
  })

  it('does NOT render "Fortsett" on a normally-completed turn, even with content', () => {
    const { queryByLabelText, unmount } = render(() => (
      <AssistantMessage {...baseProps(baseTurn({ status: undefined, content: 'Et fullstendig svar.' }), vi.fn())} />
    ))

    expect(queryByLabelText('Fortsett')).toBeNull()
    unmount()
  })

  it('does NOT render "Fortsett" on an errored turn', () => {
    const { queryByLabelText, unmount } = render(() => (
      <AssistantMessage {...baseProps(baseTurn({ status: 'error', content: 'Noe gikk galt' }), vi.fn())} />
    ))

    expect(queryByLabelText('Fortsett')).toBeNull()
    unmount()
  })

  it('does NOT render "Fortsett" when the surface offers no onContinue handler', () => {
    const { queryByLabelText, unmount } = render(() => (
      <AssistantMessage {...baseProps(baseTurn({ status: 'stopped', content: 'Delvis svar' }), undefined)} />
    ))

    expect(queryByLabelText('Fortsett')).toBeNull()
    unmount()
  })
})
