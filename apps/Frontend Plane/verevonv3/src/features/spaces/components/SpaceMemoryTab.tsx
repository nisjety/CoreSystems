import { createEffect, createSignal, For, Show } from 'solid-js'

import { ApiError, requestJson } from '@/shared/api/http'
import { useI18n } from '@/shared/i18n'
import { SpaceConfirmButton } from './SpaceConfirmButton'

/**
 * The Space memory tab (EP-8 S7): what a shared room has learned, for the
 * Space's owner or admin to read and delete.
 *
 * # Who sees what
 *
 * The gateway asks Control for a `model.space.memory.manage` decision on every
 * read and every delete, and Control issues it to the owner or an admin only.
 * Anyone else gets `restricted` here; members still benefit from the memory
 * through recall, they just cannot list it. Nothing is cached in the browser.
 *
 * # Withheld rows
 *
 * A row learned before the room's audience last changed is listed (it is still
 * stored) but not recalled: a member added later does not hear what was said
 * before they joined. The count and each row say so, so an emptier recall is
 * explained rather than looking like lost memory.
 *
 * # States
 *
 * loading, empty, restricted, disabled (the gateway switch is off), error with
 * Retry. A failed read is never shown as an empty memory.
 */
export interface SpaceMemoryTabProps {
  readonly spaceRef: string
}

/** One row as the gateway returns it. */
export interface SpaceMemoryRow {
  readonly memory_id: string
  readonly content: string
  readonly updated_at: string
  readonly provenance: string
  readonly source_label: string
  readonly thread_id: string
  readonly withheld: boolean
}

export interface SpaceMemoryList {
  readonly memories: readonly SpaceMemoryRow[]
  readonly withheld: number
}

const memoryPath = (spaceRef: string) => `/api/v1/spaces/${encodeURIComponent(spaceRef)}/memory`

/** `GET /api/v1/spaces/{space_ref}/memory`. */
export function listSpaceMemory(spaceRef: string): Promise<SpaceMemoryList> {
  return requestJson<SpaceMemoryList>(memoryPath(spaceRef))
}

/** `DELETE /api/v1/spaces/{space_ref}/memory/{memory_id}`. */
export function deleteSpaceMemory(spaceRef: string, memoryId: string): Promise<{ deleted: boolean }> {
  return requestJson<{ deleted: boolean }>(`${memoryPath(spaceRef)}/${encodeURIComponent(memoryId)}`, {
    method: 'DELETE',
  })
}

/**
 * `VITE_SPACE_MEMORY_TAB === 'true'` mounts this tab in a shared room's details (EP-8 S7). Off by default,
 * and only the literal `true` turns it on. Presentation only: the gateway and Control decide every read and delete.
 */
export const spaceMemoryTabEnabled = (): boolean => import.meta.env.VITE_SPACE_MEMORY_TAB === 'true'

type TabStatus = 'loading' | 'ready' | 'restricted' | 'disabled' | 'error'

