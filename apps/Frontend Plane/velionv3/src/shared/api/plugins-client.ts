import { requestJson } from './http'

/**
 * A durable org plugin package, mirroring capability-core's `plugin_packages`
 * row (fronted by model-gateway `/v1/plugins`). A plugin manifest can contribute
 * tools, skills, and hooks; it is inert until enabled by an admin. New packages
 * default disabled+unpinned (safe rollout).
 */
export interface Plugin {
  id: string
  org_id: string
  name: string
  version: string
  description: string
  manifest_json: unknown
  risk_level: string
  enabled: boolean
  pinned: boolean
  rollout_state: string
  created_at?: string
  updated_at?: string
}

export function listPlugins(orgId: string): Promise<{ plugins: Plugin[] }> {
  return requestJson<{ plugins: Plugin[] }>('/api/v1/plugins', {
    headers: { 'x-velion-org-id': orgId },
  })
}

/** Register a plugin package. Org id is derived server-side. Returns the new id. */
export function createPlugin(
  orgId: string,
  body: {
    name: string
    version: string
    description?: string
    manifest_json?: unknown
    risk_level?: string
  },
): Promise<{ id: string }> {
  return requestJson<{ id: string }>('/api/v1/plugins', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: { 'x-velion-org-id': orgId },
  })
}

/** Update a plugin's enabled/pinned flags, description, or rollout state. */
export function updatePlugin(
  orgId: string,
  pluginId: string,
  body: { enabled?: boolean; pinned?: boolean; description?: string; rollout_state?: string },
): Promise<unknown> {
  return requestJson<unknown>(`/api/v1/plugins/${encodeURIComponent(pluginId)}`, {
    method: 'POST',
    body: JSON.stringify(body),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function deletePlugin(orgId: string, pluginId: string): Promise<void> {
  return requestJson<void>(`/api/v1/plugins/${encodeURIComponent(pluginId)}`, {
    method: 'DELETE',
    headers: { 'x-velion-org-id': orgId },
  })
}
