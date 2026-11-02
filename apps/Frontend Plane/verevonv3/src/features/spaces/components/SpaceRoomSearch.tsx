import { createEffect, createMemo, createSignal, For, onCleanup, Show } from 'solid-js'

import { registerCommands, setMessageSearchOpener } from '@/features/command/command-registry'
import { Search, X } from '@/shared/icons'
import type { SpaceRosterMember, SpaceSearchHit } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'

import { formatRelativeWhen } from '../lib/space-thread-presentation'
import { normalizeSnippet } from '../lib/space-search-snippet'
import {
  SEARCH_MAX_CHARS,
  SEARCH_MIN_CHARS,
  createSpaceSearchController,
  forgetRecentSearches,
  readRecentSearches,
  rememberSearch,
  searchExclusionNotes,
  searchFailureRetryable,
  spaceMessageSearchEnabled,
  watchRecentSearchRetention,
  type SearchFailure,
} from '../lib/space-search-state'
import { spaceThreadPaneEnabled } from '../lib/space-thread-deep-link'
import { resolveAuthorName } from './sidepane/SpaceTurnList'
import './space-room-search.css'

export interface SpaceRoomSearchProps {
  readonly spaceRef: string
  readonly roster: () => readonly SpaceRosterMember[]
  readonly viewerSubjectId: () => string | undefined
  /** The thread open in the side pane; offers the "This thread" scope while there is one. */
  readonly openThreadId: () => string | null | undefined
  /** Opens one hit in context (the thread pane, scrolled to the message). */
  readonly onOpenHit: (hit: SpaceSearchHit) => void
}

let instanceCounter = 0

function shortcutLabel(): string {
  try {
    return /Mac|iPhone|iPad/.test(navigator.platform) ? '⌘F' : 'Ctrl+F'
  } catch {
    return 'Ctrl+F'
  }
}

/**
 * In-room message search (WS13-S5). Opens with Ctrl/Cmd+F (the command layer's `mod+f`), the
 * "Search messages" button, the palette's pinned row or `/search`. The query goes to the gateway in a
 * POST body; results are a listbox driven from the input (combobox pattern), and every state has its
 * own sentence. It shows nothing when `features.message_search` is off or the thread pane (where a hit
 * opens) is off.
 *
 * The second Ctrl/Cmd+F while the input has focus is not taken, so the browser's own find bar stays
 * reachable.
 */
