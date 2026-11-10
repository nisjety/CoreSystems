import { requestJson } from './http'

// The owner actions this organization admits for agent runs (EP-6 S11). A preset that needs an owner
// action is offered only where the org admits it, so the Agent Run Console reads this set once and
// narrows its presets with it. The server computes the set; this module only reads it.

/** The admitted set as the BFF answers it: the mode it was computed under and the ids a run view may offer. */
export type AdmittedOwnerActions = {
  /** `tickets_only` while the server's admission table is off, `table` once it is on. */
  mode: string
  /** Action ids only, as the server sorts them: never a schema or a description. */
  actionIds: string[]
}

/**
 * The admitted owner actions of the caller's organization, or null on any failure. Null is the fail
 * closed answer: a caller that gets it offers no owner action, so every preset that needs one stays
 * unavailable. A reply that is not exactly the expected shape counts as a failure.
 */
export async function listAdmittedOwnerActions(signal?: AbortSignal): Promise<AdmittedOwnerActions | null> {
  try {
    const payload: unknown = await requestJson<unknown>('/api/v1/agents/admitted-actions', { signal })
    return parseAdmittedOwnerActions(payload)
  } catch {
    return null
  }
}

/**
 * Reads the reply: an object with a string `mode` and an array of non-empty string `action_ids`.
 * Anything else returns null rather than a partial set, so one malformed entry never narrows or
 * widens the answer silently.
 */
export function parseAdmittedOwnerActions(payload: unknown): AdmittedOwnerActions | null {
  if (!payload || typeof payload !== 'object') return null
  const record = payload as Record<string, unknown>
  const mode = record.mode
  const ids = record.action_ids
  if (typeof mode !== 'string' || !Array.isArray(ids)) return null
  const actionIds: string[] = []
  for (const id of ids as unknown[]) {
    if (typeof id !== 'string' || id.trim() === '') return null
    actionIds.push(id)
  }
  return { mode, actionIds }
}
