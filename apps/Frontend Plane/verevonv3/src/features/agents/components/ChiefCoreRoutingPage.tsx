import { ArrowLeft, Compass } from 'lucide-solid'
import { createMemo, createResource, For, Show } from 'solid-js'
import { A } from '@solidjs/router'

import { getAgentInstallations } from '@/shared/api/spaces-client'
import { groupInstallationsBySpace, type SpaceRoutingEntry } from '@/features/agents/lib/chief-core-routing'
import { useI18n } from '@/shared/i18n'

/**
 * Scope plan §UI-4, "Chief/Core agent — cross-Space routing and discovery":
 * an org-wide view of which agent definitions are bound into which Spaces,
 * organized by destination Space rather than by definition (that framing is
 * `AgentInstallationsPage`'s job). Backed by the same ADR-0002 registry
 * (`apps/CROSS_SPACE_AGENT_REGISTRY_ADR_2026-08-19.md`) via
 * `getAgentInstallations()` — no new backend contract, just a different
 * client-side projection of the same read.
 *
 * This is a human-facing discovery/routing surface, not an invokable agent:
 * there is no "Chief" or "Core" persona anywhere in the runtime, and this
 * page cannot dispatch or act on anyone's behalf. "Routes and assigns"
 * (scope plan line 832) means it helps a person decide where to route a
 * task by showing them what already exists in each Space; the actual act of
 * routing is following the link into that Space and using its own Agent
 * tab, which still resolves Control's live roster before anything runs —
 * unchanged, per ADR-0002's "presence, not authority."
 */
export default function ChiefCoreRoutingPage() {
  const i18n = useI18n()
  const [installations] = createResource(getAgentInstallations)
  // installations() re-throws once the resource has errored, so the error
  // must be checked first — reading it unconditionally here would turn a
  // load failure into an uncaught rejection instead of the error Show below.
  const spaces = createMemo(() => (installations.error ? [] : groupInstallationsBySpace(installations() ?? [])))

  return (
    <div class="verevon-agent-installations">
      <header class="verevon-agent-installations__topbar">
        <A href="/agents" class="verevon-agent-installations__back" aria-label={i18n.tr('Tilbake til agenter', 'Back to agents')}>
          <ArrowLeft size={16} />
        </A>
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Agenter', 'Agents')}</p>
          <h1>{i18n.tr('Chief/Core', 'Chief/Core')}</h1>
        </div>
      </header>

      <p class="verevon-agent-installations__intro">
        {i18n.tr(
          'En oversikt på tvers av rom over hvilke agenter som allerede finnes hvor — til hjelp når du skal bestemme hvor en oppgave hører hjemme. Dette er ikke en agent du kan snakke med; klikk et rom for å åpne det og bruke dets egen Agent-fane.',
          'A cross-room view of which agents already exist where, to help decide where a task belongs. This is not an agent you can talk to; click a room to open it and use its own Agent tab.',
        )}
      </p>

      <Show when={installations.loading}>
        <p class="verevon-space-inline-status" role="status">{i18n.tr('Henter oversikten …', 'Loading the overview…')}</p>
      </Show>

      <Show when={installations.error}>
        <p class="verevon-space-projection-error" role="alert">
          {i18n.tr(
            'Oversikten kunne ikke hentes akkurat nå.',
            'The overview could not be loaded right now.',
          )}
        </p>
      </Show>

      <Show when={installations.error ? undefined : installations()}>
        <Show
          when={spaces().length > 0}
          fallback={
            <p>{i18n.tr(
              'Ingen agenter er publisert i noen av rommene dine ennå.',
              'No agents are published in any of your rooms yet.',
            )}</p>
          }
        >
          <ul class="verevon-agent-installations__list">
            <For each={spaces()}>
              {(space) => <SpaceRoutingCard space={space} />}
            </For>
          </ul>
        </Show>
      </Show>
    </div>
  )
}

function SpaceRoutingCard(props: { readonly space: SpaceRoutingEntry }) {
  const i18n = useI18n()
  const statusLabel = (status: string | undefined) => {
    switch (status) {
      case 'active': return i18n.tr('Aktiv', 'Active')
      case 'pending': return i18n.tr('Venter på bekreftelse', 'Awaiting confirmation')
      case 'paused': return i18n.tr('Pauset', 'Paused')
      case 'revoked': return i18n.tr('Tilbakekalt', 'Revoked')
      case 'failed': return i18n.tr('Feilet', 'Failed')
      default: return i18n.tr('Ukjent', 'Unknown')
    }
  }

  return (
    <li class="verevon-agent-installations__card">
      <div class="verevon-agent-installations__card-header">
        <span class="verevon-agent-installations__icon" aria-hidden="true"><Compass size={16} /></span>
        <div>
          <A href={`/spaces/${encodeURIComponent(props.space.space_ref)}`}>
            <strong>{props.space.space_name}</strong>
          </A>
        </div>
        <span class="verevon-agent-installations__count">
          {i18n.tr(
            `${props.space.agents.length} agent${props.space.agents.length === 1 ? '' : 'er'}`,
            `${props.space.agents.length} agent${props.space.agents.length === 1 ? '' : 's'}`,
          )}
        </span>
      </div>
      <ul class="verevon-agent-installations__rooms">
        <For each={props.space.agents}>
          {(agent) => (
            <li class="verevon-agent-installations__room">
              <span>{agent.name?.trim() || i18n.tr('Agent uten navn', 'Agent with no name')}</span>
              <span
                classList={{
                  'verevon-agent-installations__status': true,
                  'verevon-agent-installations__status--active': agent.status === 'active',
                }}
              >
                {statusLabel(agent.status)}
              </span>
            </li>
          )}
        </For>
      </ul>
    </li>
  )
}
