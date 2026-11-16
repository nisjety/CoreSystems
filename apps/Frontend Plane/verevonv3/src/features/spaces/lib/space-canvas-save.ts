/**
 * The room canvas's write path as the SPA runs it (WS19-S6, restore in WS19-S7): the draft rules wiki-store also
 * enforces, the one action a save goes through, the save attempt a retry keeps, and which refusals mean the head moved
 * (reload and decide, never a plain failure). A restore is a write of the same kind through its own action. Sequencing
 * only: Control and Data Plane decide every write, and the codes are the gateway's
 * (apps/gateway/src/domains/spaces/canvas.rs).
 */
import { executeAction } from '@/shared/actions/action-client'
import type { ActionActor } from '@/shared/actions/types'
import { ApiError } from '@/shared/api/http'
import {
  spaceCanvasWriteResultFrom,
  type SpaceCanvas,
  type SpaceCanvasVersion,
  type SpaceCanvasWriteResult,
} from '@/shared/api/spaces-client'

/** wiki-store's ceiling for one version's text, in bytes (`canvas.MaxContentBytes`, 64 KiB). */
export const CANVAS_MAX_BYTES = 65_536

export type DraftProblem = 'empty' | 'nul' | 'too_large'

/** The UTF-8 length of the text: the length wiki-store checks, which counts bytes and not characters. */
export function canvasTextBytes(text: string): number {
  return new TextEncoder().encode(text).length
}

/**
 * The first rule the draft breaks, or null. wiki-store takes 1 to 65,536 bytes with no NUL byte: whitespace alone
 * is a valid version and an empty draft is not, so a canvas cannot be cleared to nothing.
 */
export function canvasDraftProblem(text: string): DraftProblem | null {
  if (text.length === 0) return 'empty'
  if (text.includes('\u0000')) return 'nul'
  if (canvasTextBytes(text) > CANVAS_MAX_BYTES) return 'too_large'
  return null
}

/** One save attempt: the id it is sent under, the head it is written over, and its text. */
export type PendingSave = {
  readonly clientSaveId: string
  readonly expectedVersionId: string | null
  readonly content: string
}

/**
 * The attempt a draft is sent under. A retry of the same draft over the same head keeps its id, so the write is
 * replayed rather than repeated. A changed draft or a new base gets a new id (a reused id with other content is
 * refused as `idempotency_key_conflict`).
 */
export function pendingSaveFor(
  previous: PendingSave | null,
  expectedVersionId: string | null,
  content: string,
): PendingSave {
  if (previous && previous.expectedVersionId === expectedVersionId && previous.content === content) return previous
  return { clientSaveId: crypto.randomUUID(), expectedVersionId, content }
}

const STALE_SAVE_CODES: ReadonlySet<string> = new Set([
  'canvas_head_moved',
  'space_decision_digest_mismatch',
  'idempotency_key_conflict',
])

/**
 * Whether a refused save means the head moved under it. Such a refusal is never a plain failure: the SPA reloads
 * the latest version and decides. A create whose first attempt landed answers the digest mismatch on its retry (the
 * gateway derives a save once the canvas exists), and the reload finds the text the person saved.
 */
export function isStaleSaveRefusal(error: unknown): boolean {
  return error instanceof ApiError && error.code !== null && STALE_SAVE_CODES.has(error.code)
}

/**
 * Whether a failed save may have landed: the connection dropped, or the answer was a timeout or a 5xx. Such a draft
 * keeps its save id, so the retry replays the write. Any other refusal means nothing was stored.
 */
export function isAmbiguousSaveFailure(error: unknown): boolean {
  if (error instanceof ApiError) return error.status >= 500 || error.status === 408
  return error instanceof TypeError
}

/**
 * The head a successful save leaves on screen, from the write's own answer. A save that answered `unchanged` names
 * no new version, so the caller reads the head instead (null here).
 */
export function headAfterSave(
  result: SpaceCanvasWriteResult,
  content: string,
  spaceRef: string,
  before: SpaceCanvas | null,
): { readonly canvas: SpaceCanvas; readonly head: SpaceCanvasVersion } | null {
  if (result.unchanged) return null
  const { version } = result
  return {
    canvas: {
      canvasId: result.canvasId,
      spaceRef,
      state: 'active',
      headVersionId: version.versionId,
      headVersionNo: version.versionNo,
      createdAt: before?.createdAt ?? version.createdAt,
      updatedAt: version.createdAt,
    },
    head: { ...version, content },
  }
}

/**
 * Saves a version of the room canvas through the action registry (`spaces.save_canvas`, the contract in
 * shared/actions/domains/spaces/ws19.ts). The answer is the write's own: a stored version or the unchanged head. A
 * refusal arrives as the ApiError the gateway sent, with its typed code and details.
 */
export async function saveSpaceCanvasVersion(
  spaceRef: string,
  actor: ActionActor,
  input: { readonly expectedVersionId: string | null; readonly content: string; readonly clientSaveId: string },
): Promise<SpaceCanvasWriteResult> {
  const execution = await executeAction(
    'spaces.save_canvas',
    actor,
    {
      spaceRef,
      expectedVersionId: input.expectedVersionId,
      content: input.content,
      clientSaveId: input.clientSaveId,
    },
    { idempotencyKey: input.clientSaveId },
  )
  return spaceCanvasWriteResultFrom(execution.result)
}

/**
 * The prefix of a restore's client id. The gateway derives every write's idempotency key as `canvas-save-` plus the
 * client id, for a save and for a restore alike (`save_idempotency_key`, canvas.rs), so a restore's id carries this
 * prefix and can never equal a save's id, which is a bare UUID.
 */
export const CANVAS_RESTORE_ID_PREFIX = 'canvas-restore-'

/** One restore attempt: the version it copies, the head it is written over, and the id it is sent under. */
export type PendingRestore = {
  readonly clientSaveId: string
  readonly versionId: string
  readonly expectedVersionId: string | null
}

/**
 * The attempt a restore is sent under. A retry of the same version over the same head keeps its id, so the write is
 * replayed; another version or another head gets a new id.
 */
export function pendingRestoreFor(
  previous: PendingRestore | null,
  versionId: string,
  expectedVersionId: string | null,
): PendingRestore {
  if (previous && previous.versionId === versionId && previous.expectedVersionId === expectedVersionId) return previous
  return { clientSaveId: `${CANVAS_RESTORE_ID_PREFIX}${crypto.randomUUID()}`, versionId, expectedVersionId }
}

/**
 * Makes an earlier version of the room canvas the new head through the action registry
 * (`spaces.restore_canvas_version`). The answer is the restore's own: a stored version, which is a new head with
 * change_kind `restore`, or the unchanged head when that head already holds the text. A refusal arrives as the
 * ApiError the gateway sent, and is read by the same isStaleSaveRefusal and isAmbiguousSaveFailure as a save.
 */
export async function restoreSpaceCanvasVersion(
  spaceRef: string,
  actor: ActionActor,
  input: { readonly versionId: string; readonly expectedVersionId: string | null; readonly clientSaveId: string },
): Promise<SpaceCanvasWriteResult> {
  const execution = await executeAction(
    'spaces.restore_canvas_version',
    actor,
    {
      spaceRef,
      versionId: input.versionId,
      expectedVersionId: input.expectedVersionId,
      clientSaveId: input.clientSaveId,
    },
    { idempotencyKey: input.clientSaveId },
  )
  return spaceCanvasWriteResultFrom(execution.result)
}
