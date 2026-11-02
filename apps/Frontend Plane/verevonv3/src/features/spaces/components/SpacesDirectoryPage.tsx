import { useNavigate } from '@solidjs/router'
import { createSignal, For, onCleanup, Show } from 'solid-js'

import {
  getSpaceMembership,
  joinSpaceRoom,
  listDirectoryRooms,
  type DirectoryRoom,
} from '@/shared/api/spaces-client'
import { hasErrorCode } from '@/shared/api/http'
import { translateApiError, useI18n } from '@/shared/i18n'
import { Loader2, Search } from '@/shared/icons'

import './space-directory.css'

// WS04-S8: the room directory. Lists the organization's rooms that an owner or manager opened to
// everyone, and lets a member join one. Presentation only: Control decides what is listed (a
// private room, an archived room, a personal Space and a room the caller already belongs to are
// never in the answer), whether the organization's directory policy is on, and what role a
// self-join receives. Joining one room is never access to anything else: it grants a room role,
// not a resource grant.

/** Debounce for the search box, so typing does not send a request per keystroke. */
const SEARCH_DEBOUNCE_MS = 250
/** After a join the room's recipient audience is being prepared: poll up to this long, then open anyway. */
const READY_POLL_INTERVAL_MS = 2_000
const READY_POLL_ATTEMPTS = 15

type LoadState = 'loading' | 'ready' | 'error' | 'feature_off'