export function SpaceRoomSearch(props: SpaceRoomSearchProps) {
  const i18n = useI18n()
  const tr = i18n.tr
  const baseId = `space-search-${++instanceCounter}`
  const listId = `${baseId}-list`
  const optionId = (index: number) => `${baseId}-opt-${index}`

  const enabled = () => spaceMessageSearchEnabled() && spaceThreadPaneEnabled()
  const controller = createSpaceSearchController({
    spaceRef: () => props.spaceRef,
    threadId: () => props.openThreadId(),
  })
  onCleanup(() => controller.dispose())

  const [open, setOpen] = createSignal(false)
  const [recents, setRecents] = createSignal<readonly string[]>([])
  let input: HTMLInputElement | undefined
  let trigger: HTMLButtonElement | undefined
  let list: HTMLUListElement | undefined
  let returnFocus: HTMLElement | null = null
  let isOpen = false

  const focusInput = () => {
    requestAnimationFrame(() => {
      input?.focus()
      input?.select()
    })
  }

  const openSearch = (query?: string) => {
    if (!isOpen) {
      returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
      isOpen = true
      setOpen(true)
      setRecents(readRecentSearches())
    }
    const text = query?.trim() ?? ''
    if (text) {
      controller.setQuery(text)
      controller.submit()
    }
    focusInput()
  }

  const closeSearch = () => {
    if (!isOpen) return
    isOpen = false
    setOpen(false)
    controller.reset()
    const target = returnFocus
    returnFocus = null
    requestAnimationFrame(() => {
      if (target && target.isConnected) target.focus()
      else trigger?.focus()
    })
  }

  // The command layer: `mod+f` opens (and a second press with the input focused falls through to the
  // browser), and the palette's "Search messages" row and `/search` reach the same opener.
  createEffect(enabled, (on) => {
    if (!on) return undefined
    const disposeCommands = registerCommands([
      {
        id: 'space.search',
        labelNo: 'Søk i meldinger i rommet',
        labelEn: 'Search messages in this room',
        group: 'room',
        hotkey: 'mod+f',
        kind: 'ui',
        when: (ctx) => ctx.spaceRef !== null && document.activeElement !== input,
        run: () => openSearch(),
      },
    ])
    const disposeOpener = setMessageSearchOpener((query) => openSearch(query))
    const disposeRetention = watchRecentSearchRetention()
    return () => {
      disposeCommands()
      disposeOpener()
      disposeRetention()
    }
  })

  const people = createMemo(() => props.roster().filter((member) => member.subject_type === 'user'))
  const personName = (member: SpaceRosterMember) =>
    resolveAuthorName(member.subject_id, props.roster(), props.viewerSubjectId(), tr)

  const authorOf = (hit: SpaceSearchHit) =>
    hit.role === 'assistant'
      ? (hit.agentName ?? tr('Agent', 'Agent'))
      : resolveAuthorName(hit.authorSubjectId, props.roster(), props.viewerSubjectId(), tr)

  const openHit = (hit: SpaceSearchHit) => {
    rememberSearch(controller.answeredQuery())
    props.onOpenHit(hit)
  }

  // Keep the active option on screen while arrowing through a long list.
  createEffect(controller.activeIndex, (index) => {
    if (index < 0) return undefined
    const frame = requestAnimationFrame(() => {
      const option = list?.querySelector<HTMLElement>(`#${CSS.escape(optionId(index))}`)
      if (option && typeof option.scrollIntoView === 'function') option.scrollIntoView({ block: 'nearest' })
    })
    return () => cancelAnimationFrame(frame)
  })

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.isComposing) return
    const count = controller.hits().length
    switch (event.key) {
      case 'ArrowDown':
        if (count === 0) return
        event.preventDefault()
        controller.setActiveIndex(controller.activeIndex() + 1)
        return
      case 'ArrowUp':
        if (count === 0) return
        event.preventDefault()
        controller.setActiveIndex(Math.max(controller.activeIndex() - 1, 0))
        return
      case 'Enter': {
        event.preventDefault()
        const hit = controller.hits()[controller.activeIndex()]
        if (hit) {
          openHit(hit)
          return
        }
        rememberSearch(controller.query())
        controller.submit()
        return
      }
      case 'Escape':
        event.preventDefault()
        closeSearch()
        return
      default:
    }
  }

  const failureText = (kind: SearchFailure): string => {
    switch (kind) {
      case 'busy':
        return tr('Søket er opptatt akkurat nå. Prøv igjen om litt.', 'Search is busy right now. Try again shortly.')
      case 'timeout':
        return tr(
          'Søket tok for lang tid. Prøv igjen, eller gjør søket mer presist.',
          'The search took too long. Try again, or make the search more specific.',
        )
      case 'unavailable':
        return tr('Søket er utilgjengelig akkurat nå. Prøv igjen om litt.', 'Search is unavailable right now. Try again shortly.')
      case 'disabled':
        return tr('Meldingssøk er ikke slått på for dette rommet.', 'Message search is not turned on for this room.')
      case 'forbidden':
        return tr(
          'Du har ikke lenger tilgang til å søke her. Last inn rommet på nytt.',
          'You no longer have access to search here. Reload the room.',
        )
      case 'invalid':
        return tr('Søket er ikke gyldig. Skriv minst to tegn og prøv igjen.', 'That search is not valid. Enter at least two characters and try again.')
      default:
        return tr('Søket kunne ikke fullføres. Prøv igjen.', 'The search could not be completed. Try again.')
    }
  }

  const resultsStatus = () => {
    const count = controller.hits().length
    if (controller.phase() === 'searching') return tr('Søker …', 'Searching…')
    if (controller.phase() === 'empty') return tr('Ingen treff.', 'No results.')
    if (controller.phase() === 'results') {
      return count === 1 ? tr('1 treff.', '1 result.') : tr(`${count} treff.`, `${count} results.`)
    }
    return ''
  }

  const listVisible = () => controller.hits().length > 0

  return (
    <Show when={enabled()}>
      <div class="verevon-space-search" data-open={open() ? 'true' : undefined}>
        <Show when={!open()}>
          <button
            type="button"
            class="verevon-space-search__trigger"
            ref={(element) => { trigger = element }}
            onClick={() => openSearch()}
          >
            <Search size={15} aria-hidden="true" />
            <span>{tr('Søk i meldinger', 'Search messages')}</span>
            <kbd class="verevon-space-search__kbd" aria-hidden="true">{shortcutLabel()}</kbd>
          </button>
        </Show>

        <Show when={open()}>
          <section class="verevon-space-search__panel" role="search" aria-label={tr('Søk i meldinger i rommet', 'Search messages in this room')}>
            <div class="verevon-space-search__bar">
              <Search size={16} aria-hidden="true" />
              <input
                ref={(element) => { input = element }}
                type="text"
                class="verevon-space-search__input"
                data-space-search-input="true"
                role="combobox"
                aria-expanded={listVisible() ? 'true' : 'false'}
                aria-controls={listId}
                aria-autocomplete="list"
                aria-activedescendant={controller.activeIndex() >= 0 && listVisible() ? optionId(controller.activeIndex()) : undefined}
                aria-label={tr('Søk i meldinger', 'Search messages')}
                placeholder={tr('Søk i meldinger …', 'Search messages…')}
                autocomplete="off"
                spellcheck={false}
                maxlength={SEARCH_MAX_CHARS}
                value={controller.query()}
                onInput={(event) => controller.setQuery(event.currentTarget.value)}
                onKeyDown={onKeyDown}
              />
              <button
                type="button"
                class="verevon-space-search__close"
                aria-label={tr('Lukk søk', 'Close search')}
                onClick={closeSearch}
              >
                <X size={15} aria-hidden="true" />
              </button>
            </div>

            <div class="verevon-space-search__scopes" role="group" aria-label={tr('Søkeomfang', 'Search scope')}>
              <button
                type="button"
                class="verevon-space-search__chip"
                aria-pressed={controller.scope() === 'room' ? 'true' : 'false'}
                onClick={() => controller.setScope('room')}
              >
                {tr('Dette rommet', 'This room')}
              </button>
              <Show when={props.openThreadId()}>
                <button
                  type="button"
                  class="verevon-space-search__chip"
                  aria-pressed={controller.scope() === 'thread' ? 'true' : 'false'}
                  onClick={() => controller.setScope('thread')}
                >
                  {tr('Denne tråden', 'This thread')}
                </button>
              </Show>
              <Show when={people().length > 0}>
                <button
                  type="button"
                  class="verevon-space-search__chip"
                  aria-pressed={controller.scope() === 'person' ? 'true' : 'false'}
                  onClick={() => controller.setScope(controller.scope() === 'person' ? 'room' : 'person')}
                >
                  {tr('Fra en person', 'From a person')}
                </button>
              </Show>
              <Show when={controller.scope() === 'person'}>
                <select
                  class="verevon-space-search__person"
                  aria-label={tr('Velg person', 'Choose a person')}
                  value={controller.personId() ?? ''}
                  onChange={(event) => controller.setPerson(event.currentTarget.value || null)}
                >
                  <option value="">{tr('Velg person …', 'Choose a person…')}</option>
                  <For each={people()}>
                    {(member) => <option value={member.subject_id}>{personName(member)}</option>}
                  </For>
                </select>
              </Show>
            </div>

            <p class="sr-only" role="status" aria-live="polite">{resultsStatus()}</p>

            <Show when={controller.coverage() === 'own_threads_only'}>
              <p class="verevon-space-search__note" role="note" data-kind="coverage">
                {tr(
                  'Viser bare samtaler du selv har startet: organisasjonen har ikke slått på delt lesing i dette rommet.',
                  'Showing only conversations you started: the organization has not enabled shared reading in this room.',
                )}
              </p>
            </Show>
            <Show when={controller.truncated()}>
              <p class="verevon-space-search__note" role="note" data-kind="truncated">
                {tr('Viser de første 100 treffene. Snevre inn søket.', 'Showing the first 100 matches. Narrow the search.')}
              </p>
            </Show>
            <For each={searchExclusionNotes(controller.excluded())}>
              {(note) => (
                <p class="verevon-space-search__note" role="note" data-kind={note}>
                  {note === 'no_searchable_terms'
                    ? tr(
                        'Ordene du skrev kan ikke søkes på. Prøv andre ord.',
                        'The words you typed cannot be searched for. Try other words.',
                      )
                    : tr(
                        'Noen samtaler er ikke med i søket.',
                        'Some conversations are not included in search.',
                      )}
                </p>
              )}
            </For>

            <Show when={controller.phase() === 'idle'}>
              <div class="verevon-space-search__idle">
                <p>
                  {tr(
                    `Skriv minst ${SEARCH_MIN_CHARS} tegn for å søke i meldingene du har tilgang til.`,
                    `Type at least ${SEARCH_MIN_CHARS} characters to search the messages you can read.`,
                  )}
                </p>
                <Show when={recents().length > 0}>
                  <div class="verevon-space-search__recents">
                    <span class="verevon-space-search__recents-label">{tr('Nylige søk', 'Recent searches')}</span>
                    <For each={recents()}>
                      {(recent) => (
                        <button
                          type="button"
                          class="verevon-space-search__chip"
                          onClick={() => {
                            controller.setQuery(recent)
                            controller.submit()
                            focusInput()
                          }}
                        >
                          {recent}
                        </button>
                      )}
                    </For>
                    <button
                      type="button"
                      class="verevon-space-search__link"
                      onClick={() => {
                        forgetRecentSearches()
                        setRecents([])
                      }}
                    >
                      {tr('Fjern nylige søk', 'Clear recent searches')}
                    </button>
                  </div>
                </Show>
              </div>
            </Show>

            <Show when={controller.phase() === 'searching' && !listVisible()}>
              <p class="verevon-space-search__state" data-state="searching">{tr('Søker …', 'Searching…')}</p>
            </Show>

            <Show when={controller.phase() === 'empty'}>
              <p class="verevon-space-search__state" data-state="empty">
                {tr(`Ingen meldinger passer «${controller.answeredQuery()}».`, `No messages match "${controller.answeredQuery()}".`)}
              </p>
            </Show>

            <Show when={controller.failure()}>
              {(kind) => (
                <p class="verevon-space-search__state" data-state="error" data-failure={kind()} role="alert">
                  {failureText(kind())}
                  <Show when={searchFailureRetryable(kind())}>
                    <button type="button" class="verevon-space-search__link" onClick={() => controller.retry()}>
                      {tr('Prøv igjen', 'Try again')}
                    </button>
                  </Show>
                </p>
              )}
            </Show>

            <ul
              id={listId}
              ref={(element) => { list = element }}
              class="verevon-space-search__list"
              role="listbox"
              aria-label={tr('Søketreff', 'Search results')}
              aria-busy={controller.phase() === 'searching' ? 'true' : 'false'}
              hidden={!listVisible()}
            >
              <For each={controller.hits()}>
                {(hit, index) => (
                  <li
                    id={optionId(index())}
                    class="verevon-space-search__hit"
                    role="option"
                    aria-selected={controller.activeIndex() === index() ? 'true' : 'false'}
                    data-active={controller.activeIndex() === index() ? 'true' : undefined}
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => openHit(hit)}
                  >
                    <span class="verevon-space-search__meta">
                      <span class="verevon-space-search__author">{authorOf(hit)}</span>
                      <Show when={formatRelativeWhen(hit.createdAt, undefined, { locale: i18n.locale() })}>
                        {(when) => <time class="verevon-space-search__time" datetime={hit.createdAt}>{when()}</time>}
                      </Show>
                      <Show when={hit.threadTitle}>
                        <span class="verevon-space-search__thread">{hit.threadTitle}</span>
                      </Show>
                    </span>
                    <span class="verevon-space-search__snippet">
                      <For each={normalizeSnippet(hit.snippet)}>
                        {(segment) => (segment.matched ? <mark>{segment.text}</mark> : <>{segment.text}</>)}
                      </For>
                    </span>
                  </li>
                )}
              </For>
            </ul>

            <Show when={controller.hasMore()}>
              <button
                type="button"
                class="verevon-space-search__more"
                disabled={controller.loadingMore()}
                onClick={() => controller.loadMore()}
              >
                {controller.loadingMore() ? tr('Laster …', 'Loading…') : tr('Vis flere treff', 'Show more results')}
              </button>
            </Show>
          </section>
        </Show>
      </div>
    </Show>
  )
}
