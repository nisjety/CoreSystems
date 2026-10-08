import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  approvePlan,
  buildChatWireBody,
  groupChatModels,
  listModels,
  rejectPlan,
  shouldRequestSupportContext,
  describeFeedbackFailure,
  getChatThreadTranscript,
  getThreadContext,
  listChatThreads,
  modelSupportsAgentic,
  parseTurnTiming,
  queueInvocationInput,
  resumeStream,
  saveChatThreadSnapshot,
  streamChat,
  subscriptionModelDisplayName,
  submitFeedback,
} from './chat-client'
import type { ChatStreamHandlers } from './chat-client'
import {
  bindSupportChatThread,
  clearSupportChatThreads,
} from '@/shared/chat/support-chat-thread'
import {
  __resetRetentionForTests,
  isLocalRetentionAllowed,
} from '@/features/chat/lib/chat-retention'
import {
  readChatThreadHistory,
  upsertChatThreadHistory,
} from '@/features/chat/lib/chat-thread-history'
import { ApiError } from './http'

function sseResponse(frames: string[]): Response {
  const encoder = new TextEncoder()
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const frame of frames) controller.enqueue(encoder.encode(frame))
      controller.close()
    },
  })
  return new Response(stream, {
    status: 200,
    headers: { 'Content-Type': 'text/event-stream' },
  })
}

describe('chat-client connected event', () => {
  afterEach(() => {
    clearSupportChatThreads()
    vi.unstubAllGlobals()
  })

  it('maps the agentic run_id into ChatConnectedEvent.runId', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        `event: connected\ndata: ${JSON.stringify({
          ok: true,
          request_id: 'req_1',
          thread_id: 'thread_1',
          model: 'gpt-4o',
          run_id: 'run_1',
        })}\n\n`,
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onConnected = vi.fn()
    await streamChat({ content: 'hi' }, { onConnected })

    expect(onConnected).toHaveBeenCalledWith(
      expect.objectContaining({
        ok: true,
        requestId: 'req_1',
        threadId: 'thread_1',
        runId: 'run_1',
      }),
    )
  })
})

describe('chat-client stream-event coverage', () => {
  afterEach(() => {
    clearSupportChatThreads()
    vi.unstubAllGlobals()
  })

  it('routes memory_recall to its own handler and skips a malformed zero count', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        sseResponse([
          `event: memory_recall\ndata: ${JSON.stringify({ count: 3, latency_ms: 42 })}\n\n`,
          // The backend never emits this — it only fires when memory was
          // genuinely injected — so a 0 means a malformed payload. Rendering
          // "recalled 0 memories" would be worse than rendering nothing.
          `event: memory_recall\ndata: ${JSON.stringify({ count: 0 })}\n\n`,
        ]),
      ),
    )
    const onMemoryRecall = vi.fn()
    await streamChat({ content: 'hi' }, { onMemoryRecall })

    expect(onMemoryRecall).toHaveBeenCalledTimes(1)
    expect(onMemoryRecall).toHaveBeenCalledWith({
      count: 3,
      latencyMs: 42,
      memories: [],
    })
  })

  it('asks for the memory feature family by default so the backend emits the event', () => {
    // The recall event is opt-in server-side. Without this the injection still
    // happens but the UI can never show that it did.
    expect(buildChatWireBody({ content: 'hi' }).features).toContain('memory')
  })

  /**
   * Pinned messages ride the wire as IDS. Sending the pinned text instead would
   * let this browser assert that the user said something earlier in the thread;
   * an id can only ever select a message the durable thread already holds, and
   * model-gateway ignores one that resolves to nothing.
   */
  it('sends pinned message ids, and omits the field entirely when nothing is pinned', () => {
    const plain = buildChatWireBody({ content: 'hi' })
    // Absent, not an empty array: an ordinary turn's body stays byte-identical
    // to what it was before pinning existed.
    expect('pinned_message_ids' in plain).toBe(false)

    const pinned = buildChatWireBody({ content: 'hi', pinnedMessageIds: ['m1', 'm2'] })
    expect(pinned.pinned_message_ids).toEqual(['m1', 'm2'])

    expect('pinned_message_ids' in buildChatWireBody({ content: 'hi', pinnedMessageIds: [] })).toBe(
      false,
    )
  })

  it('never puts pinned message CONTENT on the wire', () => {
    const body = buildChatWireBody({ content: 'hi', pinnedMessageIds: ['m1'] })
    const serialized = JSON.stringify(body)
    expect(serialized).toContain('"m1"')
    // Only the id travels; there is no field carrying a pinned message's text.
    expect(serialized).not.toContain('pinned_messages')
    expect(serialized).not.toContain('pinnedMessages')
  })

  it('separates a server-side stop from a normal completion', async () => {
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue(
          sseResponse([
            `event: stopped\ndata: ${JSON.stringify({ reason: 'budget exhausted' })}\n\n`,
          ]),
        ),
    )
    const onStopped = vi.fn()
    const onDone = vi.fn()
    await streamChat({ content: 'hi' }, { onStopped, onDone })

    expect(onStopped).toHaveBeenCalledWith(
      expect.objectContaining({ reason: 'budget exhausted' }),
    )
    expect(onDone).not.toHaveBeenCalled()
  })

  it('still falls back to onDone for a caller that predates onStopped', async () => {
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue(
          sseResponse([`event: stopped\ndata: ${JSON.stringify({})}\n\n`]),
        ),
    )
    const onDone = vi.fn()
    await streamChat({ content: 'hi' }, { onDone })
    expect(onDone).toHaveBeenCalledTimes(1)
  })

  it('reads the BFF error envelope, so a broken upstream stream keeps its transport code', async () => {
    // Byte-for-byte what the BFF's `sse_transport_error_event` writes when its
    // upstream byte stream fails mid-answer. Read flat, this became an
    // anonymous "Stream error" and the turn was failed instead of resumed.
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        sseResponse([
          'event: error\ndata: {"error":{"code":"upstream_unavailable","message":"The upstream service is unavailable."}}\n\n',
          `event: error\ndata: ${JSON.stringify({ code: 'response_validation_failed', message: 'Ikke publisert.', retryable: true })}\n\n`,
        ]),
      ),
    )
    const onError = vi.fn()
    await streamChat({ content: 'hi' }, { onError })
    expect(onError).toHaveBeenNthCalledWith(1, expect.objectContaining({
      code: 'upstream_unavailable',
      message: 'The upstream service is unavailable.',
    }))
    // Model Gateway's flat codes are unchanged.
    expect(onError).toHaveBeenNthCalledWith(2, expect.objectContaining({
      code: 'response_validation_failed',
      message: 'Ikke publisert.',
      retryable: true,
    }))
  })

  it('carries a subscription refusal\'s reset time and retry delay, in either spelling', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        sseResponse([
          // Byte-for-byte the gateway's usage-limit frame (snake_case).
          `event: error\ndata: ${JSON.stringify({
            code: 'subscription_usage_limit',
            message: 'Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex.',
            retryable: false,
            resets_at: '2026-09-26T01:21:00Z',
            retry_after_ms: 52_260_000,
            request_id: 'req-1',
          })}\n\n`,
          `event: error\ndata: ${JSON.stringify({ code: 'rate_limited', message: 'x', retryAfterMs: 2_000, resetsAt: '2026-09-26T01:21:00.000Z' })}\n\n`,
          // A reset instant that does not parse, and a delay that is not a
          // delay, are dropped rather than handed to the UI.
          `event: error\ndata: ${JSON.stringify({ code: 'subscription_usage_limit', message: 'x', resets_at: 'tomorrow', retry_after_ms: -5 })}\n\n`,
          `event: error\ndata: ${JSON.stringify({ code: 'subscription_upstream_unavailable', message: 'x', retry_after_ms: '100' })}\n\n`,
        ]),
      ),
    )
    const onError = vi.fn()
    await streamChat({ content: 'hi' }, { onError })
    expect(onError).toHaveBeenCalledTimes(4)
    expect(onError.mock.calls[0]?.[0]).toEqual({
      code: 'subscription_usage_limit',
      message: 'Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex.',
      retryable: false,
      runId: undefined,
      requestId: 'req-1',
      resetsAt: '2026-09-26T01:21:00Z',
      retryAfterMs: 52_260_000,
    })
    expect(onError.mock.calls[1]?.[0]).toMatchObject({ code: 'rate_limited', retryAfterMs: 2_000, resetsAt: '2026-09-26T01:21:00.000Z' })
    for (const call of onError.mock.calls.slice(2)) {
      expect(call[0]).not.toHaveProperty('resetsAt')
      expect(call[0]).not.toHaveProperty('retryAfterMs')
    }
  })

  it('carries stop_reason through done so a truncated answer is visible', async () => {
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue(
          sseResponse([
            `event: done\ndata: ${JSON.stringify({ stop_reason: 'stream_incomplete' })}\n\n`,
          ]),
        ),
    )
    const onDone = vi.fn()
    await streamChat({ content: 'hi' }, { onDone })
    expect(onDone).toHaveBeenCalledWith(
      expect.objectContaining({ stopReason: 'stream_incomplete' }),
    )
  })

  it('surfaces an unrecognised event instead of dropping it, but ignores STREAM_ envelopes', async () => {
    // The gateway relays upstream SSE verbatim with no allowlist, so an event
    // this client has not learned still arrives. Dropping it silently is how
    // a new backend capability stays invisible.
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue(
          sseResponse([
            `event: some_future_event\ndata: ${JSON.stringify({ hello: 'world' })}\n\n`,
            `event: STREAM_OPENED\ndata: ${JSON.stringify({ noise: true })}\n\n`,
          ]),
        ),
    )
    const onUnknownEvent = vi.fn()
    await streamChat({ content: 'hi' }, { onUnknownEvent })

    expect(onUnknownEvent).toHaveBeenCalledTimes(1)
    expect(onUnknownEvent).toHaveBeenCalledWith({
      name: 'some_future_event',
      payload: { hello: 'world' },
    })
  })
})

