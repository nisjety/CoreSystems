import { useLocation } from '@solidjs/router'
import { Hash, MessageCircle, Pin, Plus, Users } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'

import {
  createRoom,
  getSpaceThreads,
  listSpaces,
  type SpaceSummary,
  type SpaceThread,
} from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { createResource } from '@/shared/lib/create-resource-compat'
import { spaceDisplayName } from '@/features/spaces/lib/space-name'
import {
  isSpaceObserved,
  isSpaceWorking,
  liveThreadsIn,
  unreadThreadIdsIn,
} from '@/features/spaces/lib/space-live-work'
import { SidebarPanelTitle, SidebarSearchField } from './CoreSidebarPrimitives'

/**
 * The expanded core sidebar is the one room navigator. It reads the same
 * server-composed list and thread projection as the Space page, but does not
 * create a browser-side authority aggregate or keep an alternate room shell.
 */
export function SpacesExpandedSidebarPanel(props: { onCollapse: () => void }) {
  const i18n = useI18n()
  const location = useLocation()
  const [query, setQuery] = createSignal('')
  const [spacesLoadFailed, setSpacesLoadFailed] = createSignal(false)
  const [spaces, { refetch: refetchSpaces }] = createResource(async () => {
    setSpacesLoadFailed(false)
    try {
      return await listSpaces()
    } catch {
      setSpacesLoadFailed(true)
      return []
    }
  })
  const [creatingRoom, setCreatingRoom] = createSignal(false)
  const [createRoomError, setCreateRoomError] = createSignal('')
  const [roomFormOpen, setRoomFormOpen] = createSignal(false)
  const [roomName, setRoomName] = createSignal('')

  // A named room, alongside the organization's own channel. Until this existed
  // the "Channels" heading sat over a list that could only ever hold one entry.
  async function createNamedRoom(event: Event) {
    event.preventDefault()
    const name = roomName().trim()
    if (creatingRoom() || !name) return
    setCreatingRoom(true)
    setCreateRoomError('')
    try {
      await createRoom(name)
      setRoomName('')
      setRoomFormOpen(false)
      // The room exists but is not registered yet, so it will not appear in the
      // listing until Control accepts it. Refetch anyway: when registration is
      // quick the room shows up, and when it is not the list is simply
      // unchanged rather than showing a room nothing can be done in.
      await refetchSpaces()
    } catch {
      setCreateRoomError(i18n.tr(
        'Rommet kunne ikke opprettes. Ingenting ble klargjort.',
        'The room could not be created. Nothing was provisioned.',
      ))
    } finally {
      setCreatingRoom(false)
    }
  }
  const routeSpaceRef = createMemo(() => spaceRefFromPath(location.pathname))
  const selectedSpaceRef = createMemo(() => routeSpaceRef() ?? defaultSpace(spaces())?.space_ref)
  const [threadsLoadFailed, setThreadsLoadFailed] = createSignal(false)
  let latestThreadRequestRef: string | undefined
  const [threads] = createResource(selectedSpaceRef, async (spaceRef) => {
    latestThreadRequestRef = spaceRef
    setThreadsLoadFailed(false)
    try {
      return await getSpaceThreads(spaceRef)
    } catch {
      if (latestThreadRequestRef === spaceRef) setThreadsLoadFailed(true)
      return undefined
    }
  })
  const normalizedQuery = createMemo(() => query().trim().toLocaleLowerCase())
  const personalSpaces = createMemo(() => (spaces() ?? []).filter((space) => space.kind === 'personal'))
  // Channels, Slack-shaped: the organization's own room pins to the top, the
  // rest keep the server's order. Sorted copy — never mutate the resource value.
  const otherSpaces = createMemo(() => (spaces() ?? [])
    .filter((space) => space.kind !== 'personal')
    .slice()
    .sort((a, b) => Number(b.is_organization_room === true) - Number(a.is_organization_room === true)))
  const visiblePersonalSpaces = createMemo(() => personalSpaces().filter((space) => matchesSpace(space, normalizedQuery(), i18n.tr)))
  const visibleOtherSpaces = createMemo(() => otherSpaces().filter((space) => matchesSpace(space, normalizedQuery(), i18n.tr)))
  const visibleThreads = createMemo(() => (threads()?.threads ?? []).filter((thread) => matchesThread(thread, normalizedQuery())))
  const selectedSpace = createMemo(() => threads()?.space ?? (selectedSpaceRef()
    ? (spaces() ?? []).find((space) => space.space_ref === selectedSpaceRef())
    : undefined))

  return (
    <div class="core-sidebar-panel core-sidebar-spaces-panel">
      <SidebarPanelTitle onCollapse={props.onCollapse}>{i18n.tr('Rom', 'Spaces')}</SidebarPanelTitle>
      <SidebarSearchField
        ariaLabel={i18n.tr('Søk i rom og samtaler', 'Search rooms and conversations')}
        placeholder={i18n.tr('Søk i rom og samtaler', 'Search rooms and conversations')}
        value={query()}
        onChange={setQuery}
      />

      <nav class="core-sidebar-panel-nav" aria-label={i18n.tr('Romnavigasjon', 'Spaces navigation')}>
        <Show when={spaces.loading}>
          <p class="core-sidebar-empty verevon-sidebar-row-normal" role="status">{i18n.tr('Laster rom …', 'Loading spaces…')}</p>
        </Show>
        <Show when={spacesLoadFailed()}>
          <p class="core-sidebar-empty verevon-sidebar-row-normal" role="alert">
            {i18n.tr('Rom kunne ikke lastes. Prøv igjen fra Rom-siden.', 'Spaces could not be loaded. Try again from the Spaces page.')}
          </p>
        </Show>

        <Show when={!spaces.loading && !spacesLoadFailed() && visibleOtherSpaces().length > 0}>
          <SidebarGroup title={i18n.tr('Kanaler', 'Channels')}>
            <For each={visibleOtherSpaces()}>
              {(space) => <SpaceLink space={space} active={space.space_ref === selectedSpaceRef()} />}
            </For>
          </SidebarGroup>
        </Show>

        <Show when={!spaces.loading && !spacesLoadFailed()}>
          <Show
            when={roomFormOpen()}
            fallback={
              <button
                type="button"
                class="core-sidebar-panel-link core-sidebar-new-room"
                onClick={() => setRoomFormOpen(true)}
              >
                <Plus class="core-sidebar-panel-link__icon" strokeWidth={1.7} />
                <span class="core-sidebar-panel-link__label verevon-sidebar-row-normal">
                  {i18n.tr('Nytt rom', 'New room')}
                </span>
              </button>
            }
          >
            <form class="core-sidebar-new-room-form" onSubmit={(event) => void createNamedRoom(event)}>
              <input
                type="text"
                value={roomName()}
                aria-label={i18n.tr('Navn på rommet', 'Room name')}
                placeholder={i18n.tr('Navn på rommet', 'Room name')}
                maxlength={120}
                disabled={creatingRoom()}
                onInput={(event) => setRoomName(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Escape') {
                    setRoomFormOpen(false)
                    setRoomName('')
                  }
                }}
                ref={(element) => queueMicrotask(() => element.focus())}
              />
              <div class="core-sidebar-new-room-form__actions">
                <button type="submit" disabled={creatingRoom() || !roomName().trim()}>
                  {creatingRoom() ? i18n.tr('Oppretter …', 'Creating…') : i18n.tr('Opprett', 'Create')}
                </button>
                <button
                  type="button"
                  disabled={creatingRoom()}
                  onClick={() => {
                    setRoomFormOpen(false)
                    setRoomName('')
                  }}
                >
                  {i18n.tr('Avbryt', 'Cancel')}
                </button>
              </div>
            </form>
          </Show>
        </Show>

        <Show when={createRoomError()}>
          {(message) => (
            <p class="core-sidebar-empty verevon-sidebar-row-normal" role="alert">{message()}</p>
          )}
        </Show>

        <Show when={!spaces.loading && !spacesLoadFailed() && visiblePersonalSpaces().length > 0}>
          <SidebarGroup title={i18n.tr('Personlig rom', 'Personal Space')}>
            <For each={visiblePersonalSpaces()}>
              {(space) => <SpaceLink space={space} active={space.space_ref === selectedSpaceRef()} />}
            </For>
          </SidebarGroup>
        </Show>

        <Show when={!spaces.loading && !spacesLoadFailed() && (spaces() ?? []).length === 0 && !normalizedQuery()}>
          <p class="core-sidebar-empty verevon-sidebar-row-normal">
            {i18n.tr('Ingen rom er tilgjengelige.', 'No Spaces are available.')}
          </p>
        </Show>

        <Show when={selectedSpace() && !threadsLoadFailed()}>
          <SidebarGroup title={i18n.tr('Alle samtaler', 'All conversations')}>
            <Show when={threads.loading}>
              <p class="core-sidebar-empty verevon-sidebar-row-normal" role="status">{i18n.tr('Laster samtaler …', 'Loading conversations…')}</p>
            </Show>
            <Show when={!threads.loading && visibleThreads().length > 0}>
              <nav aria-label={i18n.tr(`Samtaler i ${selectedSpace()?.name ?? ''}`, `Conversations in ${selectedSpace()?.name ?? ''}`)}>
                <For each={visibleThreads()}>{(thread) => <SpaceThreadLink thread={thread} />}</For>
              </nav>
            </Show>
            <Show when={!threads.loading && visibleThreads().length === 0}>
              <p class="core-sidebar-empty verevon-sidebar-row-normal">
                {normalizedQuery()
                  ? i18n.tr('Ingen samtaler matcher søket.', 'No conversations match the search.')
                  : i18n.tr('Ingen samtaler i dette rommet ennå.', 'No conversations in this Space yet.')}
              </p>
            </Show>
          </SidebarGroup>
        </Show>

        <Show when={threadsLoadFailed()}>
          <p class="core-sidebar-empty verevon-sidebar-row-normal" role="alert">
            {i18n.tr('Samtaler kunne ikke lastes. Romtilgangen er uendret.', 'Conversations could not be loaded. Space access is unchanged.')}
          </p>
        </Show>
      </nav>
    </div>
  )
}

