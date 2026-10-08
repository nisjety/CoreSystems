import { useLocation, useSearchParams } from '@solidjs/router'
import {
  Check,
  ChevronDown,
  ChevronRight,
  CircleUserRound,
  Fingerprint,
  Folder,
  Mail,
  MessageCircleMore,
  MessageSquarePlus,
  MoreVertical,
  PanelLeftClose,
  PanelLeftOpen,
  PenLine,
  Pin,
  PinOff,
  Trash2,
  X,
} from '@/shared/icons'
import type { JSX } from '@solidjs/web'
import { Dynamic } from '@solidjs/web'
import { createEffect, createMemo, createSignal, For, Match, Show, Switch } from 'solid-js'
import {
  CHAT_ACTIVE_THREAD_CHANGED_EVENT,
  CHAT_ACTIVE_THREAD_KEY,
  CHAT_THREAD_HISTORY_CHANGED_EVENT,
  CHAT_THREAD_HISTORY_KEY,
  CHAT_THREAD_TITLE_MAX_LENGTH,
  clearActiveChatThreadId,
  clearChatThreadHistory,
  readActiveChatThreadId,
  readChatThreadTranscript,
  readChatThreadHistory,
  removeChatThreadHistoryItem,
  renameChatThreadLocally,
  replaceChatThreadHistory,
  selectChatThread,
  togglePersonaEnabledChatThread,
  togglePinnedChatThread,
  upsertChatThreadHistory,
  upsertChatThreadTranscript,
  type ChatThreadHistoryItem,
} from '@/features/chat/lib/chat-thread-history'
import { SpaceConfirmButton } from '@/features/spaces/components/SpaceConfirmButton'
import { AgentsExpandedSidebarPanel } from '@/features/core/components/sidebar/CoreSidebarAgentsPanel'
import { KnowledgeExpandedSidebarPanel } from '@/features/core/components/sidebar/CoreSidebarKnowledgePanel'
import { SupportExpandedSidebarPanel } from '@/features/core/components/sidebar/CoreSidebarSupportPanel'
import { SpacesExpandedSidebarPanel } from '@/features/core/components/sidebar/CoreSidebarSpacesPanel'
import {
  SidebarPanelTitle,
  SidebarSearchField,
} from '@/features/core/components/sidebar/CoreSidebarPrimitives'
import {
  AccountExpandedSidebarPanel,
  SettingsExpandedSidebarPanel,
} from '@/features/core/components/sidebar/CoreSidebarSettingsPanels'
import type { VerevonRoute } from '@/features/core/lib/shell-data'
import {
  applyDemoModeNavGate,
  getSidebarSectionForPath,
  isSidebarPathActive,
  sidebarSearchAction,
  sidebarSections,
  type SidebarPanelGroup,
  type SidebarPanelItem,
  type SidebarSection,
} from '@/features/core/lib/sidebar-navigation'
import { clearChatThreads, deleteChatThread, listChatThreads, saveChatThreadSnapshot } from '@/shared/api/chat-client'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { shouldShowWorkspaceAdminNavigation } from '@/shared/session/access'
import { getSession } from '@/shared/session/session-store'
import { chatAppearanceActive } from '@/features/chat/lib/chat-appearance-preview'
import { FilledRailIcon } from './FilledRailIcon'
import { ChatSidebarOrganizer } from './sidebar/ChatSidebarOrganizer'
import { homeOverviewActionSchema, homeOverviewGroup, homeOverviewSidebarItems, parseHomeOverview } from '@/shared/actions/home-overview'

export const SIDEBAR_MINIMIZED_WIDTH = 60
export const SIDEBAR_EXPANDED_WIDTH = 320

export function CoreSidebar(props: {
  activeRoute: VerevonRoute
  expanded: boolean
  expandedWidth?: number
  expansionLocked?: boolean
  onExpandedChange: (expanded: boolean) => void
  onOpenSearch: () => void
}) {
  const i18n = useI18n()
  const location = useLocation()
  const session = getSession()
  const visibleSections = () => applyDemoModeNavGate(sidebarSections)
    .filter((section) => section.id !== 'settings' || shouldShowWorkspaceAdminNavigation(session))
    .map((section) => localizeSidebarSection(section, i18n))
  const activeSection = () => getSidebarSectionForPath(location.pathname, props.activeRoute, visibleSections())
  const mainSections = () => visibleSections().filter((section) => !section.pinnedBottom)
  const pinnedSections = () => visibleSections().filter((section) => section.pinnedBottom)
  const accountActive = () => location.pathname === '/account' || location.pathname.startsWith('/account/')
  const width = () => `${props.expanded ? (props.expandedWidth ?? SIDEBAR_EXPANDED_WIDTH) : SIDEBAR_MINIMIZED_WIDTH}px`

  const openSection = () => props.onExpandedChange(true)

  return (
    <aside id="core-workspace-sidebar" class="verevon-sidebar-themed core-sidebar verevon-sidebar-type" style={{ width: width() }} aria-label={i18n.tr('Primærnavigasjon', 'Primary navigation')}>
      <div class="core-sidebar__body">
        <div class="core-sidebar__rail">
          <div class="core-sidebar__top-actions">
            <Show when={!props.expansionLocked}>
              <MiniActionButton
                label={props.expanded ? i18n.tr('Slå sammen sidefelt', 'Collapse sidebar') : i18n.tr('Utvid sidefelt', 'Expand sidebar')}
                active={false}
                onClick={() => props.onExpandedChange(!props.expanded)}
              >
                <Show when={props.expanded} fallback={<PanelLeftOpen class="size-[18px]" strokeWidth={1.75} />}>
                  <PanelLeftClose class="size-[18px]" strokeWidth={1.75} />
                </Show>
              </MiniActionButton>
            </Show>
          </div>

          <nav class="core-sidebar__mini-nav" aria-label={i18n.tr('Arbeidsområdeseksjoner', 'Workspace sections')}>
            <div class="core-sidebar__mini-stack">
              <For each={mainSections()}>
                {(section) => (
                  <MiniSectionLink
                    section={section}
                    active={!accountActive() && activeSection().id === section.id}
                    onOpen={openSection}
                  />
                )}
              </For>
            </div>
            <div class="core-sidebar__spacer" />
          </nav>

          <div class="core-sidebar__bottom-actions">
            <div class="core-sidebar__mini-divider" />
            <MiniAccountLink active={accountActive()} i18n={i18n} onOpen={openSection}>
              <CircleUserRound class="size-[18px]" strokeWidth={1.65} />
            </MiniAccountLink>
            <For each={pinnedSections()}>
              {(section) => (
                <MiniSectionLink
                  section={section}
                  active={!accountActive() && activeSection().id === section.id}
                  onOpen={openSection}
                />
              )}
            </For>
            <MiniActionButton
              label={i18n.tr('Søk', 'Search')}
              active={false}
              onClick={() => {
                props.onOpenSearch()
                props.onExpandedChange(true)
              }}
            >
              <sidebarSearchAction.icon class="size-[18px]" strokeWidth={1.65} />
            </MiniActionButton>
          </div>
        </div>

        <Show keyed when={props.expanded ? activeSection() : null}>
          {(section) => (
            <>
              <div aria-hidden="true" class="core-sidebar__splitter" />
              <ExpandedSidebarPanel
                activeSection={section}
                pathname={`${location.pathname}${location.search}`}
                onCollapse={() => props.onExpandedChange(false)}
              />
            </>
          )}
        </Show>
      </div>
    </aside>
  )
}