describe('mid-run queued input', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  const jsonResponse = (status: number, body: unknown) =>
    new Response(JSON.stringify(body), {
      status,
      headers: { 'Content-Type': 'application/json' },
    })

  it('routes queued_input to its own handler and ignores a frame with no text', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        sseResponse([
          `event: queued_input\ndata: ${JSON.stringify({ messages: ['bruk EUR', '  '] })}\n\n`,
          // A delivery we cannot show is not a delivery. Reporting it would tell
          // the user their message arrived without saying which one.
          `event: queued_input\ndata: ${JSON.stringify({ messages: [] })}\n\n`,
        ]),
      ),
    )
    const onQueuedInput = vi.fn()
    await streamChat({ content: 'hi' }, { onQueuedInput })

    expect(onQueuedInput).toHaveBeenCalledTimes(1)
    expect(onQueuedInput).toHaveBeenCalledWith({ messages: ['bruk EUR'] })
  })

  it('reports acceptance, including a delivery that was not durably recorded', async () => {
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue(
          jsonResponse(202, {
            request_id: 'req-1',
            queued: true,
            pending: 2,
            persisted: false,
          }),
        ),
    )
    // `persisted: false` means the agent WILL read it but the thread does not
    // record it — the reply would otherwise appear in history answering nothing.
    await expect(queueInvocationInput('req-1', 'bruk EUR')).resolves.toEqual({
      outcome: 'queued',
      pending: 2,
      persisted: false,
    })
  })

  /**
   * The ordinary race, not a failure: the stream ended between the keystroke and
   * the request. It has to be distinguishable, because the caller's correct
   * response is to send the message as a normal turn — and the original bug was
   * exactly a lost message with no signal.
   */
  it('separates "the run already ended" from a refusal', async () => {
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue(
          jsonResponse(404, {
            error: { code: 'not_found', message: 'no active stream' },
          }),
        ),
    )
    await expect(queueInvocationInput('req-1', 'bruk EUR')).resolves.toEqual({
      outcome: 'run_ended',
    })
  })

  it('handles the browser-facing run-ended protocol without producing an HTTP 404', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        jsonResponse(200, {
          request_id: 'req-1',
          queued: false,
          error: 'no active stream',
          resend_as_new_turn: true,
        }),
      ),
    )

    await expect(queueInvocationInput('req-1', 'bruk EUR')).resolves.toEqual({
      outcome: 'run_ended',
    })
  })

  it('keeps the two refusal reasons apart, since one means wait and the other means shorten', async () => {
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue(
          jsonResponse(429, {
            error: { code: 'too_many', message: 'for mange i kø' },
          }),
        ),
    )
    await expect(queueInvocationInput('req-1', 'a')).resolves.toEqual({
      outcome: 'refused',
      reason: 'too_many',
      message: 'for mange i kø',
    })

    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue(
          jsonResponse(413, {
            error: { code: 'too_long', message: 'for lang' },
          }),
        ),
    )
    await expect(queueInvocationInput('req-1', 'a')).resolves.toEqual({
      outcome: 'refused',
      reason: 'too_long',
      message: 'for lang',
    })
  })

  /**
   * The browser may say which Space and thread it thinks it is on, but those are
   * hints: the BFF mints the append decision and the Model Plane appends to the
   * thread its own stream registration recorded. Pinned because dropping these
   * would silently break Space-scoped threads (no decision to mint against).
   */
  it('passes the space and thread hints through for the BFF to mint against', async () => {
    const bodies: string[] = []
    vi.stubGlobal(
      'fetch',
      vi.fn().mockImplementation((_url: unknown, init?: RequestInit) => {
        bodies.push(String(init?.body ?? ''))
        return Promise.resolve(
          jsonResponse(202, { queued: true, pending: 1, persisted: true }),
        )
      }),
    )
    await queueInvocationInput('req-1', 'bruk EUR', {
      threadId: 'thread-9',
      spaceRef: 'space-3',
    })
    expect(JSON.parse(bodies[0] ?? '')).toEqual({
      content: 'bruk EUR',
      thread_id: 'thread-9',
      space_ref: 'space-3',
    })
  })

  it('omits the hints entirely when there are none, rather than sending empties', async () => {
    const bodies: string[] = []
    vi.stubGlobal(
      'fetch',
      vi.fn().mockImplementation((_url: unknown, init?: RequestInit) => {
        bodies.push(String(init?.body ?? ''))
        return Promise.resolve(
          jsonResponse(202, { queued: true, pending: 1, persisted: true }),
        )
      }),
    )
    await queueInvocationInput('req-1', 'bruk EUR')
    // An empty `space_ref` would read to the BFF as a Space selection and send
    // it looking for a decision that cannot exist.
    expect(JSON.parse(bodies[0] ?? '')).toEqual({ content: 'bruk EUR' })
  })
})

