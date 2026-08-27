import { useLocation } from '@solidjs/router'
import { Hash, MessageCircle, Users } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'

import {
  getSpaceThreads,
  listSpaces,
  type SpaceSummary,
  type SpaceThread,
} from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { createResource } from '@/shared/lib/create-resource-compat'
import { spaceDisplayName } from '@/features/spaces/lib/space-name'
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
  const [spaces] = createResource(async () => {
    setSpacesLoadFailed(false)
    try {
      return await listSpaces()
    } catch {
      setSpacesLoadFailed(true)
      return []
    }
  })
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
  return (
    <a
      href={spaceHref(props.space.space_ref)}
      link
      aria-current={props.active ? 'page' : undefined}
      class={cn('core-sidebar-panel-link', props.active && 'verevon-sidebar-panel-active core-sidebar-panel-link--active')}
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
    </a>
  )
}

function SpaceThreadLink(props: { thread: SpaceThread }) {
  const i18n = useI18n()
  const title = () => threadTitle(props.thread, i18n)
  return (
    <a href={spaceHref(props.thread.space_id)} link class="core-sidebar-panel-link core-sidebar-space-thread" aria-label={threadLinkLabel(props.thread, i18n)}>
      <MessageCircle class="core-sidebar-panel-link__icon" strokeWidth={1.7} />
      <span class="core-sidebar-panel-link__label verevon-sidebar-row-normal">{title()}</span>
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

function threadLinkLabel(thread: SpaceThread, i18n: ReturnType<typeof useI18n>): string {
  const detail = thread.preview?.trim() || i18n.tr('Åpne samtale', 'Open conversation')
  const status = thread.latest_run_status === 'running'
    ? i18n.tr('Jobber', 'Working')
    : thread.latest_run_status === 'awaiting_approval'
      ? i18n.tr('Trenger godkjenning', 'Needs approval')
      : thread.latest_run_status === 'completed'
        ? i18n.tr('Fullført', 'Completed')
        : i18n.tr('Samtale åpen', 'Conversation open')
  return `${i18n.tr('Åpne', 'Open')} ${threadTitle(thread, i18n)}. ${detail} ${status}.`
}
