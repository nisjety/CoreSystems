// WS10-S5b: the per-tool policy of a shared Space. The gateway (apps/gateway/src/domains/spaces/
// tool_policy.rs) relays Control's store and answers:
//   GET  /api/v1/spaces/{space_ref}/tool-policy[?agent_subject_id=]
//   PUT  /api/v1/spaces/{space_ref}/tool-policy                       (the Space ceiling)
//   PUT  /api/v1/spaces/{space_ref}/agents/{binding_ref}/tool-policy  (one agent's layer)
// 404 `feature_disabled` with the gateway flag off (the editor is then hidden), 403
// `tool_policy_role_required` / `space_kind_unsupported` / `active_membership_required`, 400
// `tool_policy_invalid` (`details.reason`) / `unsupported_policy_field`, 404 `agent_binding_not_found`,
// 409 `tool_policy_revision_conflict` (reload the layer) / `approval_agent_not_in_space`, 503
// `tool_policy_unavailable`. They arrive as an ApiError carrying the code. The action contract is
// `spaces.set_tool_policy` (shared/actions/domains/spaces/ws10b.ts). A policy only tightens
// (allow < ask < block); the UI mirrors that for display, Control decides.
import { requestJson } from '../http'

export type SpaceToolLevel = 'allow' | 'ask' | 'block'
/** The layer that decided a tool's merged level; `default` means no layer speaks for it. */
export type SpaceToolSource = 'space_ceiling' | 'agent' | 'default'

export type SpaceToolPolicyEntry = {
  readonly toolKey: string
  readonly level: SpaceToolLevel
}

/** One stored layer. Revision 0 means nothing is stored yet: a first write sends 0. */
export type SpaceToolPolicyLayer = {
  readonly layer: string
  readonly agentSubjectId?: string
  readonly revision: number
  readonly entries: readonly SpaceToolPolicyEntry[]
}

export type SpaceMergedToolLevel = {
  readonly level: SpaceToolLevel
  readonly source: SpaceToolSource
}

export type SpaceMergedToolPolicy = {
  /** Every key some layer names, including `*`. */
  readonly tools: Readonly<Record<string, SpaceMergedToolLevel>>
  readonly ceilingRevision: number
  readonly agentRevision: number
  readonly digest: string
}

export type SpaceToolPolicyView = {
  readonly ceiling: SpaceToolPolicyLayer
  readonly agents: readonly SpaceToolPolicyLayer[]
  readonly merged: SpaceMergedToolPolicy
}

export type SpaceToolPolicyWrite = {
  readonly policy: SpaceToolPolicyLayer
  readonly merged: SpaceMergedToolPolicy
  /** Whether Application's binding projection caught up; Control's write stands either way. */
  readonly projection?: 'synced' | 'pending'
}

type RawEntry = { tool_key?: unknown; level?: unknown }
type RawLayer = { layer?: unknown; agent_subject_id?: unknown; revision?: unknown; entries?: unknown }
type RawMerged = {
  tools?: unknown
  ceiling_revision?: unknown
  agent_revision?: unknown
  digest?: unknown
}
type RawWrite = { policy?: RawLayer; merged?: RawMerged; projection?: unknown }

function level(value: unknown): SpaceToolLevel {
  // An unknown level is read as the strictest one: never show a looser policy than was stored.
  return value === 'allow' || value === 'ask' || value === 'block' ? value : 'block'
}

function source(value: unknown): SpaceToolSource {
  return value === 'space_ceiling' || value === 'agent' ? value : 'default'
}

function normalizeLayer(raw: RawLayer | null | undefined, fallbackLayer: string): SpaceToolPolicyLayer {
  const entries = Array.isArray(raw?.entries) ? (raw?.entries as RawEntry[]) : []
  return {
    layer: typeof raw?.layer === 'string' ? raw.layer : fallbackLayer,
    agentSubjectId: typeof raw?.agent_subject_id === 'string' && raw.agent_subject_id ? raw.agent_subject_id : undefined,
    revision: typeof raw?.revision === 'number' ? raw.revision : 0,
    entries: entries
      .filter((entry) => typeof entry?.tool_key === 'string' && entry.tool_key)
      .map((entry) => ({ toolKey: entry.tool_key as string, level: level(entry.level) })),
  }
}

function normalizeMerged(raw: RawMerged | null | undefined): SpaceMergedToolPolicy {
  const tools: Record<string, SpaceMergedToolLevel> = {}
  if (raw?.tools && typeof raw.tools === 'object') {
    for (const [key, value] of Object.entries(raw.tools as Record<string, { level?: unknown; source?: unknown }>)) {
      tools[key] = { level: level(value?.level), source: source(value?.source) }
    }
  }
  return {
    tools,
    ceilingRevision: typeof raw?.ceiling_revision === 'number' ? raw.ceiling_revision : 0,
    agentRevision: typeof raw?.agent_revision === 'number' ? raw.agent_revision : 0,
    digest: typeof raw?.digest === 'string' ? raw.digest : '',
  }
}

/**
 * Control's stored Space ceiling and agent layers. With `agentSubjectId` the `merged` view is for
 * that agent (the level per tool and the layer that decided it); without it, for the ceiling alone.
 */
export async function getSpaceToolPolicy(
  spaceRef: string,
  agentSubjectId?: string,
): Promise<SpaceToolPolicyView> {
  const query = agentSubjectId ? `?agent_subject_id=${encodeURIComponent(agentSubjectId)}` : ''
  const response = await requestJson<{
    ceiling?: RawLayer
    agents?: RawLayer[] | null
    merged?: RawMerged
  }>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/tool-policy${query}`)
  return {
    ceiling: normalizeLayer(response.ceiling, 'space_ceiling'),
    agents: (response.agents ?? []).map((layer) => normalizeLayer(layer, 'agent')),
    merged: normalizeMerged(response.merged),
  }
}

function writeBody(entries: readonly SpaceToolPolicyEntry[], expectedRevision: number): string {
  return JSON.stringify({
    entries: entries.map((entry) => ({ tool_key: entry.toolKey, level: entry.level })),
    expected_revision: expectedRevision,
  })
}

function normalizeWrite(response: RawWrite): SpaceToolPolicyWrite {
  return {
    policy: normalizeLayer(response.policy, 'space_ceiling'),
    merged: normalizeMerged(response.merged),
    projection: response.projection === 'synced' || response.projection === 'pending' ? response.projection : undefined,
  }
}

/** Replace the Space ceiling (every agent of the room). A stale revision answers 409. */
export async function setSpaceToolPolicy(
  spaceRef: string,
  entries: readonly SpaceToolPolicyEntry[],
  expectedRevision: number,
): Promise<SpaceToolPolicyWrite> {
  const response = await requestJson<RawWrite>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/tool-policy`, {
    method: 'PUT',
    body: writeBody(entries, expectedRevision),
  })
  return normalizeWrite(response)
}

/** Replace one bound agent's layer. A stale revision answers 409. */
export async function setSpaceAgentToolPolicy(
  spaceRef: string,
  bindingRef: string,
  entries: readonly SpaceToolPolicyEntry[],
  expectedRevision: number,
): Promise<SpaceToolPolicyWrite> {
  const response = await requestJson<RawWrite>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/agents/${encodeURIComponent(bindingRef)}/tool-policy`,
    { method: 'PUT', body: writeBody(entries, expectedRevision) },
  )
  return normalizeWrite(response)
}