describe('chat-client tool wiring', () => {
  it('carries conversation isolation and suppresses automatic support/web/agent context', () => {
    const body = buildChatWireBody({ content: 'Finn oppfølging i innboksen', sourceScope: 'conversation', browseWeb: true, deepResearch: true, generateImage: true, planMode: true })
    expect(body.features).toContain('conversation_only')
    expect(body.features).not.toContain('agentic')
    expect(body.support_context_query).toBeUndefined()
    expect(body.browse_web).toBe(false)
    expect(body.deep_research).toBe(false)
    expect(body.generate_image).toBe(false)
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('requests the tools family by default (so knowledge_search/fetch_url attach server-side) but declares no client tools, and never web_search', () => {
    const body = buildChatWireBody({ content: 'hi' })

    // model-gateway merges its own builtins (knowledge_search, fetch_url) into
    // the tool list whenever 'tools' is present, regardless of the client's
    // own (here empty) tools array — see buildChatWireBody's comment.
    expect(body.features).toEqual(expect.arrayContaining(['tools']))
    expect(body.tools).toEqual([])
  })

  it('requests web_search only when browsing is enabled', () => {
    const body = buildChatWireBody({ content: 'latest news', browseWeb: true })

    expect(body.browse_web).toBe(true)
    expect(body.features).toEqual(expect.arrayContaining(['tools']))
    expect(body.tools).toEqual([
      expect.objectContaining({ name: 'web_search' }),
    ])
  })

  it('routes a connected subscription without incompatible tool features', () => {
    const body = buildChatWireBody({
      content: 'hello from my plan',
      model: 'gpt-5.6-luna',
      provider: 'openai-codex-subscription',
      subscriptionConnectionId: 'conn-subscription-1',
      browseWeb: true,
      generateImage: true,
      deepResearch: true,
      attachments: [{ kind: 'image', data_base64: 'abc' }],
    })

    expect(body).toMatchObject({
      model: 'gpt-5.6-luna',
      provider: 'openai-codex-subscription',
      subscription_connection_id: 'conn-subscription-1',
      browse_web: false,
      generate_image: false,
      deep_research: false,
      plan_mode: false,
      attachments: [],
      tools: [],
    })
    // `tools` DOES stay on for a subscription turn: the subscription route
    // proposes tool calls through the scoped broker and model-gateway executes
    // them, so this is what lets a subscription turn reach
    // get_weather/code_interpreter at all. Without plan mode there is no
    // `agentic` run either.
    expect(body.features).toContain('tools')
    expect(body.features).not.toContain('agentic')
  })

  // A5: Do on the ChatGPT subscription. The composer offers it only when the
  // server advertises the `agentic` capability; what reaches the wire is the
  // agentic feature and plan_mode, and nothing else the route cannot do.
  it('carries Do on a subscription turn as the agentic feature plus plan_mode, and nothing else the route refuses', () => {
    const body = buildChatWireBody({
      content: 'Research this',
      model: 'gpt-5.6-terra',
      provider: 'openai-codex-subscription',
      subscriptionConnectionId: 'conn-subscription-1',
      planMode: true,
      // None of these may ride along: the server refuses an agentic
      // subscription run that also asks for them.
      browseWeb: true,
      generateImage: true,
      deepResearch: true,
      attachments: [{ kind: 'image', data_base64: 'abc' }],
      actions: [{ id: 'skill-1', name: 'Quality report', kind: 'skill' }],
    })

    expect(body).toMatchObject({
      model: 'gpt-5.6-terra',
      provider: 'openai-codex-subscription',
      subscription_connection_id: 'conn-subscription-1',
      plan_mode: true,
      browse_web: false,
      generate_image: false,
      deep_research: false,
      attachments: [],
      tools: [],
      zdr: false,
    })
    expect(body.features).toEqual(expect.arrayContaining(['agentic', 'tools']))
    expect(body).not.toHaveProperty('skill_ids')
  })

  it('sends neither agentic nor plan_mode for a conversation-only subscription turn, since the server admits plan_mode only with agentic', () => {
    const body = buildChatWireBody({
      content: 'Only this chat',
      model: 'gpt-5.6-terra',
      provider: 'openai-codex-subscription',
      subscriptionConnectionId: 'conn-subscription-1',
      planMode: true,
      sourceScope: 'conversation',
    })

    expect(body.plan_mode).toBe(false)
    expect(body.features).not.toContain('agentic')
  })

  it('keeps plan_mode and the agentic feature together on a subscription turn, in both states', () => {
    for (const planMode of [true, false]) {
      const body = buildChatWireBody({
        content: 'hello',
        model: 'gpt-5.6-terra',
        provider: 'openai-codex-subscription',
        subscriptionConnectionId: 'conn-subscription-1',
        planMode,
      })
      expect(body.plan_mode).toBe(planMode)
      expect((body.features as string[]).includes('agentic')).toBe(planMode)
    }
  })

  it('leaves Do on a non-subscription turn exactly as it was', () => {
    const body = buildChatWireBody({ content: 'Plan it', planMode: true })

    expect(body.plan_mode).toBe(true)
    expect(body.features).toEqual(expect.arrayContaining(['agentic', 'tools']))
    expect(buildChatWireBody({ content: 'Plan it' }).plan_mode).toBe(false)
    expect(buildChatWireBody({ content: 'Plan it' }).features).not.toContain('agentic')
  })

  it('reads the agentic capability from the catalogue entry alone, and treats an unlisted model as not agentic', () => {
    expect(modelSupportsAgentic({ capabilities: ['tools', 'agentic'] })).toBe(true)
    expect(modelSupportsAgentic({ capabilities: ['tools'] })).toBe(false)
    expect(modelSupportsAgentic({ capabilities: undefined })).toBe(false)
    expect(modelSupportsAgentic(undefined)).toBe(false)
  })

  it('maps a catalogue feature list carrying agentic onto the model capabilities', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({
      models: [
        { id: 'gpt-5.6-terra', provider: 'openai-codex-subscription', features: ['tools', 'agentic'] },
        { id: 'gpt-5.6-luna', provider: 'openai-codex-subscription', features: ['tools'] },
      ],
    }), { status: 200, headers: { 'Content-Type': 'application/json' } })))

    const [terra, luna] = await listModels()
    expect(modelSupportsAgentic(terra)).toBe(true)
    expect(modelSupportsAgentic(luna)).toBe(false)
    vi.unstubAllGlobals()
  })

  it('names and groups ChatGPT-plan models as subscriptions, not Codex products', () => {
    expect(subscriptionModelDisplayName('gpt-5.6-luna')).toBe('GPT 5.6 Luna Subscription')
    expect(subscriptionModelDisplayName('gpt-5.3-codex-spark')).toBe('GPT 5.3 Spark Subscription')
    expect(groupChatModels([{
      id: 'gpt-5.6-luna',
      name: 'GPT 5.6 Luna Subscription',
      provider: 'openai-codex-subscription',
    }])[0]?.label).toBe('Subscription')
  })

  it('sends only a requested Space reference for a new thread, never authority metadata', () => {
    const body = buildChatWireBody({
      content: 'Plan my work',
      spaceRef: 'personal-space-1',
    })

    expect(body.space_ref).toBe('personal-space-1')
    expect(body).not.toHaveProperty('space_context')
    expect(body).not.toHaveProperty('space_decision_token')
  })

  it('keeps a Support-derived thread read-only on every later Chat turn', () => {
    bindSupportChatThread({ userId: 'user-1', orgId: 'org-1', conversationId: 'conversation-1' }, 'support_thread')

    const body = buildChatWireBody({
      content: 'Follow up',
      threadId: 'support_thread',
      browseWeb: true,
      generateImage: true,
      deepResearch: true,
      planMode: true,
      actions: [{ id: 'send', name: 'Send message', kind: 'tool' }],
    })

    expect(body.features).not.toEqual(expect.arrayContaining(['tools', 'agentic']))
    expect(body).toMatchObject({
      browse_web: false,
      generate_image: false,
      deep_research: false,
      plan_mode: false,
      tools: [],
    })
  })

  it('requests permission-filtered Support context for support intelligence questions', () => {
    expect(shouldRequestSupportContext('Which support tickets are at SLA risk?')).toBe(true)
    expect(shouldRequestSupportContext('Show me conversations that need follow-up.')).toBe(true)
    expect(shouldRequestSupportContext('What is the weather today?')).toBe(false)

    expect(buildChatWireBody({ content: 'Summarize unresolved customer issues this week.' })).toMatchObject({
      support_context_query: 'Summarize unresolved customer issues this week.',
    })
  })

  it('does not add broad Support lookup to a Support-derived thread', () => {
    expect(buildChatWireBody({
      content: 'Which support tickets are at SLA risk?',
      threadId: 'support_thread',
    })).not.toHaveProperty('support_context_query')
  })

  it('maps batched citation events into individual citations (legacy payload, no claim/anchor keys)', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        `event: citations\ndata: ${JSON.stringify({
          citations: [
            { id: 'c1', title: 'Result', url: 'https://example.com', snippet: 'Snippet' },
          ],
        })}\n\n`,
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onCitation = vi.fn()
    await streamChat({ content: 'hi' }, { onCitation })

    expect(onCitation).toHaveBeenCalledWith({
      id: 'c1',
      title: 'Result',
      url: 'https://example.com',
      snippet: 'Snippet',
    })
  })

  it('maps a full citation payload (claim_id/source_group_id/source_group_label/start/end) through to onCitation', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        `event: citations\ndata: ${JSON.stringify({
          citations: [
            {
              id: 'c1',
              title: 'Refund policy',
              url: 'https://intranet/refunds',
              snippet: 'Snippet',
              claim_id: 'req-1:claim-0',
              source_group_id: 'kb:sharepoint',
              source_group_label: 'Microsoft 365',
              start: 4,
              end: 9,
            },
          ],
        })}\n\n`,
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onCitation = vi.fn()
    await streamChat({ content: 'hi' }, { onCitation })

    expect(onCitation).toHaveBeenCalledWith({
      id: 'c1',
      title: 'Refund policy',
      url: 'https://intranet/refunds',
      snippet: 'Snippet',
      claimId: 'req-1:claim-0',
      sourceGroupId: 'kb:sharepoint',
      sourceGroupLabel: 'Microsoft 365',
      start: 4,
      end: 9,
    })
  })

  it('maps a follow_ups event into onFollowUps, dropping non-string/empty entries', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        `event: follow_ups\ndata: ${JSON.stringify({
          suggestions: ['Hva med frakt til Bergen?', '', 42, 'Kan jeg spore ordren?'],
          request_id: 'req_1',
        })}\n\n`,
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onFollowUps = vi.fn()
    await streamChat({ content: 'hi' }, { onFollowUps })

    expect(onFollowUps).toHaveBeenCalledWith({
      suggestions: ['Hva med frakt til Bergen?', 'Kan jeg spore ordren?'],
      requestId: 'req_1',
    })
  })

  it('never calls onFollowUps when the event carries no usable suggestions', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        `event: follow_ups\ndata: ${JSON.stringify({ suggestions: [] })}\n\n`,
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onFollowUps = vi.fn()
    await streamChat({ content: 'hi' }, { onFollowUps })

    expect(onFollowUps).not.toHaveBeenCalled()
  })

  /** plan-gate §2.1: the model's structured plan-mode plan. */
  it('maps a plan_proposed event into onPlanProposed with its steps', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        `event: plan_proposed\ndata: ${JSON.stringify({
          plan_id: 'plan_1',
          summary: 'Send the overdue invoice',
          steps: [
            { id: 'step_1', title: 'Draft the email', operation: '' },
            { id: 'step_2', title: 'Send it', operation: 'email_send' },
          ],
          request_id: 'req_1',
        })}\n\n`,
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onPlanProposed = vi.fn()
    await streamChat({ content: 'hi' }, { onPlanProposed })

    expect(onPlanProposed).toHaveBeenCalledWith({
      planId: 'plan_1',
      summary: 'Send the overdue invoice',
      steps: [
        { id: 'step_1', title: 'Draft the email', operation: '' },
        { id: 'step_2', title: 'Send it', operation: 'email_send' },
      ],
      requestId: 'req_1',
    })
  })

  it('never calls onPlanProposed when the frame is missing an id or summary', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([`event: plan_proposed\ndata: ${JSON.stringify({ steps: [] })}\n\n`]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onPlanProposed = vi.fn()
    await streamChat({ content: 'hi' }, { onPlanProposed })

    expect(onPlanProposed).not.toHaveBeenCalled()
  })
})

