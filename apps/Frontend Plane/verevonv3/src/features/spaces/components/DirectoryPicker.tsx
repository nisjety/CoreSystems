import { Loader2 } from '@/shared/icons'
import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js'

import {
  resolveDirectory,
  searchDirectory,
  type DirectoryAgent,
  type DirectoryEntry,
  type DirectoryEntryKind,
  type DirectoryPerson,
  type ResolveDirectoryKind,
} from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'

import './space-directory-picker.css'

const SEARCH_DEBOUNCE_MS = 250
const SEARCH_PAGE_SIZE = 20
const SEARCH_MAX_CHARS = 100

/**
 * The shared people-and-agent picker (WS04-S9): add-member now, the DM start (WS12) and the @ menu
 * (WS08) later. It lists what `GET /api/v1/directory/people?kinds=` returns, which is presence and
 * never authority, so:
 *
 * - a person row may offer a pick action (`onPickPerson`), because the caller's next call is a
 *   Control-decided write that re-checks everything;
 * - an agent row is labelled "Agent, not a person" and is NEVER offered a pick action: an agent
 *   is added to a room through the agent dialogs, not as a member.
 *
 * The server decides who the caller may see (same organization, the room's own members left out
 * through `excludeMemberOf`); this only debounces typing, pages with the server's cursor and drops
 * a stale answer. A failed search is shown as a failure, never as "no results".
 */
export interface DirectoryPickerProps {
  /** Which kinds to list; defaults to people and agents. */
  readonly kinds?: readonly DirectoryEntryKind[]
  /** A room whose members the server leaves out (honoured only when the caller is in it). */
  readonly excludeMemberOf?: string
  /** Hide people already known to be excluded by the caller (Control's roster). */
  readonly isExcluded?: (subjectId: string) => boolean
  /** Label of the pick action on a person row. */
  readonly pickLabel: string
  /** Label while a pick is in flight. */
  readonly pickingLabel: string
  /** The subject id whose pick is in flight; every pick action is disabled while one is. */
  readonly busyId?: string | undefined
  readonly onPickPerson: (person: DirectoryPerson) => void
  /** Search box label; defaults to "Search for people and agents". */
  readonly label?: string
  /**
   * A-REC-03: Enter resolves what was typed (`POST /api/v1/directory/resolve`) instead of waiting for the search: the list
   * then shows the one exact match, or up to five candidates, or nothing. It never picks: the person still presses the
   * row's own pick action. Off by default.
   */
  readonly resolveOnEnter?: boolean
}

/** The resolver kind for the kinds a picker lists: one kind is that kind, anything else is `any`. */
export function resolveKindFor(kinds: readonly DirectoryEntryKind[] | undefined): ResolveDirectoryKind {
  if (kinds && kinds.length === 1) return kinds[0] === 'agent' ? 'agent' : 'person'
  return 'any'
}