function ExpandedSidebarPanel(props: {
  activeSection: SidebarSection
  pathname: string
  onCollapse: () => void
}) {
  const [searchQuery, setSearchQuery] = createSignal('')
  const [activeTabId, setActiveTabId] = createSignal<string | null>(null)
  const panelTabs = () => props.activeSection.panelTabs ?? []
  // A memo, so the effect below runs when the group changes and not for every other query parameter
  // (the Home tab lives in ?tab=): an effect re-runs whenever its source is invalidated, equal value or not.
  const firstPanelTabId = createMemo(() => props.activeSection.id === 'overview' && chatAppearanceActive('dashboard')
    ? homeOverviewGroup(parseHomeOverview(new URLSearchParams(props.pathname.split('?')[1] ?? '').get('overview')))
    : panelTabs()[0]?.id ?? null)
  const panelKind = createMemo(() => {
    if (props.activeSection.id === 'messages') return 'messages'
    if (props.activeSection.id === 'spaces') return 'spaces'
    if (props.activeSection.id === 'inbox') return 'support'
    if (props.activeSection.id === 'agents') return 'agents'
    if (props.activeSection.id === 'knowledge') return 'knowledge'
    if (props.pathname === '/account' || props.pathname.startsWith('/account/')) return 'account'
    if (props.activeSection.id === 'settings') return 'settings'
    return 'generic'
  })

  createEffect(
    () => firstPanelTabId(),
    (nextTabId) => {
      setActiveTabId(nextTabId)
      setSearchQuery('')
    },
  )

  return (
    <Switch
      fallback={
        <GenericSidebarPanel
          activeSection={props.activeSection}
          activeTabId={activeTabId()}
          onActiveTabChange={setActiveTabId}
          onCollapse={props.onCollapse}
          pathname={props.pathname}
          panelTabs={panelTabs()}
          searchQuery={searchQuery()}
          onSearchQueryChange={setSearchQuery}
        />
      }
    >
      <Match when={panelKind() === 'messages'}>
        <ChatSidebarPanel onCollapse={() => props.onCollapse()} />
      </Match>
      <Match when={panelKind() === 'spaces'}>
        <SpacesExpandedSidebarPanel onCollapse={props.onCollapse} />
      </Match>
      <Match when={panelKind() === 'support'}>
        <SupportExpandedSidebarPanel onCollapse={props.onCollapse} />
      </Match>
      <Match when={panelKind() === 'agents'}>
        <AgentsExpandedSidebarPanel onCollapse={props.onCollapse} />
      </Match>
      <Match when={panelKind() === 'knowledge'}>
        <KnowledgeExpandedSidebarPanel onCollapse={props.onCollapse} />
      </Match>
      <Match when={panelKind() === 'account'}>
        <AccountExpandedSidebarPanel onCollapse={props.onCollapse} />
      </Match>
      <Match when={panelKind() === 'settings'}>
        <SettingsExpandedSidebarPanel onCollapse={props.onCollapse} />
      </Match>
    </Switch>
  )
}

function GenericSidebarPanel(props: {
  activeSection: SidebarSection
  activeTabId: string | null
  onActiveTabChange: (tabId: string) => void
  onCollapse: () => void
  onSearchQueryChange: (value: string) => void
  panelTabs: NonNullable<SidebarSection['panelTabs']>
  pathname: string
  searchQuery: string
}) {
  const i18n = useI18n()
  return (
    <div class="core-sidebar-panel">
      <SidebarPanelTitle onCollapse={props.onCollapse}>{props.activeSection.label}</SidebarPanelTitle>
      <SidebarSearchField
        ariaLabel={i18n.tr('Filtrer sidefeltseksjon', 'Filter sidebar section')}
        placeholder={i18n.tr('Filtrer denne seksjonen', 'Filter this section')}
        value={props.searchQuery}
        onChange={props.onSearchQueryChange}
      />

      <Show when={props.panelTabs.length}>
        <div class="core-sidebar-tabs verevon-sidebar-row-strong">
          <For each={props.panelTabs}>
            {(tab) => (
              <button
                type="button"
                onClick={() => props.onActiveTabChange(tab.id)}
                title={tab.label}
                class={{ 'verevon-sidebar-tab-active core-sidebar-tabs__tab--active': props.activeTabId === tab.id }}
              >
                {tab.label}
              </button>
            )}
          </For>
        </div>
      </Show>

      <SidebarPanelNavigation
        section={props.activeSection}
        pathname={props.pathname}
        activeTabId={props.activeTabId}
        searchQuery={props.searchQuery}
      />
    </div>
  )
}

