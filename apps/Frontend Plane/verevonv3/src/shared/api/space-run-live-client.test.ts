import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  parseRoomRunFrame,
  spaceRunLivePath,
  streamSpaceRunLive,
  type RoomRunFrame,
  type SpaceRunLiveHandlers,
} from './space-run-live-client'

// WS09-S5: the member live view of an agent run. The frames come from the golden that Model Plane's
// projector is built against, so every golden frame must read to the typed frame written here.

const golden = JSON.parse(
  readFileSync(join(__dirname, 'contracts', 'space-room-run-frames.v1.json'), 'utf8'),
) as { frames: Record<string, Record<string, unknown>> }

// Each golden frame name, the SSE event it travels as, and the typed frame it must read to.
const GOLDEN_FRAMES: Record<string, { event: string; frame: RoomRunFrame }> = {
  snapshot: {
    event: 'snapshot',
    frame: {
      type: 'snapshot',
      snapshot: {
        runId: 'run_01',
        threadId: 'thr_01',
        startedBy: 'usr_01',
        agentName: 'Driftsassistent',
        phase: 'running',
        startedAt: 1790000000,
        text: { offset: 0, value: 'Blåbær 😀', truncated: false, visible: true },
        steps: [{ id: 'call_1', class: 'web', state: 'ok' }],
        steer: 1,
        plan: { planId: 'plan_1', steps: 3 },
        artifacts: [{ id: 'art_1', kind: 'document' }],
        expiresAt: 1790000120,
      },
    },
  },
  snapshot_text_hidden: {
    event: 'snapshot',
    frame: {
      type: 'snapshot',
      snapshot: {
        runId: 'run_01',
        threadId: 'thr_01',
        startedBy: 'usr_01',
        agentName: '',
        phase: 'running',
        startedAt: 1790000000,
        text: { offset: 42, value: '', truncated: false, visible: false },
        steps: [],
        steer: 0,
        plan: null,
        artifacts: [],
        expiresAt: 1790000120,
      },
    },
  },
  text: { event: 'text', frame: { type: 'text', offset: 8, delta: ' og mer' } },
  text_progress: { event: 'text_progress', frame: { type: 'text_progress', chars: 42 } },
  step: {
    event: 'step',
    frame: { type: 'step', step: { id: 'call_1', class: 'search_knowledge', state: 'running' } },
  },
  step_finished: {
    event: 'step',
    frame: { type: 'step', step: { id: 'call_1', class: 'search_knowledge', state: 'ok', ms: 840 } },
  },
  steer: { event: 'steer', frame: { type: 'steer', count: 1 } },
  plan: { event: 'plan', frame: { type: 'plan', planId: 'plan_1', steps: 3 } },
  artifact: { event: 'artifact', frame: { type: 'artifact', id: 'art_1', artifactKind: 'document' } },
  finished_completed: {
    event: 'finished',
    frame: { type: 'finished', outcome: 'completed', reason: null },
  },
  finished_stopped: { event: 'finished', frame: { type: 'finished', outcome: 'stopped', reason: null } },
  finished_failed: {
    event: 'finished',
    frame: { type: 'finished', outcome: 'failed', reason: 'model_unavailable' },
  },
  reauth_required: { event: 'reauth_required', frame: { type: 'reauth_required' } },
  error: {
    event: 'error',
    frame: { type: 'error', code: 'run_stream_expired', retryable: false, feature: null },
  },
  error_zdr: {
    event: 'error',
    frame: { type: 'error', code: 'zdr_feature_not_supported', retryable: false, feature: 'agent_run' },
  },
}

describe('parseRoomRunFrame against the golden frames (WS09-S5)', () => {
  it('maps every golden frame to its typed frame, and names every golden frame', () => {
    expect(Object.keys(GOLDEN_FRAMES).sort()).toEqual(Object.keys(golden.frames).sort())
    for (const [name, expected] of Object.entries(GOLDEN_FRAMES)) {
      const data = JSON.stringify(golden.frames[name])
      expect(parseRoomRunFrame({ event: expected.event, data }), name).toEqual(expected.frame)
    }
  })
})

