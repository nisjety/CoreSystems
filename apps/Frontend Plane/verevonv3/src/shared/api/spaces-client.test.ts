// WS04-S6: the room member calls of the spaces client against the Control writer routes.

import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('./http', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./http')>()
  return { ...actual, requestJson: vi.fn() }
})

import { requestJson } from './http'
import {
  addSpaceMember,
  changeSpaceMemberRole,
  getSpaceRoster,
  getSpaceRosterView,
  leaveSpaceRoom,
  removeSpaceMember,
} from './spaces-client'

const mockedRequest = vi.mocked(requestJson)

afterEach(() => {
  vi.clearAllMocks()
})

describe('addSpaceMember', () => {
  it('sends the role only when one is given', async () => {
    mockedRequest.mockResolvedValue({ member_count: 3, changed: true, role: 'editor' })
    await addSpaceMember('space 1', 'user_2')
    expect(mockedRequest).toHaveBeenLastCalledWith('/api/v1/spaces/space%201/members', {
      method: 'POST',
      body: JSON.stringify({ member_id: 'user_2' }),
    })
    await addSpaceMember('space 1', 'user_2', 'manager')
    expect(mockedRequest).toHaveBeenLastCalledWith('/api/v1/spaces/space%201/members', {
      method: 'POST',
      body: JSON.stringify({ member_id: 'user_2', role: 'manager' }),
    })
  })

  it('maps the audience state and the legacy answer without one', async () => {
    mockedRequest.mockResolvedValueOnce({ member_count: 3, changed: true, role: 'viewer', audience: { state: 'pending' } })
    await expect(addSpaceMember('space_1', 'user_2', 'viewer')).resolves.toEqual({
      memberCount: 3,
      changed: true,
      role: 'viewer',
      audience: 'pending',
    })
    for (const state of ['current', 'unconfirmed'] as const) {
      mockedRequest.mockResolvedValueOnce({ member_count: 1, changed: true, audience: { state } })
      expect((await addSpaceMember('space_1', 'user_2')).audience).toBe(state)
    }
    // Legacy gateway: no audience at all; an unknown state is not trusted.
    mockedRequest.mockResolvedValueOnce({ member_count: 2, changed: true })
    expect((await addSpaceMember('space_1', 'user_2')).audience).toBeUndefined()
    mockedRequest.mockResolvedValueOnce({ member_count: 2, changed: true, audience: { state: 'bogus' } })
    expect((await addSpaceMember('space_1', 'user_2')).audience).toBeUndefined()
    mockedRequest.mockResolvedValueOnce({})
    await expect(addSpaceMember('space_1', 'user_2')).resolves.toMatchObject({ memberCount: 0, changed: false })
  })
})

describe('removeSpaceMember', () => {
  it('DELETEs the named member and maps the audience state', async () => {
    mockedRequest.mockResolvedValue({ member_count: 2, changed: true, left: false, audience: { state: 'current' } })
    await expect(removeSpaceMember('space_1', 'user/2')).resolves.toEqual({
      memberCount: 2,
      changed: true,
      left: false,
      audience: 'current',
    })
    expect(mockedRequest).toHaveBeenCalledWith('/api/v1/spaces/space_1/members/user%2F2', { method: 'DELETE' })
  })
})

describe('changeSpaceMemberRole', () => {
  it('PATCHes the role with the expected revision when one is given', async () => {
    mockedRequest.mockResolvedValue({ role: 'manager', from_role: 'editor', changed: true })
    await expect(changeSpaceMemberRole('space_1', 'user_2', 'manager', 4)).resolves.toEqual({
      role: 'manager',
      fromRole: 'editor',
      changed: true,
    })
    expect(mockedRequest).toHaveBeenCalledWith('/api/v1/spaces/space_1/members/user_2', {
      method: 'PATCH',
      body: JSON.stringify({ role: 'manager', expected_revision: 4 }),
    })
  })

  it('omits the revision when none is known (a revision of 0 is still sent)', async () => {
    mockedRequest.mockResolvedValue({})
    await expect(changeSpaceMemberRole('space_1', 'user_2', 'viewer')).resolves.toEqual({
      role: 'viewer',
      fromRole: '',
      changed: false,
    })
    expect(mockedRequest).toHaveBeenLastCalledWith('/api/v1/spaces/space_1/members/user_2', {
      method: 'PATCH',
      body: JSON.stringify({ role: 'viewer' }),
    })
    await changeSpaceMemberRole('space_1', 'user_2', 'viewer', 0)
    expect(mockedRequest).toHaveBeenLastCalledWith('/api/v1/spaces/space_1/members/user_2', {
      method: 'PATCH',
      body: JSON.stringify({ role: 'viewer', expected_revision: 0 }),
    })
  })

  it('lets a typed Control refusal reach the caller', async () => {
    mockedRequest.mockRejectedValue(Object.assign(new Error('refused'), { code: 'room_member_changed' }))
    await expect(changeSpaceMemberRole('space_1', 'user_2', 'viewer', 1)).rejects.toMatchObject({
      code: 'room_member_changed',
    })
  })
})

describe('leaveSpaceRoom', () => {
  it('DELETEs .../members/me', async () => {
    mockedRequest.mockResolvedValue({ changed: true, left: true, member_count: 3 })
    await expect(leaveSpaceRoom('space_1')).resolves.toEqual({ changed: true, left: true, memberCount: 3 })
    expect(mockedRequest).toHaveBeenCalledWith('/api/v1/spaces/space_1/members/me', { method: 'DELETE' })
  })

  it('does not claim a leave the server did not confirm', async () => {
    mockedRequest.mockResolvedValue({})
    await expect(leaveSpaceRoom('space_1')).resolves.toEqual({ changed: false, left: false, memberCount: 0 })
  })
})

describe('getSpaceRosterView', () => {
  const members = [{ subject_type: 'user', subject_id: 'user_1', role: 'owner', revision: 1 }]

  it('reports writerApi only when the server said true', async () => {
    mockedRequest.mockResolvedValueOnce({ members, writer_api: true })
    await expect(getSpaceRosterView('space_1')).resolves.toEqual({ members, writerApi: true })
    expect(mockedRequest).toHaveBeenLastCalledWith('/api/v1/spaces/space_1/roster')
    for (const writer of [false, undefined, 'true', 1, null]) {
      mockedRequest.mockResolvedValueOnce({ members, writer_api: writer })
      expect((await getSpaceRosterView('space_1')).writerApi).toBe(false)
    }
  })

  it('leaves the plain roster call unchanged', async () => {
    mockedRequest.mockResolvedValue({ members, writer_api: true })
    await expect(getSpaceRoster('space_1')).resolves.toEqual(members)
  })
})
