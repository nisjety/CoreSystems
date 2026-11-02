import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js'

import {
  openSpaceDm,
  searchDirectoryPeople,
  type DirectoryPerson,
  type SpaceDmOpenResult,
} from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'

import './space-dm.css'

/** Others besides the caller: a DM holds 2 people, a group DM up to 9 (Control's bound). */
export const DM_MAX_OTHERS = 8
const SEARCH_DEBOUNCE_MS = 250
const SEARCH_PAGE_SIZE = 20

/**
 * "New message": pick one person for a DM or up to eight for a group DM, then open it. Control
 * decides whether the organization may have DMs and whether these people can be in one; opening
 * the same set again returns the same DM. A native modal `<dialog>` (focus trap, `Escape`, inert
 * background come from the platform), like the sidebar's move dialog.
 */
export function DmStartDialog(props: {
  open: boolean
  onClose: () => void
  /** Called with the DM (new or existing) once it is open. */
  onOpened: (result: SpaceDmOpenResult) => void
}) {
  const i18n = useI18n()
  let dialog: HTMLDialogElement | undefined
  let input: HTMLInputElement | undefined
  const [query, setQuery] = createSignal('')
  const [found, setFound] = createSignal<readonly DirectoryPerson[]>([])
  const [searching, setSearching] = createSignal(false)
  const [searchError, setSearchError] = createSignal('')
  const [picked, setPicked] = createSignal<readonly DirectoryPerson[]>([])
  const [submitting, setSubmitting] = createSignal(false)
  const [error, setError] = createSignal('')
  let searchSeq = 0

  const reset = () => {
    setQuery('')
    setFound([])
    setPicked([])
    setError('')
    setSearchError('')
    setSubmitting(false)
  }

  createEffect(
    () => props.open,
    (isOpen) => {
      if (isOpen) {
        queueMicrotask(() => {
          if (dialog && !dialog.open) dialog.showModal()
          input?.focus()
        })
      } else {
        if (dialog?.open) dialog.close()
        reset()
      }
    },
  )

  // A fresh first page whenever the dialog opens or the typed text settles.
  createEffect(
    () => ({ isOpen: props.open, q: query() }),
    ({ isOpen, q }) => {
      if (!isOpen) return undefined
      const seq = ++searchSeq
      const timer = window.setTimeout(async () => {
        setSearching(true)
        setSearchError('')
        try {
          const page = await searchDirectoryPeople(q, { limit: SEARCH_PAGE_SIZE })
          if (seq !== searchSeq) return
          setFound(page.people.filter((person) => !person.isSelf))
        } catch (err) {
          if (seq !== searchSeq) return
          setFound([])
          setSearchError(
            translateApiError(err, i18n.tr, {
              no: 'Personsøket mislyktes. Prøv igjen om litt.',
              en: 'The people search failed. Try again shortly.',
            }),
          )
        } finally {
          if (seq === searchSeq) setSearching(false)
        }
      }, SEARCH_DEBOUNCE_MS)
      return () => window.clearTimeout(timer)
    },
  )
  onCleanup(() => {
    searchSeq += 1
  })

  const isPicked = (person: DirectoryPerson) => picked().some((item) => item.subjectId === person.subjectId)
  const toggle = (person: DirectoryPerson) => {
    setError('')
    if (isPicked(person)) {
      setPicked(picked().filter((item) => item.subjectId !== person.subjectId))
      return
    }
    if (picked().length >= DM_MAX_OTHERS) return
    setPicked([...picked(), person])
  }
  const atLimit = () => picked().length >= DM_MAX_OTHERS

  async function submit(event: Event) {
    event.preventDefault()
    if (submitting() || picked().length === 0) return
    setSubmitting(true)
    setError('')
    try {
      const result = await openSpaceDm(picked().map((person) => person.subjectId))
      props.onOpened(result)
    } catch (err) {
      setError(
        translateApiError(err, i18n.tr, {
          no: 'Meldingen kunne ikke åpnes. Ingenting er endret.',
          en: 'The message could not be opened. Nothing was changed.',
        }),
      )
      setSubmitting(false)
    }
  }

  return (
    <dialog
      ref={dialog}
      class="space-dm-dialog"
      aria-labelledby="space-dm-dialog-title"
      onClose={() => props.onClose()}
      onCancel={() => props.onClose()}
    >
      <form onSubmit={(event) => void submit(event)}>
        <h2 id="space-dm-dialog-title">{i18n.tr('Ny melding', 'New message')}</h2>
        <p class="space-dm-dialog__hint">
          {i18n.tr(
            'Velg én person for en direktemelding, eller opptil åtte for en gruppesamtale. Deltakerne kan ikke endres etterpå.',
            'Pick one person for a direct message, or up to eight for a group message. The people cannot be changed afterwards.',
          )}
        </p>

        <Show when={picked().length > 0}>
          <ul class="space-dm-dialog__picked" aria-label={i18n.tr('Valgte personer', 'Chosen people')}>
            <For each={picked()}>
              {(person) => (
                <li>
                  <button
                    type="button"
                    class="space-dm-dialog__chip"
                    aria-label={i18n.tr(`Fjern ${person.displayName || person.email}`, `Remove ${person.displayName || person.email}`)}
                    onClick={() => toggle(person)}
                  >
                    {person.displayName || person.email}
                    <span aria-hidden="true"> ×</span>
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>

        <label class="space-dm-dialog__search">
          <span>{i18n.tr('Søk etter personer', 'Search people')}</span>
          <input
            ref={input}
            type="search"
            value={query()}
            maxlength={100}
            autocomplete="off"
            onInput={(event) => setQuery(event.currentTarget.value)}
          />
        </label>

        <Show when={searchError()}>
          <p role="alert">{searchError()}</p>
        </Show>
        <Show when={searching()}>
          <p role="status" class="space-dm-dialog__hint">{i18n.tr('Søker …', 'Searching…')}</p>
        </Show>
        <Show when={!searching() && !searchError() && found().length === 0}>
          <p role="status" class="space-dm-dialog__hint">{i18n.tr('Ingen personer funnet.', 'No people found.')}</p>
        </Show>

        <ul class="space-dm-dialog__results" aria-label={i18n.tr('Personer', 'People')}>
          <For each={found()}>
            {(person) => (
              <li>
                <label class="space-dm-dialog__person">
                  <input
                    type="checkbox"
                    checked={isPicked(person)}
                    disabled={!isPicked(person) && atLimit()}
                    onChange={() => toggle(person)}
                  />
                  <span class="space-dm-dialog__name">{person.displayName || person.email}</span>
                  <Show when={person.displayName && person.email}>
                    <span class="space-dm-dialog__email">{person.email}</span>
                  </Show>
                </label>
              </li>
            )}
          </For>
        </ul>
        <Show when={atLimit()}>
          <p class="space-dm-dialog__hint">
            {i18n.tr('Du har valgt det høyeste antallet personer.', 'You have chosen the most people allowed.')}
          </p>
        </Show>

        <Show when={error()}>
          <p role="alert">{error()}</p>
        </Show>

        <div class="space-dm-dialog__footer">
          <button type="button" onClick={() => props.onClose()}>{i18n.tr('Avbryt', 'Cancel')}</button>
          <button type="submit" disabled={submitting() || picked().length === 0}>
            {submitting() ? i18n.tr('Åpner …', 'Opening…') : i18n.tr('Start melding', 'Start message')}
          </button>
        </div>
      </form>
    </dialog>
  )
}
