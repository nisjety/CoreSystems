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
  /**
   * EP-11 S5: the skill's unpublished edit, when it has one. Turns keep using the live text until
   * the draft is published. Present only while the server's draft switch is on.
   */
  draft?: SkillDraft | null
  draft_updated_at?: string
  draft_by?: string
}

/** A skill's unpublished edit (EP-11 S5): the same fields as a version body. */
export interface SkillDraft {
  name?: string
  description?: string
  content?: string
  trigger_keywords?: string[]
  trigger_file_patterns?: string[]
  tool_restrictions?: string[]
  bundle_sha256?: string
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

/**
 * The org's skills as the Skills page lists them. `drafts_enabled` (EP-11 S5) is true only while
 * the server saves edits of published skills as drafts; absent, the page offers no draft controls.
 * `import_enabled` (EP-11 S8) is true only while the server imports skills from a file; absent,
 * the page offers no import.
 */
export function listSkills(
  orgId: string,
): Promise<{ skills: Skill[]; drafts_enabled?: boolean; import_enabled?: boolean }> {
  return requestJson<{ skills: Skill[]; drafts_enabled?: boolean; import_enabled?: boolean }>('/api/v1/skills', {
    headers: { 'x-verevon-org-id': orgId },
  })
}

/**
 * Mirrors capability-core's bound on an uploaded bundle (`maxSkillBundleBodyBytes`: the 8 MiB of
 * files and room for the zip headers); the server checks again.
 */
export const MAX_SKILL_IMPORT_BYTES = 8 * 1024 * 1024 + 256 * 1024

/** What an import created: a switched-off skill whose text and files wait as a draft. */
export interface SkillImportResult {
  id: string
  name: string
  scope: 'org' | 'user'
  enabled: false
  draft: true
  /** The draft bundle, present when the zip held files beside SKILL.md. */
  bundle?: { manifest_sha256: string; file_count: number; total_bytes: number }
  /** Skill lint warnings of an import that went ahead. */
  warnings?: Array<{ code: string; field: string; severity: string }>
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  // Chunked, so a large file never overflows the argument list of String.fromCharCode.
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000))
  }
  return btoa(binary)
}

/**
 * Import a skill from a `SKILL.md` or a zip with `SKILL.md` and its files (EP-11 S8, action
 * `skills.import`). The server checks the file, creates the skill switched off and keeps its text
 * and files as a draft: nothing is used until the draft is published and the skill switched on.
 * Files beside `SKILL.md` are an admin's to import. A file over the bundle cap is refused here
 * before it is read.
 */
export async function importSkillFile(
  orgId: string,
  file: Blob,
  scope: 'org' | 'user' = 'user',
): Promise<SkillImportResult> {
  if (file.size > MAX_SKILL_IMPORT_BYTES) {
    throw new RangeError('skill_import_too_large')
  }
  const content = bytesToBase64(new Uint8Array(await file.arrayBuffer()))
  return requestJson<SkillImportResult>('/api/v1/skills/import', {
    method: 'POST',
    body: JSON.stringify({ content_base64: content, scope }),
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

/**
 * Update a skill's enabled flag, description, or content. While the server's draft switch is on,
 * a description or content change of an accepted skill is saved as its draft (action
 * `skills.draft.save`) and the answer carries `draft: true`.
 */
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

/**
 * Publish a skill's draft: it becomes the live text and a new version (EP-11 S5, action
 * `skills.publish`). The server answers 404 while drafts are off or when there is no draft.
 */
export function publishSkillDraft(
  orgId: string,
  skillId: string,
): Promise<{ id: string; version?: number; published?: boolean }> {
  return requestJson<{ id: string; version?: number; published?: boolean }>(
    `/api/v1/skills/${encodeURIComponent(skillId)}/publish`,
    { method: 'POST', headers: { 'x-verevon-org-id': orgId } },
  )
}

/** Discard a skill's draft; the live text stays (EP-11 S5, action `skills.draft.discard`). */
export function discardSkillDraft(orgId: string, skillId: string): Promise<void> {
  return requestJson<void>(`/api/v1/skills/${encodeURIComponent(skillId)}/draft`, {
    method: 'DELETE',
    headers: { 'x-verevon-org-id': orgId },
  })
}

/**
 * Restore an old version of a skill as its draft (EP-11 S5, action `skills.version.restore`). It
 * is not live until the draft is published, which cuts a new version.
 */
export function restoreSkillVersion(
  orgId: string,
  skillId: string,
  version: number,
): Promise<{ id: string; draft?: boolean; restored_from?: number; bundle_restored?: boolean }> {
  return requestJson<{ id: string; draft?: boolean; restored_from?: number; bundle_restored?: boolean }>(
    `/api/v1/skills/${encodeURIComponent(skillId)}/versions/${encodeURIComponent(String(version))}/restore`,
    { method: 'POST', headers: { 'x-verevon-org-id': orgId } },
  )
}

export function deleteSkill(orgId: string, skillId: string): Promise<void> {
  return requestJson<void>(`/api/v1/skills/${encodeURIComponent(skillId)}`, {
    method: 'DELETE',
    headers: { 'x-verevon-org-id': orgId },
  })
}
