import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { ActionActor } from '@/shared/actions/types'

// WS04-S7: the SPA's one archive write path. The component tests mock this seam; this test pins what it sends to the
// action registry (the action id and the input) and how it reads the gateway's reply and refusals.

const actions = vi.hoisted(() => ({ executeAction: vi.fn() }))
vi.mock('@/shared/actions/action-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/actions/action-client')>()),
  ...actions,
}))

const { ApiError } = await import('@/shared/api/api-error')
const { setSpaceRoomArchived } = await import('./space-room-archive')

const ACTOR: ActionActor = { type: 'human', userId: 'user-kari', orgId: 'org-1' }

describe('setSpaceRoomArchived', () => {
  beforeEach(() => {
    actions.executeAction.mockReset()
  })

  it('archives through spaces.archive_room with only the room ref as input', async () => {
    actions.executeAction.mockResolvedValue({
      result: { state: 'archived', changed: true, archived_at: '2026-10-31T10:00:00Z' },
    })
    const result = await setSpaceRoomArchived('room-1', ACTOR, true, { idempotencyKey: 'key-1' })
    expect(actions.executeAction).toHaveBeenCalledTimes(1)
    expect(actions.executeAction).toHaveBeenCalledWith('spaces.archive_room', ACTOR, { spaceRef: 'room-1' }, {
      idempotencyKey: 'key-1',
    })
    expect(result).toEqual({ state: 'archived', changed: true, archivedAt: '2026-10-31T10:00:00Z' })
  })

  it('restores through spaces.unarchive_room', async () => {
    actions.executeAction.mockResolvedValue({ result: { state: 'active', changed: false, archived_at: null } })
    const result = await setSpaceRoomArchived('room-1', ACTOR, false)
    expect(actions.executeAction).toHaveBeenCalledWith('spaces.unarchive_room', ACTOR, { spaceRef: 'room-1' }, {})
    expect(result).toEqual({ state: 'active', changed: false, archivedAt: null })
  })

  it('passes a typed refusal through unchanged', async () => {
    const refusal = new ApiError('The organization room cannot be archived.', 409, 'room_not_archivable')
    actions.executeAction.mockRejectedValue(refusal)
    await expect(setSpaceRoomArchived('org-room', ACTOR, true)).rejects.toBe(refusal)
  })

  it('fails closed on a reply that does not match the contract', async () => {
    for (const result of [null, {}, { state: 'gone', changed: true }, { state: 'archived', changed: 'yes' }]) {
      actions.executeAction.mockResolvedValueOnce({ result })
      await expect(setSpaceRoomArchived('room-1', ACTOR, true)).rejects.toThrow()
    }
  })
})
