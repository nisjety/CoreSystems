import { describe, expect, it, vi } from 'vitest'
import {
  buildAgentRunInputBody,
  dispatchAgUiChatEvent,
  parseAgUiSseEvent,
  toAgUiEvent,
  type AgUiEvent,
} from './ag-ui-client'
import { toVerevonUiEvent } from '@/shared/chat/verevon-ui-events'

describe('AG-UI client adapter', () => {
  it('builds TanStack-compatible RunAgentInput bodies for the gateway', () => {
    const body = buildAgentRunInputBody({
      content: 'Refresh this source',
      model: 'verevon-default',
      threadId: 'thread_1',
      sessionKey: 'thread_1',
      browseWeb: true,
      generateImage: false,
      deepResearch: true,
      planMode: true,
      effort: 'deep',
      spaceRef: 'space_1',
      mentionedAgentRef: 'agent_1',
      minPrivacyTier: 'eu_resident',
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
        model: 'verevon-default',
        profile: 'chat',
        sessionKey: 'thread_1',
        generateImage: false,
        browseWeb: true,
        deepResearch: true,
        planMode: true,
        effort: 'deep',
        spaceRef: 'space_1',
        mentionedAgentRef: 'agent_1',
        minPrivacyTier: 'eu_resident',
      },
    })
    // knowledge.recrawl_source is a known registry action but Model eligibility
    // is fail-closed (see model-eligibility.ts / agent-tools.ts) until an owner
    // exposes a governed operation contract, so it is stripped here rather than
    // forwarded as a callable tool.
    expect((body.tools as Array<{ name: string }>).map((tool) => tool.name)).toEqual([
      'web_search',
    ])
    expect(body.data).toEqual(body.forwardedProps)
  })

  it('parses typed AG-UI SSE payloads', () => {
    const event = parseAgUiSseEvent({
      data: JSON.stringify({
        type: 'TEXT_MESSAGE_CONTENT',
        timestamp: '2026-08-30T10:00:00Z',
        metadata: { verevon: { adapter: 'native-chat' } },
        runId: 'run_1',
        messageId: 'msg_1',
        delta: 'Hello',
      }),
    })

    expect(event).toMatchObject({
      type: 'TEXT_MESSAGE_CONTENT',
      timestamp: '2026-08-30T10:00:00Z',
      runId: 'run_1',
      messageId: 'msg_1',
      delta: 'Hello',
    })
  })

  it('accepts AG-UI JSON-fragment tool arguments while retaining native args', () => {
    const onToolCall = vi.fn()

    dispatchAgUiChatEvent(
      { type: 'TOOL_CALL_ARGS', toolCallId: 'tool_1', delta: '{"sourceId":"src_1"}' },
      { onToolCall },
    )

    expect(onToolCall).toHaveBeenCalledWith({
      id: 'tool_1',
      name: undefined,
      args: { sourceId: 'src_1' },
    })
  })

  it('serializes public Verevon events only through safe AG-UI equivalents', () => {
    expect(toAgUiEvent({
      type: 'message.delta',
      at: '2026-08-30T10:00:00Z',
      delta: 'Hello',
    })).toMatchObject({
      type: 'TEXT_MESSAGE_CHUNK',
      delta: 'Hello',
      timestamp: '2026-08-30T10:00:00Z',
    })

    expect(toAgUiEvent({
      type: 'unknown',
      at: '2026-08-30T10:00:00Z',
      name: 'future',
      payload: {},
    })).toBeNull()
  })

  it('round-trips a generic pause without turning CUSTOM into an opaque event', () => {
    const serialized = toAgUiEvent({
      type: 'run.paused',
      at: '2026-08-30T10:00:00Z',
      runId: 'run_approval',
      pauseKind: 'approval',
      detail: 'Awaiting approval',
    })

    expect(serialized).toMatchObject({ type: 'CUSTOM', name: 'run_paused' })
    expect(toVerevonUiEvent('CUSTOM', serialized as Record<string, unknown>)).toMatchObject({
      type: 'run.paused',
      runId: 'run_approval',
      pauseKind: 'approval',
      detail: 'Awaiting approval',
    })
  })

  it('maps AG-UI lifecycle and text events to chat handlers', () => {
    const onConnected = vi.fn()
    const onMessage = vi.fn()
    const onDone = vi.fn()

    const events: AgUiEvent[] = [
      { type: 'RUN_STARTED', runId: 'run_1', threadId: 'thread_1' },
      { type: 'TEXT_MESSAGE_CONTENT', runId: 'run_1', delta: 'Hi' },
      { type: 'TEXT_MESSAGE_END', runId: 'run_1', messageId: 'msg_1' },
      { type: 'RUN_FINISHED', runId: 'run_1', modelUsed: 'verevon-default', outputTokens: 3 },
    ]

    for (const event of events) {
      dispatchAgUiChatEvent(event, { onConnected, onMessage, onDone })
    }

    expect(onConnected).toHaveBeenCalledWith({
      ok: true,
      requestId: 'run_1',
      threadId: 'thread_1',
      runId: 'run_1',
      model: undefined,
    })
    expect(onMessage).toHaveBeenCalledWith({ content: 'Hi', requestId: 'run_1' })
    expect(onDone).toHaveBeenCalledTimes(1)
    expect(onDone).toHaveBeenCalledWith({
      requestId: 'run_1',
      modelUsed: 'verevon-default',
      outputTokens: 3,
    })
  })

  it('keeps an AG-UI interrupt as a resumable pause instead of completion', () => {
    const onDone = vi.fn()
    const onUiEvent = vi.fn()

    dispatchAgUiChatEvent({
      type: 'RUN_FINISHED',
      runId: 'run_approval',
      outcome: {
        type: 'interrupt',
        interrupts: [{ id: 'approval_1', reason: 'needs approval' }],
      },
    }, { onDone, onUiEvent })

    expect(onDone).not.toHaveBeenCalled()
    expect(onUiEvent).toHaveBeenCalledWith(expect.objectContaining({
      type: 'run.paused',
      runId: 'run_approval',
      interrupts: [{ id: 'approval_1', reason: 'needs approval' }],
    }))
  })

  it('keeps a stopped RUN_FINISHED outcome distinct from normal completion', () => {
    const onStopped = vi.fn()
    const onDone = vi.fn()

    dispatchAgUiChatEvent({
      type: 'RUN_FINISHED',
      runId: 'run_stopped',
      requestId: 'request_stopped',
      outcome: { type: 'success', status: 'stopped' },
    }, { onStopped, onDone })

    expect(onDone).not.toHaveBeenCalled()
    expect(onStopped).toHaveBeenCalledWith({ requestId: 'request_stopped', reason: undefined })
    expect(toVerevonUiEvent('RUN_FINISHED', {
      runId: 'run_stopped',
      requestId: 'request_stopped',
      outcome: { type: 'success', status: 'stopped' },
    })).toMatchObject({ type: 'run.stopped', requestId: 'request_stopped' })
  })

  it('dispatches canonical Verevon CUSTOM events through focused chat callbacks', () => {
    const onCitation = vi.fn()
    const onUiEvent = vi.fn()

    dispatchAgUiChatEvent({
      type: 'CUSTOM',
      name: 'citation.added',
      value: {
        type: 'citation.added',
        id: 'claim-source-1',
        title: 'Authoritative source',
        url: 'https://example.com/source',
        snippet: 'Evidence',
        claimId: 'claim-1',
        sourceGroupId: 'group-1',
        start: 4,
        end: 18,
      },
    }, { onCitation, onUiEvent })

    expect(onCitation).toHaveBeenCalledWith({
      id: 'claim-source-1',
      title: 'Authoritative source',
      url: 'https://example.com/source',
      snippet: 'Evidence',
      claimId: 'claim-1',
      sourceGroupId: 'group-1',
      start: 4,
      end: 18,
    })
    expect(onUiEvent).toHaveBeenCalledWith(expect.objectContaining({ type: 'citation.added', claimId: 'claim-1' }))
  })
})