describe('approvePlan / rejectPlan (plan-gate §2.3)', () => {
  function jsonResponse(data: unknown): Response {
    return new Response(JSON.stringify({ data }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    })
  }

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('posts decision=approve with plan_id and wire-shaped edited_steps, and normalizes the returned plan', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        granted_rung: 'workspace_write',
        was_in_plan_mode: true,
        persisted: true,
        plan: {
          id: 'plan_1',
          run_id: 'run_1',
          thread_id: 'thread_1',
          state: 'PLAN_STATE_APPROVED',
          summary: 'Send the overdue invoice',
          steps: [{ id: 'step_1', title: 'Draft the email', state: 'PLAN_STEP_STATE_PENDING' }],
        },
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await approvePlan('run_1', 'workspace_write', 'writes its report', 'plan_1', [
      { stepId: 'step_1', title: 'Draft the email', removed: false },
      { title: 'A brand new step', removed: false },
    ])

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toContain('/api/v1/agents/runs/run_1/plan-approval')
    expect(init.method).toBe('POST')
    expect(JSON.parse(init.body as string)).toEqual({
      decision: 'approve',
      granted_rung: 'workspace_write',
      justification: 'writes its report',
      plan_id: 'plan_1',
      edited_steps: [
        { step_id: 'step_1', title: 'Draft the email', operation: '', removed: false },
        { step_id: '', title: 'A brand new step', operation: '', removed: false },
      ],
    })
    expect(result.grantedRung).toBe('workspace_write')
    expect(result.plan?.id).toBe('plan_1')
    expect(result.plan?.state).toBe('PLAN_STATE_APPROVED')
    expect(result.plan?.steps?.[0]?.title).toBe('Draft the email')
  })

  it('omits plan_id/edited_steps for the pure rung-grant path (no structured plan)', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ granted_rung: 'workspace_write', was_in_plan_mode: true, persisted: true }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await approvePlan('run_1', 'workspace_write', 'writes its report')

    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(JSON.parse(init.body as string)).toEqual({
      decision: 'approve',
      granted_rung: 'workspace_write',
      justification: 'writes its report',
      plan_id: '',
      edited_steps: [],
    })
    expect(result.plan).toBeUndefined()
  })

  it('posts decision=reject with the plan id and reason, granting nothing', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        plan: { id: 'plan_1', state: 'PLAN_STATE_REJECTED', summary: 'Send the overdue invoice' },
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await rejectPlan('run_1', 'plan_1', 'Ikke nødvendig lenger')

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toContain('/api/v1/agents/runs/run_1/plan-approval')
    expect(JSON.parse(init.body as string)).toEqual({
      decision: 'reject',
      justification: 'Ikke nødvendig lenger',
      plan_id: 'plan_1',
      edited_steps: [],
    })
    expect(result.plan?.state).toBe('PLAN_STATE_REJECTED')
  })
})

