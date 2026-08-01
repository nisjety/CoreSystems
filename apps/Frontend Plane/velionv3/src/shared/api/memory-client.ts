import { requestJson } from './http'

/**
 * A single durable memory entry ("what do you remember about me"), backed by
 * session-core's `MemoryService.ListMemory` (user-scoped, across every
 * thread — never scoped to the conversation the caller happens to have
 * open). Fronted by model-gateway `/v1/memories`.
 */
export interface MemoryEntry {
  memoryId: string
  topic: string
  content: string
  updatedAt: string
}

export interface ListMemoriesResult {
  memories: MemoryEntry[]
  /**
   * True when the semantic backend could not be reached or is otherwise not
   * fully verified — e.g. a Zero Data Retention caller, for whom this is
   * always empty + degraded rather than an error. `memories` is still the
   * best available answer (durable index results are unaffected).
   */
  degraded: boolean
  degradationReason: string
}

interface ListMemoriesResponse {
  memories?: Array<{
    memory_id?: string
    memoryId?: string
    topic?: string
    content?: string
    updated_at?: string
    updatedAt?: string
  }>
  degraded?: boolean
  degradation_reason?: string
  degradationReason?: string
}

function normalizeMemory(raw: NonNullable<ListMemoriesResponse['memories']>[number]): MemoryEntry | null {
  const memoryId = raw.memory_id ?? raw.memoryId ?? ''
  if (!memoryId) return null
  return {
    memoryId,
    topic: raw.topic ?? '',
    content: raw.content ?? '',
    updatedAt: raw.updated_at ?? raw.updatedAt ?? '',
  }
}

/** Lists the signed-in user's durable memories, most-recently-updated first. */
export async function listMemories(limit?: number): Promise<ListMemoriesResult> {
  const path = limit ? `/api/v1/memory?limit=${encodeURIComponent(String(limit))}` : '/api/v1/memory'
  const response = await requestJson<ListMemoriesResponse>(path)
  const memories = (response.memories ?? [])
    .map(normalizeMemory)
    .filter((entry): entry is MemoryEntry => entry !== null)
  return {
    memories,
    degraded: response.degraded ?? false,
    degradationReason: response.degradation_reason ?? response.degradationReason ?? '',
  }
}

export interface DeleteMemoryResult {
  deleted: boolean
  degraded: boolean
  degradationReason: string
}

interface DeleteMemoryResponse {
  deleted?: boolean
  degraded?: boolean
  degradation_reason?: string
  degradationReason?: string
}

/**
 * Deletes a single memory by id. Destructive and non-recoverable —
 * session-core removes the durable record immediately and best-effort purges
 * the semantic backend copy; callers must confirm with the user before
 * invoking this.
 */
export async function deleteMemory(memoryId: string): Promise<DeleteMemoryResult> {
  const response = await requestJson<DeleteMemoryResponse>(
    `/api/v1/memory/${encodeURIComponent(memoryId)}`,
    { method: 'DELETE' },
  )
  return {
    deleted: response.deleted ?? false,
    degraded: response.degraded ?? false,
    degradationReason: response.degradation_reason ?? response.degradationReason ?? '',
  }
}