export function ChatSidebarPanel(props: { onCollapse: () => void }) {
  const i18n = useI18n()
  const [error, setError] = createSignal<string | null>(null)
  const [loading, setLoading] = createSignal(false)
  const [sessions, setSessions] = createSignal<ChatThreadHistoryItem[]>([])
  const [activeThreadId, setActiveThreadId] = createSignal<string | null>(null)
  // Which row's "more options" popover is open, keyed by threadId. A single
  // signal (rather than local state per row) guarantees at most one is open
  // at a time -- opening a second closes the first automatically.
  const [openMenuThreadId, setOpenMenuThreadId] = createSignal<string | null>(null)
  // Which row is mid-rename, and its live draft text. Also single-flight: a
  // second rename cannot start while one is already in progress.
  const [renamingThreadId, setRenamingThreadId] = createSignal<string | null>(null)
  const [renameDraft, setRenameDraft] = createSignal('')

  const refreshLocalSessions = () => {
    setActiveThreadId(readActiveChatThreadId())
    setSessions(readChatThreadHistory())
  }

  const migrateLocalSessions = async (
    localSessions: ChatThreadHistoryItem[],
    serverSessions: ChatThreadHistoryItem[],
  ) => {
    const serverThreadIds = new Set(serverSessions.map((item) => item.threadId))
    const unsynced = localSessions.filter((item) => !serverThreadIds.has(item.threadId))
    if (unsynced.length === 0) return false

    const results = await Promise.allSettled(unsynced.map((item) => {
      const transcript = readChatThreadTranscript(item.threadId)
      return saveChatThreadSnapshot(item.threadId, {
        title: item.title,
        preview: item.preview,
        updatedAt: transcript?.updatedAt ?? item.updatedAt,
        turns: transcript?.turns,
        taskSteps: transcript?.taskSteps,
      })
    }))

    return results.some((result) => result.status === 'fulfilled')
  }

  const refreshServerSessions = async () => {
    const localSessions = readChatThreadHistory()
    setLoading(localSessions.length === 0)
    setError(null)
    try {
      const serverSessions = await listChatThreads()
      setActiveThreadId(readActiveChatThreadId())
      setSessions(replaceChatThreadHistory([...serverSessions, ...localSessions]))

      if (await migrateLocalSessions(localSessions, serverSessions)) {
        const refreshedSessions = await listChatThreads().catch(() => serverSessions)
        setSessions(replaceChatThreadHistory([...refreshedSessions, ...localSessions]))
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke laste lagrede samtaler.', 'Could not load saved conversations.'))
      refreshLocalSessions()
    } finally {
      setLoading(false)
    }
  }

  createEffect(
    () => undefined,
    () => {
      refreshLocalSessions()
      void refreshServerSessions()

      const handleHistoryChange = () => {
        refreshLocalSessions()
      }
      const handleStorageChange = (event: StorageEvent) => {
        if (
          event.key === CHAT_ACTIVE_THREAD_KEY ||
          event.key === CHAT_THREAD_HISTORY_KEY ||
          event.key === null
        ) {
          refreshLocalSessions()
        }
      }

      window.addEventListener(CHAT_ACTIVE_THREAD_CHANGED_EVENT, handleHistoryChange)
      window.addEventListener(CHAT_THREAD_HISTORY_CHANGED_EVENT, handleHistoryChange)
      window.addEventListener('storage', handleStorageChange)
      return () => {
        window.removeEventListener(CHAT_ACTIVE_THREAD_CHANGED_EVENT, handleHistoryChange)
        window.removeEventListener(CHAT_THREAD_HISTORY_CHANGED_EVENT, handleHistoryChange)
        window.removeEventListener('storage', handleStorageChange)
      }
    },
  )

  const clearHistory = () => {
    clearChatThreadHistory()
    clearActiveChatThreadId()
    setSessions([])
    setActiveThreadId(null)
    setError(null)
    void clearChatThreads().catch(() => undefined)
    props.onCollapse()
  }

  const openThread = (threadId: string) => {
    selectChatThread(threadId)
    setActiveThreadId(threadId)
  }

  const startNewChat = () => {
    clearActiveChatThreadId()
    setActiveThreadId(null)
    setError(null)
  }

  /**
   * Presentation-level dedupe: the stored history should hold one entry per
   * thread, but if it ever contains repeats (e.g. a server + local merge, or
   * a corrupted snapshot that self-heals on click) the menu must still render
   * each threadId exactly once — first occurrence wins (list is newest-first).
   */
  const visibleSessions = createMemo(() => {
    const seen = new Set<string>()
    return sessions().filter((item) => {
      if (seen.has(item.threadId)) return false
      seen.add(item.threadId)
      return true
    })
  })

  const sessionCount = () => visibleSessions().length
  // Storage already orders pinned-first (see `sortByUpdatedAtDesc` in
  // chat-thread-history.ts); filtering preserves that relative order, so no
  // re-sort is needed here.
  const pinnedSessions = createMemo(() => visibleSessions().filter((item) => item.pinned))
  const unpinnedSessions = createMemo(() => visibleSessions().filter((item) => !item.pinned))
  // Pin is server-owned (see `ChatThreadSession.pinned` in chat-client.ts) so
  // it follows the user across devices -- DashboardComposer's history panel
  // already round-trips it via `saveChatThreadSnapshot`. The local flip via
  // `togglePinnedChatThread` is optimistic UI only: without the write-through,
  // `refreshServerSessions` (onMount, or any later resync) reads the server's
  // untouched value and silently reverts the toggle the next time this panel
  // refreshes -- so the server call is load-bearing, not decorative.
  const togglePin = (event: MouseEvent, threadId: string) => {
    event.stopPropagation()
    const next = !sessions().find((item) => item.threadId === threadId)?.pinned
    setSessions(togglePinnedChatThread(threadId))
    void saveChatThreadSnapshot(threadId, { pinned: next }).catch(() => {
      setSessions(togglePinnedChatThread(threadId))
      setError(i18n.tr('Kunne ikke feste samtalen.', 'Could not pin the conversation.'))
    })
  }

  /**
   * Persona spec — the per-thread override of the user's global persona
   * switch, next to the pin toggle above (same optimistic-write-with-revert
   * shape: `saveChatThreadSnapshot` is the load-bearing call, the local flip
   * is UI-only and would otherwise be silently reverted by the next resync).
   * `session-core` rejects this write outright for a ZDR/temporary thread
   * (`update_thread_presentation`'s ZDR guard) — but a temporary chat is
   * never saved to this durable list in the first place, so every row this
   * menu can reach is already a persisted, non-ZDR thread.
   */
  const togglePersona = (threadId: string) => {
    const next = !(sessions().find((item) => item.threadId === threadId)?.personaEnabled ?? true)
    setSessions(togglePersonaEnabledChatThread(threadId))
    void saveChatThreadSnapshot(threadId, { personaEnabled: next }).catch(() => {
      setSessions(togglePersonaEnabledChatThread(threadId))
      setError(i18n.tr('Kunne ikke endre persona for samtalen.', 'Could not change persona for the conversation.'))
    })
  }

  const startRename = (item: ChatThreadHistoryItem) => {
    setOpenMenuThreadId(null)
    setRenameDraft(item.title)
    setRenamingThreadId(item.threadId)
  }

  const cancelRename = () => {
    setRenamingThreadId(null)
    setRenameDraft('')
  }

  /**
   * Commit the in-progress rename. Trimmed and capped to the same length the
   * store would otherwise silently truncate to (`CHAT_THREAD_TITLE_MAX_LENGTH`,
   * `normalizeHistoryInput`); an empty result is treated as a cancel rather
   * than a save, matching Escape's behaviour, since "renaming to nothing" has
   * no clear meaning and normalizing it would fall back to a generic label
   * the user never asked for.
   *
   * Writes through `saveChatThreadSnapshot` with `{ title }` ONLY -- omitting
   * `pinned`/`preview` the same way the pin toggle above omits everything but
   * `pinned` -- so `save_thread` (gateway) leaves every other field untouched
   * (see `SaveThreadRequest` in `domains/chat/history.rs`: absent fields are
   * "leave as is", not "clear").
   */
  const commitRename = (threadId: string) => {
    const trimmed = renameDraft().trim()
    setRenamingThreadId(null)
    setRenameDraft('')
    if (!trimmed) return
    const current = sessions().find((item) => item.threadId === threadId)
    if (!current) return
    const next = trimmed.length > CHAT_THREAD_TITLE_MAX_LENGTH
      ? trimmed.slice(0, CHAT_THREAD_TITLE_MAX_LENGTH)
      : trimmed
    if (next === current.title) return
    const previousTitle = current.title
    setSessions(renameChatThreadLocally(threadId, next))
    void saveChatThreadSnapshot(threadId, { title: next }).catch(() => {
      setSessions(renameChatThreadLocally(threadId, previousTitle))
      setError(i18n.tr('Kunne ikke endre navn på samtalen.', 'Could not rename the conversation.'))
    })
  }

  /**
   * Permanently delete one thread (explicit confirmation happens in the row
   * menu via `SpaceConfirmButton` before this ever runs). Removed from the
   * local cache optimistically -- reinserted on a failed server delete, same
   * revert shape as `togglePin` and `commitRename` above -- and, when the
   * deleted thread was the open one, the active pointer is cleared so the
   * user lands on a fresh chat rather than a dead one. `use-chat-controller`
   * already listens for `CHAT_ACTIVE_THREAD_CHANGED_EVENT` and resets the
   * chat view itself; this only has to clear the pointer.
   *
   * On a failed server delete, the revert must undo BOTH side effects of the
   * optimistic removal, not just the history-list entry:
   * `removeChatThreadHistoryItem` also drops the thread's cached transcript
   * (see its own implementation), so that is captured up front and put back
   * alongside the history row -- otherwise a thread that in fact was never
   * deleted would reload from a cold server fetch instead of its cache next
   * time it is opened. And if the deleted thread was the one open, clearing
   * the active pointer already navigated the reader to a fresh chat before
   * the request even settled; a failure means that thread was NOT deleted, so
   * leaving them stranded on a fresh chat -- while the very same thread
   * reappears in the list above them -- is its own bug. Restore the pointer
   * the same way `openThread` does.
   */
  const deleteThread = async (threadId: string) => {
    const current = sessions().find((item) => item.threadId === threadId)
    const transcript = readChatThreadTranscript(threadId)
    const wasActive = activeThreadId() === threadId
    setSessions(removeChatThreadHistoryItem(threadId))
    if (wasActive) {
      clearActiveChatThreadId()
      setActiveThreadId(null)
    }
    setError(null)
    try {
      await deleteChatThread(threadId)
    } catch {
      if (current) setSessions(upsertChatThreadHistory(current))
      if (transcript) upsertChatThreadTranscript(transcript)
      if (wasActive) {
        selectChatThread(threadId)
        setActiveThreadId(threadId)
      }
      setError(i18n.tr('Kunne ikke slette samtalen.', 'Could not delete the conversation.'))
    }
  }

  const sessionRow = (item: ChatThreadHistoryItem, onMoveToProject?: () => void) => (
    <div class="core-chat-session-row">
      <Show
        when={renamingThreadId() !== item.threadId}
        fallback={
          <form
            class="core-chat-session__rename"
            onSubmit={(event) => {
              event.preventDefault()
              commitRename(item.threadId)
            }}
          >
            <input
              ref={(element) => {
                // Autofocus + select-all on the same tick the field mounts,
                // mirroring the popover's own focus-on-open handling below --
                // `focus()` before layout is a no-op, so this waits one
                // microtask rather than trying it synchronously in the ref.
                queueMicrotask(() => {
                  element.focus()
                  element.select()
                })
              }}
              class="core-chat-session__rename-input"
              value={renameDraft()}
              maxlength={CHAT_THREAD_TITLE_MAX_LENGTH}
              aria-label={i18n.tr('Nytt navn på samtalen', 'New conversation name')}
              onInput={(event) => setRenameDraft(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key !== 'Escape') return
                event.preventDefault()
                cancelRename()
              }}
            />
            <button
              type="submit"
              class="core-chat-session__rename-action"
              aria-label={i18n.tr('Lagre navn', 'Save name')}
              title={i18n.tr('Lagre navn', 'Save name')}
              // A pointerdown-triggered blur would exit rename mode (and
              // unmount this very button) before its own click ever fires --
              // keeping focus on the input until the click completes is what
              // makes the mouse path work at all, not just the keyboard one.
              onMouseDown={(event) => event.preventDefault()}
            >
              <Check class="size-3.5" />
            </button>
            <button
              type="button"
              class="core-chat-session__rename-action core-chat-session__rename-action--danger"
              aria-label={i18n.tr('Avbryt', 'Cancel')}
              title={i18n.tr('Avbryt', 'Cancel')}
              onMouseDown={(event) => event.preventDefault()}
              onClick={cancelRename}
            >
              <X class="size-3.5" />
            </button>
          </form>
        }
      >
        <button
          type="button"
          class={['core-chat-session', { 'core-chat-session--active': item.threadId === activeThreadId() }]}
          onClick={() => openThread(item.threadId)}
          aria-current={item.threadId === activeThreadId() ? 'page' : undefined}
          title={`${item.title} · ${formatChatUpdatedAt(item.updatedAt, i18n)}`}
        >
          <div class="core-chat-session__summary">
            <span class="verevon-sidebar-row-strong" title={item.title}>{item.title}</span>
            <Show when={item.origin === 'mail_task'}>
              <span
                class="verevon-trust-chip"
                title={i18n.tr(
                  'Denne samtalen ble startet fra en videresendt e-post.',
                  'This thread was started from a forwarded email.',
                )}
              >
                <Mail class="size-3" aria-hidden="true" />
                {i18n.tr('Fra e-post', 'From email')}
              </span>
            </Show>
            <Show when={chatRunStatusChip(item.latestRunStatus, i18n)}>
              {(chip) => (
                <small class="core-chat-session__status" data-tone={chip().tone} role="status">
                  <i aria-hidden="true" />
                  {chip().label}
                </small>
              )}
            </Show>
          </div>
          <em>{formatChatUpdatedAt(item.updatedAt, i18n)}</em>
        </button>
        <button
          type="button"
          class={['core-chat-session__pin', { 'core-chat-session__pin--active': Boolean(item.pinned) }]}
          aria-label={item.pinned ? i18n.tr('Løsne fra topp', 'Unpin from top') : i18n.tr('Fest til topp', 'Pin to top')}
          title={item.pinned ? i18n.tr('Løsne fra topp', 'Unpin from top') : i18n.tr('Fest til topp', 'Pin to top')}
          onClick={(event) => togglePin(event, item.threadId)}
        >
          <Show when={item.pinned} fallback={<Pin class="size-3.5" />}>
            <PinOff class="size-3.5" />
          </Show>
        </button>
        <ChatSessionMenu
          item={item}
          open={openMenuThreadId() === item.threadId}
          onOpenChange={(open) => setOpenMenuThreadId(open ? item.threadId : null)}
          onRenameStart={() => startRename(item)}
          onMoveToProject={onMoveToProject}
          onTogglePersona={() => togglePersona(item.threadId)}
          onDelete={() => void deleteThread(item.threadId)}
        />
      </Show>
    </div>
  )

  return (
    <div class="core-chat-sidebar">
      <Show when={!chatAppearanceActive('organizer')}>
      <a href="/chat" link class="core-chat-sidebar__new" onClick={startNewChat}>
        <span>
          <MessageSquarePlus class="size-[15px]" strokeWidth={1.75} />
        </span>
        Ny samtale
      </a>
      </Show>

      {/* The empty-state fallback below already surfaces `error()` when there
          is nothing else to show. This banner covers the opposite case -- a
          pin/rename/delete failing while OTHER threads are still listed,
          which the fallback branch never renders at all. */}
      <Show when={error() && (sessionCount() > 0 || chatAppearanceActive('organizer'))}>
        <p class="core-sidebar-empty verevon-sidebar-row-normal" role="alert">{error()}</p>
      </Show>
      <Show when={chatAppearanceActive('organizer')} fallback={
      <nav class="core-chat-sidebar__sessions" aria-label={i18n.tr('Chat-samtaler', 'Chat conversations')}>
        <Show when={!loading()} fallback={<div class="core-sidebar-empty verevon-sidebar-row-normal">{i18n.tr('Laster samtaler ...', 'Loading conversations ...')}</div>}>
          <Show
            when={sessionCount() > 0}
            fallback={<div class="core-sidebar-empty verevon-sidebar-row-normal">{error() ?? i18n.tr('Åpne chat for å laste ekte samtalehistorikk.', 'Open chat to load real conversation history.')}</div>}
          >
            <Show when={pinnedSessions().length > 0}>
              <span class="verevon-sidebar-group-title">{i18n.tr('Pinnet', 'Pinned')}</span>
              <For each={pinnedSessions()}>{(item) => sessionRow(item)}</For>
            </Show>
            <For each={unpinnedSessions()}>{(item) => sessionRow(item)}</For>
          </Show>
        </Show>
      </nav>
      }>
        <ChatSidebarOrganizer sessions={visibleSessions()} loading={loading()} onNewChat={startNewChat} renderRow={sessionRow} />
      </Show>

      <button type="button" class="core-chat-sidebar__clear" onClick={clearHistory}>
        <Trash2 class="size-3.5" />
        {i18n.tr('Tøm chathistorikk', 'Clear chat history')}
      </button>
    </div>
  )
}

