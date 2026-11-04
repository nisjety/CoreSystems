import { isAttentionState } from '@/shared/attention/attention-state'
import type { AttentionCounts, AttentionItem, AttentionReason } from './attention-client'
import { readSseStream, type SseEvent } from './sse'

/**
 * The per-person attention stream (build plan WS16 S5). The server sends one snapshot, then a frame for
 * each thread that changed, a resync when it could not read the rows, and a heartbeat while nothing
 * changes. Each frame is one server-sent event named for its kind, with `<epoch>:<seq>` as its id. The
 * gateway relays the bytes unchanged, so the keys are the snake_case names Model Gateway writes.
 */

export const ATTENTION_STREAM_PATH = '/api/v1/attention/stream'

/** The gap the server keeps between heartbeats on a quiet stream. */
export const ATTENTION_HEARTBEAT_MS = 20_000

export type AttentionFrame =
  | { kind: 'snapshot'; epoch: string; seq: number; items: AttentionItem[]; counts: AttentionCounts }
  | { kind: 'changed'; epoch: string; seq: number; item: AttentionItem; counts: AttentionCounts }
  | { kind: 'resync'; epoch: string; seq: number }
  | { kind: 'heartbeat'; epoch: string; seq: number }

const REASONS: readonly AttentionReason[] = ['approval', 'plan', 'input', 'goal']

/** Reads one server-sent event as a frame. A frame this build cannot read is dropped, never guessed at. */
export function parseAttentionFrame(event: SseEvent): AttentionFrame | null {
  const position = parseEventId(event.id)
  if (!position) return null
  if (event.event === 'attention_heartbeat') return { kind: 'heartbeat', ...position }
  if (event.event === 'attention_resync') return { kind: 'resync', ...position }
  const data = recordOf(parseJson(event.data))
  if (!data) return null
  if (event.event === 'attention_snapshot') {
    const items = Array.isArray(data.items)
      ? data.items.map(normalizeItem).filter((item): item is AttentionItem => item !== null)
      : []
    return { kind: 'snapshot', ...position, items, counts: normalizeCounts(data.counts) }
  }
  if (event.event === 'attention_changed') {
    const item = normalizeItem(data.item)
    if (!item) return null
    return { kind: 'changed', ...position, item, counts: normalizeCounts(data.counts) }
  }
  return null
}

/**
 * Decides which frames of one stream a browser applies. A snapshot always starts over. Any other frame must
 * come from the epoch the snapshot named and carry the next sequence number: an old sequence number is
 * dropped, and a gap or a new epoch means frames were lost, so the stream has to be read again.
 */
export class AttentionFrameGate {
  private epoch: string | null = null
  private seq = 0

  accept(frame: AttentionFrame): 'apply' | 'stale' | 'reconnect' {
    if (frame.kind === 'snapshot') {
      this.epoch = frame.epoch
      this.seq = frame.seq
      return 'apply'
    }
    if (this.epoch === null || frame.epoch !== this.epoch) return 'reconnect'
    if (frame.seq <= this.seq) return 'stale'
    if (frame.seq !== this.seq + 1) return 'reconnect'
    this.seq = frame.seq
    return 'apply'
  }
}

/**
 * Opens the caller's attention stream and hands each frame to `onFrame` as it arrives. Resolves when the
 * stream ends, whether the server closed it, the request failed or `signal` aborted.
 */
export function openAttentionStream(options: {
  signal: AbortSignal
  onFrame: (frame: AttentionFrame) => void
  onError?: (error: unknown) => void
}): Promise<void> {
  return readSseStream(
    ATTENTION_STREAM_PATH,
    { signal: options.signal },
    (event) => {
      const frame = parseAttentionFrame(event)
      if (frame) options.onFrame(frame)
    },
    options.onError,
  )
}

function parseEventId(id: string | undefined): { epoch: string; seq: number } | null {
  if (!id) return null
  const colon = id.lastIndexOf(':')
  if (colon <= 0) return null
  const seq = Number(id.slice(colon + 1))
  if (!Number.isSafeInteger(seq) || seq < 1) return null
  return { epoch: id.slice(0, colon), seq }
}

function parseJson(text: string): unknown {
  try {
    return JSON.parse(text)
  } catch {
    return undefined
  }
}

function recordOf(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined
}

/** An item without a thread id cannot be opened, so it is dropped; an unknown state or reason is dropped alone. */
function normalizeItem(value: unknown): AttentionItem | null {
  const record = recordOf(value)
  const threadId = typeof record?.thread_id === 'string' ? record.thread_id : ''
  if (!threadId) return null
  const state = isAttentionState(record?.state) ? record.state : undefined
  const reason = REASONS.find((candidate) => candidate === record?.reason)
  const since = typeof record?.since === 'string' && record.since ? record.since : undefined
  const runId = typeof record?.run_id === 'string' && record.run_id ? record.run_id : undefined
  return {
    threadId,
    ...(state ? { state } : {}),
    ...(reason ? { reason } : {}),
    ...(since ? { since } : {}),
    unseen: record?.unseen === true,
    ...(runId ? { runId } : {}),
  }
}

function normalizeCounts(value: unknown): AttentionCounts {
  const record = recordOf(value)
  return {
    needsYou: count(record?.needs_you),
    working: count(record?.working),
    unseen: count(record?.unseen),
  }
}

function count(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) ? Math.max(0, Math.trunc(value)) : 0
}
