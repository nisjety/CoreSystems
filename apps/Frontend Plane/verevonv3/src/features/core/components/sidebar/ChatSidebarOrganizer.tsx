import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { Folder, FolderPlus, MessageSquarePlus, MoreHorizontal, Pin, Search, SlidersHorizontal, X } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'
import type { ChatOrganizationAction } from '@/shared/actions/chat-organization'
import { applyChatOrganizationAction, chatDateGroup, chatOrganizationStorageKey, emptyChatOrganization, readChatOrganization, saveChatOrganization, selectOrganizedChats } from '@/features/chat/lib/chat-organization'
import type { ChatThreadHistoryItem } from '@/features/chat/lib/chat-thread-history'
import './chat-sidebar-organizer.css'

type DialogMode = { kind: 'create' } | { kind: 'project'; id: string } | { kind: 'move'; thread: ChatThreadHistoryItem }
type ChatGroup = { id: string; label: string; project: boolean; pinned?: boolean; items: ChatThreadHistoryItem[] }
const CHAT_DRAG_TYPE = 'application/x-verevon-chat-thread'

export function ChatSidebarOrganizer(props: {
  sessions: ChatThreadHistoryItem[]
  loading: boolean
  onNewChat: () => void
  renderRow: (item: ChatThreadHistoryItem, onMove?: () => void) => JSX.Element
}) {
  const i18n = useI18n()
  const session = getSession()
  const scope = createMemo(() => chatOrganizationStorageKey(session.user?.id, session.activeOrg?.id))
  const [organization, setOrganization] = createSignal(emptyChatOrganization())
  const [query, setQuery] = createSignal('')
  const [searchOpen, setSearchOpen] = createSignal(false)
  const [mode, setMode] = createSignal<DialogMode | null>(null)
  const [draftName, setDraftName] = createSignal('')
  const [destination, setDestination] = createSignal('')
  const [error, setError] = createSignal('')
  const [draggedThreadId, setDraggedThreadId] = createSignal<string | null>(null)
  const [dropTarget, setDropTarget] = createSignal<string | null>(null)
  const [moveNotice, setMoveNotice] = createSignal('')
  // Native drag events can arrive before Solid flushes its signal updates.
  // Keep the transfer identity synchronous; signals only drive visual feedback.
  let activeDrag: { threadId: string; scope: string } | null = null
  let dialog: HTMLDialogElement | undefined
  let nameInput: HTMLInputElement | undefined
  let searchInput: HTMLInputElement | undefined
  let viewMenu: HTMLDetailsElement | undefined

  createEffect(scope, (key) => {
    activeDrag = null
    setDraggedThreadId(null)
    setDropTarget(null)
    setMoveNotice('')
    setOrganization(readChatOrganization(key))
    setMode(null)
    const storageChanged = (event: StorageEvent) => {
      if (event.key === key || event.key === null) setOrganization(readChatOrganization(key))
    }
    window.addEventListener('storage', storageChanged)
    return () => window.removeEventListener('storage', storageChanged)
  })
  createEffect(mode, (next) => {
    if (next) queueMicrotask(() => { if (dialog && !dialog.open) dialog.showModal(); nameInput?.focus() })
    else if (dialog?.open) dialog.close()
  })
  createEffect(() => undefined, () => {
    const outside = (event: PointerEvent) => {
      if (viewMenu?.open && event.target instanceof Node && !viewMenu.contains(event.target)) viewMenu.open = false
    }
    document.addEventListener('pointerdown', outside)
    return () => document.removeEventListener('pointerdown', outside)
  })

  const dispatch = (...actions: ChatOrganizationAction[]) => {
    const key = scope()
    if (!key) throw new Error(i18n.tr('Logg inn på et arbeidsområde først.', 'Sign in to a workspace first.'))
    // Solid batches signal writes. Apply related changes in one transaction so
    // a view update cannot overwrite a project created in the same event.
    const next = actions.reduce(applyChatOrganizationAction, organization())
    saveChatOrganization(key, next)
    setOrganization(next)
  }
  const endDrag = () => {
    activeDrag = null
    setDraggedThreadId(null)
    setDropTarget(null)
  }
  const canDrag = () => Boolean(scope()) && organization().view.group === 'project'
  const startDrag = (event: DragEvent, item: ChatThreadHistoryItem) => {
    const key = scope()
    if (!key || !canDrag() || !event.dataTransfer ||
      (event.target instanceof Element && event.target.closest('input, textarea, select, [contenteditable="true"]'))) {
      event.preventDefault()
      return
    }
    activeDrag = { threadId: item.threadId, scope: key }
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData(CHAT_DRAG_TYPE, item.threadId)
    setDraggedThreadId(item.threadId)
    setMoveNotice('')
  }
  const acceptsDrag = (event: DragEvent, group: ChatGroup) => {
    return canDrag() && (group.project || group.id === 'recent') && activeDrag?.scope === scope() &&
      Boolean(event.dataTransfer?.types.includes(CHAT_DRAG_TYPE)) &&
      props.sessions.some((item) => item.threadId === activeDrag?.threadId)
  }
  const dragOver = (event: DragEvent, group: ChatGroup) => {
    if (!acceptsDrag(event, group)) return
    event.preventDefault()
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
    setDropTarget(group.id)
  }
  const dropChat = (event: DragEvent, group: ChatGroup) => {
    event.preventDefault()
    event.stopPropagation()
    if (!acceptsDrag(event, group) || !activeDrag || event.dataTransfer?.getData(CHAT_DRAG_TYPE) !== activeDrag.threadId) {
      endDrag()
      return
    }
    const thread = props.sessions.find((item) => item.threadId === activeDrag?.threadId)
    const projectId = group.project ? group.id : null
    try {
      // Reuse the same validated, scoped action as the accessible Move menu.
      dispatch({ type: 'thread.move', threadId: activeDrag.threadId, projectId })
      setMoveNotice(i18n.tr(`Flyttet til ${group.label}.`, `Moved to ${group.label}.`) +
        (thread?.pinned ? i18n.tr(' Samtalen er fortsatt festet.', ' The chat stays pinned.') : ''))
    } catch (reason) {
      setMoveNotice(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke flytte samtalen.', 'Could not move the chat.'))
    }
    endDrag()
  }
  createEffect(() => undefined, () => {
    const cancelDrag = (event: KeyboardEvent) => { if (event.key === 'Escape') endDrag() }
    window.addEventListener('dragend', endDrag)
    window.addEventListener('keydown', cancelDrag)
    return () => {
      window.removeEventListener('dragend', endDrag)
      window.removeEventListener('keydown', cancelDrag)
    }
  })
  const updateView = (patch: Partial<ReturnType<typeof organization>['view']>) => {
    const next = applyChatOrganizationAction(organization(), { type: 'view.update', view: { ...organization().view, ...patch } })
    const key = scope()
    if (key) saveChatOrganization(key, next)
    setOrganization(next)
  }
  const matches = createMemo(() => selectOrganizedChats(props.sessions, organization(), query()))
  const groups = createMemo<ChatGroup[]>(() => {
    const staticState = organization()
    const rows = matches()
    const groups: ChatGroup[] = []
    const pinned = rows.filter((item) => item.pinned)
    if (pinned.length) groups.push({ id: 'pinned', label: i18n.tr('Festet', 'Pinned'), project: false, items: pinned })
    const rest = rows.filter((item) => !item.pinned)
    if (staticState.view.group === 'project') {
      const projects = [...staticState.projects].sort((a, b) => Number(b.pinned) - Number(a.pinned) || a.name.localeCompare(b.name))
      for (const project of projects) {
        const items = rest.filter((item) => staticState.assignments[item.threadId] === project.id)
        if (items.length || (!query().trim() && staticState.view.filter === 'all')) groups.push({ id: project.id, label: project.name, project: true, pinned: project.pinned, items })
      }
      const recent = rest.filter((item) => !staticState.projects.some((project) => project.id === staticState.assignments[item.threadId]))
      // Keep Recent available as a destination even when every chat is filed.
      groups.push({ id: 'recent', label: i18n.tr('Nylige', 'Recent'), project: false, items: recent })
    } else if (staticState.view.group === 'date') {
      for (const [id, label] of [['today', i18n.tr('I dag', 'Today')], ['week', i18n.tr('Siste 7 dager', 'Last 7 days')], ['earlier', i18n.tr('Tidligere', 'Earlier')]] as const) {
        const items = rest.filter((item) => chatDateGroup(item.updatedAt) === id)
        if (items.length) groups.push({ id, label, project: false, items })
      }
    } else if (rest.length) groups.push({ id: 'all', label: i18n.tr('Samtaler', 'Conversations'), project: false, items: rest })
    return groups
  })
  const beginProject = (id?: string) => {
    setError('')
    setDraftName(id ? organization().projects.find((project) => project.id === id)?.name ?? '' : '')
    setMode(id ? { kind: 'project', id } : { kind: 'create' })
  }
  const beginMove = (thread: ChatThreadHistoryItem) => {
    setError('')
    setDestination(organization().assignments[thread.threadId] ?? '')
    setMode({ kind: 'move', thread })
  }
  const submit = () => {
    const current = mode()
    if (!current) return
    try {
      if (current.kind === 'move') dispatch({ type: 'thread.move', threadId: current.thread.threadId, projectId: destination() || null })
      else if (current.kind === 'project') dispatch({ type: 'project.rename', id: current.id, name: draftName() })
      else {
        dispatch(
          { type: 'project.create', id: crypto.randomUUID(), name: draftName() },
          { type: 'view.update', view: { ...organization().view, group: 'project', filter: 'all' } },
        )
        setQuery('')
      }
      setMode(null)
    } catch (reason) { setError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke lagre.', 'Could not save.')) }
  }

  return (
    <div class="chat-organizer" data-dragging={draggedThreadId() ? 'true' : undefined}>
      <div class="chat-organizer__header">
        <strong>{i18n.tr('Samtaler', 'Chats')}</strong>
        <button type="button" aria-label={i18n.tr('Søk i samtaler', 'Search chats')} title={i18n.tr('Søk i samtaler', 'Search chats')} aria-expanded={searchOpen() ? 'true' : 'false'} onClick={() => { setSearchOpen((open) => !open); queueMicrotask(() => searchInput?.focus()) }}><Search size={17} /></button>
        <details ref={viewMenu} class="chat-organizer__view" onKeyDown={(event) => { if (event.key === 'Escape' && viewMenu?.open) { viewMenu.open = false; viewMenu.querySelector('summary')?.focus() } }}>
          <summary title={i18n.tr('Grupper, sorter og filtrer', 'Group, sort and filter')} aria-label={i18n.tr('Grupper, sorter og filtrer', 'Group, sort and filter')}><SlidersHorizontal size={17} /></summary>
          <div class="chat-organizer__view-panel">
            <label>{i18n.tr('Grupper etter', 'Group by')}<select value={organization().view.group} onChange={(event) => updateView({ group: event.currentTarget.value as 'project' | 'date' | 'none' })}><option value="project">{i18n.tr('Prosjekt', 'Project')}</option><option value="date">{i18n.tr('Dato', 'Date')}</option><option value="none">{i18n.tr('Ingen gruppering', 'No grouping')}</option></select></label>
            <label>{i18n.tr('Sorter etter', 'Sort by')}<select value={organization().view.sort} onChange={(event) => updateView({ sort: event.currentTarget.value as 'recent' | 'oldest' | 'title' })}><option value="recent">{i18n.tr('Sist oppdatert', 'Recently updated')}</option><option value="oldest">{i18n.tr('Eldste først', 'Oldest first')}</option><option value="title">{i18n.tr('Tittel A–Å', 'Title A–Z')}</option></select></label>
            <label>{i18n.tr('Vis', 'Show')}<select value={organization().view.filter} onChange={(event) => updateView({ filter: event.currentTarget.value as 'all' | 'pinned' | 'active' | 'attention' })}><option value="all">{i18n.tr('Alle samtaler', 'All chats')}</option><option value="pinned">{i18n.tr('Bare festede', 'Pinned only')}</option><option value="active">{i18n.tr('Aktive kjøringer', 'Active runs')}</option><option value="attention">{i18n.tr('Trenger oppmerksomhet', 'Needs attention')}</option></select></label>
          </div>
        </details>
        <button type="button" aria-label={i18n.tr('Nytt prosjekt', 'New project')} title={i18n.tr('Nytt prosjekt', 'New project')} disabled={!scope()} onClick={() => beginProject()}><FolderPlus size={18} /></button>
      </div>
      <Show when={searchOpen()}>
        <div class="chat-organizer__search"><Search size={15} /><input ref={searchInput} type="search" aria-label={i18n.tr('Søk i samtaler', 'Search chats')} placeholder={i18n.tr('Søk i samtaler …', 'Search chats…')} value={query()} onInput={(event) => setQuery(event.currentTarget.value)} /><button type="button" aria-label={i18n.tr('Lukk samtalesøk', 'Close chat search')} onClick={() => { setQuery(''); setSearchOpen(false) }}><X size={14} /></button></div>
      </Show>
      <a href="/chat" link class="core-chat-sidebar__new" onClick={() => props.onNewChat()}><MessageSquarePlus size={17} />{i18n.tr('Ny samtale', 'New chat')}</a>
      <Show when={organization().view.filter !== 'all' || query().trim()}><button type="button" class="chat-organizer__clear-filter" onClick={() => { setQuery(''); updateView({ filter: 'all' }) }}>{i18n.tr('Fjern filtre', 'Clear filters')} · {matches().length}<X size={12} /></button></Show>
      <nav class="chat-organizer__list core-chat-sidebar__sessions" aria-label={i18n.tr('Chat-samtaler', 'Chat conversations')}>
        <Show when={!props.loading} fallback={<p class="chat-organizer__empty">{i18n.tr('Laster samtaler …', 'Loading chats…')}</p>}>
          <Show when={groups().length} fallback={<p class="chat-organizer__empty">{i18n.tr('Ingen samtaler passer dette utvalget.', 'No chats match this view.')}</p>}>
            <For each={groups()}>{(group) => (
              <details class="chat-organizer__group" open
                data-drop-target={group.project ? group.id : group.id === 'recent' ? 'recent' : undefined}
                data-drop-active={dropTarget() === group.id ? 'true' : undefined}
                onDragEnter={(event) => dragOver(event, group)}
                onDragOver={(event) => dragOver(event, group)}
                onDragLeave={(event) => {
                  if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) setDropTarget(null)
                }}
                onDrop={(event) => dropChat(event, group)}>
                <summary><Show when={group.project} fallback={<Show when={group.id === 'pinned'}><Pin size={13} /></Show>}><Folder size={16} /></Show><span>{group.label}</span><Show when={group.pinned}><Pin size={12} /></Show><small>{group.items.length}</small></summary>
                <Show when={group.project}><button type="button" class="chat-organizer__project-menu" aria-label={i18n.tr(`Administrer prosjektet ${group.label}`, `Manage project ${group.label}`)} title={i18n.tr('Administrer prosjekt', 'Manage project')} onClick={() => beginProject(group.id)}><MoreHorizontal size={16} /></button></Show>
                <For each={group.items}>{(item) => (
                  <div class="chat-organizer__draggable" draggable={canDrag() ? 'true' : 'false'}
                    data-dragged={draggedThreadId() === item.threadId ? 'true' : undefined}
                    title={canDrag() ? i18n.tr('Dra til et prosjekt eller Nylige. Du kan også bruke Flytt til prosjekt i menyen.', 'Drag to a project or Recent. You can also use Move to project in the menu.') : undefined}
                    onDragStart={(event) => startDrag(event, item)} onDragEnd={endDrag}>
                    {props.renderRow(item, () => beginMove(item))}
                  </div>
                )}</For>
                <Show when={(group.project || group.id === 'recent') && !group.items.length}><p class="chat-organizer__empty">{i18n.tr('Slipp en samtale her, eller bruk Flytt til prosjekt.', 'Drop a chat here, or use Move to project.')}</p></Show>
              </details>
            )}</For>
          </Show>
        </Show>
      </nav>
      <p class="chat-organizer__move-notice" role="status" aria-live="polite">{moveNotice()}</p>
      <p class="chat-organizer__local-note">{i18n.tr('Prosjekter lagres i denne nettleseren.', 'Projects are saved in this browser.')}</p>

      <dialog ref={dialog} class="chat-project-dialog" aria-labelledby="chat-project-dialog-title" onClose={() => setMode(null)} onCancel={() => setMode(null)}>
        <form onSubmit={(event) => { event.preventDefault(); submit() }}>
          <h2 id="chat-project-dialog-title">{mode()?.kind === 'move' ? i18n.tr('Flytt til prosjekt', 'Move to project') : mode()?.kind === 'project' ? i18n.tr('Administrer prosjekt', 'Manage project') : i18n.tr('Nytt prosjekt', 'New project')}</h2>
          <p>{i18n.tr('Organiser samtalene dine i denne nettleseren. Samtalens innhold endres ikke.', 'Organize your chats in this browser. Conversation content stays unchanged.')}</p>
          <Show when={mode()?.kind === 'move'} fallback={<label>{i18n.tr('Prosjektnavn', 'Project name')}<input ref={nameInput} required maxlength={80} value={draftName()} onInput={(event) => setDraftName(event.currentTarget.value)} /></label>}>
            <label>{i18n.tr('Prosjekt', 'Project')}<select value={destination()} onChange={(event) => setDestination(event.currentTarget.value)}><option value="">{i18n.tr('Ingen prosjekt (nylige)', 'No project (Recent)')}</option><For each={organization().projects}>{(project) => <option value={project.id}>{project.name}</option>}</For></select></label>
          </Show>
          <Show when={error()}><p role="alert">{error()}</p></Show>
          <Show when={mode()?.kind === 'project'}>
            <div class="chat-project-dialog__manage">
              <button type="button" onClick={() => { const staticCurrent = mode(); if (staticCurrent?.kind !== 'project') return; const pinned = organization().projects.find((project) => project.id === staticCurrent.id)?.pinned; try { dispatch({ type: 'project.pin', id: staticCurrent.id, pinned: !pinned }) } catch (reason) { setError(String(reason)) } }}>{organization().projects.find((project) => project.id === (mode() as { id?: string })?.id)?.pinned ? i18n.tr('Løsne prosjekt', 'Unpin project') : i18n.tr('Fest prosjekt', 'Pin project')}</button>
              <button type="button" onClick={() => { const current = mode(); if (current?.kind !== 'project') return; try { dispatch({ type: 'project.remove', id: current.id }); setMode(null) } catch (reason) { setError(String(reason)) } }}>{i18n.tr('Fjern prosjekt, behold samtaler', 'Remove project, keep chats')}</button>
            </div>
          </Show>
          <div class="chat-project-dialog__footer"><button type="button" onClick={() => setMode(null)}>{i18n.tr('Avbryt', 'Cancel')}</button><button type="submit">{mode()?.kind === 'create' ? i18n.tr('Opprett prosjekt', 'Create project') : i18n.tr('Lagre', 'Save')}</button></div>
        </form>
      </dialog>
    </div>
  )
}