/**
 * A thread row's "more options" popover: Rename and Delete. Controlled by the
 * parent (`open`/`onOpenChange`) rather than owning its own open state, so the
 * panel can guarantee at most one row's menu is open at a time -- the same
 * reason `CoreSidebarKnowledgePanel`'s combobox tracks `open` next to its
 * trigger rather than inside a `For` callback.
 *
 * Unlike that combobox (a listbox where focus never leaves the trigger while
 * arrow keys move a highlighted index), this is a command menu: opening it
 * moves real focus onto its first action, Tab/Shift+Tab walk its two buttons
 * natively, and Escape closes it and returns focus to the trigger -- the
 * standard ARIA menu-button pattern, appropriate here because the two things
 * inside it are commands to run, not values to pick.
 */
function ChatSessionMenu(props: {
  item: ChatThreadHistoryItem
  open: boolean
  onOpenChange: (open: boolean) => void
  onRenameStart: () => void
  onMoveToProject?: () => void
  onTogglePersona: () => void
  onDelete: () => void
}) {
  const i18n = useI18n()
  let rootRef: HTMLDivElement | undefined
  let triggerRef: HTMLButtonElement | undefined
  let firstItemRef: HTMLButtonElement | undefined

  const close = (refocusTrigger: boolean) => {
    props.onOpenChange(false)
    if (refocusTrigger) triggerRef?.focus()
  }

  createEffect(
    () => props.open,
    (isOpen) => {
      if (!isOpen) return
      queueMicrotask(() => firstItemRef?.focus())

      const closeOnOutsidePointer = (event: PointerEvent) => {
        const target = event.target as Node | null
        if (target && rootRef?.contains(target)) return
        close(false)
      }
      const closeOnEscape = (event: KeyboardEvent) => {
        if (event.key !== 'Escape') return
        event.stopPropagation()
        close(true)
      }

      document.addEventListener('pointerdown', closeOnOutsidePointer, true)
      window.addEventListener('keydown', closeOnEscape, true)
      return () => {
        document.removeEventListener('pointerdown', closeOnOutsidePointer, true)
        window.removeEventListener('keydown', closeOnEscape, true)
      }
    },
  )

  return (
    <div ref={rootRef} class="core-chat-session__menu-root">
      <button
        ref={triggerRef}
        type="button"
        class="core-chat-session__menu-trigger"
        aria-label={i18n.tr(`Flere valg for ${props.item.title}`, `More options for ${props.item.title}`)}
        aria-haspopup="menu"
        aria-expanded={props.open ? 'true' : 'false'}
        onClick={(event) => {
          event.stopPropagation()
          props.onOpenChange(!props.open)
        }}
      >
        <MoreVertical class="size-3.5" />
      </button>
      <Show when={props.open}>
        <menu
          class="verevon-popover core-chat-session__menu"
          role="menu"
          aria-label={i18n.tr(`Handlinger for ${props.item.title}`, `Actions for ${props.item.title}`)}
        >
          <li role="presentation">
            <button
              ref={firstItemRef}
              type="button"
              role="menuitem"
              class="core-chat-session__menu-item"
              onClick={(event) => {
                event.stopPropagation()
                props.onRenameStart()
              }}
            >
              <PenLine class="size-3.5" />
              {i18n.tr('Gi nytt navn', 'Rename')}
            </button>
          </li>
          <Show when={props.onMoveToProject}>
            <li role="presentation">
              <button type="button" role="menuitem" class="core-chat-session__menu-item" onClick={() => { close(false); props.onMoveToProject?.() }}>
                <Folder class="size-3.5" />
                {i18n.tr('Flytt til prosjekt', 'Move to project')}
              </button>
            </li>
          </Show>
          <li role="presentation">
            <button
              type="button"
              role="menuitemcheckbox"
              aria-checked={props.item.personaEnabled === false ? 'false' : 'true'}
              class="core-chat-session__menu-item"
              onClick={(event) => {
                event.stopPropagation()
                close(false)
                props.onTogglePersona()
              }}
            >
              <Fingerprint class="size-3.5" />
              {props.item.personaEnabled === false
                ? i18n.tr('Slå på persona for denne samtalen', 'Turn persona on for this thread')
                : i18n.tr('Slå av persona for denne samtalen', 'Turn persona off for this thread')}
            </button>
          </li>
          <li role="presentation">
            <SpaceConfirmButton
              role="menuitem"
              label={i18n.tr('Slett', 'Delete')}
              consequence={i18n.tr(
                'Slettes for godt og kan ikke gjenopprettes.',
                'Deleted permanently and cannot be recovered.',
              )}
              confirmLabel={i18n.tr(
                `Bekreft sletting av «${props.item.title}»`,
                `Confirm deleting "${props.item.title}"`,
              )}
              class="core-chat-session__menu-item core-chat-session__menu-item--danger"
              onConfirm={() => props.onDelete()}
            />
          </li>
        </menu>
      </Show>
    </div>
  )
}

