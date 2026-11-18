import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { SpaceMessagePolicyHints } from '@/shared/api/spaces-client'

const editSpaceMessage = vi.fn()
const deleteSpaceMessage = vi.fn()

vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  editSpaceMessage: (...args: unknown[]) => editSpaceMessage(...args),
  deleteSpaceMessage: (...args: unknown[]) => deleteSpaceMessage(...args),
}))

import { createSpaceTurnLifecycle, type LifecycleTurn, type SpaceTurnLifecycleInput } from './space-message-lifecycle'

const KARI = 'sub-kari'
const OLA = 'sub-ola'

const receipt = { messageId: 'msg-own', revisionState: 'edited', revisionNo: 2, deduplicated: false }

const ownMessage: LifecycleTurn = { role: 'user', messageId: 'msg-own', authorSubjectId: KARI }
const otherMessage: LifecycleTurn = { role: 'user', messageId: 'msg-other', authorSubjectId: OLA }
const agentAnswer: LifecycleTurn = { role: 'assistant', messageId: 'msg-agent' }

function hints(reviseEntitled: boolean, moderateEntitled: boolean): SpaceMessagePolicyHints {
  return { revise_entitled: reviseEntitled, moderate_entitled: moderateEntitled, original_retention: 'purge' }
}

// The default viewer is an editor (Kari) in a thread Ola started, with revise entitled and no moderation.
function lifecycle(overrides: Partial<SpaceTurnLifecycleInput> = {}) {
  const onChanged = vi.fn()
  const turns = createSpaceTurnLifecycle({
    spaceRef: () => 'room-a',
    threadId: () => 'thr-1',
    enabled: () => true,
    viewerSubjectId: () => KARI,
    viewerRole: () => 'editor',
    threadOwnerSubjectId: () => OLA,
    policy: () => hints(true, false),
    archived: () => false,
    onChanged,
    ...overrides,
  })
  return { turns, onChanged }
}

beforeEach(() => {
  editSpaceMessage.mockReset()
  deleteSpaceMessage.mockReset()
  editSpaceMessage.mockResolvedValue(receipt)
  deleteSpaceMessage.mockResolvedValue({ ...receipt, revisionState: 'deleted' })
})

// WS12-S7: these rules decide what the toolbar offers. Control and Session Core decide the call itself.
describe('which edit and delete controls a turn offers (WS12-S7)', () => {
  it('an editor edits and deletes their own message', () => {
    const { turns } = lifecycle()
    expect(turns.canEdit(ownMessage)).toBe(true)
    expect(turns.canDelete(ownMessage)).toBe(true)
  })

  it('a viewer offers neither control, not even on their own words, and no moderation', () => {
    const { turns } = lifecycle({ viewerRole: () => 'viewer', policy: () => hints(true, true) })
    expect(turns.canEdit(ownMessage)).toBe(false)
    expect(turns.canDelete(ownMessage)).toBe(false)
    expect(turns.canDelete(otherMessage)).toBe(false)
  })

  it('a manager without moderate_entitled edits and deletes their own words, but not someone else\'s message', () => {
    const { turns } = lifecycle({ viewerRole: () => 'manager', policy: () => hints(true, false) })
    expect(turns.canEdit(ownMessage)).toBe(true)
    expect(turns.canDelete(ownMessage)).toBe(true)
    expect(turns.canEdit(otherMessage)).toBe(false)
    expect(turns.canDelete(otherMessage)).toBe(false)
  })

  it('a manager with moderate_entitled may delete someone else\'s message, but still may not edit it', () => {
    const { turns } = lifecycle({ viewerRole: () => 'manager', policy: () => hints(true, true) })
    expect(turns.canDelete(otherMessage)).toBe(true)
    expect(turns.canEdit(otherMessage)).toBe(false)
  })

  it('the editor who started a thread may delete the agent answer in it, but not edit it', () => {
    const { turns } = lifecycle({ threadOwnerSubjectId: () => KARI })
    expect(turns.canDelete(agentAnswer)).toBe(true)
    expect(turns.canEdit(agentAnswer)).toBe(false)
  })

  it('an agent answer in a thread someone else started is not the viewer\'s to delete', () => {
    const { turns } = lifecycle()
    expect(turns.canDelete(agentAnswer)).toBe(false)
  })

  it('with revise_entitled false, the author may neither edit nor delete their own message', () => {
    const { turns } = lifecycle({ policy: () => hints(false, false) })
    expect(turns.canEdit(ownMessage)).toBe(false)
    expect(turns.canDelete(ownMessage)).toBe(false)
  })

  it('a deleted turn offers no control, even to a moderator', () => {
    const { turns } = lifecycle({ viewerRole: () => 'manager', policy: () => hints(true, true) })
    expect(turns.canEdit({ ...ownMessage, revisionState: 'deleted' })).toBe(false)
    expect(turns.canDelete({ ...otherMessage, revisionState: 'deleted' })).toBe(false)
  })

  it('an archived room offers no control, even to a moderator', () => {
    const { turns } = lifecycle({ viewerRole: () => 'manager', policy: () => hints(true, true), archived: () => true })
    expect(turns.canEdit(ownMessage)).toBe(false)
    expect(turns.canDelete(ownMessage)).toBe(false)
    expect(turns.canDelete(otherMessage)).toBe(false)
  })

  it('with the message_revise flag off, no control is offered, even to a moderator', () => {
    const { turns } = lifecycle({ enabled: () => false, viewerRole: () => 'manager', policy: () => hints(true, true) })
    expect(turns.canEdit(ownMessage)).toBe(false)
    expect(turns.canDelete(ownMessage)).toBe(false)
    expect(turns.canDelete(otherMessage)).toBe(false)
    expect(turns.canDelete(agentAnswer)).toBe(false)
  })
})

