import { describe, expect, it, vi } from 'vitest'
import {
  buildAgentRunInputBody,
  dispatchAgUiChatEvent,
  parseAgUiSseEvent,
  type AgUiEvent,
} from './ag-ui-client'

describe('AG-UI client adapter', () => {
  it('builds TanStack-compatible RunAgentInput bodies for the gateway', () => {
    const body = buildAgentRunInputBody({
      content: 'Refresh this source',
      model: 'velion-default',
      threadId: 'thread_1',
      sessionKey: 'thread_1',
      browseWeb: true,
      generateImage: false,
      actions: [{ id: 'knowledge.recrawl_source', name: 'Recrawl source', kind: 'tool' }],
    })

    expect(body).toMatchObject({
      threadId: 'thread_1',
      messages: [
        {
          role: 'user',
          content: 'Refresh this source',
        },
      ],
      forwardedProps: {
        model: 'velion-default',
        profile: 'chat',
        sessionKey: 'thread_1',
        generateImage: false,
        browseWeb: true,
      },
    })
    expect((body.tools as Array<{ name: string }>).map((tool) => tool.name)).toEqual([
      'web_search',
      'knowledge.recrawl_source',
    ])
    expect(body.data).toEqual(body.forwardedProps)
  })

  it('parses typed AG-UI SSE payloads', () => {
    const event = parseAgUiSseEvent({
      data: JSON.stringify({
        type: 'TEXT_MESSAGE_CONTENT',
        runId: 'run_1',
        messageId: 'msg_1',
        delta: 'Hello',
      }),
    })

    expect(event).toMatchObject({
      type: 'TEXT_MESSAGE_CONTENT',
      runId: 'run_1',
      messageId: 'msg_1',
      delta: 'Hello',
    })
  })

  it('maps AG-UI lifecycle and text events to chat handlers', () => {
    const onConnected = vi.fn()
    const onMessage = vi.fn()
    const onDone = vi.fn()

    const events: AgUiEvent[] = [
      { type: 'RUN_STARTED', runId: 'run_1', threadId: 'thread_1' },
      { type: 'TEXT_MESSAGE_CONTENT', runId: 'run_1', delta: 'Hi' },
      { type: 'RUN_FINISHED', runId: 'run_1', modelUsed: 'velion-default', outputTokens: 3 },
    ]

    for (const event of events) {
      dispatchAgUiChatEvent(event, { onConnected, onMessage, onDone })
    }

    expect(onConnected).toHaveBeenCalledWith({
      ok: true,
      requestId: 'run_1',
      threadId: 'thread_1',
      model: undefined,
    })
    expect(onMessage).toHaveBeenCalledWith({ content: 'Hi', requestId: 'run_1' })
    expect(onDone).toHaveBeenCalledWith({
      requestId: 'run_1',
      modelUsed: 'velion-default',
      outputTokens: 3,
    })
  })
})