type ChatRunStatusChip = { label: string; tone: 'active' | 'attention' }

/** Keep the personal thread rail calm: only active or attention states show a
 * hint. Completed/cancelled runs remain discoverable through the transcript
 * without turning every row into an operator console. */
function chatRunStatusChip(
  value: string | undefined,
  i18n: ReturnType<typeof useI18n>,
): ChatRunStatusChip | undefined {
  const status = value?.trim().toLowerCase()
  if (!status) return undefined
  if (status === 'queued' || status === 'pending') {
    return { label: i18n.tr('I kø', 'Queued'), tone: 'active' }
  }
  if (status === 'running' || status === 'in_progress') {
    return { label: i18n.tr('Pågår', 'Running'), tone: 'active' }
  }
  if (
    status === 'awaiting_approval' ||
    status === 'waiting_approval' ||
    status === 'paused' ||
    status === 'blocked' ||
    status === 'ambiguous'
  ) {
    return { label: i18n.tr('Trenger oppmerksomhet', 'Needs attention'), tone: 'attention' }
  }
  if (status === 'failed' || status === 'timed_out' || status === 'denied' || status === 'rejected') {
    return { label: i18n.tr('Mislyktes', 'Failed'), tone: 'attention' }
  }
  return undefined
}

function formatChatUpdatedAt(value: string, i18n: ReturnType<typeof useI18n>) {
  const updatedAt = Date.parse(value)
  if (Number.isNaN(updatedAt)) return i18n.tr('Lagret tråd', 'Saved thread')
  const ageMs = Date.now() - updatedAt
  if (i18n.locale() === 'no') {
    if (ageMs < 60_000) return 'Akkurat nå'
    if (ageMs < 3_600_000) return `${Math.max(1, Math.round(ageMs / 60_000))}m siden`
    if (ageMs < 86_400_000) return `${Math.max(1, Math.round(ageMs / 3_600_000))}t siden`
    return new Intl.DateTimeFormat('nb-NO', { month: 'short', day: 'numeric' }).format(new Date(updatedAt))
  }
  if (ageMs < 60_000) return 'Just now'
  if (ageMs < 3_600_000) return `${Math.max(1, Math.round(ageMs / 60_000))}m ago`
  if (ageMs < 86_400_000) return `${Math.max(1, Math.round(ageMs / 3_600_000))}h ago`
  return new Intl.DateTimeFormat('en-US', { month: 'short', day: 'numeric' }).format(new Date(updatedAt))
}

