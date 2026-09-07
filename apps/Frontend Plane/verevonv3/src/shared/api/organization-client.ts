import { requestJson } from './http'

export type OrganizationSummary = {
  id: string
  name: string
  slug: string
}

export type SupportAIMode = 'off' | 'assist' | 'review'
export type OrganizationAISettings = { zdr: boolean; supportAiMode: SupportAIMode }

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

/** One person in the organization, as the org roster reports them. */
export type OrganizationMember = {
  userId: string
  name?: string
  email: string
  role: string
  status: string
}

function memberFrom(value: unknown): OrganizationMember | null {
  if (!value || typeof value !== 'object') return null
  const record = value as Record<string, unknown>
  const userId = typeof record.userId === 'string' ? record.userId : typeof record.user_id === 'string' ? record.user_id : ''
  const email = typeof record.email === 'string' ? record.email : ''
  if (!userId.trim()) return null
  return {
    userId,
    ...(typeof record.name === 'string' && record.name.trim() ? { name: record.name.trim() } : {}),
    email,
    role: typeof record.role === 'string' ? record.role : '',
    status: typeof record.status === 'string' ? record.status : '',
  }
}

/**
 * Everyone in the organization, for choosing who to add to a room.
 *
 * Only `active` members are returned. An invitation is not yet a membership and
 * a suspension deliberately withholds access, so offering either as someone to
 * put in a room would hand out content on the strength of a state that says
 * not to — the same rule the organization room's own roster sync applies.
 */
export async function listOrganizationMembers(organizationId: string): Promise<OrganizationMember[]> {
  const payload = await requestJson<{ members?: unknown[] } | unknown[]>(
    `/api/v1/orgs/${encodeURIComponent(organizationId)}/members`,
  )
  const raw = Array.isArray(payload)
    ? payload
    : Array.isArray((payload as { members?: unknown[] })?.members)
      ? (payload as { members: unknown[] }).members
      : []
  return raw
    .map(memberFrom)
    .filter((member): member is OrganizationMember => member !== null && member.status === 'active')
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
export async function getOrganizationAISettings(organizationId: string): Promise<OrganizationAISettings> {
  const payload = await requestJson<unknown>(
    `/api/v1/orgs/${encodeURIComponent(organizationId)}`,
  )
  const metadata = record(record(payload)?.metadata)
  const interactiveRetention = record(metadata?.interactiveRetention)
  const zdr = interactiveRetention?.zdr
  const supportAiMode = record(metadata?.supportAi)?.mode
  return {
    zdr: typeof zdr === 'boolean' ? zdr : false,
    supportAiMode: supportAiMode === 'off' || supportAiMode === 'assist' || supportAiMode === 'review'
      ? supportAiMode
      : 'review',
  }
}

export async function getOrganizationZdr(organizationId: string): Promise<boolean> {
  return (await getOrganizationAISettings(organizationId)).zdr
}

export async function getOrganizationSupportAIMode(organizationId: string): Promise<SupportAIMode> {
  return (await getOrganizationAISettings(organizationId)).supportAiMode
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

export async function updateOrganizationSupportAIMode(organizationId: string, supportAiMode: SupportAIMode): Promise<SupportAIMode> {
  const payload = await requestJson<unknown>(`/api/v1/orgs/${encodeURIComponent(organizationId)}/settings`, {
    method: 'PATCH', body: JSON.stringify({ supportAiMode }),
  })
  const metadata = record(record(payload)?.metadata)
  const mode = record(metadata?.supportAi)?.mode
  return mode === 'off' || mode === 'assist' || mode === 'review' ? mode : supportAiMode
}

/**
 * ADR-0003's org layer of the authored-instruction hierarchy. Any active org
 * member may read it (it shapes every member's chat turns); writing is
 * org-admin gated at the gateway (`orgs/instructions.rs`). Storage is Convex
 * (`organizations.instructions`), a different backend than the ZDR/support-AI
 * posture above — hence a separate `/instructions` route rather than a third
 * field on `/settings`.
 */
export async function getOrganizationInstructions(organizationId: string): Promise<string> {
  const payload = await requestJson<unknown>(
    `/api/v1/orgs/${encodeURIComponent(organizationId)}/instructions`,
  )
  const instructions = record(payload)?.instructions
  return typeof instructions === 'string' ? instructions : ''
}

export async function updateOrganizationInstructions(
  organizationId: string,
  instructions: string,
): Promise<string> {
  const payload = await requestJson<unknown>(
    `/api/v1/orgs/${encodeURIComponent(organizationId)}/instructions`,
    {
      method: 'PATCH',
      body: JSON.stringify({ instructions }),
    },
  )
  const saved = record(payload)?.instructions
  return typeof saved === 'string' ? saved : instructions
}