function SidebarGroup(props: { title: string; children: JSX.Element }) {
  return (
    <section class="core-sidebar-group">
      <div class="core-sidebar-group__header">
        <span class="verevon-sidebar-group-title">{props.title}</span>
      </div>
      <div class="core-sidebar-group__items">{props.children}</div>
    </section>
  )
}

function SpaceLink(props: { space: SpaceSummary; active: boolean }) {
  const i18n = useI18n()
  // Slack's channel dot, for agent work. Read from the live-work store, which
  // the Space page publishes on its own poll — the sidebar fetches nothing
  // extra. It can only know about rooms whose page has published, so an
  // unobserved room shows no dot rather than a calm one: absence here means
  // "not looked", never "idle".
  const working = () => isSpaceObserved(props.space.space_ref) && isSpaceWorking(props.space.space_ref)
  return (
    <a
      href={spaceHref(props.space.space_ref)}
      link
      aria-current={props.active ? 'page' : undefined}
      class={cn('core-sidebar-panel-link', props.active && 'verevon-sidebar-panel-active core-sidebar-panel-link--active')}
      data-working={working() ? 'true' : undefined}
    >
      <Show
        when={props.space.kind === 'personal'}
        fallback={<Hash class="core-sidebar-panel-link__icon" strokeWidth={1.7} />}
      >
        <Users class="core-sidebar-panel-link__icon" strokeWidth={1.7} />
      </Show>
      <span class="core-sidebar-panel-link__label verevon-sidebar-row-strong">
        {spaceDisplayName(props.space, i18n.tr)}
      </span>
      <Show when={working()}>
        <span
          class="core-sidebar-working-dot"
          role="img"
          aria-label={i18n.tr('Agentarbeid pågår', 'Agent work in progress')}
        />
      </Show>
    </a>
  )
}

