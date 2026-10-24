import { requestJson } from './http'

/**
 * A single durable memory entry ("what do you remember about me"), backed by
 * session-core's `MemoryService.ListMemory` (user-scoped, across every
 * thread — never scoped to the conversation the caller happens to have
 * open). Fronted by model-gateway `/v1/memories`.
 */
/**
 * How a memory came to exist.
 *
 * `'unknown'` is a real answer, not a placeholder: rows written before
 * provenance was recorded cannot be attributed, and showing them as `'stated'`
 * would tell the user they said something they may not have.
 */
export type MemoryProvenance = 'stated' | 'inferred' | 'unknown'

export interface MemoryEntry {
  memoryId: string
  topic: string
  content: string
  updatedAt: string
  provenance: MemoryProvenance
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
    provenance?: string
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
    // Anything unrecognised — including a field an older gateway does not send
    // at all — is 'unknown'. Never default to a real value.
    provenance:
      raw.provenance === 'stated' || raw.provenance === 'inferred' ? raw.provenance : 'unknown',
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

export interface CorrectMemoryResult {
  /**
   * The corrected entry's id. NOT the same id passed in — session-core's
   * MemoryService has no update-in-place RPC, so the gateway indexes a fresh
   * entry with the corrected content and deletes the original; callers must
   * swap this in for the old id in whatever list they are rendering.
   */
  memoryId: string
  /** Whether the original entry was actually removed (see `degraded`). */
  deleted: boolean
  /**
   * What became of the original, so the caller never claims it was replaced
   * when it was not:
   *  - `removed`: deleted; the correction replaced it.
   *  - `not_found`: not the caller's to remove (an organisation entry or
   *    another user's; session-core answers 404 for those exactly as for a
   *    missing one), or already gone. It may still be recalled.
   *  - `not_removed`: the delete failed or was declined.
   */
  original: CorrectedOriginal
  degraded: boolean
  degradationReason: string
}

export type CorrectedOriginal = 'removed' | 'not_found' | 'not_removed'

interface CorrectMemoryResponse {
  memory_id?: string
  memoryId?: string
  deleted?: boolean
  original?: string
  degraded?: boolean
  degradation_reason?: string
  degradationReason?: string
}

/**
 * Read the original's fate. An older BFF sends only `deleted`; its `false`
 * cannot say why, so it reads as `not_removed`, never as removed.
 */
function correctedOriginal(response: CorrectMemoryResponse): CorrectedOriginal {
  if (response.original === 'removed' || response.original === 'not_found' || response.original === 'not_removed') {
    // `removed` is believed only alongside the boolean that means the same.
    return response.original === 'removed' && response.deleted !== true ? 'not_removed' : response.original
  }
  return response.deleted === true ? 'removed' : 'not_removed'
}

/**
 * Corrects a single memory's content, keeping it a "USER"-topic entry.
 *
 * `threadId` is only used to satisfy the write path's thread-ownership check
 * (the same one the Dreaming extractor's own writes go through) — pass
 * whichever thread the correction was made from; the corrected entry is not
 * scoped to it and is still recalled the same as any other personal memory.
 */
export async function correctMemory(
  memoryId: string,
  threadId: string,
  content: string,
): Promise<CorrectMemoryResult> {
  const response = await requestJson<CorrectMemoryResponse>(
    `/api/v1/memory/${encodeURIComponent(memoryId)}`,
    {
      method: 'PATCH',
      body: JSON.stringify({ threadId, content }),
    },
  )
  const original = correctedOriginal(response)
  return {
    memoryId: response.memory_id ?? response.memoryId ?? '',
    deleted: original === 'removed',
    original,
    degraded: response.degraded ?? false,
    degradationReason: response.degradation_reason ?? response.degradationReason ?? '',
  }
}

/** The user's own choice about their memory. */
export type MemoryState = 'on' | 'paused' | 'off'

/**
 * What decided whether memory is used. `zdr`, `org_disabled` and
 * `policy_unavailable` override the user's own choice; the chosen state is still
 * reported so the selector shows what they picked.
 */
export type MemoryControlsReason =
  | 'on'
  | 'user_paused'
  | 'user_off'
  | 'org_disabled'
  | 'zdr'
  | 'policy_unavailable'
  | 'unknown'

export interface MemoryControls {
  /** `null` for a state this client does not know: never read as `on`. */
  state: MemoryState | null
  reason: MemoryControlsReason
  useAllowed: boolean
  generationAllowed: boolean
  orgMemoryPolicy: string
  /** How many of the user's rows the list hides (a count, never a row). */
  hiddenRows: number
  updatedAt: string | null
}

interface MemoryControlsResponse {
  state?: string
  reason?: string
  use_allowed?: boolean
  generation_allowed?: boolean
  org_memory_policy?: string
  hidden_rows?: number
  updated_at?: string | null
}

const CONTROL_REASONS: readonly string[] = [
  'on',
  'user_paused',
  'user_off',
  'org_disabled',
  'zdr',
  'policy_unavailable',
]

function normalizeControls(raw: MemoryControlsResponse): MemoryControls {
  return {
    state: raw.state === 'on' || raw.state === 'paused' || raw.state === 'off' ? raw.state : null,
    reason: CONTROL_REASONS.includes(raw.reason ?? '') ? (raw.reason as MemoryControlsReason) : 'unknown',
    // A missing flag is "not allowed": the safe reading of an older gateway.
    useAllowed: raw.use_allowed ?? false,
    generationAllowed: raw.generation_allowed ?? false,
    orgMemoryPolicy: raw.org_memory_policy ?? '',
    hiddenRows: raw.hidden_rows ?? 0,
    updatedAt: raw.updated_at ?? null,
  }
}

/** The signed-in user's memory state and the decision taken from it. */
export async function getMemoryControls(): Promise<MemoryControls> {
  return normalizeControls(await requestJson<MemoryControlsResponse>('/api/v1/memory/controls'))
}

/** Sets the signed-in user's own memory state; the server refuses a ZDR session. */
export async function setMemoryState(state: MemoryState): Promise<MemoryControls> {
  return normalizeControls(
    await requestJson<MemoryControlsResponse>('/api/v1/memory/controls', {
      method: 'PUT',
      body: JSON.stringify({ state }),
    }),
  )
}

export interface ResetMemoryResult {
  durableDeleted: number
  /** Rows whose semantic copy could not be confirmed erased; their reads stay suppressed. */
  semanticUnconfirmed: number
  degraded: boolean
  degradationReason: string
}

interface ResetMemoryResponse {
  durable_deleted?: number
  semantic_unconfirmed?: number
  degraded?: boolean
  degradation_reason?: string
}

/**
 * Erases everything Verevon remembers about the signed-in user. Not
 * recoverable: callers must confirm with the user first. `confirm: true` is
 * always sent, because the server refuses a reset without it.
 */
export async function resetMemory(): Promise<ResetMemoryResult> {
  const response = await requestJson<ResetMemoryResponse>('/api/v1/memory/reset', {
    method: 'POST',
    body: JSON.stringify({ confirm: true }),
  })
  return {
    durableDeleted: response.durable_deleted ?? 0,
    semanticUnconfirmed: response.semantic_unconfirmed ?? 0,
    degraded: response.degraded ?? false,
    degradationReason: response.degradation_reason ?? '',
  }
}

/**
 * "Husk dette": saves one fact the user chose. Rejects (409
 * `memory_not_enabled`) while memory is paused, off or disabled by the
 * organization, so a caller never reports a save that did not happen.
 * `threadId` only proves the caller owns a thread.
 */
export async function rememberMemory(threadId: string, content: string): Promise<{ memoryId: string }> {
  const response = await requestJson<{ memory_id?: string; memoryId?: string }>('/api/v1/memory', {
    method: 'POST',
    body: JSON.stringify({ threadId, content }),
  })
  return { memoryId: response.memory_id ?? response.memoryId ?? '' }
}
