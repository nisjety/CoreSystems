/**
 * WS04-S7: archive and restore a room as the SPA runs it. The one write path is the action registry
 * (`spaces.archive_room` and `spaces.unarchive_room`). The gateway checks the caller's live role through Control
 * (domains/spaces/room_admin.rs `set_room_archived`), and the reply is read by `spaceRoomArchiveFrom`, which fails
 * closed. The codes a refusal carries are the gateway's; their words live in SpaceRoomArchive.tsx.
 */
import { executeAction } from '@/shared/actions/action-client'
import type { ActionActor } from '@/shared/actions/types'
import { spaceRoomArchiveFrom, type SpaceRoomArchiveResult } from '@/shared/api/spaces-client'

/**
 * Archives (`archive: true`) or restores one room through the action registry. A refusal arrives as the ApiError the
 * gateway sent, with its typed code. A reply that does not match the contract throws, so no state is shown that was
 * not read.
 */
export async function setSpaceRoomArchived(
  spaceRef: string,
  actor: ActionActor,
  archive: boolean,
  options: { readonly idempotencyKey?: string } = {},
): Promise<SpaceRoomArchiveResult> {
  const execution = await executeAction(
    archive ? 'spaces.archive_room' : 'spaces.unarchive_room',
    actor,
    { spaceRef },
    options,
  )
  return spaceRoomArchiveFrom(execution.result)
}