describe('parseRoomRunFrame drops what it does not know (WS09-S5)', () => {
  it('drops an event name it does not know, and a frame with no event name', () => {
    expect(parseRoomRunFrame({ event: 'reasoning_delta', data: '{"delta":"x"}' })).toBeUndefined()
    expect(parseRoomRunFrame({ data: '{}' })).toBeUndefined()
  })

  it('drops malformed JSON and a payload that is not an object', () => {
    expect(parseRoomRunFrame({ event: 'text', data: '{"offset":0,"delta":' })).toBeUndefined()
    expect(parseRoomRunFrame({ event: 'steer', data: '[1]' })).toBeUndefined()
    expect(parseRoomRunFrame({ event: 'steer', data: '"count"' })).toBeUndefined()
  })

  it('drops a step in an unknown state, and reads an unknown class as tool', () => {
    expect(
      parseRoomRunFrame({ event: 'step', data: JSON.stringify({ id: 'call_9', class: 'web', state: 'paused' }) }),
    ).toBeUndefined()
    expect(
      parseRoomRunFrame({ event: 'step', data: JSON.stringify({ id: 'call_9', class: 'teleport', state: 'ok' }) }),
    ).toEqual({ type: 'step', step: { id: 'call_9', class: 'tool', state: 'ok' } })
  })

  it('skips a step in an unknown state inside a snapshot and keeps the rest', () => {
    const data = JSON.stringify({
      ...golden.frames.snapshot,
      steps: [
        { id: 'call_a', class: 'web', state: 'paused' },
        { id: 'call_b', class: 'code', state: 'error' },
      ],
    })
    const frame = parseRoomRunFrame({ event: 'snapshot', data })
    expect(frame?.type === 'snapshot' ? frame.snapshot.steps : null).toEqual([{ id: 'call_b', class: 'code', state: 'error' }])
  })
})

describe('streamSpaceRunLive ends (WS09-S5)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  function handlers(): SpaceRunLiveHandlers & { onFrame: ReturnType<typeof vi.fn>; onEnd: ReturnType<typeof vi.fn> } {
    return { onFrame: vi.fn(), onEnd: vi.fn() }
  }

  it('reports a refused attach as failed with its status, code and feature, and sends no frame', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(
      JSON.stringify({
        error: {
          code: 'zdr_feature_not_supported',
          message: 'Not available with zero data retention.',
          feature: 'agent_run',
        },
      }),
      { status: 403, headers: { 'Content-Type': 'application/json' } },
    )))
    const live = handlers()

    await streamSpaceRunLive('space_1', 'run_1', live)

    expect(live.onFrame).not.toHaveBeenCalled()
    expect(live.onEnd).toHaveBeenCalledTimes(1)
    expect(live.onEnd).toHaveBeenCalledWith({
      kind: 'failed',
      status: 403,
      code: 'zdr_feature_not_supported',
      feature: 'agent_run',
    })
  })

  it('reports a stream the server closes as closed, and passes each frame with its SSE id', async () => {
    const encoder = new TextEncoder()
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(encoder.encode('id: 7\nevent: text\ndata: {"offset":0,"delta":"hi"}\n\n'))
        controller.enqueue(encoder.encode('event: steer\ndata: {"count":1}\n\n'))
        controller.close()
      },
    })
    const fetchMock = vi.fn(async () => new Response(body, {
      status: 200,
      headers: { 'Content-Type': 'text/event-stream' },
    }))
    vi.stubGlobal('fetch', fetchMock)
    const live = handlers()

    await streamSpaceRunLive('space 1', 'run/1', live)

    expect(String(fetchMock.mock.calls[0]?.[0])).toContain(spaceRunLivePath('space 1', 'run/1'))
    expect(spaceRunLivePath('space 1', 'run/1')).toBe('/api/v1/spaces/space%201/runs/run%2F1/live')
    expect(live.onFrame.mock.calls).toEqual([
      [{ type: 'text', offset: 0, delta: 'hi' }, '7'],
      [{ type: 'steer', count: 1 }, undefined],
    ])
    expect(live.onEnd).toHaveBeenCalledTimes(1)
    expect(live.onEnd).toHaveBeenCalledWith({ kind: 'closed' })
  })

  it('reports an attach the caller aborted as aborted, not as closed', async () => {
    const controller = new AbortController()
    controller.abort()
    vi.stubGlobal('fetch', vi.fn(async () => {
      throw new DOMException('The operation was aborted.', 'AbortError')
    }))
    const live = handlers()

    await streamSpaceRunLive('space_1', 'run_1', live, { signal: controller.signal })

    expect(live.onFrame).not.toHaveBeenCalled()
    expect(live.onEnd).toHaveBeenCalledTimes(1)
    expect(live.onEnd).toHaveBeenCalledWith({ kind: 'aborted' })
  })
})