describe('the edit and delete calls (WS12-S7)', () => {
  it('a retry of the same text reuses its idempotency key, and new text gets a fresh one', async () => {
    editSpaceMessage.mockRejectedValueOnce(new Error('network'))
    const { turns, onChanged } = lifecycle()

    await expect(turns.edit(ownMessage, 'Hei Ola')).rejects.toThrow('network')
    await turns.edit(ownMessage, 'Hei Ola')
    await turns.edit(ownMessage, 'Hei Ola igjen')

    const keys = editSpaceMessage.mock.calls.map((call) => (call[3] as { idempotencyKey: string }).idempotencyKey)
    expect(keys).toHaveLength(3)
    expect(keys[0]).toMatch(/^[0-9a-f]{32}$/)
    expect(keys[1]).toBe(keys[0])
    expect(keys[2]).not.toBe(keys[0])
    expect(editSpaceMessage).toHaveBeenNthCalledWith(2, 'room-a', 'thr-1', 'msg-own', {
      content: 'Hei Ola',
      idempotencyKey: keys[0],
    })
    expect(editSpaceMessage).toHaveBeenNthCalledWith(3, 'room-a', 'thr-1', 'msg-own', {
      content: 'Hei Ola igjen',
      idempotencyKey: keys[2],
    })
    // Only the two edits that landed ask the transcript to be read again.
    expect(onChanged).toHaveBeenCalledTimes(2)
  })

  it('a failed edit does not ask for a re-read', async () => {
    editSpaceMessage.mockRejectedValueOnce(new Error('network'))
    const { turns, onChanged } = lifecycle()
    await expect(turns.edit(ownMessage, 'Hei Ola')).rejects.toThrow('network')
    expect(onChanged).not.toHaveBeenCalled()
  })

  it('a moderator\'s removal of someone else\'s message says why, and an author\'s removal does not', async () => {
    const { turns, onChanged } = lifecycle({ viewerRole: () => 'manager', policy: () => hints(true, true) })

    await turns.remove(otherMessage)
    await turns.remove(ownMessage)

    expect(deleteSpaceMessage).toHaveBeenNthCalledWith(1, 'room-a', 'thr-1', 'msg-other', {
      idempotencyKey: expect.stringMatching(/^[0-9a-f]{32}$/),
      moderationReason: 'other',
    })
    expect(deleteSpaceMessage).toHaveBeenNthCalledWith(2, 'room-a', 'thr-1', 'msg-own', {
      idempotencyKey: expect.stringMatching(/^[0-9a-f]{32}$/),
    })
    expect(deleteSpaceMessage.mock.calls[1]?.[3]).not.toHaveProperty('moderationReason')
    expect(onChanged).toHaveBeenCalledTimes(2)
  })
})
