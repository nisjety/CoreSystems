import { isAttentionState, type AttentionState } from '@/shared/attention/attention-state'
import { requestJson } from './http'

/** Why a waiting thread waits, when the server names it. */
export type AttentionReason = 'approval' | 'plan' | 'input'

/** One thread the attention listing names. Identifiers and the closed state only. */
export type AttentionItem = {
  threadId: string
  state?: AttentionState
  reason?: AttentionReason
  /** When the state began (ISO 8601). Absent when the state has no start. */
  since?: string
  unseen: boolean
  runId?: string
}

export type AttentionCounts = { needsYou: number; working: number; unseen: number }

export type AttentionSnapshot = {
  /** False while the server's attention switch is off. The items and counts are then empty. */
  enabled: boolean
  items: AttentionItem[]
  counts: AttentionCounts
  /** Whether the live stream and web push are served. Both read false until their slices land. */
  capabilities: { stream: boolean; push: boolean }
}

const REASONS: readonly AttentionReason[] = ['approval', 'plan', 'input']

/** Fired on the window when the server's attention state may have moved, so the open rail reads it again. */
export const ATTENTION_CHANGED_EVENT = 'verevon:attention-changed'

export function announceAttentionChanged(): void {
  if (typeof window !== 'undefined') window.dispatchEvent(new Event(ATTENTION_CHANGED_EVENT))
}

export async function getAttention(): Promise<AttentionSnapshot> {
  return normalizeAttention(await requestJson<unknown>('/api/v1/attention'))
}

/** Marks one of the caller's threads read. Session Core refuses a zero-retention caller, and that refusal rejects here. */
export async function markThreadSeen(threadId: string): Promise<void> {
  await requestJson<unknown>('/api/v1/attention/seen', {
    method: 'POST',
    body: JSON.stringify({ threadId }),
  })
}

function normalizeAttention(value: unknown): AttentionSnapshot {
  const record = objectValue(value)
  const counts = objectValue(record?.counts)
  const capabilities = objectValue(record?.capabilities)
  const rawItems = record?.items
  const items: AttentionItem[] = []
  if (Array.isArray(rawItems)) {
    for (const raw of rawItems) {
      const item = normalizeItem(raw)
      if (item) items.push(item)
    }
  }
  return {
    enabled: record?.enabled === true,
    items,
    counts: {
      needsYou: count(counts?.needsYou),
      working: count(counts?.working),
      unseen: count(counts?.unseen),
    },
    capabilities: {
      stream: capabilities?.stream === true,
      push: capabilities?.push === true,
    },
  }
}

/** An item without a thread id cannot be opened, so it is dropped; an unknown state or reason is dropped alone. */
function normalizeItem(value: unknown): AttentionItem | null {
  const record = objectValue(value)
  const threadId = typeof record?.threadId === 'string' ? record.threadId : ''
  if (!threadId) return null
  const state = isAttentionState(record?.state) ? record.state : undefined
  const reason = REASONS.find((candidate) => candidate === record?.reason)
  const since = typeof record?.since === 'string' && record.since ? record.since : undefined
  const runId = typeof record?.runId === 'string' && record.runId ? record.runId : undefined
  return {
    threadId,
    ...(state ? { state } : {}),
    ...(reason ? { reason } : {}),
    ...(since ? { since } : {}),
    unseen: record?.unseen === true,
    ...(runId ? { runId } : {}),
  }
}

function objectValue(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined
}

function count(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) ? Math.max(0, Math.trunc(value)) : 0
}
