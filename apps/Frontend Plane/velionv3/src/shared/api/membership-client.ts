import { requestJson } from './http'

export type MembershipRole = 'member' | 'admin'

export async function inviteMember(
  organizationId: string,
  email: string,
  role: MembershipRole,
): Promise<void> {
  await requestJson(`/api/v1/orgs/${encodeURIComponent(organizationId)}/members/invite`, {
    method: 'POST',
    body: JSON.stringify({ email, role }),
  })
}

export async function updateMemberRole(
  organizationId: string,
  userId: string,
  role: MembershipRole,
): Promise<void> {
  await requestJson(
    `/api/v1/orgs/${encodeURIComponent(organizationId)}/members/${encodeURIComponent(userId)}/role`,
    { method: 'PATCH', body: JSON.stringify({ role }) },
  )
}

export async function removeMember(organizationId: string, userId: string): Promise<void> {
  await requestJson(
    `/api/v1/orgs/${encodeURIComponent(organizationId)}/members/${encodeURIComponent(userId)}`,
    { method: 'DELETE' },
  )
}