describe('buildChatWireBody — retrieval scope (chat-wave2 scope spec)', () => {
  it('omits retrieval_scope when unset or auto, matching the byte-identical default', () => {
    expect(buildChatWireBody({ content: 'hi' }).retrieval_scope).toBeUndefined()
    expect(buildChatWireBody({ content: 'hi', retrievalScope: 'auto' }).retrieval_scope).toBeUndefined()
  })

  it('sends retrieval_scope when the caller picked knowledge_only or web_only', () => {
    expect(buildChatWireBody({ content: 'hi', retrievalScope: 'knowledge_only' }).retrieval_scope).toBe('knowledge_only')
    expect(buildChatWireBody({ content: 'hi', retrievalScope: 'web_only' }).retrieval_scope).toBe('web_only')
  })

  it('forces retrieval_scope absent when sourceScope is conversation, even if a restrictive retrievalScope was also passed', () => {
    const body = buildChatWireBody({
      content: 'hi',
      sourceScope: 'conversation',
      retrievalScope: 'web_only',
    })
    expect(body.retrieval_scope).toBeUndefined()
    // Conversation-only's own exclusions still apply unchanged alongside it.
    expect(body.features).toContain('conversation_only')
  })
})

describe('chat invoke privacy tier (Venice tiering)', () => {
  function jsonResponse(data: unknown): Response {
    return new Response(JSON.stringify(data), {
      headers: { 'Content-Type': 'application/json' },
    })
  }

  // Opt-in ONLY: a default send must stay byte-identical to the pre-tier
  // behavior — unspecified means no constraint server-side.
  it('omits min_privacy_tier unless explicitly selected', () => {
    const body = buildChatWireBody({ content: 'hi' })
    expect(body).not.toHaveProperty('min_privacy_tier')
  })

  it('emits min_privacy_tier snake_case when a tier is selected', () => {
    const body = buildChatWireBody({ content: 'hi', minPrivacyTier: 'sovereign' })
    expect(body.min_privacy_tier).toBe(4)
  })

  it('never emits min_privacy_tier for unspecified, even if passed', () => {
    const body = buildChatWireBody({ content: 'hi', minPrivacyTier: 'unspecified' })
    expect(body).not.toHaveProperty('min_privacy_tier')
  })

  it('discloses per-model privacy_tier/residency from /v1/models and degrades unknown values to neutral', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ models: [
        { id: 'm-eu', name: 'EU Model', privacy_tier: 'eu_resident', residency: 'eu-central-1' },
        { id: 'm-sov', name: 'Sovereign Model', privacyTier: 'sovereign' },
        { id: 'm-garbage', name: 'Garbage Model', privacy_tier: 'fort_knox' },
        { id: 'm-old', name: 'Old Shape Model' },
      ] }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const models = await listModels()

    expect(models).toEqual(expect.arrayContaining([
      expect.objectContaining({ id: 'm-eu', privacyTier: 'eu_resident', residency: 'eu-central-1' }),
      expect.objectContaining({ id: 'm-sov', privacyTier: 'sovereign' }),
    ]))
    // Unknown/garbage values must NOT surface as any claim at all.
    expect(models.find((model) => model.id === 'm-garbage')?.privacyTier).toBeUndefined()
    expect(models.find((model) => model.id === 'm-old')?.privacyTier).toBeUndefined()
  })
})