export function DirectoryPicker(props: DirectoryPickerProps) {
  const i18n = useI18n()
  const [query, setQuery] = createSignal('')
  const [searchQuery, setSearchQuery] = createSignal('')
  const [found, setFound] = createSignal<readonly DirectoryEntry[]>([])
  const [nextCursor, setNextCursor] = createSignal<string | null>(null)
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal('')
  let debounceTimer: ReturnType<typeof setTimeout> | undefined
  let seq = 0
  onCleanup(() => {
    if (debounceTimer) clearTimeout(debounceTimer)
    seq += 1
  })

  const onInput = (value: string) => {
    setQuery(value)
    if (debounceTimer) clearTimeout(debounceTimer)
    debounceTimer = setTimeout(() => setSearchQuery(value.trim()), SEARCH_DEBOUNCE_MS)
  }

  async function run(q: string, cursor: string | null) {
    const mine = ++seq
    setLoading(true)
    setError('')
    try {
      const page = await searchDirectory(q, {
        kinds: props.kinds,
        limit: SEARCH_PAGE_SIZE,
        cursor,
        excludeMemberOf: props.excludeMemberOf,
      })
      if (mine !== seq) return
      setFound((prev) => (cursor ? [...prev, ...page.results] : page.results))
      setNextCursor(page.nextCursor)
    } catch (err) {
      if (mine !== seq) return
      if (!cursor) {
        setFound([])
        setNextCursor(null)
      }
      setError(
        translateApiError(err, i18n.tr, {
          no: 'Søket mislyktes. Prøv igjen om litt.',
          en: 'The search failed. Try again shortly.',
        }),
      )
    } finally {
      if (mine === seq) setLoading(false)
    }
  }

  // Enter (with `resolveOnEnter`): one resolver answer replaces the list. A pending search is dropped so it cannot
  // overwrite the answer; an outage is said as a failure, never as "no results".
  async function resolveTyped(q: string) {
    if (debounceTimer) clearTimeout(debounceTimer)
    const mine = ++seq
    setLoading(true)
    setError('')
    try {
      const answer = await resolveDirectory({ query: q, kind: resolveKindFor(props.kinds) })
      if (mine !== seq) return
      setNextCursor(null)
      if (answer.status === 'ok') setFound([answer.match])
      else if (answer.status === 'ambiguous') setFound(answer.candidates)
      else {
        setFound([])
        if (answer.reason === 'directory_unavailable') {
          setError(i18n.tr('Katalogen svarer ikke. Prøv igjen om litt.', 'The directory is not answering. Try again shortly.'))
        }
      }
    } catch (err) {
      if (mine !== seq) return
      setError(
        translateApiError(err, i18n.tr, {
          no: 'Oppslaget mislyktes. Prøv igjen om litt.',
          en: 'The lookup failed. Try again shortly.',
        }),
      )
    } finally {
      if (mine === seq) setLoading(false)
    }
  }

  // A new first page whenever the debounced text or the room changes.
  createEffect(
    () => ({ q: searchQuery(), ref: props.excludeMemberOf }),
    ({ q }) => {
      void run(q, null)
    },
  )

  const visible = () =>
    found().filter((entry) => entry.kind === 'agent' || !(props.isExcluded?.(entry.subjectId) ?? false))

  return (
    <div class="verevon-directory-picker">
      <label class="verevon-space-member-picker__role">
        <span>{props.label ?? i18n.tr('Søk etter personer og agenter', 'Search for people and agents')}</span>
        <input
          type="search"
          class="verevon-space-agent__control"
          value={query()}
          maxlength={SEARCH_MAX_CHARS}
          autocomplete="off"
          placeholder={i18n.tr('Navn eller e-post', 'Name or email')}
          onInput={(event) => onInput(event.currentTarget.value)}
          // A search box is not a field of the form around it. Enter in it would make the browser press that form's
          // default button, and the access dialog's default button saves the draft it holds. The search itself needs
          // no Enter: it runs on its own once the text has been still for a moment.
          onKeyDown={(event) => {
            if (event.key !== 'Enter') return
            event.preventDefault()
            const typed = event.currentTarget.value.trim()
            if (props.resolveOnEnter && typed) void resolveTyped(typed)
          }}
        />
      </label>

      <Show when={loading() && found().length === 0}>
        <p class="verevon-space-inline-status" role="status">
          {i18n.tr('Søker …', 'Searching…')}
        </p>
      </Show>

      <Show when={error()}>
        {(message) => (
          <p class="verevon-space-projection-error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={!loading() && !error() && visible().length === 0}>
        <p class="verevon-space-member-picker__empty">
          {searchQuery()
            ? i18n.tr('Ingen treff på søket.', 'No results match that search.')
            : i18n.tr('Ingen flere å vise.', 'No one else to show.')}
        </p>
      </Show>

      <ul class="verevon-space-member-picker__list">
        <For each={visible()}>
          {(entry) =>
            entry.kind === 'agent' ? (
              <AgentRow agent={entry} />
            ) : (
              <li>
                <span class="verevon-space-member-picker__who">
                  <strong>{entry.displayName || entry.email}</strong>
                  <Show when={entry.displayName && entry.email}>
                    <small>{entry.email}</small>
                  </Show>
                </span>
                <button
                  type="button"
                  class="verevon-space-agent__control"
                  disabled={props.busyId !== undefined}
                  onClick={() => props.onPickPerson(entry)}
                >
                  <Show when={props.busyId === entry.subjectId} fallback={props.pickLabel}>
                    <Loader2 size={14} aria-hidden="true" />
                    {props.pickingLabel}
                  </Show>
                </button>
              </li>
            )
          }
        </For>
      </ul>

      <Show when={nextCursor()}>
        {(cursor) => (
          <button
            type="button"
            class="verevon-space-agent__control"
            disabled={loading()}
            onClick={() => void run(searchQuery(), cursor())}
          >
            <Show when={loading()} fallback={i18n.tr('Vis flere', 'Show more')}>
              <Loader2 size={14} aria-hidden="true" />
              {i18n.tr('Henter …', 'Loading…')}
            </Show>
          </button>
        )}
      </Show>
    </div>
  )
}

function AgentRow(props: { readonly agent: DirectoryAgent & { kind: 'agent' } }) {
  const i18n = useI18n()
  return (
    <li class="verevon-directory-picker__agent">
      <span class="verevon-space-member-picker__who">
        <strong>{props.agent.displayName || props.agent.agentRef}</strong>
        <small>{props.agent.description}</small>
        <Show when={props.agent.installedIn.length > 0}>
          <small>
            {i18n.tr(
              `I ${props.agent.installedIn.length} av rommene dine`,
              `In ${props.agent.installedIn.length} of your rooms`,
            )}
          </small>
        </Show>
      </span>
      {/* Presence, never a member: no action is offered on an agent. */}
      <span class="verevon-directory-picker__badge">{i18n.tr('Agent, ikke en person', 'Agent, not a person')}</span>
    </li>
  )
}
