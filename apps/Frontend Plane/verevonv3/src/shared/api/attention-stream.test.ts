import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  ATTENTION_STREAM_PATH,
  AttentionFrameGate,
  openAttentionStream,
  parseAttentionFrame,
  type AttentionFrame,
} from './attention-stream'

const counts = { needsYou: 0, working: 0, unseen: 0 }

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('parseAttentionFrame', () => {
  it('reads the snapshot in the snake_case names the stream carries, with its epoch and sequence', () => {
    const frame = parseAttentionFrame({
      event: 'attention_snapshot',
      id: 'a1b2-c3d4:1',
      data: JSON.stringify({
        items: [
          {
            thread_id: 'thread-1',
            state: 'needs_approval',
            reason: 'approval',
            since: '2026-10-31T08:00:00Z',
            unseen: true,
            run_id: 'run-1',
          },
        ],
        counts: { needs_you: 1, working: 0, unseen: 1 },
      }),
    })

    expect(frame).toEqual({
      kind: 'snapshot',
      epoch: 'a1b2-c3d4',
      seq: 1,
      items: [
        {
          threadId: 'thread-1',
          state: 'needs_approval',
          reason: 'approval',
          since: '2026-10-31T08:00:00Z',
          unseen: true,
          runId: 'run-1',
        },
      ],
      counts: { needsYou: 1, working: 0, unseen: 1 },
    })
  })

  it('reads a changed frame, and keeps the idle state that tells the browser a thread has left', () => {
    const frame = parseAttentionFrame({
      event: 'attention_changed',
      id: 'epoch:7',
      data: JSON.stringify({ item: { thread_id: 'thread-2', state: 'idle', unseen: false }, counts }),
    })

    expect(frame).toEqual({
      kind: 'changed',
      epoch: 'epoch',
      seq: 7,
      item: { threadId: 'thread-2', state: 'idle', unseen: false },
      counts,
    })
  })

  it('reads the resync and heartbeat frames, which carry no body', () => {
    expect(parseAttentionFrame({ event: 'attention_resync', id: 'e:2', data: '{}' })).toEqual({
      kind: 'resync',
      epoch: 'e',
      seq: 2,
    })
    expect(parseAttentionFrame({ event: 'attention_heartbeat', id: 'e:3', data: '{}' })).toEqual({
      kind: 'heartbeat',
      epoch: 'e',
      seq: 3,
    })
  })

  it('drops a frame it cannot read instead of guessing at it', () => {
    expect(
      parseAttentionFrame({ event: 'attention_changed', id: 'e:2', data: JSON.stringify({ item: { state: 'working' } }) }),
    ).toBeNull()
    expect(parseAttentionFrame({ event: 'attention_snapshot', id: 'e', data: '{}' })).toBeNull()
    expect(parseAttentionFrame({ event: 'attention_snapshot', id: 'e:0', data: '{}' })).toBeNull()
    expect(parseAttentionFrame({ event: 'attention_snapshot', id: undefined, data: '{}' })).toBeNull()
    expect(parseAttentionFrame({ event: 'chat_delta', id: 'e:1', data: '{}' })).toBeNull()
    expect(parseAttentionFrame({ event: 'attention_changed', id: 'e:3', data: 'not json' })).toBeNull()
  })

  it('drops an unknown state from an item and keeps the item itself', () => {
    const frame = parseAttentionFrame({
      event: 'attention_changed',
      id: 'e:4',
      data: JSON.stringify({ item: { thread_id: 'thread-3', state: 'paused_for_lunch' }, counts }),
    })

    expect(frame).toMatchObject({ kind: 'changed', item: { threadId: 'thread-3' } })
    expect(frame && 'item' in frame ? frame.item.state : 'missing').toBeUndefined()
  })
})

describe('AttentionFrameGate', () => {
  const snapshot = (seq: number): AttentionFrame => ({ kind: 'snapshot', epoch: 'e', seq, items: [], counts })
  const heartbeat = (epoch: string, seq: number): AttentionFrame => ({ kind: 'heartbeat', epoch, seq })

  it('applies a snapshot and then the frames that follow it in order', () => {
    const gate = new AttentionFrameGate()

    expect(gate.accept(snapshot(1))).toBe('apply')
    expect(gate.accept(heartbeat('e', 2))).toBe('apply')
    expect(gate.accept(heartbeat('e', 3))).toBe('apply')
  })

  it('drops a frame whose sequence number is not newer, and asks for a reconnect on a gap or a new epoch', () => {
    const gate = new AttentionFrameGate()
    gate.accept(snapshot(1))
    gate.accept(heartbeat('e', 2))

    expect(gate.accept(heartbeat('e', 2))).toBe('stale')
    expect(gate.accept(heartbeat('e', 5))).toBe('reconnect')
    expect(gate.accept(heartbeat('other-epoch', 3))).toBe('reconnect')
  })

  it('asks for a reconnect when a frame arrives before any snapshot', () => {
    expect(new AttentionFrameGate().accept(heartbeat('e', 1))).toBe('reconnect')
  })

  it('starts over from a snapshot of a new epoch', () => {
    const gate = new AttentionFrameGate()
    gate.accept(snapshot(4))

    expect(gate.accept({ kind: 'snapshot', epoch: 'restarted', seq: 1, items: [], counts })).toBe('apply')
    expect(gate.accept(heartbeat('restarted', 2))).toBe('apply')
  })
})

describe('openAttentionStream', () => {
  it('reads the stream route and hands each frame to the caller as it arrives', async () => {
    const body = 'event: attention_heartbeat\nid: e1:1\ndata: {}\n\n'
    const fetchSpy = vi.fn(async () => fakeSseResponse(body))
    vi.stubGlobal('fetch', fetchSpy)
    const frames: AttentionFrame[] = []

    await openAttentionStream({ signal: new AbortController().signal, onFrame: (frame) => frames.push(frame) })

    expect(fetchSpy).toHaveBeenCalledWith(
      expect.stringContaining(ATTENTION_STREAM_PATH),
      expect.objectContaining({ method: 'GET' }),
    )
    expect(frames).toEqual([{ kind: 'heartbeat', epoch: 'e1', seq: 1 }])
  })
})

/** A response whose body is one server-sent text, read in a single chunk. */
function fakeSseResponse(text: string): Response {
  const chunk = new TextEncoder().encode(text)
  let sent = false
  const reader = {
    read: async () => {
      if (sent) return { done: true, value: undefined }
      sent = true
      return { done: false, value: chunk }
    },
  }
  return { ok: true, status: 200, body: { getReader: () => reader } } as unknown as Response
}
