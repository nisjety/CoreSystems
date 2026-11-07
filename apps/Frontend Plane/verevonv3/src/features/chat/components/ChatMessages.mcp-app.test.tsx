// @vitest-environment jsdom

/**
 * Where MCP Apps are drawn (EP-4 S9). Only a chat assistant answer with
 * `VITE_MCP_APPS`, `VITE_CHAT_INLINE_WIDGETS` and `VITE_CHAT_WIDGET_BRIDGE` all
 * `true` reads apps, only for settled `mcp__<server>__<tool>` calls, at most two
 * on their own; a surface that is not the chat answer (the Space room), a
 * temporary chat and the switch off read nothing at all.
 */

import { cleanup, fireEvent, render, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import * as chatClient from '@/shared/api/chat-client'
import { AssistantMessage } from './ChatMessages'
import { McpAppsForAnswer } from './McpAppFrame'
import type { ChatToolCall, ChatTurn } from './chat-types'

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return { ...actual, getMcpAppResource: vi.fn() }
})

const call = (id: string, name: string, overrides: Partial<ChatToolCall> = {}): ChatToolCall => ({
  id,
  name,
  args: {},
  status: 'done',
  output: 'ok',
  ...overrides,
})

const turn = (toolCalls: ChatToolCall[], overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'asst-1',
  role: 'assistant',
  content: 'Her er tallene.',
  createdAt: '2026-11-05T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  toolCalls,
  ...overrides,
})

const handlers = () => ({
  copied: false,
  onBranch: vi.fn(),
  onCopy: vi.fn(),
  onFeedback: vi.fn(async () => true),
  onRegenerate: vi.fn(),
  onApprovalDecision: vi.fn(),
  onApprovePlan: vi.fn(),
  onViewSteps: vi.fn(),
})

function switchesOn() {
  vi.stubEnv('VITE_MCP_APPS', 'true')
  vi.stubEnv('VITE_CHAT_INLINE_WIDGETS', 'true')
  vi.stubEnv('VITE_CHAT_WIDGET_BRIDGE', 'true')
}

async function settle() {
  for (let i = 0; i < 3; i += 1) {
    await Promise.resolve()
    flush()
  }
}

const getResource = () => vi.mocked(chatClient.getMcpAppResource)

beforeEach(() => {
  getResource().mockImplementation(async (_serverId, tool) => ({
    uri: `ui://apps/${tool}`,
    html: `<p>${tool}</p>`,
    needsNetwork: false,
  }))
})

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
  getResource().mockReset()
})

describe('MCP Apps under a chat answer', () => {
  it('a settled MCP call with an app is drawn under the answer', async () => {
    switchesOn()
    const { container } = render(() => (
      <AssistantMessage {...handlers()} message={turn([call('c1', 'mcp__srv-erp__show_table')])} />
    ))
    await waitFor(() => expect(container.querySelector('.verevon-mcp-app iframe')).not.toBeNull())
    expect(getResource()).toHaveBeenCalledWith('srv-erp', 'show_table')
    expect(container.textContent).toContain('App fra srv-erp')
  })

  it('non mcp tool has no app', async () => {
    switchesOn()
    const { container } = render(() => (
      <AssistantMessage
        {...handlers()}
        message={turn([
          call('c1', 'web_search'),
          // Still running, failed or without output: nothing to draw yet.
          call('c2', 'mcp__srv-erp__show_table', { status: 'running', output: undefined }),
          call('c3', 'mcp__srv-erp__show_table', { status: 'error', error: 'down' }),
        ])}
      />
    ))
    await settle()
    expect(getResource()).not.toHaveBeenCalled()
    expect(container.querySelector('.verevon-mcp-app')).toBeNull()
  })

  it('room surface has no app', async () => {
    switchesOn()
    // McpAppsForAnswer outside a chat answer's live surface, as the Space room would draw it.
    const { container } = render(() => <McpAppsForAnswer calls={[call('c1', 'mcp__srv-erp__show_table')]} />)
    await settle()
    expect(getResource()).not.toHaveBeenCalled()
    expect(container.querySelector('iframe')).toBeNull()
  })

  it('a temporary chat and the switch off read nothing', async () => {
    switchesOn()
    const temporary = render(() => (
      <AssistantMessage {...handlers()} temporaryChat message={turn([call('c1', 'mcp__srv-erp__show_table')])} />
    ))
    await settle()
    expect(temporary.container.querySelector('.verevon-mcp-app')).toBeNull()
    temporary.unmount()

    vi.stubEnv('VITE_MCP_APPS', 'TRUE')
    const off = render(() => <AssistantMessage {...handlers()} message={turn([call('c1', 'mcp__srv-erp__show_table')])} />)
    await settle()
    expect(off.container.querySelector('.verevon-mcp-app')).toBeNull()
    expect(getResource()).not.toHaveBeenCalled()
  })

  it('third app is click to load', async () => {
    switchesOn()
    const { container } = render(() => (
      <AssistantMessage
        {...handlers()}
        message={turn([
          call('c1', 'mcp__srv-erp__show_table'),
          call('c2', 'mcp__srv-erp__show_chart'),
          call('c3', 'mcp__srv-erp__show_map'),
        ])}
      />
    ))
    await waitFor(() => expect(container.querySelectorAll('.verevon-mcp-app iframe')).toHaveLength(2))
    const show = Array.from(container.querySelectorAll('button')).find((button) => button.textContent?.includes('Vis app (show_map)'))
    expect(show).toBeDefined()
    fireEvent.click(show!)
    await waitFor(() => expect(container.querySelectorAll('.verevon-mcp-app iframe')).toHaveLength(3))
    // One read per tool: the lookup and the frame share it.
    expect(getResource()).toHaveBeenCalledTimes(3)
  })
})
