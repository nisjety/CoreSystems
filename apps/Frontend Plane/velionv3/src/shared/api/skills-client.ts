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
  created_at?: string
  updated_at?: string
}

export function listSkills(orgId: string): Promise<{ skills: Skill[] }> {
  return requestJson<{ skills: Skill[] }>('/api/v1/skills', {
    headers: { 'x-velion-org-id': orgId },
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
    headers: { 'x-velion-org-id': orgId },
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
    headers: { 'x-velion-org-id': orgId },
  })
}

export function deleteSkill(orgId: string, skillId: string): Promise<void> {
  return requestJson<void>(`/api/v1/skills/${encodeURIComponent(skillId)}`, {
    method: 'DELETE',
    headers: { 'x-velion-org-id': orgId },
  })
}
