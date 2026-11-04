import { requestJson } from './http'

/**
 * Authored templates and presets (EP-11 S6). Both are rows of the skills registry with a `kind`,
 * so they share a skill's lifecycle (versions, lint, owner and sharing) and are never injected into
 * a turn as a skill: model-gateway lists skills only.
 *
 * - A template is plain text. Picking `/name` puts it in the composer's draft with `{{input}}`
 *   filled from the rest of the line; nothing is sent until the person sends.
 * - A preset is a closed set of visible controls. Applying one only sets controls the person could
 *   set by hand; a persona, a tool allow-list or autonomy is an agent definition (EP-6), not a preset.
 *
 * Availability comes from the list route, not a build flag: while the server's
 * `AUTHORED_KINDS_ENABLED` is off it answers an empty list with `kinds_enabled: false`.
 */

export type AuthoredKind = 'template' | 'preset'

/** The one placeholder a template may hold (the authored-behaviour lint refuses any other). */
export const TEMPLATE_PLACEHOLDER = '{{input}}'

/** Most skills one preset may pick, as the lint and the composer bound it. */
export const MAX_PRESET_SKILLS = 4

/** A preset's settings: the closed schema the server lints (`preset_settings_invalid`, `preset_unknown_key`). */
export type PresetSettings = {
  modeId?: string
  browseWeb?: boolean
  /** A console preset never turns planning off, so the only value is true. */
  planMode?: true
  skillIds?: string[]
  templateId?: string
}

export type AuthoredBehaviour = {
  id: string
  kind: AuthoredKind
  name: string
  description: string
  /** A template's text, or a preset's goal text. */
  content: string
  scope?: 'org' | 'user'
  owner_user_id?: string
  enabled?: boolean
  settings: PresetSettings
}

export type AuthoredList = {
  items: AuthoredBehaviour[]
  /** False while the server's authored kinds are off (or the route did not say). */
  kindsEnabled: boolean
}

type ListPayload = { skills?: unknown; kinds_enabled?: unknown } | null

/**
 * The authored behaviours of one kind this person may use. A row whose `kind` is not the one asked
 * for is dropped: an older route that ignores `?kind=` answers skills, which must never be offered
 * as templates or presets.
 */
export async function listAuthored(kind: AuthoredKind, signal?: AbortSignal): Promise<AuthoredList> {
  const payload = await requestJson<ListPayload>(`/api/v1/skills?kind=${kind}`, { signal })
  const rows: unknown[] = Array.isArray(payload?.skills) ? (payload?.skills as unknown[]) : []
  const items = rows.flatMap((row): AuthoredBehaviour[] => {
    if (!row || typeof row !== 'object') return []
    const record = row as Record<string, unknown>
    if (record.kind !== kind || typeof record.id !== 'string' || !record.id.trim()) return []
    if (record.enabled === false || (record.review_state !== undefined && record.review_state !== 'accepted')) return []
    return [
      {
        id: record.id,
        kind,
        name: typeof record.name === 'string' ? record.name : '',
        description: typeof record.description === 'string' ? record.description : '',
        content: typeof record.content === 'string' ? record.content : '',
        scope: record.scope === 'org' || record.scope === 'user' ? record.scope : undefined,
        owner_user_id: typeof record.owner_user_id === 'string' ? record.owner_user_id : undefined,
        enabled: record.enabled !== false,
        settings: kind === 'preset' ? presetSettingsFrom(record.settings) : {},
      },
    ]
  })
  return { items, kindsEnabled: payload?.kinds_enabled === true }
}

/** Saves a private template of the caller's own. The owner is the session; no org is sent. */
export function createTemplate(body: { name: string; description?: string; content: string }): Promise<{ id: string }> {
  return requestJson<{ id: string }>('/api/v1/skills', {
    method: 'POST',
    body: JSON.stringify({ ...body, kind: 'template', scope: 'user', enabled: true }),
  })
}

/** Saves a private preset of the caller's own. Plan mode is always on. */
export function createPreset(body: {
  name: string
  description?: string
  content: string
  settings: PresetSettings
}): Promise<{ id: string }> {
  return requestJson<{ id: string }>('/api/v1/skills', {
    method: 'POST',
    body: JSON.stringify({
      ...body,
      settings: { ...body.settings, planMode: true },
      kind: 'preset',
      scope: 'user',
      enabled: true,
    }),
  })
}

/** Changes a template's or preset's description, text or (preset only) settings. */
export function updateAuthored(
  id: string,
  body: { description?: string; content?: string; settings?: PresetSettings },
): Promise<{ id: string }> {
  return requestJson<{ id: string }>(`/api/v1/skills/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    body: JSON.stringify(body.settings ? { ...body, settings: { ...body.settings, planMode: true } } : body),
  })
}

/**
 * A template's text with every `{{input}}` replaced by what followed the command on the line. A
 * template without the placeholder keeps that text after a blank line, so nothing typed is lost.
 * The result goes into the draft, never straight to the model.
 */
export function fillTemplate(text: string, input: string): string {
  const value = input.trim()
  if (text.includes(TEMPLATE_PLACEHOLDER)) return text.split(TEMPLATE_PLACEHOLDER).join(value)
  return value ? `${text}\n\n${value}` : text
}

/** Only the keys and types of the closed schema survive; anything else is dropped, never applied. */
export function presetSettingsFrom(value: unknown): PresetSettings {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return {}
  const record = value as Record<string, unknown>
  const settings: PresetSettings = {}
  if (typeof record.modeId === 'string' && record.modeId) settings.modeId = record.modeId
  if (typeof record.browseWeb === 'boolean') settings.browseWeb = record.browseWeb
  if (record.planMode === true) settings.planMode = true
  if (Array.isArray(record.skillIds)) {
    settings.skillIds = record.skillIds
      .filter((id): id is string => typeof id === 'string' && id.length > 0)
      .slice(0, MAX_PRESET_SKILLS)
  }
  if (typeof record.templateId === 'string' && record.templateId) settings.templateId = record.templateId
  return settings
}
