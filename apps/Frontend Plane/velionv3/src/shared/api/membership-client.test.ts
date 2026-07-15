import { afterEach, describe, expect, it, vi } from 'vitest'
import { inviteMember, removeMember, updateMemberRole } from './membership-client'

function ok(data: unknown = {}): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status: 200,
  })
}

describe('membership client', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('uses canonical member/admin roles for invitations', async () => {
    const fetchMock = vi.fn(async () => ok())
    vi.stubGlobal('fetch', fetchMock)

    await inviteMember('org_1', 'teammate@example.com', 'member')

    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/org_1/members/invite')
    expect(JSON.parse(String(init.body))).toEqual({
      email: 'teammate@example.com',
      role: 'member',
    })
  })

  it('updates roles and removes members through Auth-owned gateway contracts', async () => {
    const fetchMock = vi.fn(async () => ok())
    vi.stubGlobal('fetch', fetchMock)

    await updateMemberRole('org_1', 'user_2', 'admin')
    await removeMember('org_1', 'user_2')

    const [rolePath, roleInit] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    const [removePath, removeInit] = fetchMock.mock.calls[1] as unknown as [string, RequestInit]
    expect(rolePath).toBe('/api/v1/orgs/org_1/members/user_2/role')
    expect(roleInit.method).toBe('PATCH')
    expect(removePath).toBe('/api/v1/orgs/org_1/members/user_2')
    expect(removeInit.method).toBe('DELETE')
  })
})
