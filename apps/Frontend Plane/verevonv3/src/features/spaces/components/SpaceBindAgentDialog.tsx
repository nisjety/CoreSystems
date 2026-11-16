import { createSignal, For, Show } from 'solid-js'

import { Check, Puzzle, X } from '@/shared/icons'

import {
  agentNotAddableReason,
  bindSpaceAgent,
  getInstallableSpaceAgents,
  isAgentAddRefusalReason,
  type InstallableSpaceAgent,
  type SpaceAgentAddState,
} from '@/shared/api/spaces-client'
import { hasErrorCode } from '@/shared/api/api-error'
import { ERROR_COPY } from '@/shared/i18n/errors'
import { translateApiError, useI18n } from '@/shared/i18n'
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

/**
 * The row's state from Control's admission answer (WS17-S5). An older gateway sends no `add_state`,
 * and then `already_bound` alone decides.
 */
function rowState(agent: InstallableSpaceAgent): SpaceAgentAddState {
  return agent.add_state ?? (agent.already_bound ? 'already_bound' : 'addable')
}

/** The sentence for a reason a row or a refused bind names, from errors.ts; an unknown reason gets the generic line. */
function refusalCopy(reason: string | null | undefined, tr: (noText: string, enText: string) => string): string {
  const copy = ERROR_COPY[isAgentAddRefusalReason(reason) ? reason : 'agent_not_addable']
  return tr(copy.no, copy.en)
}

export function SpaceBindAgentDialog(props: SpaceBindAgentDialogProps) {
  const i18n = useI18n()
  const [installable, { refetch }] = createResource(
    () => (props.open() ? props.spaceRef : undefined),
    getInstallableSpaceAgents,
  )
  const [pendingRef, setPendingRef] = createSignal<string | undefined>(undefined)
  const [failed, setFailed] = createSignal<{ readonly ref: string; readonly message: string } | undefined>(undefined)

  function close(): void {
    props.onClose()
  }

  async function bind(agent: InstallableSpaceAgent): Promise<void> {
    if (rowState(agent) !== 'addable' || pendingRef()) return
    setFailed(undefined)
    setPendingRef(agent.agent_ref)
    try {
      await bindSpaceAgent(props.spaceRef, agent.agent_ref)
      props.onBound?.()
      void refetch()
    } catch (error) {
      // A refusal names its own reason (403 agent_not_addable). Anything else keeps the honest partial
      // state: the binding may exist as `pending` even when this request errors.
      const message = hasErrorCode(error, 'agent_not_addable')
        ? refusalCopy(agentNotAddableReason(error), i18n.tr)
        : translateApiError(error, i18n.tr, {
            no: 'Kunne ikke bekreftes som medlem av rommet ennå. Sjekk Agent-fanen.',
            en: 'Could not be confirmed as a room member yet. Check the Agent tab.',
          })
      setFailed({ ref: agent.agent_ref, message })
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
                          <Show when={agent.owner_label?.trim()}>
                            {(owner) => <small>{i18n.tr(`Eier: ${owner()}`, `Owner: ${owner()}`)}</small>}
                          </Show>
                          <Show when={rowState(agent) === 'blocked'}>
                            <small class="verevon-space-bind-agent__reason">
                              {refusalCopy(agent.reason, i18n.tr)}
                            </small>
                          </Show>
                        </div>
                        <Show
                          when={rowState(agent) !== 'already_bound'}
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
                            disabled={rowState(agent) === 'blocked' || pendingRef() === agent.agent_ref}
                            onClick={() => void bind(agent)}
                          >
                            {pendingRef() === agent.agent_ref
                              ? i18n.tr('Legger til …', 'Adding…')
                              : i18n.tr('Legg til', 'Add')}
                          </button>
                        </Show>
                        <Show when={failed()?.ref === agent.agent_ref}>
                          <p class="verevon-space-projection-error" role="alert">
                            {failed()?.message}
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