describe('chat-client server thread history', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('lists server-backed chat thread sessions', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(
      new Response(JSON.stringify({
        data: {
          sessions: [
            {
              threadId: 'thread_1',
              title: 'Server saved chat',
              preview: 'Last answer',
              updatedAt: '2026-06-17T10:00:00.000Z',
              spaceRef: 'space_1',
              latest_run_id: 'run_1',
              latest_run_status: 'awaiting_approval',
              latest_run_updated_at: '2026-06-17T10:01:00.000Z',
            },
          ],
        },
      }), { headers: { 'Content-Type': 'application/json' }, status: 200 }),
    ))

    await expect(listChatThreads()).resolves.toEqual([
      {
        threadId: 'thread_1',
        title: 'Server saved chat',
        preview: 'Last answer',
        updatedAt: '2026-06-17T10:00:00.000Z',
        // Absent from the payload -> false. A gateway index written before pins
        // existed must decode as unpinned, not fail the whole listing.
        pinned: false,
        // Absent from the payload -> true, the mirror-image default: a
        // thread saved before persona existed has always had it on.
        personaEnabled: true,
        latestRunId: 'run_1',
        latestRunStatus: 'awaiting_approval',
        latestRunUpdatedAt: '2026-06-17T10:01:00.000Z',
        spaceRef: 'space_1',
      },
    ])
  })

  it('reads a server-owned pin, and treats anything non-true as unpinned', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({
          sessions: [
            { threadId: 't1', title: 'pinned', preview: '', updatedAt: '2026-06-17T10:00:00.000Z', pinned: true },
            { threadId: 't2', title: 'truthy string', preview: '', updatedAt: '2026-06-17T10:00:00.000Z', pinned: 'yes' },
            { threadId: 't3', title: 'absent', preview: '', updatedAt: '2026-06-17T10:00:00.000Z' },
          ],
        }),
        { headers: { 'Content-Type': 'application/json' }, status: 200 },
      ),
    )
    vi.stubGlobal('fetch', fetchMock)

    const sessions = await listChatThreads()

    // Strict `=== true`: a truthy non-boolean must not be read as a pin.
    expect(sessions.map((session) => session.pinned)).toEqual([true, false, false])
  })

  /**
   * The retention verdict is applied in the CLIENT, not at each call site:
   * `listChatThreads` has three independent callers, and a policy any one of
   * them can forget to apply is the same defect class as the in-memory ZDR gate
   * it replaces. Assert the wiring, not just the module.
   */
  it('applies the server ZDR posture carried on a threads listing', async () => {
    __resetRetentionForTests()
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(
      new Response(JSON.stringify({
        data: { sessions: [], retention: { zdr: true } },
      }), { headers: { 'Content-Type': 'application/json' }, status: 200 }),
    ))

    expect(isLocalRetentionAllowed()).toBe(true)
    await listChatThreads()
    expect(isLocalRetentionAllowed()).toBe(false)
    __resetRetentionForTests()
  })

  /** A save the server refused to retain must not leave a device-local copy. */
  it('drops the local copy when a save comes back unretained', async () => {
    __resetRetentionForTests()
    upsertChatThreadHistory({
      threadId: 'thread_zdr',
      title: 'Temporary',
      preview: 'secret',
      updatedAt: '2026-06-17T10:00:00.000Z',
    })
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(
      new Response(JSON.stringify({
        data: {
          session: { threadId: 'thread_zdr', title: 'Verevon Chat', preview: '', updatedAt: '2026-06-17T10:00:00.000Z' },
          retained: false,
        },
      }), { headers: { 'Content-Type': 'application/json' }, status: 200 }),
    ))

    const saved = await saveChatThreadSnapshot('thread_zdr', {
      title: 'Temporary',
      preview: 'secret',
      updatedAt: '2026-06-17T10:00:00.000Z',
      turns: [{ id: 'user_1', role: 'user', content: 'secret' }],
      taskSteps: [],
    })

    expect(saved).toBeNull()
    expect(readChatThreadHistory().map((item) => item.threadId)).not.toContain('thread_zdr')
  })

  it('saves a transcript snapshot to the thread endpoint', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({
        data: {
          session: {
            threadId: 'thread_1',
            title: 'Question',
            preview: 'Answer',
            updatedAt: '2026-06-17T10:00:00.000Z',
          },
        },
      }), { headers: { 'Content-Type': 'application/json' }, status: 200 }),
    )
    vi.stubGlobal('fetch', fetchMock)

    await saveChatThreadSnapshot('thread_1', {
      title: 'Question',
      preview: 'Answer',
      updatedAt: '2026-06-17T10:00:00.000Z',
      turns: [{ id: 'user_1', role: 'user', content: 'Question' }],
      taskSteps: [],
    })

    expect(fetchMock).toHaveBeenCalledWith('/api/v1/chat/threads/thread_1', expect.objectContaining({
      method: 'PUT',
      credentials: 'include',
    }))
    const firstCall = fetchMock.mock.calls[0]
    expect(firstCall).toBeTruthy()
    const init = firstCall?.[1]
    expect(JSON.parse(String(init?.body))).toMatchObject({
      title: 'Question',
      turns: [{ id: 'user_1', role: 'user', content: 'Question' }],
    })
  })

  /**
   * Persona spec: `personaEnabled` on `SaveChatThreadSnapshotRequest` is
   * optional so an ordinary snapshot save (title/preview/turns on every
   * turn) never silently touches the per-thread toggle — same
   * "omit = leave unchanged" contract `pinned` already has.
   */
  it('omits personaEnabled from the wire body when the caller does not set it', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({
        data: {
          session: {
            threadId: 'thread_1',
            title: 'Question',
            preview: 'Answer',
            updatedAt: '2026-06-17T10:00:00.000Z',
          },
        },
      }), { headers: { 'Content-Type': 'application/json' }, status: 200 }),
    )
    vi.stubGlobal('fetch', fetchMock)

    await saveChatThreadSnapshot('thread_1', {
      title: 'Question',
      preview: 'Answer',
      updatedAt: '2026-06-17T10:00:00.000Z',
    })

    const init = fetchMock.mock.calls[0]?.[1]
    expect('personaEnabled' in JSON.parse(String(init?.body))).toBe(false)
  })

  it('round-trips an explicit personaEnabled through the snapshot save', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({
        data: {
          session: {
            threadId: 'thread_1',
            title: 'Question',
            preview: 'Answer',
            updatedAt: '2026-06-17T10:00:00.000Z',
            personaEnabled: false,
          },
        },
      }), { headers: { 'Content-Type': 'application/json' }, status: 200 }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const saved = await saveChatThreadSnapshot('thread_1', {
      title: 'Question',
      preview: 'Answer',
      updatedAt: '2026-06-17T10:00:00.000Z',
      personaEnabled: false,
    })

    const init = fetchMock.mock.calls[0]?.[1]
    expect(JSON.parse(String(init?.body))).toMatchObject({ personaEnabled: false })
    expect(saved?.personaEnabled).toBe(false)
  })

  it('loads a rich server transcript when one exists', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(
      new Response(JSON.stringify({
        data: {
          transcript: {
            threadId: 'thread_1',
            updatedAt: '2026-06-17T10:00:00.000Z',
            turns: [{ id: 'asst_1', role: 'assistant', content: 'Answer' }],
            taskSteps: [{ id: 'step_1', title: 'Search', detail: 'Done', status: 'done' }],
          },
        },
      }), { headers: { 'Content-Type': 'application/json' }, status: 200 }),
    ))

    await expect(getChatThreadTranscript('thread_1')).resolves.toMatchObject({
      threadId: 'thread_1',
      turns: [{ id: 'asst_1', role: 'assistant', content: 'Answer' }],
      taskSteps: [{ id: 'step_1', title: 'Search' }],
    })
  })
})

describe('chat feedback', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  function jsonResponse(body: unknown, status: number): Response {
    return new Response(JSON.stringify(body), {
      status,
      headers: { 'Content-Type': 'application/json' },
    })
  }

  it('posts the thumb polarity and never asserts a skill id', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ accepted: true }, 200))
    vi.stubGlobal('fetch', fetchMock)

    await submitFeedback('req_1', 'positive', { note: 'nyttig', runId: 'run_1' })

    expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/chat/feedback',
      expect.objectContaining({ method: 'POST' }),
    )
    const body = JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))
    expect(body).toMatchObject({ requestId: 'req_1', rating: 'positive', note: 'nyttig', run_id: 'run_1' })
    // Which skills steered the answer is resolved server-side; a client-asserted
    // skill id would let a rating be aimed at any skill in the org.
    expect(body.skill_id).toBeUndefined()
    expect(body.skillId).toBeUndefined()
  })

  it('rejects rather than resolving when the rating was not recorded', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(
      jsonResponse({ error: { code: 'turn_not_rateable', message: 'gone' } }, 409),
    ))

    // A resolved promise here is the reported defect: the UI lights the thumb
    // while nothing persisted.
    await expect(submitFeedback('req_1', 'negative')).rejects.toBeInstanceOf(ApiError)
  })

  it('describes each failure so the surface can show it quietly', () => {
    expect(describeFeedbackFailure(new ApiError('gone', 409, 'turn_not_rateable')))
      .toContain('ikke vurderes lenger')
    expect(describeFeedbackFailure(new ApiError('denied', 403, null)))
      .toContain('ikke tilgang')
    expect(describeFeedbackFailure(new ApiError('zdr', 412, null)))
      .toContain('Zero Data Retention')
    // Anything unrecognised still says the rating did not stick.
    expect(describeFeedbackFailure(new Error('boom'))).toContain(
      'Kunne ikke lagre',
    )
  })
})

describe('extended-thinking effort', () => {
  it('omits the field entirely on an ordinary turn', () => {
    for (const effort of [undefined, 'standard' as const]) {
      const body = buildChatWireBody({
        content: 'hei',
        threadId: 't1',
        effort,
      })
      expect('effort' in body).toBe(false)
    }
  })

  it('sends the profile when a non-default effort is chosen', () => {
    for (const effort of ['quick', 'deep'] as const) {
      const body = buildChatWireBody({
        content: 'hei',
        threadId: 't1',
        effort,
      })
      expect(body.effort).toBe(effort)
    }
  })

  it('keeps the reasoning family so the deltas can actually arrive', () => {
    // Requesting thinking while dropping the `reasoning` feature would spend the
    // tokens and discard the output.
    const body = buildChatWireBody({
      content: 'hei',
      threadId: 't1',
      effort: 'deep',
    })
    expect(body.features).toContain('reasoning')
  })
})