function SpaceThreadLink(props: { thread: SpaceThread }) {
  const i18n = useI18n()
  const title = () => threadTitle(props.thread, i18n)
  // This panel fetched the thread list once, on mount, so its status was stale
  // for the rest of the session. When the room's page has published a fresher
  // projection, that wins for the live-ness question; the mounted copy is
  // still the source for title and preview.
  const liveNow = () => liveThreadsIn(props.thread.space_id).some((t) => t.thread_id === props.thread.thread_id)
  const current = (): SpaceThread =>
    isSpaceObserved(props.thread.space_id)
      ? { ...props.thread, latest_run_status: liveNow() ? 'running' : props.thread.latest_run_status === 'running' ? 'completed' : props.thread.latest_run_status }
      : props.thread
  // New since the reader arrived, from the page's published derivation (item
  // 4b) — the same set the timeline badges, so the two never disagree.
  const unread = () => unreadThreadIdsIn(props.thread.space_id).has(props.thread.thread_id)
  return (
    <a
      href={spaceHref(props.thread.space_id)}
      link
      class="core-sidebar-panel-link core-sidebar-space-thread"
      aria-label={threadLinkLabel(current(), i18n, { unread: unread() })}
      data-working={liveNow() ? 'true' : undefined}
      data-pinned={props.thread.pinned === true ? 'true' : undefined}
      data-unread={unread() ? 'true' : undefined}
    >
      <Show
        when={props.thread.pinned === true}
        fallback={<MessageCircle class="core-sidebar-panel-link__icon" strokeWidth={1.7} />}
      >
        <Pin class="core-sidebar-panel-link__icon core-sidebar-panel-link__icon--pinned" strokeWidth={1.7} />
      </Show>
      <span class="core-sidebar-panel-link__label verevon-sidebar-row-normal">{title()}</span>
      <Show when={liveNow()}>
        <span class="core-sidebar-working-dot" aria-hidden="true" />
      </Show>
      <Show when={unread() && !liveNow()}>
        <span class="core-sidebar-unread-dot" aria-hidden="true" />
      </Show>
    </a>
  )
}

