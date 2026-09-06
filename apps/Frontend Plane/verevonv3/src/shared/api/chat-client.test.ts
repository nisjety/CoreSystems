import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  buildChatWireBody,
  listModels,
  shouldRequestSupportContext,
  describeFeedbackFailure,
  getChatThreadTranscript,
  getThreadContext,
  listChatThreads,
  queueInvocationInput,
  saveChatThreadSnapshot,
  streamChat,
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

  it('maps batched citation events into individual citations', async () => {
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
    expect(body.min_privacy_tier).toBe('sovereign')
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