function SidebarPanelNavigation(props: {
  section: SidebarSection
  pathname: string
  activeTabId: string | null
  searchQuery: string
}) {
  const i18n = useI18n()
  const normalizedSearchQuery = () => normalizeSearchText(props.searchQuery.trim())
  const isSearchActive = () => normalizedSearchQuery().length > 0
  const [expandedGroups, setExpandedGroups] = createSignal<Set<string>>(new Set())
  const [expandedItems, setExpandedItems] = createSignal<Set<string>>(new Set())
  const activeItemIds = () => getActiveItemIds(props.section, props.pathname)
  const visibleGroups = createMemo(() => {
    const query = normalizedSearchQuery()
    const searchActive = query.length > 0
    const activeTabId = props.activeTabId

    return props.section.panelGroups.reduce<SidebarPanelGroup[]>((groups, group) => {
      const items = group.items.filter((item) => {
        if (!searchActive && activeTabId && item.tabId && item.tabId !== activeTabId) return false
        if (!searchActive) return true
        return [item.label, item.description, item.href].some((value) => normalizeSearchText(value).includes(query))
      })

      if (items.length === 0) return groups
      return [...groups, { ...group, items }]
    }, [])
  })
  const topGroups = () => visibleGroups().filter((group) => !group.alignBottom)
  const bottomGroups = () => visibleGroups().filter((group) => group.alignBottom)

  // The items the address lands on, as one string, for the same reason: a query parameter that
  // lands on the same items must not collapse the groups the person has opened.
  const activeItemKey = createMemo(() => Array.from(getActiveItemIds(props.section, props.pathname)).join('\n'))
  createEffect(
    () => ({
      expandedGroups: new Set(getDefaultExpandedGroups(props.section)),
      expandedItems: new Set(activeItemKey() ? activeItemKey().split('\n') : []),
    }),
    (next) => {
      setExpandedGroups(next.expandedGroups)
      setExpandedItems(next.expandedItems)
    },
  )

  const toggleGroup = (groupId: string) => {
    setExpandedGroups((current) => {
      const next = new Set(current)
      if (next.has(groupId)) next.delete(groupId)
      else next.add(groupId)
      return next
    })
  }

  const toggleItem = (itemId: string) => {
    setExpandedItems((current) => {
      const next = new Set(current)
      if (next.has(itemId)) next.delete(itemId)
      else next.add(itemId)
      return next
    })
  }

  return (
    <Show
      when={visibleGroups().length > 0}
      fallback={<div class="core-sidebar-empty verevon-sidebar-row-normal">{i18n.tr('Ingen treff i denne seksjonen.', 'No matches in this section.')}</div>}
    >
      <nav class="core-sidebar-panel-nav" aria-label={i18n.tr(`${props.section.label} navigasjon`, `${props.section.label} navigation`)}>
        <div>
          <For each={topGroups()}>
            {(group) => (
              <SidebarGroup
                group={group}
                pathname={props.pathname}
                activeItemIds={activeItemIds()}
                expandedGroups={expandedGroups()}
                expandedItems={expandedItems()}
                isSearchActive={isSearchActive()}
                onToggleGroup={toggleGroup}
                onToggleItem={toggleItem}
              />
            )}
          </For>
        </div>
        <Show when={bottomGroups().length}>
          <div class="core-sidebar-panel-nav__bottom">
            <For each={bottomGroups()}>
              {(group) => (
                <SidebarGroup
                  group={group}
                  pathname={props.pathname}
                  activeItemIds={activeItemIds()}
                  expandedGroups={expandedGroups()}
                  expandedItems={expandedItems()}
                  isSearchActive={isSearchActive()}
                  onToggleGroup={toggleGroup}
                  onToggleItem={toggleItem}
                />
              )}
            </For>
          </div>
        </Show>
      </nav>
    </Show>
  )
}