describe('resume cursor tracking', () => {
  it('reports every frame id so a reconnect can resume incrementally', () => {
    const seen: string[] = []
    const handlers: ChatStreamHandlers = {
      onFrameId: (id) => seen.push(id),
      onMessage: () => {},
    }
    // Simulate what readSseStream hands the dispatcher, including a frame with
    // no id (rich events used to carry none at all).
    for (const event of [
      { event: 'chunk', data: '{"delta":"a"}', id: '0' },
      { event: 'usage', data: '{"input_tokens":1}', id: '1' },
      { event: 'chunk', data: '{"delta":"b"}' },
      { event: 'chunk', data: '{"delta":"c"}', id: '3' },
    ]) {
      if (event.id) handlers.onFrameId?.(event.id)
    }
    expect(seen).toEqual(['0', '1', '3'])
    // The LAST reported id is the resume cursor; an idless frame must not reset it.
    expect(seen.at(-1)).toBe('3')
  })
})

describe('getThreadContext', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('normalizes the itemized window and reports the assembler total', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(
            JSON.stringify({
              thread_id: 't1',
              estimated_tokens: 1234,
              budget_tokens: 8000,
              segments: [
                {
                  kind: 'system',
                  content: 'You are Verevon',
                  estimated_tokens: 40,
                },
                {
                  kind: 'grounding',
                  content: 'doc excerpt',
                  estimated_tokens: 900,
                },
              ],
            }),
            { status: 200, headers: { 'content-type': 'application/json' } },
          ),
      ),
    )
    const context = await getThreadContext('t1')
    expect(context.threadId).toBe('t1')
    // The assembler's own total, not a sum of the segments: if the two disagree
    // the inspector must show what the assembler believes.
    expect(context.estimatedTokens).toBe(1234)
    expect(context.budgetTokens).toBe(8000)
    expect(context.segments.map((segment) => segment.kind)).toEqual([
      'system',
      'grounding',
    ])
  })

  it('drops a segment with no kind rather than showing an unlabelled prompt block', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(
            JSON.stringify({
              thread_id: 't1',
              segments: [
                { content: 'mystery text', estimated_tokens: 10 },
                { kind: 'system' },
              ],
            }),
            { status: 200, headers: { 'content-type': 'application/json' } },
          ),
      ),
    )
    const context = await getThreadContext('t1')
    expect(context.segments).toHaveLength(1)
    expect(context.segments[0]?.kind).toBe('system')
    // Missing numbers become 0, never NaN — an inspector rendering NaN tokens
    // looks broken rather than empty.
    expect(context.segments[0]?.estimatedTokens).toBe(0)
    expect(context.estimatedTokens).toBe(0)
  })

  it('scopes to a run when one is given', async () => {
    // Collect the requested URLs as they arrive, rather than reaching into the
    // mock's call tuple — which needs a typed parameter the linter then reports
    // as unused.
    const urls: string[] = []
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL) => {
        urls.push(String(input))
        return new Response(JSON.stringify({ thread_id: 't1', segments: [] }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        })
      }),
    )
    await getThreadContext('t1', 'run-9')
    expect(urls[0]).toContain('run_id=run-9')
    // A blank run id must not send an empty parameter.
    await getThreadContext('t1', '   ')
    expect(urls[1]).not.toContain('run_id')
  })
})

describe('recalled-memory provenance', () => {
  const frame = (memories: unknown) =>
    `event: memory_recall\ndata: ${JSON.stringify({ count: 1, latency_ms: 5, memories })}\n\n`

  it('carries the label, preview and both provenance axes', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        sseResponse([
          frame([
            {
              memory_id: 'mem-1',
              role: 'recall',
              origin: 'stated',
              label: 'USER',
              preview: 'Prefers metric units',
            },
          ]),
        ]),
      ),
    )
    const onMemoryRecall = vi.fn()
    await streamChat({ content: 'hi' }, { onMemoryRecall })
    expect(onMemoryRecall.mock.calls[0]?.[0].memories).toEqual([
      {
        memoryId: 'mem-1',
        role: 'recall',
        origin: 'stated',
        label: 'USER',
        preview: 'Prefers metric units',
      },
    ])
  })

  /**
   * The honesty rule at the client boundary: an origin this build does not
   * recognise must read as `unrecorded`, never as `stated`. Presenting an
   * unknown value as "you told me this" manufactures consent the record does
   * not support.
   */
  it('degrades an unknown origin to unrecorded, never to stated', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        sseResponse([
          frame([
            {
              memory_id: 'm1',
              role: 'recall',
              origin: 'some_future_value',
              label: 'X',
              preview: 'p',
            },
            { memory_id: 'm2', role: 'recall', label: 'X', preview: 'p' },
          ]),
        ]),
      ),
    )
    const onMemoryRecall = vi.fn()
    await streamChat({ content: 'hi' }, { onMemoryRecall })
    const origins = onMemoryRecall.mock.calls[0]?.[0].memories.map(
      (memory: { origin: string }) => memory.origin,
    )
    expect(origins).toEqual(['unrecorded', 'unrecorded'])
  })

  it('drops a row with no id or nothing readable, and keeps the rest', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        sseResponse([
          frame([
            { role: 'recall', origin: 'stated', label: 'X', preview: 'no id' },
            { memory_id: 'm2', origin: 'stated', label: 'X', preview: '   ' },
            {
              memory_id: 'm3',
              origin: 'inferred',
              label: 'X',
              preview: 'keeps',
            },
          ]),
        ]),
      ),
    )
    const onMemoryRecall = vi.fn()
    await streamChat({ content: 'hi' }, { onMemoryRecall })
    const memories = onMemoryRecall.mock.calls[0]?.[0].memories
    expect(memories).toHaveLength(1)
    expect(memories[0].memoryId).toBe('m3')
  })

  it('tolerates a malformed memories field without losing the count', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(sseResponse([frame('not-an-array')])),
    )
    const onMemoryRecall = vi.fn()
    await streamChat({ content: 'hi' }, { onMemoryRecall })
    // The count is the signal; a broken list must not suppress it.
    expect(onMemoryRecall.mock.calls[0]?.[0]).toMatchObject({
      count: 1,
      memories: [],
    })
  })
})

describe('buildChatWireBody — explicit skills (the composer `/` picker)', () => {
  it('sends picked skills as skill_ids, and never as tool specs', () => {
    const body = buildChatWireBody({
      content: 'hi',
      actions: [
        { id: 'sk-1', name: 'Innkjøpsrutine', kind: 'skill' },
        { id: 'cap-1', name: 'Some capability', kind: 'capability' },
      ],
    })
    expect(body.skill_ids).toEqual(['sk-1'])
    const tools = body.tools as Array<{ name: string }>
    expect(tools.some((tool) => tool.name === 'sk-1')).toBe(false)
  })

  it('trims, de-duplicates and caps skill ids at four', () => {
    const body = buildChatWireBody({
      content: 'hi',
      actions: ['a', ' a ', 'b', 'c', 'd', 'e'].map((id) => ({ id, name: id, kind: 'skill' as const })),
    })
    expect(body.skill_ids).toEqual(['a', 'b', 'c', 'd'])
  })

  it('omits the field entirely when no skill was picked', () => {
    expect('skill_ids' in buildChatWireBody({ content: 'hi' })).toBe(false)
    expect(
      'skill_ids' in buildChatWireBody({ content: 'hi', actions: [{ id: 'cap-1', name: 'Cap', kind: 'capability' }] }),
    ).toBe(false)
  })
})

