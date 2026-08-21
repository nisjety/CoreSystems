import { createSignal, For, Show } from 'solid-js'

import { Check, Puzzle, X } from '@/shared/icons'

import { bindSpaceAgent, getInstallableSpaceAgents, type InstallableSpaceAgent } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'

/**
 * Bind an EXISTING org agent definition to this room (scope plan §UI-3) — the
 * counterpart to `SpaceCreateAgentDialog`'s "author a new one" flow. The list
 * comes from the org's Agent Studio definitions; picking one runs the same
 * governed two-step as creation, just skipping authorship. `already_bound` is
 * shown, not hidden — the list tells the truth about the room's state instead
 * of silently omitting a definition someone already added.
 */

export interface SpaceBindAgentDialogProps {
  readonly spaceRef: string
  readonly open: () => boolean
  readonly onClose: () => void
  /** Called after Control has confirmed the bound agent's room membership. */
  readonly onBound?: () => void
}

export function SpaceBindAgentDialog(props: SpaceBindAgentDialogProps) {
  const i18n = useI18n()
  const [installable, { refetch }] = createResource(
    () => (props.open() ? props.spaceRef : undefined),
    getInstallableSpaceAgents,
  )
  const [pendingRef, setPendingRef] = createSignal<string | undefined>(undefined)
  const [failedRef, setFailedRef] = createSignal<string | undefined>(undefined)

  function close(): void {
    props.onClose()
  }

  async function bind(agent: InstallableSpaceAgent): Promise<void> {
    if (agent.already_bound || pendingRef()) return
    setFailedRef(undefined)
    setPendingRef(agent.agent_ref)
    try {
      await bindSpaceAgent(props.spaceRef, agent.agent_ref)
      props.onBound?.()
      void refetch()
    } catch {
      // The gateway's own vocabulary for the honest partial state: the
      // binding may exist as `pending` even when this request errors.
      setFailedRef(agent.agent_ref)
    } finally {
      setPendingRef(undefined)
    }
  }

  return (
    <Show when={props.open()}>
      <div class="verevon-space-create-agent" role="presentation" onClick={(event) => { if (event.target === event.currentTarget) close() }}>
        <section
          class="verevon-space-create-agent__dialog"
          role="dialog"
          aria-modal="true"
          aria-labelledby="space-bind-agent-title"
        >
          <header>
            <h3 id="space-bind-agent-title">{i18n.tr('Legg til en eksisterende agent', 'Add an existing agent')}</h3>
            <button type="button" onClick={close} aria-label={i18n.tr('Lukk', 'Close')}>
              <X size={16} />
            </button>
          </header>

          <p class="verevon-space-create-agent__note">
            {i18n.tr(
              'Velg en agent fra organisasjonen. Den blir medlem av dette rommet med samme forsiktige policy som en nyopprettet romagent.',
              'Pick an agent from the organization. It becomes a member of this room with the same cautious policy as a freshly created room agent.',
            )}
          </p>

          <Show when={installable.loading}>
            <p class="verevon-space-inline-status" role="status">{i18n.tr('Henter agenter …', 'Loading agents…')}</p>
          </Show>

          <Show when={installable.error}>
            <p class="verevon-space-projection-error" role="alert">
              {i18n.tr(
                'Organisasjonens agenter kunne ikke hentes.',
                'The organization\'s agents could not be loaded.',
              )}
            </p>
          </Show>

          <Show when={installable.error ? undefined : installable()}>
            {(list) => (
              <Show
                when={list().length > 0}
                fallback={
                  <p>{i18n.tr(
                    'Organisasjonen har ingen andre agenter ennå.',
                    'The organization has no other agents yet.',
                  )}</p>
                }
              >
                <ul class="verevon-space-bind-agent__list">
                  <For each={list()}>
                    {(agent) => (
                      <li class="verevon-space-bind-agent__row">
                        <span class="verevon-space-bind-agent__icon" aria-hidden="true"><Puzzle size={16} /></span>
                        <div class="verevon-space-bind-agent__body">
                          <strong>{agent.name?.trim() || i18n.tr('Agent uten navn', 'Agent with no name')}</strong>
                          <Show when={agent.description?.trim()}>
                            {(description) => <small>{description()}</small>}
                          </Show>
                        </div>
                        <Show
                          when={!agent.already_bound}
                          fallback={
                            <span class="verevon-space-bind-agent__status">
                              <Check size={14} aria-hidden="true" />
                              {i18n.tr('Allerede lagt til', 'Already added')}
                            </span>
                          }
                        >
                          <button
                            type="button"
                            class="verevon-space-secondary-action"
                            disabled={pendingRef() === agent.agent_ref}
                            onClick={() => void bind(agent)}
                          >
                            {pendingRef() === agent.agent_ref
                              ? i18n.tr('Legger til …', 'Adding…')
                              : i18n.tr('Legg til', 'Add')}
                          </button>
                        </Show>
                        <Show when={failedRef() === agent.agent_ref}>
                          <p class="verevon-space-projection-error" role="alert">
                            {i18n.tr(
                              'Kunne ikke bekreftes som medlem av rommet ennå. Sjekk Agent-fanen.',
                              'Could not be confirmed as a room member yet. Check the Agent tab.',
                            )}
                          </p>
                        </Show>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            )}
          </Show>
        </section>
      </div>
    </Show>
  )
}
