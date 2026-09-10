import { requestJson } from './http'

/**
 * A durable org skill, mirroring capability-core's `agent_skills` row shape
 * (fronted by model-gateway `/v1/skills`). A skill's `content` (markdown body)
 * is injected into the live chat prompt when its `trigger_keywords` match the
 * turn — see model-gateway `sse.rs::fetch_skill_context`.
 */
export interface Skill {
  id: string
  org_id: string
  name: string
  description: string
  content: string
  trigger_keywords: string[]
  trigger_file_patterns: string[]
  tool_restrictions: string[]
  enabled: boolean
  /**
   * Ownership (SKILL-1): `org` is visible to the whole organization, `user` is
   * private to `owner_user_id` until shared. These are the only two scopes the
   * registry can store — there is no `space` scope (a CHECK constraint, not a
   * convention), which is why the room composer badges "Organisasjon" and
   * "Personlig" and nothing else.
   */
  scope?: 'org' | 'user'
  owner_user_id?: string
  shared_with?: string[]
  created_at?: string
  updated_at?: string
}

/** Mirrors model-gateway `MAX_REQUESTED_SKILLS`; the server caps again. */
export const MAX_PICKED_SKILLS = 4

/**
 * The enabled skills this member may use in a composer. Org and visibility are
 * resolved server-side from the session (capability-core filters `user`-scoped
 * skills to the owner and explicit shares), so no hint header is needed.
 */
export async function listAvailableSkills(signal?: AbortSignal): Promise<Skill[]> {
  const payload = await requestJson<{ skills?: Skill[] } | null>('/api/v1/skills', { signal })
  return (payload?.skills ?? []).filter((skill) => skill && skill.enabled !== false && skill.id?.trim())
}

export function listSkills(orgId: string): Promise<{ skills: Skill[] }> {
  return requestJson<{ skills: Skill[] }>('/api/v1/skills', {
    headers: { 'x-verevon-org-id': orgId },
  })
}

/**
 * Create a durable org skill. Org id is derived server-side from the verified
 * session; the header is a presentation hint only. Returns the new skill id.
 */
export function createSkill(
  orgId: string,
  body: {
    name: string
    description?: string
    content: string
    trigger_keywords?: string[]
    enabled?: boolean
  },
): Promise<{ id: string }> {
  return requestJson<{ id: string }>('/api/v1/skills', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: { 'x-verevon-org-id': orgId },
  })
}

/** Update a skill's enabled flag, description, or content. */
export function updateSkill(
  orgId: string,
  skillId: string,
  body: { enabled?: boolean; description?: string; content?: string },
): Promise<unknown> {
  return requestJson<unknown>(`/api/v1/skills/${encodeURIComponent(skillId)}`, {
    method: 'POST',
    body: JSON.stringify(body),
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function deleteSkill(orgId: string, skillId: string): Promise<void> {
  return requestJson<void>(`/api/v1/skills/${encodeURIComponent(skillId)}`, {
    method: 'DELETE',
    headers: { 'x-verevon-org-id': orgId },
  })
}
