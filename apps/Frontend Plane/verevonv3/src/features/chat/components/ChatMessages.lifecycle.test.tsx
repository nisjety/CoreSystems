// @vitest-environment jsdom

/**
 * The run lifecycle cues on an assistant turn (WS15 S5): one plain note per notice, a retrying line while a tool is
 * being retried, and nothing at all for a plain stream. Every sentence is built from the gateway's counts and closed
 * tokens, so a token this build does not know reads as the generic sentence, and a summary that failed is never
 * worded as one that worked. Both locales are read: the English reader must not get Norwegian copy.
 */

import { render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { expectNoNorwegian, renderInLocale } from '@/test/locale-leak'
import { AssistantMessage } from './ChatMessages'
import type { ChatTurn, LifecycleNotice } from './chat-types'

const baseTurn = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'asst-1',
  role: 'assistant',
  content: 'Svaret er ferdig.',
  createdAt: '2026-11-01T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  ...overrides,
})

const messageProps = (message: ChatTurn) => ({
  copied: false,
  message,
  onBranch: vi.fn(),
  onCopy: vi.fn(),
  onFeedback: vi.fn(async () => true),
  onRegenerate: vi.fn(),
  onApprovalDecision: vi.fn(),
  onApprovePlan: vi.fn(),
  onViewSteps: vi.fn(),
})

const renderTurn = (message: ChatTurn) => render(() => <AssistantMessage {...messageProps(message)} />)

const notes = () => Array.from(document.querySelectorAll<HTMLElement>('.verevon-chat-lifecycle-notice'))
const sentences = () => notes().map((node) => node.textContent)
const thinkingLine = () => document.querySelector('.verevon-chat-thinking__activity')?.textContent ?? null

const retry = (overrides: Partial<Extract<LifecycleNotice, { kind: 'retry' }>> = {}): LifecycleNotice => ({
  kind: 'retry',
  tool: 'web_search',
  attempt: 2,
  maxAttempts: 3,
  reason: 'timeout',
  ...overrides,
})

afterEach(() => {
  window.localStorage.clear()
})

describe('AssistantMessage lifecycle notices', () => {
  it('says each notice once, as a note, in the order it happened', () => {
    const { unmount } = renderTurn(baseTurn({
      lifecycle: [
        retry(),
        { kind: 'compaction', scope: 'history', outcome: 'summarized' },
        { kind: 'context', change: 'history_shed', messagesDropped: 1 },
        { kind: 'retrieval', wordingKey: 'retrieval.zdr_dense_unavailable' },
      ],
    }))
    expect(sentences()).toEqual([
      'Prøver Web search på nytt etter tidsavbrudd (forsøk 2 av 3).',
      'Eldre meldinger ble oppsummert for å få plass i konteksten.',
      '1 eldre melding ble utelatt fra konteksten.',
      'Svaret bygger bare på tekstsøk i kildene, fordi semantisk søk ikke er tilgjengelig under Zero Data Retention.',
    ])
    expect(notes().every((node) => node.getAttribute('role') === 'note')).toBe(true)
    unmount()
  })

  it('reads a failed summary as truncation, never as a summary', () => {
    const { unmount } = renderTurn(baseTurn({
      lifecycle: [{ kind: 'compaction', scope: 'history', outcome: 'truncated' }],
    }))
    expect(sentences()).toEqual([
      'Oppsummeringen av eldre meldinger mislyktes, så de ble kuttet for å få plass.',
    ])
    unmount()
  })

  it('reads a cleared tool result as removed, and a plural shed as a count', () => {
    const { unmount } = renderTurn(baseTurn({
      lifecycle: [
        { kind: 'compaction', scope: 'tool_results', outcome: 'cleared' },
        { kind: 'context', change: 'history_shed', messagesDropped: 4 },
      ],
    }))
    expect(sentences()).toEqual([
      'Gamle verktøyresultater ble fjernet for å få plass i konteksten.',
      '4 eldre meldinger ble utelatt fra konteksten.',
    ])
    unmount()
  })

  it('gives an unknown token the generic sentence, and an unnamed tool a plain one', () => {
    const { unmount } = renderTurn(baseTurn({
      lifecycle: [
        { kind: 'compaction', scope: 'history', outcome: 'future_outcome' },
        { kind: 'context', change: 'future_change' },
        retry({ tool: undefined, reason: 'future_reason' }),
      ],
    }))
    expect(sentences()).toEqual([
      'Konteksten ble justert.',
      'Konteksten ble justert.',
      'Prøver verktøyet på nytt (forsøk 2 av 3).',
    ])
    unmount()
  })

  it('renders no notice for a plain turn', () => {
    const { unmount } = renderTurn(baseTurn())
    expect(notes()).toHaveLength(0)
    expect(thinkingLine()).toBeNull()
    unmount()
  })

  /**
   * The waiting line says which try is about to run, while the tool is out. It is read from the retry notice that
   * names the call, so a tool that is merely slow never reads as being retried.
   */
  it('says the retry on the waiting line while the tool is being retried', () => {
    const { unmount } = renderTurn(baseTurn({
      content: '',
      status: 'waiting',
      streaming: true,
      toolCalls: [{ id: 'tc-1', name: 'web_search', status: 'running' }],
      lifecycle: [retry({ callId: 'tc-1' })],
    }))
    expect(thinkingLine()).toBe('Prøver Web search på nytt (forsøk 2 av 3)')
    unmount()
  })

  it('does not call a slow tool a retry when no notice names it', () => {
    const { unmount } = renderTurn(baseTurn({
      content: '',
      status: 'waiting',
      streaming: true,
      toolCalls: [{ id: 'tc-1', name: 'web_search', status: 'running' }],
    }))
    expect(thinkingLine()).toBe('Kjører Web search')
    unmount()
  })

  it('reads every notice in English under the English locale, with no Norwegian copy', () => {
    const { unmount } = renderInLocale('en', () => (
      <AssistantMessage {...messageProps(baseTurn({
        lifecycle: [
          retry(),
          { kind: 'compaction', scope: 'history', outcome: 'truncated' },
          { kind: 'context', change: 'history_shed', messagesDropped: 2 },
          { kind: 'retrieval', wordingKey: 'retrieval.zdr_dense_unavailable' },
        ],
      }))} />
    ))
    expect(sentences()).toEqual([
      'Retrying Web search after a timeout (attempt 2 of 3).',
      'Summarizing older messages failed, so they were cut to fit the context.',
      '2 older messages were left out of the context.',
      'The answer uses text search in the sources only, because semantic search is not available under Zero Data Retention.',
    ])
    for (const node of notes()) expectNoNorwegian(node)
    unmount()
  })

  it('reads the retrying line in English under the English locale', () => {
    const { unmount } = renderInLocale('en', () => (
      <AssistantMessage {...messageProps(baseTurn({
        content: '',
        status: 'waiting',
        streaming: true,
        toolCalls: [{ id: 'tc-1', name: 'web_search', status: 'running' }],
        lifecycle: [retry({ callId: 'tc-1' })],
      }))} />
    ))
    expect(thinkingLine()).toBe('Retrying Web search (attempt 2 of 3)')
    unmount()
  })
})
