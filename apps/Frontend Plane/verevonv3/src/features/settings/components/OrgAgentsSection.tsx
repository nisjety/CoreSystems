import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  ORG_AGENT_STATE_CHANGED,
  listOrgAgents,
  reactivateOrgAgent,
  retireOrgAgent,
  suspendOrgAgent,
  type OrgAgentView,
} from '@/shared/api/org-agent-client'
import { ApiError } from '@/shared/api/http'
import { getAgentInstallations } from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SpaceConfirmButton } from '@/features/spaces/components/SpaceConfirmButton'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * Organization agents (W3-DEC-ORGAGENT, owner decision D19). An organization owner or admin lists
 * the org-visible agents and suspends, reactivates or retires one. Control owns the change, the
 * room cascade and the audit row. This section only draws it: `hasWorkspaceAdminAccess` decides what
 * to show, and the gateway and Control decide what is allowed. Every change carries the revision the
 * admin read, so a stale list is refused (agent_state_changed) and reloaded instead of overwritten.
 */

type Tr = (no: string, en: string) => string

type AgentAction = 'suspend' | 'reactivate' | 'retire'

export function OrgAgentsSection() {
  const i18n = useI18n()
  const tr: Tr = (no, en) => i18n.tr(no, en)
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))
  const askedOrg = () => (isAdmin() && orgId() ? orgId() : undefined)

  const [agents, { refetch }] = createResource(askedOrg, (id) => listOrgAgents(id))
  // The org agent list holds ids and lifecycle only: Control keeps no names. The names live with the agent definitions,
  // which the installations list carries for every agent a room holds, and `agent-<definition id>` is the subject id the
  // two lists share. The names are read once per visit, so a name stays on its row after a suspend or a retire takes the
  // agent out of every room (the later list would no longer name it). An agent no room held when the page opened shows
  // by its id alone. A failed read leaves ids only: a name is a help to the admin, never a reason to hide the list.
  const [installed] = createResource(askedOrg, async () => {
    try {
      return await getAgentInstallations()
    } catch {
      return []
    }
  })
  const names = createMemo<ReadonlyMap<string, string>>(() => {
    const byId = new Map<string, string>()
    for (const definition of installed() ?? []) {
      const name = definition.name?.trim()
      if (name) byId.set(`agent-${definition.agent_ref}`, name)
    }
    return byId
  })
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [notice, setNotice] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)

  const lifecycleLabel = (lifecycle: string): string => {
    switch (lifecycle) {
      case 'active':
        return tr('Aktiv', 'Active')
      case 'suspended':
        return tr('Suspendert', 'Suspended')
      case 'retired':
        return tr('Avsluttet', 'Retired')
      default:
        return lifecycle
    }
  }

  const doneNotice = (action: AgentAction): string => {
    switch (action) {
      case 'suspend':
        return tr('Agenten er suspendert og fjernet fra alle rom.', 'The agent is suspended and removed from every room.')
      case 'reactivate':
        return tr('Agenten er aktiv igjen. Rom må legge den til på nytt.', 'The agent is active again. Rooms must add it again.')
      case 'retire':
        return tr('Agenten er avsluttet og fjernet fra alle rom. Den beholdes for revisjon.', 'The agent is retired and removed from every room. It is kept for audit.')
    }
  }

  const run = async (agent: OrgAgentView, action: AgentAction) => {
    const org = orgId()
    if (busyId() || !org) return
    setBusyId(agent.agent_subject_id)
    setNotice(null)
    setActionError(null)
    try {
      const change =
        action === 'suspend' ? suspendOrgAgent : action === 'reactivate' ? reactivateOrgAgent : retireOrgAgent
      const result = await change(org, agent.agent_subject_id, agent.revision)
      setNotice(
        result.changed
          ? doneNotice(action)
          : tr('Ingen endring. Agenten har allerede denne tilstanden.', 'No change. The agent already has that state.'),
      )
      void refetch()
    } catch (err) {
      if (err instanceof ApiError && err.code === ORG_AGENT_STATE_CHANGED) {
        setActionError(
          tr(
            'Agenten er endret av noen andre. Listen er oppdatert; sjekk den og prøv igjen.',
            'Someone else changed this agent. The list has been refreshed; check it and try again.',
          ),
        )
        void refetch()
      } else {
        setActionError(
          translateApiError(err, tr, {
            no: 'Endringen ble ikke lagret.',
            en: 'The change was not saved.',
          }),
        )
      }
    } finally {
      setBusyId(null)
    }
  }

  return (
    <Show when={askedOrg()}>
      <section class="verevon-settings-section" aria-labelledby="org-agents-title">
        <SectionHeader
          title={tr('Organisasjonens agenter', 'Organization agents')}
          description={tr(
            'Agenter som er delt med hele organisasjonen. Eiere og administratorer kan suspendere, aktivere igjen eller avslutte dem. Avsluttede agenter beholdes for revisjon.',
            'Agents shared with the whole organization. Owners and admins can suspend, reactivate or retire them. Retired agents are kept for audit.',
          )}
        />
        <Show when={agents.error}>
          <p role="alert" class="verevon-settings-empty-row">
            {translateApiError(agents.error, tr, {
              no: 'Agentlisten kunne ikke lastes.',
              en: 'The agent list could not be loaded.',
            })}
          </p>
        </Show>
        <Show when={actionError()}>
          <p role="alert" class="verevon-settings-empty-row">{actionError()}</p>
        </Show>
        <Show when={notice()}>
          <p role="status" class="verevon-settings-empty-row">{notice()}</p>
        </Show>
        <Show when={!agents.loading} fallback={<p class="verevon-settings-empty-row">{tr('Laster agenter …', 'Loading agents …')}</p>}>
          <Show
            when={(agents() ?? []).length > 0}
            fallback={
              <p class="verevon-settings-empty-row">
                {tr('Ingen agenter er delt med organisasjonen ennå.', 'No agents are shared with the organization yet.')}
              </p>
            }
          >
            <ul class="verevon-settings-list">
              <For each={agents() ?? []}>
                {(agent) => (
                  <li class="verevon-settings-list-row">
                    <span>
                      <Show when={names().get(agent.agent_subject_id)}>
                        {(name) => <strong>{name()} </strong>}
                      </Show>
                      <code>{agent.agent_subject_id}</code>
                    </span>
                    <span>{lifecycleLabel(agent.lifecycle)}</span>
                    <Show when={agent.lifecycle === 'active'}>
                      <SettingsButton
                        type="button"
                        disabled={busyId() !== null}
                        onClick={() => void run(agent, 'suspend')}
                      >
                        {tr('Suspender', 'Suspend')}
                      </SettingsButton>
                    </Show>
                    <Show when={agent.lifecycle === 'suspended'}>
                      <SettingsButton
                        type="button"
                        disabled={busyId() !== null}
                        onClick={() => void run(agent, 'reactivate')}
                      >
                        {tr('Aktiver igjen', 'Reactivate')}
                      </SettingsButton>
                    </Show>
                    <Show when={agent.lifecycle === 'active' || agent.lifecycle === 'suspended'}>
                      <SpaceConfirmButton
                        label={tr('Avslutt', 'Retire')}
                        consequence={tr(
                          'Agenten fjernes fra alle rom og kan ikke aktiveres igjen.',
                          'The agent is removed from every room and cannot be reactivated.',
                        )}
                        confirmLabel={tr('Bekreft avslutning av agenten', 'Confirm retiring the agent')}
                        disabled={busyId() !== null}
                        onConfirm={() => void run(agent, 'retire')}
                      />
                    </Show>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </Show>
      </section>
    </Show>
  )
}
