import { useLocation, useNavigate } from '@solidjs/router'
import { EyeOff, FolderPlus, Hash, MessageCircle, Pin, Plus, SlidersHorizontal, Users } from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, onCleanup, Show, untrack } from 'solid-js'
import type { JSX } from '@solidjs/web'

import {
  createRoom,
  getSpaceThreads,
  listSpaces,
  spaceDmEnabled,
  type SpaceSummary,
  type SpaceThread,
} from '@/shared/api/spaces-client'
import { announce } from '@/shared/a11y/announcer'
import type { SidebarAction, SidebarGroupId } from '@/shared/actions/space-sidebar'
import { translateApiError, useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { createResource } from '@/shared/lib/create-resource-compat'
import { getSession } from '@/shared/session/session-store'
import { narrateSidebarMove } from '@/features/spaces/lib/space-narration'
import { spaceDisplayName } from '@/features/spaces/lib/space-name'
import { buildSidebarModel, type SidebarModelSection } from '@/features/spaces/lib/space-sidebar-organization'
import {
  createSpaceSidebarStore,
  spaceSidebarStorageKey,
  type SidebarStoreState,
} from '@/features/spaces/lib/space-sidebar-store'
import { emptySidebarDoc } from '@/shared/actions/space-sidebar'
import { spaceThreadHref } from '@/features/spaces/lib/space-thread-deep-link'
import {
  isSpaceObserved,
  isSpaceWorking,
  liveThreadsIn,
  unreadThreadIdsIn,
} from '@/features/spaces/lib/space-live-work'
import { setSpaceNotifyLevel, type SpaceNotifyLevel } from '@/shared/api/spaces-attention-client'
import { refreshSpaceAttention, spaceAttentionRoom, spaceAttentionTotals } from '@/features/spaces/lib/space-attention'
import { SidebarPanelTitle, SidebarSearchField } from './CoreSidebarPrimitives'
import { SpaceAttentionReadAll, SpaceRoomAttentionBadge } from './SpaceAttentionBadge'
import { SpaceMoreUnreadPill } from './SpaceMoreUnreadPill'
import { SpaceMoveDialog, SpaceSectionNameDialog } from './SpaceMoveDialog'
import { SpaceRoomMenu, type SpaceMenuItem } from './SpaceRoomMenu'
import { SpaceSidebarSection } from './SpaceSidebarSection'
import { spaceSidebarGroups } from './space-sidebar-groups'
import './space-dm-group'
import { DmStartDialog } from '@/features/spaces/components/DmStartDialog'
import { isDmKind } from '@/features/spaces/lib/space-kind'

/** Drag payload type for a room row (the chat organiser uses `application/x-verevon-chat-thread`). */
const SPACE_ROOM_DRAG_TYPE = 'application/x-verevon-space-room'

/** What the panel draws while there is no signed-in scope (no org or user): nothing to organise. */
const NO_SCOPE_STATE: SidebarStoreState = {
  mode: 'server',
  revision: 0,
  doc: emptySidebarDoc(),
  status: 'idle',
  canEdit: false,
  browserOnly: false,
}

/** Where a row is drawn, which decides what a drop on it or an `Alt+Arrow` does. */
type RowContext =
  | { kind: 'starred'; refs: readonly string[] }
  | { kind: 'section'; id: string; refs: readonly string[]; manual: boolean }
  | { kind: 'group'; id: string }
  | { kind: 'hidden' }
  /** A computed overlay (Needs you, Unread, Agent work): a second copy of a room, never dragged or reordered. */
  | { kind: 'computed' }

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
  // WS12-S3: "New message" (a DM or group DM). The button shows only when the listing said DMs are on.
  const navigate = useNavigate()
  const [dmDialogOpen, setDmDialogOpen] = createSignal(false)

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
  // Channels, Slack-shaped: the organization's own room pins to the top, archived
  // rooms sink to the bottom, the rest keep the server's order. Sorted copy —
  // never mutate the resource value.
  const otherSpaces = createMemo(() => (spaces() ?? [])
    .filter((space) => space.kind !== 'personal' && !isDmKind(space.kind))
    .slice()
    .sort((a, b) => Number(a.archived === true) - Number(b.archived === true)
      || Number(b.is_organization_room === true) - Number(a.is_organization_room === true)))
  const visiblePersonalSpaces = createMemo(() => personalSpaces().filter((space) => matchesSpace(space, normalizedQuery(), i18n.tr)))
  const visibleOtherSpaces = createMemo(() => otherSpaces().filter((space) => matchesSpace(space, normalizedQuery(), i18n.tr)))
  // Legacy panel only (the organiser draws DMs through the registered `dms` group). A DM the caller hid is not listed.
  const visibleDmSpaces = createMemo(() => (spaces() ?? [])
    .filter((space) => isDmKind(space.kind) && space.hidden !== true)
    .filter((space) => matchesSpace(space, normalizedQuery(), i18n.tr)))
  const visibleThreads = createMemo(() => (threads()?.threads ?? []).filter((thread) => matchesThread(thread, normalizedQuery())))
  const selectedSpace = createMemo(() => threads()?.space ?? (selectedSpaceRef()
    ? (spaces() ?? []).find((space) => space.space_ref === selectedSpaceRef())
    : undefined))

  // ---- Personal organisation (WS16-S9) -------------------------------------------------------
  // The room list still comes from `listSpaces()` above (Control's index); the sidebar document only
  // decides how those rooms are drawn: stars, custom sections, hidden rooms, per-group sort and
  // collapse. It never adds a room and nothing in authorization reads it. A 404 from the gateway
  // (flag off) selects the legacy panel below; an organisation under Zero Data Retention keeps the
  // document in this browser only (the store never PUTs in that mode).
  const session = getSession()
  const scopeKey = createMemo(() => spaceSidebarStorageKey(session.activeOrg?.id, session.user?.id))
  const store = createMemo(() => {
    const key = scopeKey()
    if (!key) return null
    const created = untrack(() => createSpaceSidebarStore({ scopeKey: key }))
    onCleanup(() => created.dispose())
    return created
  })
  const sidebar = createMemo<SidebarStoreState>(() => store()?.state() ?? NO_SCOPE_STATE)
  const legacy = () => sidebar().mode === 'disabled'
  createEffect(
    () => ({ target: store(), list: spaces(), failed: spacesLoadFailed() }),
    ({ target, list, failed }) => {
      // Prune only against a listing we trust: never against a failed or empty one.
      target?.setVisibleRefs(!failed && list && list.length > 0 ? new Set(list.map((space) => space.space_ref)) : null)
    },
  )
  const [showHidden, setShowHidden] = createSignal(false)
  // WS16-S10: computed sections read WS11's attention store. Until the store holds a live answer
  // (surface off, first poll pending, ZDR-empty) slots 1 and 5 and the Smart sections menu do not
  // render, and nothing falls back to the browser store.
  const attentionLive = () => spaceAttentionTotals() !== undefined
  const attentionOf = (ref: string) => {
    const room = spaceAttentionRoom(ref)
    return room
      ? { needsYou: room.needsYou, mentions: room.mentions, unreadThreads: room.unreadThreads, working: room.working, level: room.level }
      : undefined
  }
  const [navEl, setNavEl] = createSignal<HTMLElement>()
  const [notice, setNotice] = createSignal('')
  // Signal writes are batched, so the message a failed `run` leaves for a dialog is kept here too.
  let lastError = ''
  const model = createMemo(() => buildSidebarModel({
    spaces: spaces() ?? [],
    doc: sidebar().doc,
    groups: spaceSidebarGroups(),
    query: normalizedQuery(),
    matches: (space, term) => matchesSpace(space, term, i18n.tr),
    displayName: (space) => spaceDisplayName(space, i18n.tr),
    locale: i18n.localeCode(),
    attention: attentionLive() ? attentionOf : undefined,
  }))
  const groupDefs = new Map(spaceSidebarGroups().map((def) => [def.id as string, def]))
  const roomLabel = (space: SpaceSummary) => spaceDisplayName(space, i18n.tr)
  const sectionName = (id: string) => sidebar().doc.sections.find((section) => section.id === id)?.name ?? ''
  const homeLabel = (space: SpaceSummary) => {
    const def = spaceSidebarGroups().find((group) => group.match(space)) ?? groupDefs.get('channels')
    return def ? def.label(i18n.tr) : i18n.tr('Rom', 'Spaces')
  }
  const sectionOf = (ref: string) => sidebar().doc.sections.find((section) => section.refs.includes(ref))?.id ?? ''

  /** One dispatch, one announcement. Failures show in the alert line; nothing is half-applied. */
  function run(actions: SidebarAction[], spoken?: string): boolean {
    const target = store()
    if (!target) return false
    try {
      target.dispatch(...actions)
      lastError = ''
      setNotice('')
      if (spoken) announce(spoken, { politeness: 'polite', kind: 'status' })
      return true
    } catch (reason) {
      lastError = translateApiError(reason, i18n.tr, {
        no: 'Kunne ikke lagre oppsettet.',
        en: 'Could not save your layout.',
      })
      setNotice(lastError)
      return false
    }
  }
  const moved = (space: SpaceSummary, destination: string) => narrateSidebarMove(i18n.tr, roomLabel(space), destination).message

  const toggleStar = (space: SpaceSummary, isStarred: boolean) => {
    const ref = space.space_ref
    if (isStarred) run([{ type: 'unstar', ref }], moved(space, homeLabel(space)))
    else run([{ type: 'star', ref }], moved(space, i18n.tr('Stjernemerkede', 'Starred')))
  }
  const toggleHide = (space: SpaceSummary, isHidden: boolean) => {
    const ref = space.space_ref
    if (isHidden) run([{ type: 'room.unhide', ref }], moved(space, homeLabel(space)))
    else run([{ type: 'room.hide', ref }], moved(space, i18n.tr('Skjulte rom', 'Hidden rooms')))
  }
  const moveToSection = (space: SpaceSummary, sectionId: string | null, beforeRef?: string | null) => {
    const destination = sectionId ? sectionName(sectionId) : homeLabel(space)
    return run([{ type: 'room.move', ref: space.space_ref, sectionId, beforeRef: beforeRef ?? null }], moved(space, destination))
  }
  const toggleSection = (section: SidebarModelSection['section']) => {
    run([{ type: 'section.collapse', id: section.id, collapsed: !section.collapsed }])
  }
  const toggleGroup = (id: string, collapsed: boolean) => {
    run([{ type: 'group.update', groupId: id as SidebarGroupId, collapsed: !collapsed }])
  }

  // Dialogs
  const [moveFor, setMoveFor] = createSignal<SpaceSummary | null>(null)
  const [sectionDialog, setSectionDialog] = createSignal<{ mode: 'create' | 'rename'; id: string; initial: string } | null>(null)
  const [dialogError, setDialogError] = createSignal('')
  const closeDialogs = () => {
    setMoveFor(null)
    setSectionDialog(null)
    setDialogError('')
  }
  const submitMove = (sectionId: string | null) => {
    const space = moveFor()
    if (!space) return
    if (moveToSection(space, sectionId)) closeDialogs()
    else setDialogError(lastError)
  }
  const submitSectionName = (name: string) => {
    const dialog = sectionDialog()
    if (!dialog) return
    const action: SidebarAction = dialog.mode === 'create'
      ? { type: 'section.create', id: crypto.randomUUID(), name }
      : { type: 'section.rename', id: dialog.id, name }
    if (run([action])) closeDialogs()
    else setDialogError(lastError)
  }

  // Reordering by keyboard (Alt+ArrowUp/Down) and by the row menu: the same actions as a drop.
  function reorder(space: SpaceSummary, context: RowContext, direction: -1 | 1) {
    if (normalizedQuery() || (context.kind !== 'starred' && !(context.kind === 'section' && context.manual))) return
    const refs = context.refs
    const index = refs.indexOf(space.space_ref)
    const target = index + direction
    if (index < 0 || target < 0 || target >= refs.length) return
    const beforeRef = (direction === -1 ? refs[target] : refs[target + 1]) ?? null
    const ok = run(
      [context.kind === 'starred'
        ? { type: 'star.move', ref: space.space_ref, beforeRef }
        : { type: 'room.move', ref: space.space_ref, sectionId: context.id, beforeRef }],
      i18n.tr(
        `${roomLabel(space)}: plass ${target + 1} av ${refs.length}`,
        `${roomLabel(space)}: position ${target + 1} of ${refs.length}`,
      ),
    )
    if (ok) {
      // The list re-renders; put focus back on the row that moved.
      requestAnimationFrame(() => {
        document
          .querySelector<HTMLElement>(`[data-room-ref="${CSS.escape(space.space_ref)}"] > a`)
          ?.focus()
      })
    }
  }
  const canReorder = (context: RowContext) =>
    !normalizedQuery() && (context.kind === 'starred' || (context.kind === 'section' && context.manual))

  // Drag and drop (native HTML5, the organiser pattern). `Escape` cancels. Dropping is the same
  // validated action the menu and the dialog dispatch.
  let activeDrag: { ref: string } | null = null
  const [dragRef, setDragRef] = createSignal<string | null>(null)
  const [dropTarget, setDropTarget] = createSignal<string | null>(null)
  const [dropBefore, setDropBefore] = createSignal<string | null>(null)
  const endDrag = () => {
    activeDrag = null
    setDragRef(null)
    setDropTarget(null)
    setDropBefore(null)
  }
  createEffect(() => undefined, () => {
    const cancel = (event: KeyboardEvent) => { if (event.key === 'Escape') endDrag() }
    window.addEventListener('dragend', endDrag)
    window.addEventListener('keydown', cancel)
    return () => {
      window.removeEventListener('dragend', endDrag)
      window.removeEventListener('keydown', cancel)
    }
  })
  const startDrag = (event: DragEvent, space: SpaceSummary) => {
    if (!sidebar().canEdit || !event.dataTransfer
      || (event.target instanceof Element && event.target.closest('input, textarea, select, [contenteditable="true"]'))) {
      event.preventDefault()
      return
    }
    activeDrag = { ref: space.space_ref }
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData(SPACE_ROOM_DRAG_TYPE, space.space_ref)
    setDragRef(space.space_ref)
    setNotice('')
  }
  const accepts = (event: DragEvent) =>
    sidebar().canEdit && activeDrag !== null && Boolean(event.dataTransfer?.types.includes(SPACE_ROOM_DRAG_TYPE))
  const draggedSpace = () => (spaces() ?? []).find((space) => space.space_ref === activeDrag?.ref)
  const overTarget = (event: DragEvent, id: string) => {
    if (!accepts(event)) return
    event.preventDefault()
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
    setDropTarget(id)
  }
  const leaveTarget = (event: DragEvent) => {
    if (!(event.relatedTarget instanceof Node) || !(event.currentTarget as Node).contains(event.relatedTarget)) {
      setDropTarget(null)
      setDropBefore(null)
    }
  }
  /** Drop on a whole section / group / Starred header: put the room at the end. */
  const dropOn = (event: DragEvent, target: { kind: 'starred' } | { kind: 'section'; id: string } | { kind: 'group' }) => {
    event.preventDefault()
    const space = draggedSpace()
    const valid = accepts(event) && space && event.dataTransfer?.getData(SPACE_ROOM_DRAG_TYPE) === activeDrag?.ref
    if (valid && space) {
      if (target.kind === 'starred') {
        if (!sidebar().doc.starred.includes(space.space_ref)) toggleStar(space, false)
      } else if (target.kind === 'section') moveToSection(space, target.id)
      else moveToSection(space, null)
    }
    endDrag()
  }
  /** Drop on a row of Starred or a manual section: put the room before that row. */
  const dropBeforeRow = (event: DragEvent, context: RowContext, before: SpaceSummary) => {
    if (!accepts(event) || (context.kind !== 'starred' && !(context.kind === 'section' && context.manual))) return
    event.preventDefault()
    event.stopPropagation()
    const space = draggedSpace()
    if (space && space.space_ref !== before.space_ref) {
      if (context.kind === 'starred') {
        const actions: SidebarAction[] = []
        if (!sidebar().doc.starred.includes(space.space_ref)) actions.push({ type: 'star', ref: space.space_ref })
        actions.push({ type: 'star.move', ref: space.space_ref, beforeRef: before.space_ref })
        run(actions, moved(space, i18n.tr('Stjernemerkede', 'Starred')))
      } else {
        moveToSection(space, context.id, before.space_ref)
      }
    }
    endDrag()
  }

  function menuItems(space: SpaceSummary, context: RowContext): SpaceMenuItem[] {
    const ref = space.space_ref
    const isStarred = sidebar().doc.starred.includes(ref)
    const isHidden = sidebar().doc.hidden.includes(ref)
    const items: SpaceMenuItem[] = [
      {
        id: 'star',
        label: isStarred ? i18n.tr('Fjern stjerne', 'Unstar') : i18n.tr('Stjernemerk', 'Star'),
        onSelect: () => toggleStar(space, isStarred),
      },
      {
        id: 'move',
        label: i18n.tr('Flytt til gruppe …', 'Move to section…'),
        onSelect: () => { setDialogError(''); setMoveFor(space) },
      },
      {
        id: 'hide',
        label: isHidden ? i18n.tr('Vis igjen', 'Unhide') : i18n.tr('Skjul', 'Hide'),
        onSelect: () => toggleHide(space, isHidden),
      },
      ...muteItems(space),
      ...notifyLevelItems(space),
    ]
    if (canReorder(context) && context.kind !== 'group' && context.kind !== 'hidden' && context.kind !== 'computed') {
      const index = context.refs.indexOf(ref)
      items.push(
        { id: 'up', label: i18n.tr('Flytt opp', 'Move up'), disabled: index <= 0, onSelect: () => reorder(space, context, -1) },
        { id: 'down', label: i18n.tr('Flytt ned', 'Move down'), disabled: index < 0 || index >= context.refs.length - 1, onSelect: () => reorder(space, context, 1) },
      )
    }
    return items
  }

  // WS11-S4: the caller's own notification level for the room (a closed vocabulary, allowed under
  // ZDR). Offered only once the attention aggregate has answered for this room, so a gateway with the
  // surface off shows no dead items; the checkmark is the level the server last reported.
  // WS16-S10: Mute is WS11's notify level `off`, not a sidebar preference (there is exactly one mute
  // store and it is not agent pause). Unmute restores the default level, `mentions`. Offered only
  // once the aggregate has answered for the room, like the level items.
  function muteItems(space: SpaceSummary): SpaceMenuItem[] {
    const current = spaceAttentionRoom(space.space_ref)?.level
    if (!current) return []
    const muted = current === 'off'
    const next: SpaceNotifyLevel = muted ? 'mentions' : 'off'
    const spoken = muted
      ? i18n.tr(`${roomLabel(space)} er ikke lenger dempet.`, `${roomLabel(space)} unmuted.`)
      : i18n.tr(`${roomLabel(space)} er dempet.`, `${roomLabel(space)} muted.`)
    return [{
      id: 'mute',
      checked: muted,
      label: i18n.tr('Demp rom', 'Mute room'),
      onSelect: () => {
        void setSpaceNotifyLevel(space.space_ref, next)
          .then(() => {
            announce(spoken, { key: 'space-notify-level' })
            refreshSpaceAttention()
          })
          .catch((error: unknown) => {
            announce(translateApiError(error, i18n.tr), { politeness: 'assertive' })
          })
      },
    }]
  }

  /** The computed sections the person turns on or off (stored in the sidebar document). */
  function computedItems(): SpaceMenuItem[] {
    const computed = sidebar().doc.computed
    const options: ReadonlyArray<{ key: keyof typeof computed; no: string; en: string }> = [
      { key: 'needsYou', no: 'Trenger deg', en: 'Needs you' },
      { key: 'unread', no: 'Uleste', en: 'Unread' },
      { key: 'working', no: 'Agentarbeid', en: 'Agent work' },
      { key: 'muted', no: 'Dempede', en: 'Muted' },
    ]
    return options.map((option) => ({
      id: `computed-${option.key}`,
      checked: computed[option.key],
      label: i18n.tr(option.no, option.en),
      onSelect: () => {
        const on = !computed[option.key]
        run(
          [{ type: 'computed.toggle', key: option.key, on }],
          i18n.tr(`${option.no}: ${on ? 'på' : 'av'}`, `${option.en} section ${on ? 'on' : 'off'}`),
        )
      },
    }))
  }

  function notifyLevelItems(space: SpaceSummary): SpaceMenuItem[] {
    const current = spaceAttentionRoom(space.space_ref)?.level
    if (!current) return []
    const options: ReadonlyArray<{ level: SpaceNotifyLevel; no: string; en: string }> = [
      { level: 'all', no: 'Varsler: alle meldinger', en: 'Notify: all messages' },
      { level: 'mentions', no: 'Varsler: bare omtaler', en: 'Notify: mentions only' },
    ]
    return options.map((option) => ({
      id: `notify-${option.level}`,
      label: `${option.level === current ? '✓ ' : ''}${i18n.tr(option.no, option.en)}`,
      onSelect: () => {
        if (option.level === current) return
        void setSpaceNotifyLevel(space.space_ref, option.level)
          .then(() => {
            announce(i18n.tr(option.no, option.en), { key: 'space-notify-level' })
            refreshSpaceAttention()
          })
          .catch((error: unknown) => {
            announce(translateApiError(error, i18n.tr), { politeness: 'assertive' })
          })
      },
    }))
  }

  function sectionMenuItems(entry: SidebarModelSection): SpaceMenuItem[] {
    const { section } = entry
    const index = sidebar().doc.sections.findIndex((item) => item.id === section.id)
    const total = sidebar().doc.sections.length
    const nextSection = sidebar().doc.sections[index + 2]?.id ?? null
    return [
      {
        id: 'rename',
        label: i18n.tr('Gi nytt navn …', 'Rename…'),
        onSelect: () => { setDialogError(''); setSectionDialog({ mode: 'rename', id: section.id, initial: section.name }) },
      },
      {
        id: 'sort',
        label: section.sort === 'name' ? i18n.tr('Sorter manuelt', 'Sort by hand') : i18n.tr('Sorter etter navn', 'Sort by name'),
        onSelect: () => { run([{ type: 'section.sort', id: section.id, sort: section.sort === 'name' ? 'manual' : 'name' }]) },
      },
      {
        id: 'up',
        label: i18n.tr('Flytt gruppen opp', 'Move section up'),
        disabled: index <= 0,
        onSelect: () => { run([{ type: 'section.move', id: section.id, beforeId: sidebar().doc.sections[index - 1]?.id ?? null }]) },
      },
      {
        id: 'down',
        label: i18n.tr('Flytt gruppen ned', 'Move section down'),
        disabled: index < 0 || index >= total - 1,
        onSelect: () => { run([{ type: 'section.move', id: section.id, beforeId: nextSection }]) },
      },
      {
        id: 'remove',
        label: i18n.tr('Fjern gruppen, behold rommene', 'Remove section, keep rooms'),
        danger: true,
        onSelect: () => { run([{ type: 'section.remove', id: section.id }]) },
      },
    ]
  }

  function groupMenuItems(id: string, sort: 'name' | 'activity' | null): SpaceMenuItem[] {
    return [{
      id: 'sort',
      label: sort === 'name' ? i18n.tr('Standard rekkefølge', 'Default order') : i18n.tr('Sorter etter navn', 'Sort by name'),
      onSelect: () => { run([{ type: 'group.update', groupId: id as SidebarGroupId, sort: sort === 'name' ? null : 'name' }]) },
    }]
  }

  /** A room row: the link, its drag behaviour and its menu. */
  function renderRow(space: SpaceSummary, context: RowContext): JSX.Element {
    const overlay = context.kind === 'computed'
    const draggable = () => sidebar().canEdit && !overlay
    // The pill watches rows that carry a server badge; the overlay copy is skipped so a room counts once.
    const badgeN = () => {
      if (overlay || isSpaceObserved(space.space_ref)) return undefined
      const badge = spaceAttentionRoom(space.space_ref)?.badge
      return badge && badge.kind === 'count' && badge.n > 0 ? badge.n : undefined
    }
    return (
      <div
        class="space-sidebar-row"
        data-room-ref={overlay ? undefined : space.space_ref}
        data-overlay-ref={overlay ? space.space_ref : undefined}
        data-badge-n={badgeN()}
        data-muted={model().mutedRefs.has(space.space_ref) ? 'true' : undefined}
        draggable={draggable() ? 'true' : 'false'}
        data-dragged={dragRef() === space.space_ref ? 'true' : undefined}
        data-drop-before={dropBefore() === space.space_ref ? 'true' : undefined}
        title={draggable() ? i18n.tr('Dra til en gruppe, eller bruk menyen.', 'Drag to a section, or use the menu.') : undefined}
        onDragStart={(event) => startDrag(event, space)}
        onDragEnd={endDrag}
        onDragOver={(event) => {
          if (!accepts(event) || (context.kind !== 'starred' && !(context.kind === 'section' && context.manual))) return
          if (activeDrag?.ref === space.space_ref) return
          event.preventDefault()
          if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
          setDropBefore(space.space_ref)
        }}
        onDrop={(event) => dropBeforeRow(event, context, space)}
        onKeyDown={(event) => {
          if (!event.altKey || (event.key !== 'ArrowUp' && event.key !== 'ArrowDown')) return
          if (!canReorder(context)) return
          event.preventDefault()
          reorder(space, context, event.key === 'ArrowUp' ? -1 : 1)
        }}
      >
        <SpaceLink space={space} active={space.space_ref === selectedSpaceRef()} muted={model().mutedRefs.has(space.space_ref)} />
        <SpaceRoomMenu
          class="space-sidebar-row__menu"
          label={i18n.tr(`Valg for ${roomLabel(space)}`, `Options for ${roomLabel(space)}`)}
          disabled={!sidebar().canEdit}
          items={menuItems(space, context)}
        />
      </div>
    )
  }

  return (
    <div class="core-sidebar-panel core-sidebar-spaces-panel">
      <SidebarPanelTitle onCollapse={props.onCollapse}>{i18n.tr('Rom', 'Spaces')}</SidebarPanelTitle>
      <SidebarSearchField
        ariaLabel={i18n.tr('Søk i rom og samtaler', 'Search rooms and conversations')}
        placeholder={i18n.tr('Søk i rom og samtaler', 'Search rooms and conversations')}
        value={query()}
        onChange={setQuery}
      />

      <SpaceAttentionReadAll />

      <Show when={!legacy() && attentionLive() && sidebar().canEdit}>
        <div class="space-sidebar-computed-bar">
          <SpaceRoomMenu
            label={i18n.tr('Smarte seksjoner', 'Smart sections')}
            items={computedItems()}
            icon={
              <>
                <SlidersHorizontal size={14} aria-hidden="true" />
                <span>{i18n.tr('Smarte seksjoner', 'Smart sections')}</span>
              </>
            }
          />
        </div>
      </Show>

      <nav ref={setNavEl} class="core-sidebar-panel-nav" aria-label={i18n.tr('Romnavigasjon', 'Spaces navigation')}>
        <Show when={spaces.loading}>
          <p class="core-sidebar-empty verevon-sidebar-row-normal" role="status">{i18n.tr('Laster rom …', 'Loading spaces…')}</p>
        </Show>
        <Show when={spacesLoadFailed()}>
          <p class="core-sidebar-empty verevon-sidebar-row-normal" role="alert">
            {i18n.tr('Rom kunne ikke lastes. Prøv igjen fra Rom-siden.', 'Spaces could not be loaded. Try again from the Spaces page.')}
          </p>
        </Show>

        <Show when={!legacy() && !spaces.loading && !spacesLoadFailed()}>
          <For
            each={[
              { id: 'needs-you', title: i18n.tr('Trenger deg', 'Needs you'), rooms: model().needsYou },
              { id: 'unread', title: i18n.tr('Uleste', 'Unread'), rooms: model().unread },
              { id: 'working', title: i18n.tr('Agentarbeid', 'Agent work'), rooms: model().working },
            ]}
          >
            {(slot) => (
              <Show when={slot.rooms.length > 0}>
                <SpaceSidebarSection id={`computed-${slot.id}`} title={slot.title} count={slot.rooms.length} collapsed={false}>
                  <For each={slot.rooms}>{(space) => renderRow(space, { kind: 'computed' })}</For>
                </SpaceSidebarSection>
              </Show>
            )}
          </For>

          <Show when={model().starred.length > 0 || dragRef() !== null}>
            <SpaceSidebarSection
              id="starred"
              title={i18n.tr('Stjernemerkede', 'Starred')}
              count={model().starred.length}
              collapsed={false}
              emptyHint={i18n.tr('Slipp et rom her, eller bruk menyen for å stjernemerke det.', 'Drop a room here, or use its menu to star it.')}
              dropReady={dragRef() !== null}
              dropActive={dropTarget() === 'starred'}
              onDragOver={(event) => overTarget(event, 'starred')}
              onDragLeave={leaveTarget}
              onDrop={(event) => dropOn(event, { kind: 'starred' })}
            >
              <For each={model().starred}>
                {(space) => renderRow(space, { kind: 'starred', refs: model().starred.map((item) => item.space_ref) })}
              </For>
            </SpaceSidebarSection>
          </Show>

          <For each={model().sections}>
            {(entry) => (
              <Show when={!normalizedQuery() || entry.rooms.length > 0}>
                <SpaceSidebarSection
                  id={`section-${entry.section.id}`}
                  title={entry.section.name}
                  count={entry.rooms.length}
                  collapsed={entry.section.collapsed && !normalizedQuery()}
                  onToggle={sidebar().canEdit ? () => toggleSection(entry.section) : undefined}
                  emptyHint={i18n.tr('Slipp et rom her, eller bruk Flytt til gruppe.', 'Drop a room here, or use Move to section.')}
                  dropReady={dragRef() !== null}
                  dropActive={dropTarget() === `section:${entry.section.id}`}
                  onDragOver={(event) => overTarget(event, `section:${entry.section.id}`)}
                  onDragLeave={leaveTarget}
                  onDrop={(event) => dropOn(event, { kind: 'section', id: entry.section.id })}
                  menu={
                    <SpaceRoomMenu
                      label={i18n.tr(`Valg for gruppen ${entry.section.name}`, `Options for section ${entry.section.name}`)}
                      disabled={!sidebar().canEdit}
                      items={sectionMenuItems(entry)}
                    />
                  }
                >
                  <For each={entry.rooms}>
                    {(space) => renderRow(space, {
                      kind: 'section',
                      id: entry.section.id,
                      refs: entry.rooms.map((item) => item.space_ref),
                      manual: entry.section.sort === 'manual',
                    })}
                  </For>
                </SpaceSidebarSection>
              </Show>
            )}
          </For>

          <For each={model().groups}>
            {(group) => {
              const def = () => groupDefs.get(group.id)
              return (
                <SpaceSidebarSection
                  id={`group-${group.id}`}
                  title={def()?.label(i18n.tr) ?? group.id}
                  count={group.rooms.length}
                  collapsed={group.collapsed && !normalizedQuery()}
                  onToggle={sidebar().canEdit ? () => toggleGroup(group.id, group.collapsed) : undefined}
                  dropReady={dragRef() !== null && def()?.dropTarget === true}
                  dropActive={dropTarget() === `group:${group.id}`}
                  onDragOver={(event) => { if (def()?.dropTarget) overTarget(event, `group:${group.id}`) }}
                  onDragLeave={leaveTarget}
                  onDrop={(event) => dropOn(event, { kind: 'group' })}
                  menu={
                    <SpaceRoomMenu
                      label={i18n.tr(`Valg for ${def()?.label(i18n.tr) ?? group.id}`, `Options for ${def()?.label(i18n.tr) ?? group.id}`)}
                      disabled={!sidebar().canEdit}
                      items={groupMenuItems(group.id, group.sort)}
                    />
                  }
                >
                  <For each={group.rooms}>{(space) => renderRow(space, { kind: 'group', id: group.id })}</For>
                </SpaceSidebarSection>
              )
            }}
          </For>

          <Show when={model().muted.length > 0}>
            <SpaceSidebarSection id="computed-muted" title={i18n.tr('Dempede', 'Muted')} count={model().muted.length} collapsed={false}>
              <For each={model().muted}>{(space) => renderRow(space, { kind: 'hidden' })}</For>
            </SpaceSidebarSection>
          </Show>

          <Show when={model().hiddenCount > 0 && !normalizedQuery()}>
            <button
              type="button"
              class="space-sidebar-hidden-toggle"
              aria-expanded={showHidden() ? 'true' : 'false'}
              onClick={() => setShowHidden((open) => !open)}
            >
              <EyeOff size={14} aria-hidden="true" />
              {showHidden()
                ? i18n.tr(`Skjul skjulte rom (${model().hiddenCount})`, `Hide hidden rooms (${model().hiddenCount})`)
                : i18n.tr(`Vis skjulte rom (${model().hiddenCount})`, `Show hidden rooms (${model().hiddenCount})`)}
            </button>
          </Show>
          <Show when={(showHidden() || normalizedQuery()) && model().hidden.length > 0}>
            <SpaceSidebarSection
              id="hidden"
              title={i18n.tr('Skjulte rom', 'Hidden rooms')}
              count={model().hidden.length}
              collapsed={false}
            >
              <For each={model().hidden}>{(space) => renderRow(space, { kind: 'hidden' })}</For>
            </SpaceSidebarSection>
          </Show>
        </Show>

        <Show when={legacy() && !spaces.loading && !spacesLoadFailed() && visibleOtherSpaces().length > 0}>
          <SidebarGroup title={i18n.tr('Kanaler', 'Channels')}>
            <For each={visibleOtherSpaces()}>
              {(space) => <SpaceLink space={space} active={space.space_ref === selectedSpaceRef()} />}
            </For>
          </SidebarGroup>
        </Show>

        <Show when={legacy() && !spaces.loading && !spacesLoadFailed() && visibleDmSpaces().length > 0}>
          <SidebarGroup title={i18n.tr('Direktemeldinger', 'Direct messages')}>
            <For each={visibleDmSpaces()}>
              {(space) => <SpaceLink space={space} active={space.space_ref === selectedSpaceRef()} />}
            </For>
          </SidebarGroup>
        </Show>

        <Show when={spaceDmEnabled() && !spaces.loading && !spacesLoadFailed()}>
          <button
            type="button"
            class="core-sidebar-panel-link core-sidebar-new-room"
            onClick={() => setDmDialogOpen(true)}
          >
            <MessageCircle class="core-sidebar-panel-link__icon" strokeWidth={1.7} />
            <span class="core-sidebar-panel-link__label verevon-sidebar-row-normal">
              {i18n.tr('Ny melding', 'New message')}
            </span>
          </button>
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

        <Show when={!legacy() && !spaces.loading && !spacesLoadFailed() && sidebar().canEdit}>
          <button
            type="button"
            class="core-sidebar-panel-link core-sidebar-new-room"
            onClick={() => { setDialogError(''); setSectionDialog({ mode: 'create', id: '', initial: '' }) }}
          >
            <FolderPlus class="core-sidebar-panel-link__icon" strokeWidth={1.7} />
            <span class="core-sidebar-panel-link__label verevon-sidebar-row-normal">
              {i18n.tr('Ny gruppe', 'New section')}
            </span>
          </button>
        </Show>
        <Show when={!legacy() && notice()}>
          <p class="space-sidebar-notice" role="alert">{notice()}</p>
        </Show>
        <Show when={!legacy() && sidebar().browserOnly}>
          <p class="space-sidebar-local-note">{i18n.tr('Lagret bare i denne nettleseren.', 'Saved in this browser only.')}</p>
        </Show>
        <Show when={!legacy() && !sidebar().canEdit && sidebar().mode !== 'loading' && sidebar().status === 'error'}>
          <p class="space-sidebar-local-note" role="status">
            {i18n.tr('Oppsettet ditt kan ikke lagres akkurat nå. Rommene er uendret.', 'Your layout cannot be saved right now. Your rooms are unchanged.')}
          </p>
        </Show>

        <Show when={legacy() && !spaces.loading && !spacesLoadFailed() && visiblePersonalSpaces().length > 0}>
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

        <Show when={!legacy() && attentionLive()}>
          <SpaceMoreUnreadPill container={navEl} />
        </Show>
      </nav>

      <DmStartDialog
        open={dmDialogOpen()}
        onClose={() => setDmDialogOpen(false)}
        onOpened={(result) => {
          setDmDialogOpen(false)
          // The DM may still be pending registration, so it can be absent from the listing for a moment.
          void refetchSpaces()
          navigate(spaceHref(result.spaceRef))
        }}
      />
      <SpaceMoveDialog
        roomName={moveFor() ? roomLabel(moveFor() as SpaceSummary) : null}
        sections={sidebar().doc.sections.map((section) => ({ id: section.id, name: section.name }))}
        current={moveFor() ? sectionOf((moveFor() as SpaceSummary).space_ref) : ''}
        error={dialogError()}
        onSubmit={submitMove}
        onClose={closeDialogs}
      />
      <SpaceSectionNameDialog
        mode={sectionDialog()?.mode ?? null}
        initial={sectionDialog()?.initial ?? ''}
        error={dialogError()}
        onSubmit={submitSectionName}
        onClose={closeDialogs}
      />
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

function SpaceLink(props: { space: SpaceSummary; active: boolean; muted?: boolean }) {
  const i18n = useI18n()
  // Slack's channel dot, for agent work. Read from the live-work store, which
  // the Space page publishes on its own poll — the sidebar fetches nothing
  // extra. It can only know about rooms whose page has published, so an
  // unobserved room shows no dot rather than a calm one: absence here means
  // "not looked", never "idle".
  const working = () => isSpaceObserved(props.space.space_ref) && isSpaceWorking(props.space.space_ref)
  // WS11-S4: the server's cross-room badge, for rooms this browser has NOT got open. The open room
  // is narrated by its own page (the live-work store above, and the timeline's divider), so it
  // never shows a second, polled number. Absent store data draws nothing: unknown is not zero.
  const showAttention = () => !isSpaceObserved(props.space.space_ref)
  const attention = () => (showAttention() ? spaceAttentionRoom(props.space.space_ref) : undefined)
  return (
    <a
      href={spaceHref(props.space.space_ref)}
      link
      aria-current={props.active ? 'page' : undefined}
      class={cn('core-sidebar-panel-link', props.active && 'verevon-sidebar-panel-active core-sidebar-panel-link--active')}
      data-working={working() ? 'true' : undefined}
      data-attention={attention() && attention()!.badge.kind !== 'none' ? 'true' : undefined}
      data-archived={props.space.archived === true ? 'true' : undefined}
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
      <Show when={props.space.archived === true}>
        <span class="core-sidebar-archived-tag">{i18n.tr('Arkivert', 'Archived')}</span>
      </Show>
      <Show when={props.muted}>
        <svg
          class="space-sidebar-muted-icon"
          viewBox="0 0 24 24"
          width="13"
          height="13"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          role="img"
          aria-label={i18n.tr('Dempet', 'Muted')}
        >
          <path d="M8.7 3A6 6 0 0 1 18 8a21.3 21.3 0 0 0 .6 5" />
          <path d="M17 17H3s3-2 3-9a4.67 4.67 0 0 1 .3-1.7" />
          <path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" />
          <path d="m2 2 20 20" />
        </svg>
      </Show>
      <Show when={working()}>
        <span
          class="core-sidebar-working-dot"
          role="img"
          aria-label={i18n.tr('Agentarbeid pågår', 'Agent work in progress')}
        />
      </Show>
      <Show when={showAttention()}>
        <SpaceRoomAttentionBadge spaceRef={props.space.space_ref} />
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
      href={spaceThreadHref(props.thread.space_id, props.thread.thread_id)}
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
