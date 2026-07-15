import { requestJson } from './http'

export type OrganizationSummary = {
  id: string
  name: string
  slug: string
}

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null
}

function organizationSummary(value: unknown): OrganizationSummary | null {
  const organization = record(value)
  if (!organization) return null
  const id = typeof organization.id === 'string' ? organization.id.trim() : ''
  const name = typeof organization.name === 'string' ? organization.name.trim() : ''
  if (!id || !name) return null
  return {
    id,
    name,
    slug: typeof organization.slug === 'string' ? organization.slug : '',
  }
}

export async function listOrganizations(): Promise<OrganizationSummary[]> {
  const payload = await requestJson<unknown>('/api/v1/orgs')
  const values = Array.isArray(payload)
    ? payload
    : record(payload)?.organizations
  if (!Array.isArray(values)) return []
  return values.flatMap((value) => {
    const organization = organizationSummary(value)
    return organization ? [organization] : []
  })
}

export async function switchActiveOrganization(organizationId: string): Promise<void> {
  await requestJson('/api/v1/orgs/switch-active', {
    method: 'POST',
    body: JSON.stringify({ organizationId }),
  })
}