export function SpaceMemoryTab(props: SpaceMemoryTabProps) {
  const i18n = useI18n()
  const [status, setStatus] = createSignal<TabStatus>('loading')
  const [rows, setRows] = createSignal<readonly SpaceMemoryRow[]>([])
  const [withheld, setWithheld] = createSignal(0)
  const [deleting, setDeleting] = createSignal<string | undefined>(undefined)
  const [deleteFailed, setDeleteFailed] = createSignal(false)
  let callId = 0

  const statusForError = (error: unknown): TabStatus => {
    if (error instanceof ApiError && error.status === 403) return 'restricted'
    if (error instanceof ApiError && error.status === 404 && error.code === 'feature_unavailable') return 'disabled'
    return 'error'
  }

  const load = async (spaceRef: string) => {
    const id = ++callId
    if (rows().length === 0) setStatus('loading')
    try {
      const list = await listSpaceMemory(spaceRef)
      if (id !== callId) return
      setRows(list.memories)
      setWithheld(list.withheld)
      setStatus('ready')
    } catch (error) {
      if (id !== callId) return
      setRows([])
      setWithheld(0)
      setStatus(statusForError(error))
    }
  }

  createEffect(
    () => props.spaceRef,
    (spaceRef) => {
      if (spaceRef) void load(spaceRef)
    },
  )

  const remove = async (row: SpaceMemoryRow) => {
    setDeleting(row.memory_id)
    setDeleteFailed(false)
    try {
      await deleteSpaceMemory(props.spaceRef, row.memory_id)
      setRows((current) => current.filter((entry) => entry.memory_id !== row.memory_id))
      if (row.withheld) setWithheld((count) => Math.max(0, count - 1))
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

  const sourceLabel = (label: string) =>
    label === 'member_save'
      ? i18n.tr('Lagret av et medlem', 'Saved by a member')
      : i18n.tr('Sagt i rommet', 'Said in this Space')

  const threadHref = (threadId: string) =>
    `/spaces/${encodeURIComponent(props.spaceRef)}?thread_id=${encodeURIComponent(threadId)}`

  const retry = () => {
    void load(props.spaceRef)
  }

  return (
    <section class="verevon-space-view verevon-space-memory" aria-labelledby="space-memory-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Minne i rommet', 'Memory in this Space')}</p>
          <h2 id="space-memory-title">{i18n.tr('Rommets minne', 'Space memory')}</h2>
          <p>{i18n.tr(
            'Det rommet har bedt Verevon huske. Brukes bare i dette rommet, aldri i noens personlige minne.',
            'What this Space asked Verevon to remember. Used only in this Space, never in anyone’s personal memory.',
          )}</p>
        </div>
      </div>

      <Show when={status() === 'loading'}>
        <p class="verevon-space-inline-status" role="status">
          {i18n.tr('Henter rommets minne …', 'Loading this Space’s memory…')}
        </p>
      </Show>

      <Show when={status() === 'restricted'}>
        <p class="verevon-space-inline-status" role="status">
          {i18n.tr(
            'Bare eieren eller en administrator av rommet kan se og slette minnet.',
            'Only the Space’s owner or an admin can see and delete its memory.',
          )}
        </p>
      </Show>

      <Show when={status() === 'disabled'}>
        <p class="verevon-space-inline-status" role="status">
          {i18n.tr('Rommets minne er ikke slått på for dette miljøet.', 'Space memory is not turned on for this environment.')}
        </p>
      </Show>

      <Show when={status() === 'error'}>
        <div class="verevon-space-projection-error" role="alert">
          <p>{i18n.tr('Vi fikk ikke hentet rommets minne.', 'We could not load this Space’s memory.')}</p>
          <button type="button" class="verevon-space-secondary-action" onClick={retry}>
            {i18n.tr('Prøv igjen', 'Try again')}
          </button>
        </div>
      </Show>

      <Show when={status() === 'ready'}>
        <Show when={withheld() > 0}>
          <p class="verevon-space-memory__withheld" role="note" data-testid="space-memory-withheld">
            {i18n.tr(
              `${withheld()} ${withheld() === 1 ? 'minne er' : 'minner er'} holdt tilbake fordi rommets medlemmer har endret seg siden. Nye medlemmer får ikke høre det som ble sagt før de kom inn.`,
              `${withheld()} ${withheld() === 1 ? 'memory is' : 'memories are'} held back because the Space’s members have changed since. New members do not hear what was said before they joined.`,
            )}
          </p>
        </Show>
        <Show when={deleteFailed()}>
          <p class="verevon-space-projection-error" role="alert">
            {i18n.tr('Vi fikk ikke slettet minnet. Prøv igjen.', 'We could not delete that memory. Try again.')}
          </p>
        </Show>
        <Show
          when={rows().length > 0}
          fallback={
            <div class="verevon-space-empty-state" role="status">
              <div>
                <h3>{i18n.tr('Ingenting husket ennå', 'Nothing remembered yet')}</h3>
                <p>{i18n.tr(
                  'Skriv «husk at …» i rommet, så dukker det opp her.',
                  'Write “remember that …” in the Space and it shows up here.',
                )}</p>
              </div>
            </div>
          }
        >
          <ul class="verevon-space-memory__list">
            <For each={rows()}>
              {(row) => (
                <li class="verevon-space-memory__row" data-withheld={row.withheld ? 'true' : 'false'}>
                  <p class="verevon-space-memory__content">{row.content}</p>
                  <p class="verevon-space-memory__meta">
                    <span>{sourceLabel(row.source_label)}</span>
                    <Show when={row.thread_id}>
                      {' · '}
                      <a href={threadHref(row.thread_id)}>{i18n.tr('Vis samtalen', 'Open the conversation')}</a>
                    </Show>
                    <Show when={row.withheld}>
                      {' · '}
                      <span>{i18n.tr('Holdt tilbake', 'Held back')}</span>
                    </Show>
                  </p>
                  <SpaceConfirmButton
                    label={i18n.tr('Slett', 'Delete')}
                    consequence={i18n.tr(
                      'Rommet slutter å huske dette, for alle medlemmer.',
                      'The Space stops remembering this, for every member.',
                    )}
                    confirmLabel={i18n.tr('Bekreft sletting av minnet', 'Confirm deleting this memory')}
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
    </section>
  )
}
