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

/**
 * Read the organization's interactive Zero-Data-Retention posture (the org's
 * selected intent). Returns the product default (false = ZDR off) when no
 * posture is stored. Note: live token-issue enforcement is owned by auth-core's
 * managed retention policy and is independent of this stored intent.
 */
export async function getOrganizationZdr(organizationId: string): Promise<boolean> {
  const payload = await requestJson<unknown>(
    `/api/v1/orgs/${encodeURIComponent(organizationId)}`,
  )
  const metadata = record(record(payload)?.metadata)
  const interactiveRetention = record(metadata?.interactiveRetention)
  const zdr = interactiveRetention?.zdr
  return typeof zdr === 'boolean' ? zdr : false
}

/**
 * Whether the organization's plan entitles it to enable Zero Data Retention.
 * ZDR is a premium, plan-gated privacy feature; ineligible orgs see it locked.
 */
export async function getOrganizationZdrEntitled(organizationId: string): Promise<boolean> {
  const payload = await requestJson<unknown>(
    `/api/v1/orgs/${encodeURIComponent(organizationId)}/entitlements`,
  )
  const container = record(payload)
  const list = Array.isArray(container?.entitlements)
    ? container.entitlements
    : Array.isArray(payload)
      ? payload
      : []
  return list.some((entry) => {
    const entitlement = record(entry)
    return entitlement?.key === 'feature.zero_data_retention' && entitlement?.enabled === true
  })
}

/**
 * Persist the organization's interactive Zero-Data-Retention posture. Org-admin
 * gated at the gateway; enabling ZDR is plan-gated (a 402 plan_upgrade_required
 * is returned for ineligible plans). The value is durable org intent, not a
 * per-request override. Returns the resolved posture after the write.
 */
export async function updateOrganizationZdr(
  organizationId: string,
  zeroDataRetention: boolean,
): Promise<boolean> {
  const payload = await requestJson<unknown>(
    `/api/v1/orgs/${encodeURIComponent(organizationId)}/settings`,
    {
      method: 'PATCH',
      body: JSON.stringify({ zeroDataRetention }),
    },
  )
  const metadata = record(record(payload)?.metadata)
  const interactiveRetention = record(metadata?.interactiveRetention)
  const zdr = interactiveRetention?.zdr
  return typeof zdr === 'boolean' ? zdr : zeroDataRetention
}