function SidebarGroup(props: {
  group: SidebarPanelGroup
  pathname: string
  activeItemIds: Set<string>
  expandedGroups: Set<string>
  expandedItems: Set<string>
  isSearchActive: boolean
  onToggleGroup: (groupId: string) => void
  onToggleItem: (itemId: string) => void
}) {
  const i18n = useI18n()
  const groupExpanded = () => !props.group.collapsible || props.isSearchActive || props.expandedGroups.has(props.group.id)

  return (
    <section class="core-sidebar-group">
      <Show when={props.group.showHeader !== false}>
        <div class="core-sidebar-group__header">
          <span class="verevon-sidebar-group-title">{props.group.label}</span>
          <Show when={props.group.collapsible}>
            <button
              type="button"
              onClick={() => props.onToggleGroup(props.group.id)}
              aria-expanded={groupExpanded() ? 'true' : 'false'}
              aria-label={groupExpanded() ? i18n.tr(`Slå sammen ${props.group.label}`, `Collapse ${props.group.label}`) : i18n.tr(`Utvid ${props.group.label}`, `Expand ${props.group.label}`)}
              title={groupExpanded() ? i18n.tr(`Slå sammen ${props.group.label}`, `Collapse ${props.group.label}`) : i18n.tr(`Utvid ${props.group.label}`, `Expand ${props.group.label}`)}
            >
              <Show when={groupExpanded()} fallback={<ChevronRight class="size-4" strokeWidth={2.1} />}>
                <ChevronDown class="size-4" strokeWidth={2.1} />
              </Show>
            </button>
          </Show>
        </div>
      </Show>

      <Show when={groupExpanded()}>
        <div class="core-sidebar-group__items">
          <For each={props.group.items}>
            {(item) => (
              <SidebarPanelLink
                item={item}
                pathname={props.pathname}
                expanded={props.expandedItems.has(item.id) || props.activeItemIds.has(item.id)}
                onToggle={() => props.onToggleItem(item.id)}
              />
            )}
          </For>
        </div>
      </Show>
    </section>
  )
}

function SidebarPanelLink(props: {
  item: SidebarPanelItem
  pathname: string
  expanded: boolean
  onToggle: () => void
}) {
  const [searchParams, setSearchParams] = useSearchParams()
  const homeSection = () => chatAppearanceActive('dashboard') && props.pathname.split('?')[0] === '/dashboard'
    ? homeOverviewSidebarItems[props.item.id] : undefined
  const homeActive = () => parseHomeOverview(searchParams.overview) === homeSection()
  const active = () => isSidebarPathActive(props.pathname, props.item.href, props.item.aliases, props.item.exactPathMatch)
  const hasSubItems = () => Boolean(props.item.subItems?.length)

  return (
    <Show
      when={!hasSubItems()}
      fallback={
        <div>
          <button
            type="button"
            onClick={props.onToggle}
            class={cn('core-sidebar-panel-link', active() ? 'verevon-sidebar-panel-active core-sidebar-panel-link--active' : '')}
            aria-expanded={props.expanded ? 'true' : 'false'}
            title={props.item.label}
          >
            <PanelItemIcon icon={props.item.icon} active={active()} />
            <span class={cn('core-sidebar-panel-link__label', active() ? 'verevon-sidebar-row' : 'verevon-sidebar-row-normal')}>{props.item.label}</span>
            <Show when={props.expanded} fallback={<ChevronRight class="size-3.5" strokeWidth={2.2} />}>
              <ChevronDown class="size-3.5" strokeWidth={2.2} />
            </Show>
          </button>
        </div>
      }
    >
      <Show when={homeSection()} fallback={<a href={props.item.href} link aria-current={active() ? 'page' : undefined} class={cn('core-sidebar-panel-link', active() ? 'verevon-sidebar-panel-active core-sidebar-panel-link--active' : '')}>
        <PanelItemIcon icon={props.item.icon} active={active()} />
        <span class={cn('core-sidebar-panel-link__label', active() ? 'verevon-sidebar-row' : 'verevon-sidebar-row-normal')}>{props.item.label}</span>
      </a>}>
        <button type="button" aria-pressed={homeActive() ? 'true' : 'false'} class={cn('core-sidebar-panel-link', homeActive() && 'verevon-sidebar-panel-active core-sidebar-panel-link--active')} onClick={() => {
          const action = homeOverviewActionSchema.parse({ type: 'home.overview.select', section: homeSection() })
          setSearchParams({ overview: action.section === 'home' ? undefined : action.section }, { scroll: false })
        }}>
          <PanelItemIcon icon={props.item.icon} active={homeActive()} />
          <span class={cn('core-sidebar-panel-link__label', homeActive() ? 'verevon-sidebar-row' : 'verevon-sidebar-row-normal')}>{props.item.label}</span>
        </button>
      </Show>
    </Show>
  )
}

function PanelItemIcon(props: { icon: SidebarPanelItem['icon']; active: boolean }) {
  return <Dynamic component={props.icon} class={cn('core-sidebar-panel-link__icon', props.active ? 'core-sidebar-panel-link__icon--active' : '')} strokeWidth={1.7} />
}

function MiniSectionLink(props: { section: SidebarSection; active: boolean; onOpen: () => void }) {
  return (
    <a
      href={props.section.href}
      link
      onClick={() => props.onOpen()}
      aria-current={props.active ? 'page' : undefined}
      aria-label={props.section.label}
      title={props.section.label}
      class={cn('core-mini-nav-button', props.active ? 'verevon-sidebar-mini-active' : '')}
    >
      <Show when={props.active && chatAppearanceActive('navigation')} fallback={<Dynamic component={props.section.id === 'messages' && chatAppearanceActive('navigation') ? MessageCircleMore : props.section.icon} class="size-[18px]" strokeWidth={props.active ? 2 : 1.6} />}>
        <FilledRailIcon name={props.section.id} />
      </Show>
    </a>
  )
}

function MiniAccountLink(props: { active: boolean; children: JSX.Element; i18n: ReturnType<typeof useI18n>; onOpen: () => void }) {
  return (
    <a
      href="/account"
      link
      onClick={props.onOpen}
      aria-current={props.active ? 'page' : undefined}
      aria-label={props.i18n.tr('Konto', 'Account')}
      title={props.i18n.tr('Konto', 'Account')}
      class={cn('core-mini-nav-button', props.active ? 'verevon-sidebar-mini-active' : '')}
    >
      <Show when={props.active && chatAppearanceActive('navigation')} fallback={props.children}><FilledRailIcon name="account" /></Show>
    </a>
  )
}

function MiniActionButton(props: {
  active: boolean
  children: JSX.Element
  label: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={() => props.onClick()}
      aria-label={props.label}
      title={props.label}
      class={cn('core-mini-nav-button', props.active ? 'verevon-sidebar-mini-active' : '')}
    >
      {props.children}
    </button>
  )
}

function getDefaultExpandedGroups(section: SidebarSection) {
  const expandedGroups = new Set<string>()
  for (const group of section.panelGroups) {
    if (group.defaultExpanded) expandedGroups.add(group.id)
  }
  return expandedGroups
}

