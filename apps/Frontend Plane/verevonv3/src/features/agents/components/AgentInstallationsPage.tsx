import { ArrowLeft, Puzzle } from 'lucide-solid'
import { createResource, For, Show } from 'solid-js'
import { A } from '@solidjs/router'

import { getAgentInstallations, type AgentInstallation } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'

/**
 * Scope plan §UI-4, narrow slice: a real, honest cross-Space view of the
 * org's agent definitions — "your view of your definitions," not the full
 * Blueprints/Definitions/Page-installations/Runs/Chief-Core restructure the
 * plan describes, which needs backend contracts (an org-wide registry read,
 * page/system installation storage) that do not exist yet. Every status
 * shown here is the SAME live Control-joined status a room's own Agent tab
 * would show for that binding — this page composes existing per-Space reads,
 * it does not keep a second cache that could drift from them.
 */
export default function AgentInstallationsPage() {
  const i18n = useI18n()
  const [installations] = createResource(getAgentInstallations)

  return (
    <div class="verevon-agent-installations">
      <header class="verevon-agent-installations__topbar">
        <A href="/agents" class="verevon-agent-installations__back" aria-label={i18n.tr('Tilbake til agenter', 'Back to agents')}>
          <ArrowLeft size={16} />
        </A>
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Agenter', 'Agents')}</p>
          <h1>{i18n.tr('Installasjoner', 'Installations')}</h1>
        </div>
      </header>

      <p class="verevon-agent-installations__intro">
        {i18n.tr(
          'Agentene dine og hvilke rom de er lagt til i. Statusen for hver installasjon er den samme som rommets egen Agent-fane viser.',
          'Your agents and which rooms each one has been added to. Each installation\'s status is the same one that room\'s own Agent tab shows.',
        )}
      </p>

      <Show when={installations.loading}>
        <p class="verevon-space-inline-status" role="status">{i18n.tr('Henter installasjoner …', 'Loading installations…')}</p>
      </Show>

      <Show when={installations.error}>
        <p class="verevon-space-projection-error" role="alert">
          {i18n.tr(
            'Installasjonene kunne ikke hentes akkurat nå.',
            'Installations could not be loaded right now.',
          )}
        </p>
      </Show>

      <Show when={installations.error ? undefined : installations()}>
        {(definitions) => (
          <Show
            when={definitions().length > 0}
            fallback={
              <p>{i18n.tr(
                'Ingen agenter er publisert i noen av rommene dine ennå. Opprett eller legg til en agent fra et roms Agent-fane.',
                'No agents are published in any of your rooms yet. Create or add an agent from a room\'s Agent tab.',
              )}</p>
            }
          >
            <ul class="verevon-agent-installations__list">
              <For each={definitions()}>
                {(definition) => (
                  <li class="verevon-agent-installations__card">
                    <div class="verevon-agent-installations__card-header">
                      <span class="verevon-agent-installations__icon" aria-hidden="true"><Puzzle size={16} /></span>
                      <div>
                        <strong>{definition.name?.trim() || i18n.tr('Agent uten navn', 'Agent with no name')}</strong>
                        <Show when={definition.description?.trim()}>
                          {(description) => <p>{description()}</p>}
                        </Show>
                      </div>
                      <span class="verevon-agent-installations__count">
                        {i18n.tr(
                          `Lagt til i ${definition.installations.length} rom`,
                          `Added to ${definition.installations.length} room${definition.installations.length === 1 ? '' : 's'}`,
                        )}
                      </span>
                    </div>
                    <ul class="verevon-agent-installations__rooms">
                      <For each={definition.installations}>
                        {(installation) => <InstallationRow installation={installation} />}
                      </For>
                    </ul>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        )}
      </Show>
    </div>
  )
}

function InstallationRow(props: { readonly installation: AgentInstallation }) {
  const i18n = useI18n()
  const statusLabel = () => {
    switch (props.installation.status) {
      case 'active': return i18n.tr('Aktiv', 'Active')
      case 'pending': return i18n.tr('Venter på bekreftelse', 'Awaiting confirmation')
      case 'paused': return i18n.tr('Pauset', 'Paused')
      case 'revoked': return i18n.tr('Tilbakekalt', 'Revoked')
      case 'failed': return i18n.tr('Feilet', 'Failed')
      default: return i18n.tr('Ukjent', 'Unknown')
    }
  }

  return (
    <li class="verevon-agent-installations__room">
      <A href={`/spaces/${encodeURIComponent(props.installation.space_ref)}`}>
        {props.installation.space_name}
      </A>
      <span
        classList={{
          'verevon-agent-installations__status': true,
          'verevon-agent-installations__status--active': props.installation.status === 'active',
        }}
      >
        {statusLabel()}
      </span>
    </li>
  )
}
