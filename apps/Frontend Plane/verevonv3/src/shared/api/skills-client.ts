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
  /**
   * Where the skill came from: `background_review` is a skill Verevon learned from
   * one of its owner's conversations; `user` is one a person wrote.
   */
  origin?: 'user' | 'background_review'
  /**
   * A learned skill starts `proposed`, is used only once its owner accepts it, and is
   * `rejected` if the owner declines. Every other skill is `accepted`.
   */
  review_state?: 'proposed' | 'accepted' | 'rejected'
  /** The run a learned skill was distilled from, and why it was proposed. */
  source_run_id?: string
  proposal_reason?: string
  created_at?: string
  updated_at?: string
  /**
   * EP-11 S2: the skill's newest immutable version (0 for a proposal never accepted) and its
   * digest. Present only while the server's version history is on; absent, the page shows no
   * version and never asks for a history.
   */
  version?: number
  digest?: string
}

/** One immutable version of a skill, as the history lists it (EP-11 S2). */
export interface SkillVersion {
  version: number
  /** Lowercase hex sha256 of the versioned fields. */
  digest: string
  name: string
  description: string
  content: string
  trigger_keywords: string[]
  trigger_file_patterns: string[]
  tool_restrictions: string[]
  bundle_sha256?: string
  /** Who cut the version: `user`, `service`, `system` or `unattributed`. */
  actor_kind: string
  actor_id?: string
  created_at: string
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
    /**
     * `org` with `promoted_from` copies a skill an admin was shared (accepted, from a
     * member) into the organization. The server checks that the source is visible to
     * the caller; the copy is a new skill and the source is left as it was. `user` makes
     * a private skill of the caller's own (EP-11 S1); a member may do so only while the
     * gateway's member switch is on, see `memberSkillAuthoring`.
     */
    scope?: 'org' | 'user'
    promoted_from?: string
  },
): Promise<{ id: string }> {
  return requestJson<{ id: string }>('/api/v1/skills', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: { 'x-verevon-org-id': orgId },
  })
}

let memberSkillAuthoringAnswer: Promise<boolean> | undefined

/**
 * Whether this person may author private skills here (EP-11 S1). The gateway answers only while
 * `SKILL_MEMBER_AUTHORING` is on, so a refusal, a failure or a missing route all read as no, and
 * members see the page they saw before. One answer serves every caller in the page.
 */
export function memberSkillAuthoring(): Promise<boolean> {
  memberSkillAuthoringAnswer ??= requestJson<{ member_authoring?: boolean } | null>(
    '/api/v1/skills/authoring',
  )
    .then((payload) => payload?.member_authoring === true)
    .catch(() => false)
  return memberSkillAuthoringAnswer
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

/**
 * A skill's versions, newest first, with their text (EP-11 S2, action `skills.versions.list`).
 * Read only; the server shows the history to whoever may read the skill and answers 404 for a
 * skill the caller cannot see, or while the version history is off.
 */
export async function listSkillVersions(
  orgId: string,
  skillId: string,
  options: { limit?: number; beforeVersion?: number } = {},
): Promise<{ versions: SkillVersion[]; has_more: boolean }> {
  const query = new URLSearchParams()
  if (options.limit) query.set('limit', String(options.limit))
  if (options.beforeVersion) query.set('before_version', String(options.beforeVersion))
  const text = query.toString()
  const suffix = text ? `?${text}` : ''
  const payload = await requestJson<{ versions?: SkillVersion[]; has_more?: boolean } | null>(
    `/api/v1/skills/${encodeURIComponent(skillId)}/versions${suffix}`,
    { headers: { 'x-verevon-org-id': orgId } },
  )
  return { versions: payload?.versions ?? [], has_more: payload?.has_more === true }
}

export function deleteSkill(orgId: string, skillId: string): Promise<void> {
  return requestJson<void>(`/api/v1/skills/${encodeURIComponent(skillId)}`, {
    method: 'DELETE',
    headers: { 'x-verevon-org-id': orgId },
  })
}
