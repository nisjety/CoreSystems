// @vitest-environment jsdom

/**
 * Where a ```widget fence runs (EP-4 S1, D2). Only a chat assistant answer with
 * `VITE_CHAT_INLINE_WIDGETS=true` draws the sandboxed frame; a user message, a
 * `ChatMarkdown` outside that answer (the Space room, people's posts, exports)
 * and the switch off all keep the ordinary code block.
 */

import { render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AssistantMessage, ChatMarkdown, MessageBlock } from './ChatMessages'
import type { ChatTurn } from './chat-types'

const WIDGET = '<title>Rente</title><input type="range" min="0" max="10">'
const ANSWER = `Her er en kalkulator:\n\n\`\`\`widget\n${WIDGET}\n\`\`\`\n\nDra i skyveknappen.`

const turn = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'asst-1',
  role: 'assistant',
  content: ANSWER,
  createdAt: '2026-11-03T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  ...overrides,
})

const handlers = () => ({
  copied: false,
  onBranch: vi.fn(),
  onCopy: vi.fn(),
  onEdit: vi.fn(),
  onFeedback: vi.fn(async () => true),
  onRegenerate: vi.fn(),
  onApprovalDecision: vi.fn(),
  onApprovePlan: vi.fn(),
  onViewSteps: vi.fn(),
})

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('inline widgets in the transcript', () => {
  it('an assistant answer draws the widget', () => {
    vi.stubEnv('VITE_CHAT_INLINE_WIDGETS', 'true')
    const { container, unmount } = render(() => <AssistantMessage {...handlers()} message={turn()} />)
    flush()
    const frame = container.querySelector('iframe.verevon-inline-widget__frame')
    expect(frame?.getAttribute('sandbox')).toBe('allow-scripts')
    expect(frame?.getAttribute('srcdoc')).toContain(WIDGET)
    expect(container.querySelector('.verevon-chat-codeblock')).toBeNull()
    unmount()
  })

  it('a user message keeps the code', () => {
    vi.stubEnv('VITE_CHAT_INLINE_WIDGETS', 'true')
    const { container, unmount } = render(() => (
      <MessageBlock {...handlers()} message={turn({ id: 'user-1', role: 'user' })} />
    ))
    flush()
    expect(container.querySelector('iframe')).toBeNull()
    expect(container.textContent).toContain(WIDGET)
    unmount()
  })

  it('ChatMarkdown outside a chat answer (the Space room) keeps the code', () => {
    vi.stubEnv('VITE_CHAT_INLINE_WIDGETS', 'true')
    const { container, unmount } = render(() => <ChatMarkdown content={ANSWER} />)
    flush()
    expect(container.querySelector('iframe')).toBeNull()
    expect(container.querySelector('.verevon-chat-codeblock')?.textContent).toContain(WIDGET)
    unmount()
  })

  it('with the switch off an answer keeps the code', () => {
    vi.stubEnv('VITE_CHAT_INLINE_WIDGETS', '1')
    const { container, unmount } = render(() => <AssistantMessage {...handlers()} message={turn()} />)
    flush()
    expect(container.querySelector('iframe')).toBeNull()
    expect(container.querySelector('.verevon-chat-codeblock')?.textContent).toContain(WIDGET)
    unmount()
  })

  it('a widget fence still streaming is code even in a live answer', () => {
    vi.stubEnv('VITE_CHAT_INLINE_WIDGETS', 'true')
    const open = `Her kommer den:\n\n\`\`\`widget\n${WIDGET}`
    const { container, unmount } = render(() => (
      <AssistantMessage {...handlers()} message={turn({ content: open, streaming: true })} />
    ))
    flush()
    expect(container.querySelector('iframe')).toBeNull()
    unmount()
  })
})
