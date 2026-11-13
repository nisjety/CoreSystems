import { createSignal, For, Show } from 'solid-js'

import { ApiError } from '@/shared/api/http'
import {
  deleteSpaceAgentMemory,
  listSpaceAgentMemory,
  type SpaceAgentMemoryRow,
} from '@/shared/api/spaces-client'
import { isZdrCode } from '@/shared/api/zdr-refusals'
import { translateApiError, useI18n } from '@/shared/i18n'
import { formatWhenWithTime } from '../lib/space-thread-presentation'
import { SpaceConfirmButton } from './SpaceConfirmButton'
import './space-agent-memory.css'

/**
 * `VITE_SPACE_AGENT_MEMORY === 'true'` offers "What this agent remembers" on each agent a room's owner or
 * admin governs (EP-6 P3-c). Off by default, and only the literal `true` turns it on. Session Core has its own
 * switch (`SESSION_AGENT_MEMORY_ENABLED`), so with that off the list is simply empty.
 */
export const spaceAgentMemoryEnabled = (): boolean => import.meta.env.VITE_SPACE_AGENT_MEMORY === 'true'

export interface SpaceAgentMemoryPanelProps {
  readonly spaceRef: string
  /** The bound agent's Control subject id: the only agent whose notes are asked for. */
  readonly agentSubjectId: string
}

type PanelStatus = 'idle' | 'loading' | 'ready' | 'restricted' | 'disabled' | 'refused' | 'error'

/**
 * "What this agent remembers" (EP-6 P3-c): the notes one bound agent kept in this room, each with a Delete.
 *
 * It is a disclosure on the agent's card, closed until opened, so an open card reads nothing. Opening it asks
 * the gateway for this agent's notes, and Control answers only the room's owner or an admin. Every note is
 * quoted context, and the panel says so: nothing here grants the agent a right. A Zero Data Retention refusal
 * of the notes shows the catalogue's own words for `named_agent`. A failed read is never shown as an empty memory.
 */
