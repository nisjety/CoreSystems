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
  /** The supply-chain pins (W4-PL-1). The server omits them for a package registered with the loader off. */
  source_kind?: string
  source_ref?: string
  source_pin?: string
  manifest_digest?: string
  /** Set while the package is installed; cleared by an uninstall or a kill. */
  installed_at?: string
}

export function listPlugins(orgId: string): Promise<{ plugins: Plugin[] }> {
  return requestJson<{ plugins: Plugin[] }>('/api/v1/plugins', {
    headers: { 'x-verevon-org-id': orgId },
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
    /** Required under PLUGIN_LOADER_ENABLED: a registered source and its pin. A branch or a tag is refused. */
    source_kind?: string
    source_ref?: string
    source_pin?: string
  },
): Promise<{ id: string }> {
  return requestJson<{ id: string }>('/api/v1/plugins', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: { 'x-verevon-org-id': orgId },
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
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function deletePlugin(orgId: string, pluginId: string): Promise<void> {
  return requestJson<void>(`/api/v1/plugins/${encodeURIComponent(pluginId)}`, {
    method: 'DELETE',
    headers: { 'x-verevon-org-id': orgId },
  })
}

// The catalogue and install routes (W4-PL-3), behind PLUGIN_LOADER_ENABLED. The gateway passes them through
// (agent_actions.rs), and capability-core decides every admin, zero-data-retention, consent and kill check. The org
// comes from the verified session, so these calls send no org header. Wire fields are snake_case, as the server writes them.

/** One component of a plugin's install report: what the manifest declares and whether an install would create it. */
export interface PluginComponent {
  /** skill, mcp_server or command when an install creates it; hook, bin, script or stdio_server when it is refused. */
  kind: string
  name: string
  /** `sha256:` over the component's canonical declaration. The consent list is made of these. */
  digest: string
  /** pending (an install would create it), installed, refused or removed. */
  status: string
  /** Why a component is refused. Empty for a pending one. */
  report?: string
}

/** A component row the loader recorded on an earlier install. */
export interface PluginStoredComponent {
  kind: string
  name: string
  child_ref?: string
  status: string
  report?: string
}

/** GET /api/v1/plugins/{id}/components: what an admin reads before consenting to an install. Writes nothing. */
export interface PluginComponentsReport {
  plugin_id: string
  name: string
  version: string
  manifest_digest: string
  installed: boolean
  components: PluginComponent[]
  /** The exact digests a consenting install sends back as `consented_digests`. */
  consent_digests: string[]
  stored: PluginStoredComponent[]
}

/** A child an install created, or an uninstall removed, with its id in its own registry. */
export interface PluginChild {
  kind: string
  name: string
  id: string
  digest?: string
}

/** The answer to an install or an uninstall: the full component report and the children it touched. */
export interface PluginInstallResult {
  plugin_id: string
  installed: boolean
  components: PluginComponent[]
  children: PluginChild[]
}

/** One package the org's catalogue lists: approved, pinned to a source, and not on the kill list. */
export interface PluginCatalogEntry {
  id: string
  name: string
  version: string
  description: string
  risk_level: string
  enabled: boolean
  rollout_state: string
  source_kind: string
  source_ref: string
  source_pin: string
  manifest_digest: string
  installed: boolean
  components: Array<{ kind: string; name: string; status: string }>
}

/** The answer to a catalogue approval change. */
export interface PluginCatalogApproval {
  id: string
  catalog_approved: boolean
  updated_at: string
}

/** The answer to a kill: whether its entry is new, and the installed copies it uninstalled. */
export interface PluginKillResult {
  digest: string
  scope: string
  org_id: string
  created: boolean
  uninstalled_plugins: string[]
  children_removed: number
}

/** One kill-list entry an org admin reads: a digest killed for this org, by the org itself or by the platform. */
export interface PluginKillEntry {
  digest: string
  /** `org` when the org killed it. `global` when the platform killed it for every org; only an org kill is lifted here. */
  source: string
  /** The reason the kill recorded, as written. Empty when none was given. */
  reason: string
  /** When the kill was written, as an RFC 3339 time. */
  killed_at: string
}

/** The answer to a kill-list removal. */
export interface PluginKillRemoval {
  digest: string
  scope: string
  org_id: string
  removed: boolean
}

/** The org's approved plugin catalogue. Any member of the org may read it, zero data retention included. */
export function listPluginCatalog(): Promise<{ plugins: PluginCatalogEntry[] }> {
  return requestJson<{ plugins: PluginCatalogEntry[] }>('/api/v1/plugin-catalog')
}

/** A plugin's install report: every component, its digest, and the digests a consenting install must send. Admin only. */
export function getPluginComponents(pluginId: string): Promise<PluginComponentsReport> {
  return requestJson<PluginComponentsReport>(`/api/v1/plugins/${encodeURIComponent(pluginId)}/components`)
}

/** Installs a plugin with the digests the admin consented to. The server accepts only the exact installable set. Admin only. */
export function installPlugin(pluginId: string, consentedDigests: readonly string[]): Promise<PluginInstallResult> {
  return requestJson<PluginInstallResult>(`/api/v1/plugins/${encodeURIComponent(pluginId)}/install`, {
    method: 'POST',
    body: JSON.stringify({ consented_digests: [...consentedDigests] }),
  })
}

/** Removes what a plugin's install created and takes the plugin out of service. Admin only. */
export function uninstallPlugin(pluginId: string): Promise<PluginInstallResult> {
  return requestJson<PluginInstallResult>(`/api/v1/plugins/${encodeURIComponent(pluginId)}/uninstall`, {
    method: 'POST',
  })
}

/** Lists a pinned package in the org catalogue (approved true), or takes it out again (false). Admin only. */
export function setPluginCatalogApproval(pluginId: string, approved: boolean): Promise<PluginCatalogApproval> {
  const verb = approved ? 'approve' : 'unapprove'
  return requestJson<PluginCatalogApproval>(`/api/v1/plugin-catalog/${encodeURIComponent(pluginId)}/${verb}`, {
    method: 'POST',
  })
}

/** Puts a manifest digest on the org's kill list and uninstalls its installed copies. Org scope only. Admin only. */
export function killPluginDigest(digest: string, reason: string): Promise<PluginKillResult> {
  return requestJson<PluginKillResult>('/api/v1/plugin-kill-list', {
    method: 'POST',
    body: JSON.stringify({ digest, scope: 'org', reason }),
  })
}

/** Removes the org's kill-list entry for a digest. What the kill removed stays removed. Admin only. */
export function liftPluginKill(digest: string): Promise<PluginKillRemoval> {
  const query = new URLSearchParams({ digest, scope: 'org' })
  return requestJson<PluginKillRemoval>(`/api/v1/plugin-kill-list?${query.toString()}`, {
    method: 'DELETE',
  })
}

/** The org's kill list: its own kills and the platform's global kills that cover it. Org admin only. */
export function listPluginKills(): Promise<{ kills: PluginKillEntry[] }> {
  return requestJson<{ kills: PluginKillEntry[] }>('/api/v1/plugin-kill-list')
}

/** A source pin or a digest in short form for a person to read: 12 hex digits, with `sha256:` kept when it has one. */
export function shortPin(value: string): string {
  const prefix = value.startsWith('sha256:') ? 'sha256:' : ''
  return `${prefix}${value.slice(prefix.length, prefix.length + 12)}`
}
