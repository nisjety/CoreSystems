/**
 * WS17-S7 and WS17-S7b: sharing one of the person's own memories to a room, and taking the copy back, as the SPA runs
 * them. Both writes go through the action registry (`memory.declassify_to_space`, `memory.retract_from_space`), never a
 * direct request. The gateway and Control decide every share and take-back; this file sequences the calls and words the
 * refusals. The gate is `features.memory_declassify` on GET /api/v1/spaces/features: presentation only, never an
 * authorization input.
 */
import { executeAction } from '@/shared/actions/action-client'
import type { ActionActor } from '@/shared/actions/types'
import { hasErrorCode } from '@/shared/api/api-error'
import {
  declassifySpaceMemoryResultFrom,
  retractSpaceMemoryResultFrom,
  type DeclassifySpaceMemoryResult,
  type RetractSpaceMemoryResult,
} from '@/shared/api/memory-client'
import { getSpaceFeatures, type SpaceSummary } from '@/shared/api/spaces-client'
import { translateApiError } from '@/shared/i18n'

type Copy = { readonly no: string; readonly en: string }
type Translate = (noText: string, enText: string) => string
export type MemoryRoomOperation = 'share' | 'take_back'

/**
 * The gateway answers a room that cannot confirm the caller's authority with 503 `space_decision_unavailable`. Its shared
 * sentence says the person could not confirm they may post, which is wrong for a share or a take-back, so each names its own.
 */
const DECISION_UNAVAILABLE: Readonly<Record<MemoryRoomOperation, Copy>> = {
  share: {
    no: 'Rommet kunne ikke bekrefte tilgangen din akkurat nå. Ingenting er delt; prøv igjen om litt.',
    en: 'The room could not confirm your access right now. Nothing was shared; try again shortly.',
  },
  take_back: {
    no: 'Rommet kunne ikke bekrefte tilgangen din akkurat nå. Ingenting er endret; prøv igjen om litt.',
    en: 'The room could not confirm your access right now. Nothing was changed; try again shortly.',
  },
}

/**
 * `features.memory_declassify` from GET /api/v1/spaces/features, the shell-level report the settings page can read without a
 * room in scope. Off when the report does not say so (the gateway's shell report does not carry the key yet; see the
 * WS17-S7 depth file) or cannot be read, so the control stays hidden.
 */
export async function memoryDeclassifyOffered(): Promise<boolean> {
  try {
    const flags = await getSpaceFeatures()
    return flags.memory_declassify === true
  } catch {
    return false
  }
}

/**
 * The rooms a memory can be shared to: rooms (Control's kind `room`) that are active and not archived, by name. The list
 * carries no role, so an editor-or-above check is the server's: a room the person may not share into is refused with
 * `memory_declassify_not_authorized`. Personal Spaces, direct messages and group DMs are not rooms, and are left out.
 */
export function shareableRooms(spaces: readonly SpaceSummary[]): SpaceSummary[] {
  return spaces
    .filter((space) => space.kind === 'room' && space.lifecycle === 'active' && space.archived !== true)
    .sort((left, right) => left.name.localeCompare(right.name))
}

/**
 * Shares one reviewed memory with a room, as the actor. `clientRequestId` names this share: a retry of the same text to the
 * same room reuses it, so the room holds the copy once. The same key is the action's own retry key.
 */
export async function shareMemoryToRoom(
  actor: ActionActor,
  input: { memoryId: string; spaceRef: string; reviewedContent: string; clientRequestId: string },
): Promise<DeclassifySpaceMemoryResult> {
  const execution = await executeAction('memory.declassify_to_space', actor, input, {
    idempotencyKey: input.clientRequestId,
  })
  return declassifySpaceMemoryResultFrom(execution.result)
}

/** Takes one shared copy back from its room, as the person who shared it. `clientRequestId` names this take-back. */
export async function takeMemoryBackFromRoom(
  actor: ActionActor,
  input: { memoryId: string; spaceRef: string; clientRequestId: string },
): Promise<RetractSpaceMemoryResult> {
  const execution = await executeAction('memory.retract_from_space', actor, input, {
    idempotencyKey: input.clientRequestId,
  })
  return retractSpaceMemoryResultFrom(execution.result)
}

/**
 * The sentence for a failed share or take-back. A 503 names the operation's own words; every other code keeps its copy from
 * the shared table, and the caller's fallback covers a code with none.
 */
export function memoryRoomErrorMessage(
  err: unknown,
  tr: Translate,
  operation: MemoryRoomOperation,
  fallback: Copy,
): string {
  if (hasErrorCode(err, 'space_decision_unavailable')) {
    const copy = DECISION_UNAVAILABLE[operation]
    return tr(copy.no, copy.en)
  }
  return translateApiError(err, tr, fallback)
}