function newIdempotencyKey(): string {
  const random =
    typeof crypto !== 'undefined' && 'randomUUID' in crypto ? crypto.randomUUID() : `${Date.now()}-${Math.random()}`
  return `join-${random}`.slice(0, 64)
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

export default function SpacesDirectoryPage() {
  const i18n = useI18n()
  const navigate = useNavigate()

  const [query, setQuery] = createSignal('')
  const [state, setState] = createSignal<LoadState>('loading')
  const [enabled, setEnabled] = createSignal(true)
  const [rooms, setRooms] = createSignal<readonly DirectoryRoom[]>([])
  const [nextCursor, setNextCursor] = createSignal<string | null>(null)
  const [loadingMore, setLoadingMore] = createSignal(false)
  const [loadError, setLoadError] = createSignal('')
  // Per-room join progress: 'joining' while the request runs, 'preparing' while access is set up.
  const [progress, setProgress] = createSignal<Readonly<Record<string, 'joining' | 'preparing'>>>({})
  const [roomErrors, setRoomErrors] = createSignal<Readonly<Record<string, string>>>({})

  let controller: AbortController | undefined
  let debounce: ReturnType<typeof setTimeout> | undefined
  let disposed = false
  onCleanup(() => {
    disposed = true
    controller?.abort()
    if (debounce) clearTimeout(debounce)
  })

  function setRoomProgress(spaceRef: string, value: 'joining' | 'preparing' | null) {
    setProgress((current) => {
      const next = { ...current }
      if (value === null) delete next[spaceRef]
      else next[spaceRef] = value
      return next
    })
  }

  function setRoomError(spaceRef: string, message: string | null) {
    setRoomErrors((current) => {
      const next = { ...current }
      if (message === null) delete next[spaceRef]
      else next[spaceRef] = message
      return next
    })
  }

  async function load(q: string) {
    controller?.abort()
    const mine = new AbortController()
    controller = mine
    setState('loading')
    setLoadError('')
    try {
      const page = await listDirectoryRooms(q, { signal: mine.signal })
      if (mine.signal.aborted || disposed) return
      setEnabled(page.enabled)
      setRooms(page.rooms)
      setNextCursor(page.nextCursor)
      setState('ready')
    } catch (err) {
      if (mine.signal.aborted || disposed) return
      if (hasErrorCode(err, 'feature_disabled')) {
        setState('feature_off')
        return
      }
      setLoadError(
        translateApiError(err, i18n.tr, {
          no: 'Romkatalogen kunne ikke lastes. Prøv igjen om litt.',
          en: 'The room directory could not be loaded. Try again shortly.',
        }),
      )
      setState('error')
    }
  }

  async function loadMore() {
    const cursor = nextCursor()
    if (!cursor || loadingMore()) return
    const mine = controller
    setLoadingMore(true)
    try {
      const page = await listDirectoryRooms(query(), { cursor, signal: mine?.signal })
      if (mine?.signal.aborted || disposed) return
      setRooms((current) => {
        const seen = new Set(current.map((room) => room.spaceRef))
        return [...current, ...page.rooms.filter((room) => !seen.has(room.spaceRef))]
      })
      setNextCursor(page.nextCursor)
    } catch (err) {
      if (mine?.signal.aborted || disposed) return
      setLoadError(
        translateApiError(err, i18n.tr, {
          no: 'Flere rom kunne ikke lastes. Prøv igjen om litt.',
          en: 'More rooms could not be loaded. Try again shortly.',
        }),
      )
    } finally {
      setLoadingMore(false)
    }
  }

  function onSearch(value: string) {
    setQuery(value)
    if (debounce) clearTimeout(debounce)
    debounce = setTimeout(() => void load(value), SEARCH_DEBOUNCE_MS)
  }

  async function waitForAccess(spaceRef: string) {
    for (let attempt = 0; attempt < READY_POLL_ATTEMPTS; attempt += 1) {
      await delay(READY_POLL_INTERVAL_MS)
      if (disposed) return
      try {
        const membership = await getSpaceMembership(spaceRef)
        // `recipient_ready` is absent on an older Control: then there is nothing to wait for.
        if (membership.recipient_ready !== false) return
      } catch {
        // Not readable yet; the room page states the real status if it is still being prepared.
      }
    }
  }

  async function join(room: DirectoryRoom) {
    if (progress()[room.spaceRef]) return
    setRoomError(room.spaceRef, null)
    setRoomProgress(room.spaceRef, 'joining')
    try {
      const result = await joinSpaceRoom(room.spaceRef, newIdempotencyKey())
      if (disposed) return
      if (!result.alreadyMember) {
        setRoomProgress(room.spaceRef, 'preparing')
        await waitForAccess(room.spaceRef)
        if (disposed) return
      }
      navigate(`/spaces/${encodeURIComponent(room.spaceRef)}`)
    } catch (err) {
      if (disposed) return
      setRoomProgress(room.spaceRef, null)
      if (hasErrorCode(err, 'room_not_found')) {
        // The room closed, was archived or was removed since the list loaded.
        setRooms((current) => current.filter((item) => item.spaceRef !== room.spaceRef))
      }
      setRoomError(
        room.spaceRef,
        translateApiError(err, i18n.tr, {
          no: 'Du kunne ikke bli med i rommet. Ingenting er endret.',
          en: 'You could not join the room. Nothing was changed.',
        }),
      )
    }
  }

  void load('')

  const roleLabel = (role: DirectoryRoom['joinRole']) =>
    role === 'editor' ? i18n.tr('redaktør', 'editor') : i18n.tr('leser', 'viewer')

  return (
    <section class="space-page verevon-space-directory" aria-labelledby="spaces-directory-title">
      <header class="verevon-space-directory__header">
        <h1 id="spaces-directory-title">{i18n.tr('Finn rom', 'Browse rooms')}</h1>
        <p class="verevon-space-directory__lead">
          {i18n.tr(
            'Rom i organisasjonen din som er åpne for alle. Du ser ikke private rom, og du får bare tilgang til rommet du blir med i.',
            'Rooms in your organization that are open to everyone. Private rooms are never listed, and joining gives access to that room only.',
          )}
        </p>
        <Show when={state() === 'ready' && enabled()}>
          <label class="verevon-space-directory__search">
            <Search size={15} aria-hidden="true" />
            <span class="sr-only">{i18n.tr('Søk i rom', 'Search rooms')}</span>
            <input
              type="search"
              value={query()}
              placeholder={i18n.tr('Søk etter navn eller emne', 'Search by name or topic')}
              onInput={(event) => onSearch(event.currentTarget.value)}
            />
          </label>
        </Show>
      </header>

      <Show when={state() === 'loading'}>
        <p role="status" aria-live="polite" class="verevon-space-directory__status">
          <Loader2 size={15} class="onboarding-phase-spinner" />
          {i18n.tr('Laster rom …', 'Loading rooms…')}
        </p>
      </Show>

      <Show when={state() === 'feature_off'}>
        <p role="status" class="verevon-space-directory__status">
          {i18n.tr('Romkatalogen er ikke slått på ennå.', 'The room directory is not turned on yet.')}
        </p>
      </Show>

      {/* A failed read is not "no rooms": say so, and offer a retry. */}
      <Show when={state() === 'error'}>
        <div role="alert" class="verevon-space-directory__status">
          <p>{loadError()}</p>
          <button type="button" class="verevon-space-secondary-action" onClick={() => void load(query())}>
            {i18n.tr('Prøv igjen', 'Try again')}
          </button>
        </div>
      </Show>

      {/* The organization's policy is off: honest about it, not an empty directory. */}
      <Show when={state() === 'ready' && !enabled()}>
        <p role="status" class="verevon-space-directory__status">
          {i18n.tr(
            'Organisasjonen din har ikke slått på romkatalogen. Spør en administrator hvis du vil kunne finne og bli med i åpne rom.',
            'Your organization has not turned on the room directory. Ask an administrator if you want to find and join open rooms.',
          )}
        </p>
      </Show>

      <Show when={state() === 'ready' && enabled()}>
        <Show
          when={rooms().length > 0}
          fallback={
            <p role="status" class="verevon-space-directory__status">
              {query().trim()
                ? i18n.tr('Ingen åpne rom passer søket.', 'No open rooms match your search.')
                : i18n.tr('Det finnes ingen åpne rom du ikke allerede er med i.', 'There are no open rooms you have not already joined.')}
            </p>
          }
        >
          <ul class="verevon-space-directory__list">
            <For each={rooms()}>
              {(room) => (
                <li class="verevon-space-directory__row">
                  <div class="verevon-space-directory__text">
                    <span class="verevon-space-directory__name">{room.name}</span>
                    <Show when={room.topic}>
                      {(topic) => <span class="verevon-space-directory__topic">{topic()}</span>}
                    </Show>
                    <span class="verevon-space-directory__meta">
                      {i18n.tr(
                        `${room.memberCount} medlemmer. Du blir med som ${roleLabel(room.joinRole)}.`,
                        `${room.memberCount} members. You join as ${roleLabel(room.joinRole)}.`,
                      )}
                    </span>
                    <Show when={roomErrors()[room.spaceRef]}>
                      {(message) => (
                        <p class="verevon-space-projection-error" role="alert">{message()}</p>
                      )}
                    </Show>
                    <Show when={progress()[room.spaceRef] === 'preparing'}>
                      <p class="verevon-space-directory__meta" role="status" aria-live="polite">
                        {i18n.tr(
                          'Du er med. Tilgangen klargjøres, og rommet åpnes straks.',
                          'You are in. Access is being prepared, and the room opens shortly.',
                        )}
                      </p>
                    </Show>
                  </div>
                  <button
                    type="button"
                    class="verevon-space-secondary-action"
                    disabled={progress()[room.spaceRef] !== undefined}
                    onClick={() => void join(room)}
                  >
                    {progress()[room.spaceRef] === 'joining'
                      ? i18n.tr('Blir med …', 'Joining…')
                      : progress()[room.spaceRef] === 'preparing'
                        ? i18n.tr('Klargjør …', 'Preparing…')
                        : i18n.tr('Bli med', 'Join')}
                  </button>
                </li>
              )}
            </For>
          </ul>
          <Show when={nextCursor()}>
            <button
              type="button"
              class="verevon-space-secondary-action"
              disabled={loadingMore()}
              onClick={() => void loadMore()}
            >
              {loadingMore() ? i18n.tr('Laster …', 'Loading…') : i18n.tr('Vis flere', 'Show more')}
            </button>
          </Show>
          <Show when={loadError()}>
            <p class="verevon-space-projection-error" role="alert">{loadError()}</p>
          </Show>
        </Show>
      </Show>
    </section>
  )
}
