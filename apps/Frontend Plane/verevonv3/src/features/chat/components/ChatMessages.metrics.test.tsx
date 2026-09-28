// @vitest-environment jsdom

/**
 * The details popover's timing and cost rows.
 *
 * "Total tid" alone could not say where a 45-second answer went. The gateway's
 * `turn_timing` splits server time into provider calls and everything else;
 * those two rows appear only when it was reported, never estimated. And a
 * subscription-carried turn has no list-price cost: its wire 0.0 must never be
 * shown as "$0.0000".
 */

import { fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { describe, expect, it } from 'vitest'
import { readChatThreadTranscript, upsertChatThreadTranscript } from '@/features/chat/lib/chat-thread-history'
import { ReasoningPopover } from './ChatMessages'
import { transcriptTurnToChatTurn, turnsToTranscript } from './chat-normalizers'
import type { ChatTurn } from './chat-types'

const turn = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'asst-1',
  role: 'assistant',
  content: 'Svaret.',
  createdAt: '2026-09-23T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  modelUsed: 'gpt-5.6-terra',
  latencyMs: 43_440,
  ...overrides,
})

function openDetails(message: ChatTurn) {
  render(() => <ReasoningPopover message={message} />)
  fireEvent.click(screen.getByRole('button', { name: /Detaljer|Details/ }))
  flush()
  return document.querySelector('.verevon-chat-reasoning-popover__panel')?.textContent ?? ''
}

describe('ReasoningPopover metrics', () => {
  it('adds model and system time next to the total when turn_timing was reported', () => {
    const text = openDetails(turn({ timing: { serverMs: 43_440, providerMs: 43_220, orchestrationMs: 220 } }))
    expect(text).toContain('Total tid')
    expect(text).toMatch(/Modelltid|Model time/)
    expect(text).toContain('43.2 s')
    expect(text).toMatch(/Systemtid|System time/)
    expect(text).toContain('220 ms')
  })

  it('shows no timing rows when the gateway did not report them', () => {
    const text = openDetails(turn())
    expect(text).toContain('Total tid')
    expect(text).not.toMatch(/Modelltid|Model time|Systemtid|System time/)
  })

  it('says a subscription turn is included in the subscription instead of showing a zero cost', () => {
    const text = openDetails(turn({ costBasis: 'subscription' }))
    expect(text).toContain('Kostnad')
    expect(text).toMatch(/Inkludert i abonnementet|Included in the subscription/)
    expect(text).not.toContain('$')
  })

  it('keeps a metered cost as it was', () => {
    const text = openDetails(turn({ costUsd: 0.0031, costBasis: 'metered' }))
    expect(text).toContain('$0.0031')
  })

  it('shows the same timing rows and subscription wording after a reload', () => {
    // Through the local transcript store, exactly as a reload reads it back.
    const live = turn({
      costBasis: 'subscription',
      timing: { serverMs: 43_440, providerMs: 43_220, orchestrationMs: 220, phases: [] },
    })
    upsertChatThreadTranscript({ threadId: 'thread-metrics', turns: turnsToTranscript([live]), updatedAt: live.createdAt })
    const stored = readChatThreadTranscript('thread-metrics')?.turns.at(0)
    expect(stored).toBeDefined()
    const text = openDetails(transcriptTurnToChatTurn(stored!))
    expect(text).toMatch(/Modelltid|Model time/)
    expect(text).toContain('43.2 s')
    expect(text).toMatch(/Systemtid|System time/)
    expect(text).toContain('220 ms')
    expect(text).toMatch(/Inkludert i abonnementet|Included in the subscription/)
    expect(text).not.toContain('$')
  })
})