function spaceRefFromPath(pathname: string): string | undefined {
  if (!pathname.startsWith('/spaces/')) return undefined
  const encodedRef = pathname.slice('/spaces/'.length)
  if (!encodedRef) return undefined
  try {
    return decodeURIComponent(encodedRef)
  } catch {
    return encodedRef
  }
}

function defaultSpace(spaces: readonly SpaceSummary[] | undefined): SpaceSummary | undefined {
  if (!spaces) return undefined
  // Mirrors pickDefaultSpace on /spaces: the organization's channel first, so
  // the panel highlights the room the resolver would land in.
  return spaces.find((space) => space.kind === 'room' && space.is_organization_room === true && space.lifecycle === 'active')
    ?? spaces.find((space) => space.kind === 'room' && space.lifecycle === 'active')
    ?? spaces.find((space) => space.kind === 'personal' && space.lifecycle === 'active')
    ?? spaces.find((space) => space.kind === 'personal')
    ?? spaces.find((space) => space.lifecycle === 'active')
    ?? spaces[0]
}

function matchesSpace(
  space: SpaceSummary,
  query: string,
  tr: (no: string, en: string) => string,
): boolean {
  // Search the name on screen as well as the stored one: a personal Space reads
  // as "Personlig rom" but is stored as "Personal Space", so matching only the
  // stored value would hide the room from someone typing what they can see.
  const haystack = `${space.name} ${spaceDisplayName(space, tr)} ${space.kind} ${space.lifecycle}`
  return !query || haystack.toLocaleLowerCase().includes(query)
}

function matchesThread(thread: SpaceThread, query: string): boolean {
  return !query || `${threadTitle(thread)} ${thread.preview ?? ''}`.toLocaleLowerCase().includes(query)
}

// A Space conversation opens in its room — the shared record lives there, and
// routing it to /chat was the exact separation-of-concerns leak the product
// model forbids (Chat is Verevon's own surface, not the room's reader).
function spaceHref(spaceRef: string): string {
  return `/spaces/${encodeURIComponent(spaceRef)}`
}

function threadTitle(thread: SpaceThread, i18n?: ReturnType<typeof useI18n>): string {
  return thread.title?.trim() || thread.preview?.trim() || i18n?.tr('Samtale uten tittel', 'Untitled conversation') || 'Untitled conversation'
}

function threadLinkLabel(
  thread: SpaceThread,
  i18n: ReturnType<typeof useI18n>,
  flags: { readonly unread?: boolean } = {},
): string {
  const detail = thread.preview?.trim() || i18n.tr('Åpne samtale', 'Open conversation')
  const marks = [
    thread.pinned === true ? i18n.tr('Festet', 'Pinned') : '',
    flags.unread ? i18n.tr('Ny siden sist', 'New since your last visit') : '',
  ]
    .filter(Boolean)
    .join('. ')
  const status = thread.latest_run_status === 'running'
    ? i18n.tr('Jobber', 'Working')
    : thread.latest_run_status === 'awaiting_approval'
      ? i18n.tr('Trenger godkjenning', 'Needs approval')
      : thread.latest_run_status === 'completed'
        ? i18n.tr('Fullført', 'Completed')
        : i18n.tr('Samtale åpen', 'Conversation open')
  return `${i18n.tr('Åpne', 'Open')} ${threadTitle(thread, i18n)}. ${marks ? `${marks}. ` : ''}${detail} ${status}.`
}