describe('Q15 stream refusals are not connection loss', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  const refusal = (status: number) => vi.fn().mockResolvedValue(new Response(
    JSON.stringify({ error: { code: status === 401 ? 'unauthorized' : 'forbidden', message: 'no' } }),
    { status, headers: { 'Content-Type': 'application/json' } },
  ))

  it.each([
    [401, 'unauthorized'],
    [403, 'forbidden'],
  ])('reports a %i on the live stream as %s and asks the user to sign in again', async (status, code) => {
    vi.stubGlobal('fetch', refusal(status))
    const onError = vi.fn()
    await streamChat({ content: 'hi' }, { onError })
    expect(onError).toHaveBeenCalledTimes(1)
    expect(onError.mock.calls[0]?.[0]).toMatchObject({ code })
    expect(onError.mock.calls[0]?.[0].message).toContain('Logg inn på nytt')
  })

  it.each([
    [401, 'unauthorized'],
    [403, 'forbidden'],
  ])('reports a %i on resume as %s, not as a lost connection', async (status, code) => {
    vi.stubGlobal('fetch', refusal(status))
    const onError = vi.fn()
    await resumeStream('req-1', { onError })
    expect(onError.mock.calls[0]?.[0]).toMatchObject({ code })
  })

  it('keeps an expired resume buffer (404) and a network failure as connection loss', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(
      JSON.stringify({ error: { code: 'not_found', message: 'stream not resumable' } }),
      { status: 404 },
    )))
    const expired = vi.fn()
    await resumeStream('req-1', { onError: expired })
    expect(expired).toHaveBeenCalledWith({ code: 'connection_error', message: 'stream not resumable' })

    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('Failed to fetch')))
    const offline = vi.fn()
    await streamChat({ content: 'hi' }, { onError: offline })
    expect(offline).toHaveBeenCalledWith({ code: 'connection_error', message: 'Failed to fetch' })
  })
})

describe('turn_timing, cost_basis and result_receipt', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  const frame = (event: string, data: unknown) => `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`

  const timingPayload = {
    request_id: 'req-1',
    outcome: 'completed',
    server_ms: 43_440,
    provider_ms: 43_220,
    orchestration_ms: 220,
    marks: { connected_ms: 88, first_draft_ms: 20_010, durable_ms: 43_300, future_mark_ms: 1 },
    counts: { model_rounds: 1, provider_calls: 4, review_passes: 1, review_batches: 3, repairs: 0, reviewer_edits: 0 },
    tokens_reported: false,
    phases: [
      {
        phase: 'generation', start_ms: 90, end_ms: 20_010, round: 1, tools: ['create_artifact'],
        provider: { handler_ms: 19_930, provider_ms: 19_920, attempts: 1, response_cache_hit: false, upstream_ms: { broker_ready: 40 } },
      },
      { phase: 'review_batch', start_ms: 20_020, end_ms: 43_240, pass: 1, batch: 2, outcome: 'accepted', unknown_field: 'kept out' },
      { phase: '', start_ms: 1, end_ms: 2 },
    ],
    phases_dropped: 0,
    brand_new_field: { anything: true },
  }

  it('parses turn_timing leniently onto camelCase, ignoring unknown fields and malformed phases', () => {
    const timing = parseTurnTiming(timingPayload)
    expect(timing).toMatchObject({
      requestId: 'req-1',
      outcome: 'completed',
      serverMs: 43_440,
      providerMs: 43_220,
      orchestrationMs: 220,
      marks: { connectedMs: 88, firstDraftMs: 20_010, durableMs: 43_300 },
      counts: { modelRounds: 1, providerCalls: 4, reviewPasses: 1, reviewBatches: 3, repairs: 0, reviewerEdits: 0 },
      tokensReported: false,
      phasesDropped: 0,
    })
    expect(timing?.phases).toEqual([
      {
        phase: 'generation', startMs: 90, endMs: 20_010, round: 1, tools: ['create_artifact'],
        provider: { handlerMs: 19_930, providerMs: 19_920, attempts: 1, responseCacheHit: false, upstreamMs: { broker_ready: 40 } },
      },
      { phase: 'review_batch', startMs: 20_020, endMs: 43_240, pass: 1, batch: 2, outcome: 'accepted' },
    ])
    expect(JSON.stringify(timing)).not.toContain('kept out')
  })

  it('rejects a payload without its totals, derives a missing orchestration time, and caps phases at 64', () => {
    expect(parseTurnTiming({ outcome: 'completed' })).toBeNull()
    expect(parseTurnTiming({ server_ms: 100, provider_ms: -1 })).toBeNull()
    const many = parseTurnTiming({
      server_ms: 1_000,
      provider_ms: 1_200,
      phases: Array.from({ length: 70 }, (_, index) => ({ phase: 'dispatch', start_ms: index, end_ms: index + 1 })),
      phases_dropped: 3,
      outcome: 'mystery',
    })
    expect(many?.orchestrationMs).toBe(0)
    expect(many?.outcome).toBeUndefined()
    expect(many?.phases).toHaveLength(64)
    expect(many?.phasesDropped).toBe(9)
  })

  it('routes turn_timing, cost_basis and result_receipt to their handlers, not to onUnknownEvent', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      frame('usage', { input_tokens: 10, output_tokens: 5, cost_usd: 0, latency_ms: 900, cost_basis: 'subscription' }),
      frame('usage', { cost_usd: 0.002, cost_basis: 'something-else' }),
      frame('turn_timing', timingPayload),
      frame('turn_timing', { request_id: 'req-1' }),
      frame('result_receipt', { receipt: { scope: 'direct_answer_word_range', words: 110 }, request_id: 'req-1' }),
      frame('done', {}),
    ])))
    const onUsage = vi.fn()
    const onTurnTiming = vi.fn()
    const onResultReceipt = vi.fn()
    const onUnknownEvent = vi.fn()
    await streamChat({ content: 'hi' }, { onUsage, onTurnTiming, onResultReceipt, onUnknownEvent })
    expect(onUsage.mock.calls[0]?.[0]).toMatchObject({ costUsd: 0, costBasis: 'subscription', latencyMs: 900 })
    expect(onUsage.mock.calls[1]?.[0].costBasis).toBeUndefined()
    // The payload without totals is dropped, not delivered as zeros.
    expect(onTurnTiming).toHaveBeenCalledTimes(1)
    expect(onTurnTiming.mock.calls[0]?.[0]).toMatchObject({ providerMs: 43_220, orchestrationMs: 220 })
    expect(onResultReceipt).toHaveBeenCalledWith({ receipt: { scope: 'direct_answer_word_range', words: 110 }, requestId: 'req-1' })
    expect(onUnknownEvent).not.toHaveBeenCalled()
  })

  it('tolerates a stream that never sends turn_timing', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      frame('usage', { cost_usd: 0.0031 }),
      frame('done', {}),
    ])))
    const onUsage = vi.fn()
    const onTurnTiming = vi.fn()
    const onDone = vi.fn()
    await streamChat({ content: 'hi' }, { onUsage, onTurnTiming, onDone })
    expect(onUsage.mock.calls[0]?.[0]).toMatchObject({ costUsd: 0.0031, costBasis: undefined })
    expect(onTurnTiming).not.toHaveBeenCalled()
    expect(onDone).toHaveBeenCalledTimes(1)
  })
})