function getActiveItemIds(section: SidebarSection, pathname: string) {
  const activeItemIds = new Set<string>()
  for (const group of section.panelGroups) {
    for (const item of group.items) {
      if (isSidebarPathActive(pathname, item.href, item.aliases, item.exactPathMatch)) activeItemIds.add(item.id)
    }
  }
  return activeItemIds
}

function normalizeSearchText(value: string) {
  return value
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
}

const sidebarLabels: Record<string, readonly [string, string]> = {
  'section:overview': ['Hjem', 'Home'],
  'section:messages': ['Chat', 'Chat'],
  'section:marketing': ['Markedsføring', 'Marketing'],
  'section:studio': ['Studio', 'Studio'],
  'section:inbox': ['Support', 'Support'],
  'section:social': ['Kalender', 'Calendar'],
  'section:agents': ['Agenter', 'Agents'],
  'section:ingestions': ['Innhenting', 'Ingestions'],
  'section:knowledge': ['Kunnskap', 'Knowledge'],
  'section:insights': ['Innsikt', 'Insights'],
  'section:settings': ['Innstillinger', 'Settings'],
  'tab:my-account': ['Min konto', 'My account'],
  'tab:shared': ['Delt med meg', 'Shared with me'],
  'group:overview-core': ['Oversikt', 'Overview'],
  'group:messages-core': ['Chat', 'Chat'],
  'group:studio-create': ['Opprett', 'Create'],
  'group:studio-linked': ['Tilkoblede systemer', 'Linked systems'],
  'group:inbox-core': ['Support', 'Support'],
  'group:social-channels': ['Kanaler', 'Channels'],
  'group:social-plan': ['Plan', 'Plan'],
  'group:social-intelligence': ['Intelligens', 'Intelligence'],
  'group:social-reuse': ['Gjenbruk', 'Reuse'],
  'group:agents-core': ['Agenter', 'Agents'],
  'group:ingestions-core': ['Innhenting', 'Ingestions'],
  'group:knowledge-core': ['Kunnskap', 'Knowledge'],
  'group:insights-core': ['Måling', 'Measure'],
  'group:settings-core': ['Innstillinger', 'Settings'],
  'item:overview-home': ['Hjem', 'Home'],
  'item:overview-chat': ['Verevon Chat', 'Verevon Chat'],
  'item:overview-studio': ['Studio', 'Studio'],
  'item:overview-inbox': ['Support', 'Support'],
  'item:overview-social': ['Sosialt', 'Social'],
  'item:overview-knowledge': ['Kunnskap', 'Knowledge'],
  'item:overview-insights': ['Innsikt', 'Insights'],
  'item:overview-leads': ['Leads', 'Leads'],
  'item:overview-agents': ['Agenter', 'Agents'],
  'item:messages-start': ['Ny samtale', 'New conversation'],
  'item:messages-inbox': ['Support', 'Support'],
  'item:studio-canvas': ['Canvas', 'Canvas'],
  'item:studio-campaigns': ['Kampanjeplanlegger', 'Campaign planner'],
  'item:studio-templates': ['Maler', 'Templates'],
  'item:studio-social-drafts': ['Sosiale utkast', 'Social drafts'],
  'item:studio-knowledge-assets': ['Kunnskapsressurser', 'Knowledge assets'],
  'item:inbox-home': ['Samtaler', 'Conversations'],
  'item:support-tickets': ['Saker', 'Tickets'],
  'item:inbox-ai': ['AI-samtaler', 'AI conversations'],
  'item:ticketing-suggested': ['Foreslått av AI', 'Suggested by AI'],
  'item:ticketing-my': ['Mine saker', 'My tickets'],
  'item:ticketing-unassigned': ['Uten eier', 'Unassigned'],
  'item:ticketing-sla-risk': ['SLA-risiko', 'SLA risk'],
  'item:social-accounts': ['Kontoer', 'Accounts'],
  'item:social-calendar': ['Kalender', 'Calendar'],
  'item:social-drafts': ['Utkast', 'Drafts'],
  'item:social-approvals': ['Godkjenninger', 'Approvals'],
  'item:social-campaigns': ['Kampanjer', 'Campaigns'],
  'item:social-competitors': ['Konkurrentovervåking', 'Competitor watch'],
  'item:social-trends': ['Trender', 'Trends'],
  'item:social-evergreen': ['Evergreen-kø', 'Evergreen queue'],
  'item:agents-all': ['Alle agenter', 'All agents'],
  'item:agents-chat': ['Arbeidsflate', 'Workspace'],
  'item:ingestions-home': ['Arbeidsflate', 'Workspace'],
  'item:ingestions-knowledge': ['Kunnskap', 'Knowledge'],
  'item:ingestions-chat': ['Spør Verevon', 'Ask Verevon'],
  'item:knowledge-overview': ['Datakilder', 'Data sources'],
  'item:knowledge-chat': ['Spør kunnskapen', 'Ask knowledge'],
  'item:insights-overview': ['Oversikt', 'Overview'],
  'item:insights-social': ['Sosialt', 'Social'],
  'item:insights-inbox': ['Innboks', 'Inbox'],
  'item:insights-agents': ['Agenter', 'Agents'],
  'item:insights-campaigns': ['Kampanjer', 'Campaigns'],
  'item:insights-experiments': ['Eksperimenter', 'Experiments'],
  'item:settings-workspace': ['Arbeidsområde', 'Workspace'],
  'item:settings-members': ['Medlemmer', 'Members'],
  'item:settings-billing': ['Fakturering', 'Billing'],
  'item:settings-security': ['Sikkerhet', 'Security'],
  'item:settings-trust': ['Tillitssenter', 'Trust Center'],
  'item:settings-router-policy': ['Router-policy', 'Router policy'],
  'item:settings-finetune': ['Finjusteringsjobber', 'Fine-tune jobs'],
}

function localizeSidebarSection(section: SidebarSection, i18n: ReturnType<typeof useI18n>): SidebarSection {
  return {
    ...section,
    label: translateSidebarLabel(`section:${section.id}`, section.label, i18n),
    panelTabs: section.panelTabs?.map((tab) => ({
      ...tab,
      label: translateSidebarLabel(`tab:${tab.id}`, tab.label, i18n),
    })),
    panelGroups: section.panelGroups.map((group) => ({
      ...group,
      label: translateSidebarLabel(`group:${group.id}`, group.label, i18n),
      items: group.items.map((item) => ({
        ...item,
        label: translateSidebarLabel(`item:${item.id}`, item.label, i18n),
        subItems: item.subItems?.map((subItem) => ({
          ...subItem,
          label: translateSidebarLabel(`subitem:${subItem.id}`, subItem.label, i18n),
        })),
      })),
    })),
  }
}

function translateSidebarLabel(key: string, fallback: string, i18n: ReturnType<typeof useI18n>): string {
  const label = sidebarLabels[key]
  return label ? i18n.tr(label[0], label[1]) : fallback
}
