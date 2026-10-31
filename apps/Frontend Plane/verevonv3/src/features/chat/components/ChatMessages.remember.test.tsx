// @vitest-environment jsdom

/**
 * "Husk dette" (memory.remember) — a user action on an assistant answer. It saves
 * only on the user's click, and a paused memory is reported as paused, never as a
 * save.
 */

import { fireEvent, render, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AssistantMessage } from './ChatMessages'
import type { ChatTurn } from './chat-types'

const baseTurn = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'asst-1',
  role: 'assistant',
  content: 'Du heter Ima.',
  createdAt: '2026-10-24T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  ...overrides,
})

const baseProps = (overrides: Record<string, unknown> = {}) => ({
  copied: false,
  message: baseTurn(),
  threadId: 'thread-1',
  onBranch: vi.fn(),
  onCopy: vi.fn(),
  onFeedback: vi.fn(async () => true),
  onRegenerate: vi.fn(),
  onApprovalDecision: vi.fn(),
  onApprovePlan: vi.fn(),
  onViewSteps: vi.fn(),
  ...overrides,
})

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('AssistantMessage "Husk dette"', () => {
  it('saves the answer to memory only when clicked', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ memory_id: 'mem_1' }))
    vi.stubGlobal('fetch', fetchMock)
    const { getByLabelText, unmount } = render(() => <AssistantMessage {...baseProps()} />)

    expect(fetchMock).not.toHaveBeenCalled()
    fireEvent.click(getByLabelText('Husk dette'))
    flush()

    await waitFor(() => expect(getByLabelText('Husket')).toBeTruthy())
    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/memory')
    expect(init.method).toBe('POST')
    expect(JSON.parse(String(init.body))).toEqual({ threadId: 'thread-1', content: 'Du heter Ima.' })
    unmount()
  })

  it('says the memory is paused instead of claiming a save', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse({ error: { code: 'memory_not_enabled', message: 'memory_not_enabled:user_paused' } }, 409),
      ),
    )
    const { getByLabelText, findByText, queryByLabelText, unmount } = render(() => (
      <AssistantMessage {...baseProps()} />
    ))

    fireEvent.click(getByLabelText('Husk dette'))
    flush()

    expect(await findByText(/Minnet er pauset, av eller slått av/)).toBeTruthy()
    expect(queryByLabelText('Husket')).toBeNull()
    unmount()
  })

  it('reports a failed save as a failure, not as paused', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ error: { code: 'bad_gateway', message: 'down' } }, 502)))
    const { getByLabelText, findByText, unmount } = render(() => <AssistantMessage {...baseProps()} />)

    fireEvent.click(getByLabelText('Husk dette'))
    flush()

    expect(await findByText('Kunne ikke lagre minnet.')).toBeTruthy()
    unmount()
  })

  it('is not offered in a temporary chat or without a thread', () => {
    const temporary = render(() => <AssistantMessage {...baseProps({ temporaryChat: true })} />)
    expect(temporary.queryByLabelText('Husk dette')).toBeNull()
    temporary.unmount()

    const unsaved = render(() => <AssistantMessage {...baseProps({ threadId: null })} />)
    expect(unsaved.queryByLabelText('Husk dette')).toBeNull()
    unsaved.unmount()
  })
})
