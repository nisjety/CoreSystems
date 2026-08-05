import { afterEach, describe, expect, it, vi } from 'vitest'
import { streamRunEvents } from './run-console-client'
import { listPlans, listTodos } from './orchestration-client'

/** A single SSE frame: `event:`/`data:` lines terminated by a blank line. */
function sseFrame(event: string, data: unknown): string {
  return `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`
}

/** A `Response` whose body streams the given SSE text, like the gateway does. */
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

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ data }), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('streamRunEvents dispatch', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('streams from the run events endpoint and maps event names to handlers', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        sseFrame('run_paused_for_approval', {
          run_id: 'run_1',
          approval_id: 'appr_1',
          at: '2026-06-17T10:00:00Z',
        }),
        sseFrame('browser_observation_received', {
          run_id: 'run_1',
          plan_id: 'plan_1',
          action_id: 'act_1',
          status: 'ok',
          page_url: 'https://example.com',
          page_title: 'Example',
          at: '2026-06-17T10:00:01Z',
        }),
        sseFrame('step_update', {
          step_id: 'step_1',
          name: 'Searching',
          message: 'Looking things up',
          status: 'running',
        }),
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onRunPaused = vi.fn()
    const onBrowserObservation = vi.fn()
    const onStep = vi.fn()
    const onDone = vi.fn()

    await streamRunEvents('run 1', { onRunPaused, onBrowserObservation, onStep, onDone })

    // GET against the encoded run-events path.
    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/runs/run%201/events')
    expect(init.method).toBe('GET')

    expect(onRunPaused).toHaveBeenCalledWith({
      runId: 'run_1',
      approvalId: 'appr_1',
      at: '2026-06-17T10:00:00Z',
    })
    expect(onBrowserObservation).toHaveBeenCalledWith({
      runId: 'run_1',
      planId: 'plan_1',
      actionId: 'act_1',
      status: 'ok',
      pageUrl: 'https://example.com',
      pageTitle: 'Example',
      at: '2026-06-17T10:00:01Z',
    })
    // step_update reads from the snake_case fallbacks (step_id/name/message).
    expect(onStep).toHaveBeenCalledWith({
      id: 'step_1',
      title: 'Searching',
      detail: 'Looking things up',
      status: 'running',
    })
    expect(onDone).toHaveBeenCalledTimes(1)
  })

  it('dispatches browser_action_dispatched with reason and browser_run_paused/resumed (Phase 2)', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        sseFrame('browser_action_dispatched', {
          run_id: 'run_1',
          plan_id: 'plan_1',
          action_id: 'act_1',
          action_type: 'goto',
          url: 'https://example.com',
          reason: 'navigating to the landing page',
        }),
        sseFrame('browser_observation_received', {
          run_id: 'run_1',
          plan_id: 'plan_1',
          action_id: 'act_1',
          status: 'success',
          screenshot_ref: 'art_shot_1',
          dom_snapshot_ref: 'art_dom_1',
        }),
        sseFrame('browser_run_paused', { run_id: 'run_1', plan_id: 'plan_1' }),
        sseFrame('browser_run_resumed', { run_id: 'run_1', plan_id: 'plan_1' }),
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onBrowserAction = vi.fn()
    const onBrowserObservation = vi.fn()
    const onBrowserRunPaused = vi.fn()
    const onBrowserRunResumed = vi.fn()

    await streamRunEvents('run_1', {
      onBrowserAction,
      onBrowserObservation,
      onBrowserRunPaused,
      onBrowserRunResumed,
    })

    expect(onBrowserAction).toHaveBeenCalledWith(
      expect.objectContaining({ actionType: 'goto', reason: 'navigating to the landing page' }),
    )
    expect(onBrowserObservation).toHaveBeenCalledWith(
      expect.objectContaining({ screenshotRef: 'art_shot_1', domSnapshotRef: 'art_dom_1' }),
    )
    expect(onBrowserRunPaused).toHaveBeenCalledWith({ runId: 'run_1', planId: 'plan_1', at: undefined })
    expect(onBrowserRunResumed).toHaveBeenCalledWith({ runId: 'run_1', planId: 'plan_1', at: undefined })
  })

  it('dispatches browser_action_approval_required and browser_action_decided (Phase 5)', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        sseFrame('browser_action_approval_required', {
          run_id: 'run_1',
          plan_id: 'plan_1',
          action_id: 'act_1',
          action_type: 'click',
          url: 'https://shop.example.com/checkout',
          selector: '#pay-now',
          reason: 'clicking a checkout/payment control',
          risk_category: 'checkout',
          approval_id: '',
        }),
        sseFrame('browser_action_decided', {
          run_id: 'run_1',
          plan_id: 'plan_1',
          action_id: 'act_1',
          approval_id: 'appr_1',
          decision: 'granted',
          decided_by: 'user_1',
        }),
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onBrowserActionApprovalRequired = vi.fn()
    const onBrowserActionDecided = vi.fn()

    await streamRunEvents('run_1', { onBrowserActionApprovalRequired, onBrowserActionDecided })

    expect(onBrowserActionApprovalRequired).toHaveBeenCalledWith(
      expect.objectContaining({
        actionType: 'click',
        url: 'https://shop.example.com/checkout',
        selector: '#pay-now',
        riskCategory: 'checkout',
      }),
    )
    expect(onBrowserActionDecided).toHaveBeenCalledWith(
      expect.objectContaining({ decision: 'granted', decidedBy: 'user_1', approvalId: 'appr_1' }),
    )
  })

  it('dispatches approval_continuation_verified (Verified Outcome Foundation, verevon-roadmap.md §3b)', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        sseFrame('approval_continuation_verified', {
          run_id: 'run_1',
          delivery_id: 'delivery_1',
          approval_id: 'appr_1',
          receipt_id: 'receipt_1',
          verification_status: 'verified_success',
          verification_method: 'structural',
          verification_reason: 'provider returned authoritative receipt id booking-1',
          at: '2026-06-17T10:00:02Z',
        }),
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onApprovalContinuationVerified = vi.fn()
    await streamRunEvents('run_1', { onApprovalContinuationVerified })

    expect(onApprovalContinuationVerified).toHaveBeenCalledWith({
      runId: 'run_1',
      deliveryId: 'delivery_1',
      approvalId: 'appr_1',
      receiptId: 'receipt_1',
      verificationStatus: 'verified_success',
      verificationMethod: 'structural',
      verificationReason: 'provider returned authoritative receipt id booking-1',
      at: '2026-06-17T10:00:02Z',
    })
  })

  it('accepts camelCase payloads via the defensive fallbacks', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([sseFrame('approval_state_changed', { approvalId: 'appr_2', runId: 'run_2', to: 'GRANTED' })]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onApproval = vi.fn()
    await streamRunEvents('run_2', { onApproval })

    expect(onApproval).toHaveBeenCalledWith(
      expect.objectContaining({ approvalId: 'appr_2', runId: 'run_2', to: 'GRANTED' }),
    )
  })

  it('surfaces a connection error to onError', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ error: { code: 'unauthorized', message: 'nope' } }), {
        status: 401,
        headers: { 'Content-Type': 'application/json' },
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onError = vi.fn()
    await streamRunEvents('run_3', { onError })

    expect(onError).toHaveBeenCalledTimes(1)
    const err = onError.mock.calls[0]![0] as Error
    expect(err.message).toBe('nope')
  })
})

describe('orchestration run console reads', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('lists plans, normalizing snake_case + dropping entries without an id', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        plans: [
          { plan_id: 'plan_1', run_id: 'run_1', status: 'ACTIVE', summary: 'Do the thing' },
          { run_id: 'run_1' }, // no id → dropped
        ],
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await listPlans('run_1')

    const [path] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/orchestration/runs/run_1/plans')
    expect(result).toEqual([{ id: 'plan_1', runId: 'run_1', state: 'ACTIVE', summary: 'Do the thing' }])
  })

  it('lists todos, normalizing camelCase fields', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ todos: [{ id: 'todo_1', threadId: 'thread_1', state: 'PENDING', title: 'Step one' }] }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await listTodos('thread_1')

    const [path] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/orchestration/threads/thread_1/todos')
    expect(result).toEqual([{ id: 'todo_1', threadId: 'thread_1', state: 'PENDING', title: 'Step one' }])
  })
})