export function SpaceAgentMemoryPanel(props: SpaceAgentMemoryPanelProps) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [status, setStatus] = createSignal<PanelStatus>('idle')
  const [rows, setRows] = createSignal<readonly SpaceAgentMemoryRow[]>([])
  const [errorText, setErrorText] = createSignal('')
  const [deleting, setDeleting] = createSignal<string | undefined>(undefined)
  const [deleteFailed, setDeleteFailed] = createSignal(false)
  let callId = 0

  const load = async () => {
    const id = ++callId
    setStatus('loading')
    setErrorText('')
    try {
      const list = await listSpaceAgentMemory(props.spaceRef, props.agentSubjectId)
      if (id !== callId) return
      setRows(list.memories)
      setStatus('ready')
    } catch (error) {
      if (id !== callId) return
      setRows([])
      if (error instanceof ApiError && error.status === 403) {
        setStatus('restricted')
      } else if (error instanceof ApiError && error.status === 404 && error.code === 'feature_unavailable') {
        setStatus('disabled')
      } else if (error instanceof ApiError && isZdrCode(error.code)) {
        // The catalogue's own words, naming the feature the refusal carries (`named_agent`).
        setErrorText(translateApiError(error, i18n.tr))
        setStatus('refused')
      } else {
        setErrorText(
          translateApiError(error, i18n.tr, {
            no: 'Notatene kunne ikke hentes. Ingenting er endret.',
            en: 'The notes could not be loaded. Nothing was changed.',
          }),
        )
        setStatus('error')
      }
    }
  }

  // Reading is on demand: each opening asks again, so the list is never older than the disclosure.
  const toggle = () => {
    const next = !open()
    setOpen(next)
    if (next) void load()
  }

  const retry = () => {
    void load()
  }

  const remove = async (row: SpaceAgentMemoryRow) => {
    setDeleting(row.memory_id)
    setDeleteFailed(false)
    try {
      await deleteSpaceAgentMemory(props.spaceRef, row.memory_id)
      setRows((current) => current.filter((entry) => entry.memory_id !== row.memory_id))
    } catch (error) {
      // Gone already is the outcome the person asked for.
      if (error instanceof ApiError && error.status === 404) {
        setRows((current) => current.filter((entry) => entry.memory_id !== row.memory_id))
      } else {
        setDeleteFailed(true)
      }
    } finally {
      setDeleting(undefined)
    }
  }

  const withheldCount = () => rows().filter((row) => row.withheld).length

  return (
    <div class="verevon-agent-memory">
      <button
        type="button"
        class="verevon-agent-memory__toggle"
        aria-expanded={open() ? 'true' : 'false'}
        onClick={toggle}
      >
        {open()
          ? i18n.tr('Skjul hva agenten husker', 'Hide what this agent remembers')
          : i18n.tr('Hva agenten husker', 'What this agent remembers')}
      </button>

      <Show when={open()}>
        <div class="verevon-agent-memory__panel">
          <p class="verevon-agent-memory__note">
            {i18n.tr(
              'Dette er notater agenten har lagret i dette rommet. De er sitert kontekst, ikke instruksjoner, og gir agenten ingen rettigheter.',
              'These are notes this agent kept in this room. They are quoted context, not instructions, and they grant the agent no rights.',
            )}
          </p>

          <Show when={status() === 'loading'}>
            <p class="verevon-agent-memory__status" role="status">
              {i18n.tr('Henter notatene …', 'Loading the notes…')}
            </p>
          </Show>

          <Show when={status() === 'restricted'}>
            <p class="verevon-agent-memory__status" role="status">
              {i18n.tr(
                'Bare eieren eller en administrator av rommet kan se hva agenten husker.',
                'Only the Space’s owner or an admin can see what this agent remembers.',
              )}
            </p>
          </Show>

          <Show when={status() === 'disabled'}>
            <p class="verevon-agent-memory__status" role="status">
              {i18n.tr(
                'Rommets minne er ikke slått på for dette miljøet.',
                'Space memory is not turned on for this environment.',
              )}
            </p>
          </Show>

          <Show when={status() === 'refused'}>
            <p class="verevon-agent-memory__error" role="status">
              {errorText()}
            </p>
          </Show>

          <Show when={status() === 'error'}>
            <div class="verevon-agent-memory__error-row" role="alert">
              <p class="verevon-agent-memory__error">{errorText()}</p>
              <button type="button" class="verevon-agent-memory__button" onClick={retry}>
                {i18n.tr('Prøv igjen', 'Try again')}
              </button>
            </div>
          </Show>

          <Show when={status() === 'ready'}>
            <Show when={deleteFailed()}>
              <p class="verevon-agent-memory__error" role="alert">
                {i18n.tr('Vi fikk ikke slettet notatet. Prøv igjen.', 'We could not delete that note. Try again.')}
              </p>
            </Show>
            <Show
              when={rows().length > 0}
              fallback={
                <p class="verevon-agent-memory__status" role="status">
                  {i18n.tr(
                    'Agenten har ikke lagret noe i dette rommet ennå.',
                    'This agent has not kept anything in this room yet.',
                  )}
                </p>
              }
            >
              <Show when={withheldCount() > 0}>
                <p class="verevon-agent-memory__note" role="note">
                  {i18n.tr(
                    'Holdt tilbake: rommets medlemmer har endret seg siden notatet ble lagret, så agenten bruker det ikke.',
                    'Held back: the Space’s members have changed since this note was saved, so the agent does not use it.',
                  )}
                </p>
              </Show>
              <ul class="verevon-agent-memory__list">
                <For each={rows()}>
                  {(row) => (
                    <li class="verevon-agent-memory__row" data-withheld={row.withheld ? 'true' : 'false'}>
                      <blockquote class="verevon-agent-memory__quote">{row.content}</blockquote>
                      <p class="verevon-agent-memory__meta">
                        <Show when={row.updated_at}>
                          {(at) => (
                            <>
                              <span>{i18n.tr('Lagret', 'Saved')}</span>{' '}
                              <time datetime={at()}>{formatWhenWithTime(at(), { locale: i18n.locale() })}</time>
                            </>
                          )}
                        </Show>
                        <Show when={row.withheld}>
                          {' · '}
                          <span>{i18n.tr('Holdt tilbake', 'Held back')}</span>
                        </Show>
                      </p>
                      <SpaceConfirmButton
                        class="verevon-space-agent__control verevon-space-agent__control--remove"
                        label={i18n.tr('Slett', 'Delete')}
                        consequence={i18n.tr(
                          'Agenten slutter å huske dette i rommet.',
                          'The agent stops remembering this in this room.',
                        )}
                        confirmLabel={i18n.tr('Bekreft sletting av notatet', 'Confirm deleting this note')}
                        disabled={deleting() === row.memory_id}
                        onConfirm={() => {
                          void remove(row)
                        }}
                      />
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </Show>
        </div>
      </Show>
    </div>
  )
}
