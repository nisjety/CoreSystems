import { requestJson } from './http'

// W3-DEC-ORGAGENT (owner decision D19): org-admin client for the org-visible agents. Control
// (user-core) owns the agent rows, the canonical owner-or-admin check, the room cascade and the
// audit row; the gateway only proxies (apps/gateway/src/domains/orgs/org_agents.rs). Nothing here
// decides who may act: every refusal is shown as the server worded it. The org is the path
// segment. The revision the admin read is the only value sent in a body, so a stale screen is
// refused instead of overwriting a newer change.

/** One org-visible agent definition, as the org-admin list shows it. Ids and states only. */
export interface OrgAgentView {
  readonly agent_subject_id: string
  readonly owner_user_id?: string
  readonly visibility: string
  readonly add_policy: string
  readonly lifecycle: string
  readonly suspended_reason?: string
  readonly revision: number
}

/** The agent after a lifecycle change. `changed` is false when it already had that state. */
export interface OrgAgentChangeResult {
  readonly agent_subject_id: string
  readonly lifecycle: string
  readonly revision: number
  readonly changed: boolean
  readonly rooms_changed: number
}

/** Control answers this when the agent changed since the admin read it: reload, then act again. */
export const ORG_AGENT_STATE_CHANGED = 'agent_state_changed'

const base = (orgId: string) => `/api/v1/orgs/${encodeURIComponent(orgId)}/agents`

const agentPath = (orgId: string, agentSubjectId: string) =>
  `${base(orgId)}/${encodeURIComponent(agentSubjectId)}`

export async function listOrgAgents(orgId: string): Promise<OrgAgentView[]> {
  const data = await requestJson<{ agents: OrgAgentView[] }>(base(orgId))
  return data.agents
}

export function suspendOrgAgent(
  orgId: string,
  agentSubjectId: string,
  expectedRevision: number,
): Promise<OrgAgentChangeResult> {
  return requestJson<OrgAgentChangeResult>(`${agentPath(orgId, agentSubjectId)}/suspend`, {
    method: 'PUT',
    body: JSON.stringify({ expected_revision: expectedRevision }),
  })
}

export function reactivateOrgAgent(
  orgId: string,
  agentSubjectId: string,
  expectedRevision: number,
): Promise<OrgAgentChangeResult> {
  return requestJson<OrgAgentChangeResult>(`${agentPath(orgId, agentSubjectId)}/reactivate`, {
    method: 'PUT',
    body: JSON.stringify({ expected_revision: expectedRevision }),
  })
}

/** Retire is a soft delete: the agent is withdrawn from every room and kept for audit. */
export function retireOrgAgent(
  orgId: string,
  agentSubjectId: string,
  expectedRevision: number,
): Promise<OrgAgentChangeResult> {
  return requestJson<OrgAgentChangeResult>(agentPath(orgId, agentSubjectId), {
    method: 'DELETE',
    body: JSON.stringify({ expected_revision: expectedRevision }),
  })
}
