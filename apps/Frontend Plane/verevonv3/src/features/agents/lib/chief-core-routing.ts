import type { AgentDefinitionInstallations } from '@/shared/api/spaces-client'

/**
 * One Space and every agent definition bound into it, for the Chief/Core
 * agent's cross-Space routing view (scope plan §UI-4, "Chief/Core agent —
 * cross-Space routing and discovery"; ADR-0002's registry supplies the
 * underlying data). This is the same org-wide registry
 * `AgentInstallationsPage` reads, re-keyed by destination Space instead of
 * by source definition — the natural shape for "what could I route a task
 * to in this Space" rather than "where is this agent installed."
 */
export type SpaceRoutingEntry = {
  space_ref: string
  space_name: string
  space_kind: 'personal' | 'room' | 'project' | 'case'
  agents: readonly {
    agent_ref: string
    name?: string
    status?: 'pending' | 'active' | 'paused' | 'revoked' | 'failed'
  }[]
}

/**
 * Pure re-projection of `getAgentInstallations()`'s definition-grouped
 * response into a Space-grouped one — no network or Convex access, so this
 * is unit-testable without mocking a resource. Sorted by Space name for a
 * stable, scan-friendly order (the source array's order is registry-insert
 * order, not meaningful to a reader).
 */
export function groupInstallationsBySpace(
  definitions: readonly AgentDefinitionInstallations[],
): readonly SpaceRoutingEntry[] {
  const bySpace = new Map<string, SpaceRoutingEntry>()

  for (const definition of definitions) {
    for (const installation of definition.installations) {
      const agentEntry = {
        agent_ref: definition.agent_ref,
        name: definition.name,
        status: installation.status,
      }
      const existing = bySpace.get(installation.space_ref)
      if (existing) {
        bySpace.set(installation.space_ref, { ...existing, agents: [...existing.agents, agentEntry] })
      } else {
        bySpace.set(installation.space_ref, {
          space_ref: installation.space_ref,
          space_name: installation.space_name,
          space_kind: installation.space_kind,
          agents: [agentEntry],
        })
      }
    }
  }

  return [...bySpace.values()].sort((a, b) => a.space_name.localeCompare(b.space_name))
}
