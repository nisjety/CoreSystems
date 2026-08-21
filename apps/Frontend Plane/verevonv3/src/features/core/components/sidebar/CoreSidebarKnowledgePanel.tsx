import { useLocation, useNavigate } from '@solidjs/router'
import {
  BarChart3,
  BookOpen,
  CheckCircle2,
  ChevronDown,
  Database,
  FileText,
  Folder,
  Globe2,
  KeyRound,
  Link2,
  Plus,
  ShieldCheck,
  type LucideProps,
} from '@/shared/icons'
import { createEffect, createMemo, createSignal, createUniqueId, For, Show, type Component } from 'solid-js'
import { Dynamic } from '@solidjs/web'
import {
  SidebarEmptyState,
  SidebarPanelTitle,
  SidebarSearchField,
} from '@/features/core/components/sidebar/CoreSidebarPrimitives'
import {
  knowledgeSearchQuery,
  setKnowledgeAddSourceRequested,
  setKnowledgeSearchQuery,
} from '@/features/knowledge/state/knowledge-filter-store'
import { useI18n } from '@/shared/i18n'
import {
  buildKnowledgeSidebarLiveData,
  filterKnowledgeFolders,
  filterKnowledgeSources,
  getKnowledgeFolderSourceIds,
  getVisibleKnowledgeTags,
  loadKnowledgeSources,
  type KnowledgeSidebarFolderNode,
  type LiveKnowledgePayload,
  type LiveKnowledgeSource,
  type LiveKnowledgeSourceType,
} from '@/shared/api/knowledge-live-client'
import { cn } from '@/shared/lib/cn'

type KnowledgeIcon = Component<LucideProps>
type KnowledgeSidebarModeId = 'folders' | 'sources' | 'tags'

const knowledgeSidebarModeOptions: Array<{ id: KnowledgeSidebarModeId; label: string; icon: KnowledgeIcon }> = [
  { id: 'folders', label: 'Knowledge Base', icon: BookOpen },
  { id: 'sources', label: 'Sources', icon: Database },
  { id: 'tags', label: 'Tags', icon: KeyRound },
]
const defaultKnowledgeModeOption = knowledgeSidebarModeOptions[0]!

const sourceTypeIcon: Record<LiveKnowledgeSourceType, KnowledgeIcon> = {
  Docs: FileText,
  Notion: Link2,
  PDF: FileText,
  URL: Globe2,
}

export function KnowledgeExpandedSidebarPanel(props: { onCollapse: () => void }) {
  const i18n = useI18n()
  const navigate = useNavigate()
  const location = useLocation()
  // Sidebar clicks drive the /knowledge main pane through the shared
  // knowledge-filter store: the clicked item's label becomes the pane's
  // search query (its filterKnowledgePayload matches folder/file/source
  // titles), and a second click on the same label toggles the query off.
  const goToKnowledge = () => {
    if (location.pathname !== '/knowledge') navigate('/knowledge')
  }
  const applyKnowledgeFilter = (label: string) => {
    setKnowledgeSearchQuery(knowledgeSearchQuery() === label ? '' : label)
    goToKnowledge()
  }
  const [activeMode, setActiveMode] = createSignal<KnowledgeSidebarModeId>('folders')
  const [activeFolderId, setActiveFolderId] = createSignal('')
  const [activeSourceId, setActiveSourceId] = createSignal('')
  const [error, setError] = createSignal<string | null>(null)
  const [liveKnowledge, setLiveKnowledge] = createSignal<LiveKnowledgePayload | null>(null)
  const [loading, setLoading] = createSignal(true)
  const [searchQuery, setSearchQuery] = createSignal('')
  const normalizedSearch = () => searchQuery().trim().toLowerCase()
  const sidebarData = createMemo(() => liveKnowledge() ? buildKnowledgeSidebarLiveData(liveKnowledge()!) : null)
  const activeFolderSourceIds = () => {
    const data = sidebarData()
    if (!data) return null
    return getKnowledgeFolderSourceIds(data.folders, activeFolderId())
  }
  const scopedSources = () => {
    const data = sidebarData()
    const folderSourceIds = activeFolderSourceIds()
    if (!data) return []
    return data.sources.filter((source) => !folderSourceIds || folderSourceIds.includes(source.id))
  }
  const visibleFolders = () => filterKnowledgeFolders(sidebarData()?.folders ?? [], normalizedSearch())
  const visibleSources = () => filterKnowledgeSources(scopedSources(), normalizedSearch())
  const visibleTags = createMemo(() => getVisibleKnowledgeTags(scopedSources(), normalizedSearch()))

  createEffect(
    () => undefined,
    () => {
      const controller = new AbortController()
      setLoading(true)
      setError(null)
      loadKnowledgeSources(controller.signal)
        .then((payload) => {
          setLiveKnowledge(payload)
        })
        .catch((reason) => {
          if (controller.signal.aborted) return
          setLiveKnowledge(null)
          setError(reason instanceof Error ? reason.message : i18n.tr('Kunnskaps-API er utilgjengelig.', 'Knowledge API unavailable.'))
        })
        .finally(() => {
          if (!controller.signal.aborted) setLoading(false)
        })

      return () => controller.abort()
    },
  )

  createEffect(
    () => ({ data: sidebarData(), folderId: activeFolderId(), sourceId: activeSourceId() }),
    ({ data, folderId, sourceId }) => {
      if (!data) return
      if (!folderId || !folderExists(data.folders, folderId)) {
        setActiveFolderId(data.folders[0]?.id ?? '')
      }
      if (!sourceId || !data.sources.some((source) => source.id === sourceId)) {
        setActiveSourceId(data.sources[0]?.id ?? '')
      }
    },
  )

  return (
    <div class="core-sidebar-dedicated-panel">
      <SidebarPanelTitle onCollapse={props.onCollapse}>{i18n.tr('Kunnskap', 'Knowledge')}</SidebarPanelTitle>

      <SidebarSearchField
        ariaLabel={i18n.tr('Filtrer kunnskapsseksjon', 'Filter knowledge section')}
        class="core-sidebar-search-compact"
        value={searchQuery()}
        onChange={setSearchQuery}
      />

      <KnowledgeModeSelector value={activeMode()} onChange={setActiveMode} />

      <nav class="core-sidebar-dedicated-nav" aria-label={i18n.tr('Kunnskapsnavigasjon', 'Knowledge navigation')}>
        <Show when={activeMode() === 'folders'}>
          <div class="core-sidebar-dedicated-nav__stack core-sidebar-knowledge-stack">
            <section>
              <div class="core-sidebar-dedicated-group-header">
                <h2 class="verevon-sidebar-group-title">{i18n.tr('Samlinger', 'Collections')}</h2>
                <button
                  type="button"
                  class="core-sidebar-round-add"
                  aria-label={i18n.tr('Legg til samling', 'Add collection')}
                  title={i18n.tr('Legg til samling', 'Add collection')}
                  onClick={() => {
                    // Raise the one-shot request BEFORE navigating so the
                    // /knowledge page's consumer effect sees it on mount.
                    setKnowledgeAddSourceRequested(true)
                    goToKnowledge()
                  }}
                >
                  <Plus class="size-4" strokeWidth={1.9} />
                </button>
              </div>
              <div class="core-sidebar-link-list">
                <Show when={!loading() || sidebarData()} fallback={<SidebarEmptyState label={i18n.tr('Laster samlinger ...', 'Loading collections ...')} />}>
                  <Show when={!error()} fallback={<SidebarEmptyState label={error() ?? i18n.tr('Kunnskaps-API er utilgjengelig.', 'Knowledge API unavailable.')} />}>
                    <For each={visibleFolders()} fallback={<SidebarEmptyState label={i18n.tr('Fant ingen samlinger.', 'No collections found.')} />}>
                      {(folder) => (
                        <KnowledgeSidebarTreeItem
                          activeFolderId={activeFolderId()}
                          depth={0}
                          folder={folder}
                          onSelect={(selected) => {
                            setActiveFolderId(selected.id)
                            applyKnowledgeFilter(selected.label)
                          }}
                        />
                      )}
                    </For>
                  </Show>
                </Show>
              </div>
            </section>

            <KnowledgeSourcesList
              activeSourceId={activeSourceId()}
              error={error()}
              limit={4}
              loading={loading()}
              onSelect={(source) => {
                setActiveSourceId(source.id)
                applyKnowledgeFilter(source.title)
              }}
              sources={visibleSources()}
              title={i18n.tr('Festede kilder', 'Pinned sources')}
            />

            <section>
              <div class="core-sidebar-dedicated-group-header">
                <h2 class="verevon-sidebar-group-title">{i18n.tr('Innsikt', 'Insights')}</h2>
              </div>
              <div class="core-sidebar-link-list">
                <a href="/insights/overview" link class="core-sidebar-section-link">
                  <BarChart3 class="core-sidebar-dedicated-icon" strokeWidth={1.75} />
                  <span>{i18n.tr('Innsikt', 'Insights')}</span>
                </a>
              </div>
            </section>
          </div>
        </Show>

        <Show when={activeMode() === 'sources'}>
          <KnowledgeSourcesList
            activeSourceId={activeSourceId()}
            error={error()}
            loading={loading()}
            onSelect={(source) => {
              setActiveSourceId(source.id)
              applyKnowledgeFilter(source.title)
            }}
            sources={visibleSources()}
            title={i18n.tr('Kilder', 'Sources')}
          />
        </Show>

        <Show when={activeMode() === 'tags'}>
          <section>
            <div class="core-sidebar-dedicated-group-header">
              <h2 class="verevon-sidebar-group-title">{i18n.tr('Etiketter', 'Tags')}</h2>
              <span class="verevon-sidebar-secondary core-sidebar-muted-count">{visibleTags().length}</span>
            </div>
            <div class="core-sidebar-link-list">
              <Show when={!loading() || sidebarData()} fallback={<SidebarEmptyState label={i18n.tr('Laster etiketter ...', 'Loading tags ...')} />}>
                <Show when={!error()} fallback={<SidebarEmptyState label={error() ?? i18n.tr('Kunnskaps-API er utilgjengelig.', 'Knowledge API unavailable.')} />}>
                  <For each={visibleTags()} fallback={<SidebarEmptyState label={i18n.tr('Fant ingen etiketter.', 'No tags found.')} />}>
                    {(tag) => (
                      <button
                        type="button"
                        class={cn('core-sidebar-section-link', knowledgeSearchQuery() === tag.label && 'core-sidebar-source-link--active')}
                        aria-pressed={knowledgeSearchQuery() === tag.label ? 'true' : 'false'}
                        onClick={() => applyKnowledgeFilter(tag.label)}
                      >
                        <KeyRound class="core-sidebar-dedicated-icon" strokeWidth={1.75} />
                        <span>{tag.label}</span>
                        <span class="core-sidebar-count-pill">{tag.count}</span>
                      </button>
                    )}
                  </For>
                </Show>
              </Show>
            </div>
          </section>
        </Show>
      </nav>
    </div>
  )
}

function KnowledgeModeSelector(props: {
  onChange: (value: KnowledgeSidebarModeId) => void
  value: KnowledgeSidebarModeId
}) {
  const i18n = useI18n()
  let rootRef!: HTMLDivElement
  const [open, setOpen] = createSignal(false)
  const listboxId = createUniqueId()
  const selectedIndex = createMemo(() => {
    const optionIndex = knowledgeSidebarModeOptions.findIndex((option) => option.id === props.value)
    return optionIndex >= 0 ? optionIndex : 0
  })
  const [activeIndex, setActiveIndex] = createSignal(0)
  const selectedOption = createMemo(() => knowledgeSidebarModeOptions[selectedIndex()] ?? defaultKnowledgeModeOption)

  const selectOptionAtIndex = (nextIndex: number) => {
    const option = knowledgeSidebarModeOptions[nextIndex]
    if (!option) return

    props.onChange(option.id)
    setOpen(false)
  }

  const moveActiveOption = (direction: 1 | -1) => {
    setActiveIndex((currentIndex) => {
      const baseIndex = open() ? currentIndex : selectedIndex()
      return (baseIndex + direction + knowledgeSidebarModeOptions.length) % knowledgeSidebarModeOptions.length
    })
    setOpen(true)
  }

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      moveActiveOption(1)
      return
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      moveActiveOption(-1)
      return
    }
    if (event.key === 'Home') {
      event.preventDefault()
      setActiveIndex(0)
      setOpen(true)
      return
    }
    if (event.key === 'End') {
      event.preventDefault()
      setActiveIndex(knowledgeSidebarModeOptions.length - 1)
      setOpen(true)
      return
    }
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      if (open()) {
        selectOptionAtIndex(activeIndex())
        return
      }
      setActiveIndex(selectedIndex())
      setOpen(true)
      return
    }
    if (event.key === 'Escape') {
      setOpen(false)
    }
  }

  createEffect(
    () => selectedIndex(),
    (index) => {
      setActiveIndex(index)
    },
  )

  createEffect(
    () => open(),
    (isOpen) => {
      if (!isOpen) return

      const closeOnOutsidePointer = (event: PointerEvent) => {
        const target = event.target as Node | null
        if (target && rootRef.contains(target)) return
        setOpen(false)
      }

      const closeOnEscape = (event: KeyboardEvent) => {
        if (event.key === 'Escape') setOpen(false)
      }

      document.addEventListener('pointerdown', closeOnOutsidePointer, true)
      window.addEventListener('keydown', closeOnEscape)
      return () => {
        document.removeEventListener('pointerdown', closeOnOutsidePointer, true)
        window.removeEventListener('keydown', closeOnEscape)
      }
    },
  )

  return (
    <div ref={rootRef} class="core-sidebar-select">
      <span class="core-sidebar-select__icon">
        <Dynamic component={selectedOption().icon} class="size-3" strokeWidth={2.1} />
      </span>
      <button
        type="button"
        aria-label={i18n.tr('Velg kunnskapsvisning', 'Select knowledge view')}
        aria-haspopup="menu"
        aria-expanded={open() ? 'true' : 'false'}
        aria-controls={open() ? listboxId : undefined}
        onClick={() => {
          setActiveIndex(selectedIndex())
          setOpen((current) => !current)
        }}
        onKeyDown={handleKeyDown}
        class="core-sidebar-select__button"
      >
        <span>{knowledgeModeLabel(selectedOption().id, i18n)}</span>
      </button>
      <ChevronDown class={cn('core-sidebar-select__chevron', open() && 'rotate-180')} strokeWidth={2.2} />
      <Show when={open()}>
        <menu id={listboxId} class="verevon-popover core-sidebar-select__menu" aria-label={i18n.tr('Valg for kunnskapsvisning', 'Knowledge view options')}>
          <For each={knowledgeSidebarModeOptions}>
            {(option, index) => {
              const selected = () => option.id === props.value
              const active = () => activeIndex() === index()
              return (
                <li role="presentation">
                  <button
                    id={`${listboxId}-${option.id}`}
                    type="button"
                    role="menuitemradio"
                    aria-checked={selected() ? 'true' : 'false'}
                    class={cn(
                      'core-sidebar-select__option',
                      selected() && 'core-sidebar-select__option--selected',
                      active() && !selected() && 'core-sidebar-select__option--active',
                    )}
                    onMouseEnter={() => setActiveIndex(index())}
                    onClick={() => selectOptionAtIndex(index())}
                  >
                    <span>{knowledgeModeLabel(option.id, i18n)}</span>
                    <Show when={selected()}>
                      <CheckCircle2 class="size-3.5 core-sidebar-success" strokeWidth={2} />
                    </Show>
                  </button>
                </li>
              )
            }}
          </For>
        </menu>
      </Show>
    </div>
  )
}

function KnowledgeSourcesList(props: {
  activeSourceId: string
  error: string | null
  limit?: number
  loading: boolean
  onSelect: (source: LiveKnowledgeSource) => void
  sources: readonly LiveKnowledgeSource[]
  title: string
}) {
  const i18n = useI18n()
  const visibleSources = () => props.limit ? props.sources.slice(0, props.limit) : props.sources

  return (
    <section>
      <div class="core-sidebar-dedicated-group-header">
        <h2 class="verevon-sidebar-group-title">{props.title}</h2>
        <span class="verevon-sidebar-secondary core-sidebar-muted-count">{visibleSources().length}</span>
      </div>
      <div class="core-sidebar-link-list">
        <Show when={!props.loading || visibleSources().length > 0} fallback={<SidebarEmptyState label={i18n.tr('Laster kilder ...', 'Loading sources ...')} />}>
          <Show when={!props.error} fallback={<SidebarEmptyState label={props.error ?? i18n.tr('Kunnskaps-API er utilgjengelig.', 'Knowledge API unavailable.')} />}>
            <For each={visibleSources()} fallback={<SidebarEmptyState label={i18n.tr('Fant ingen kilder.', 'No sources found.')} />}>
              {(source) => {
                const active = () => props.activeSourceId === source.id
                return (
                  <button
                    type="button"
                    aria-pressed={active() ? 'true' : 'false'}
                    onClick={() => props.onSelect(source)}
                    class={cn('core-sidebar-source-link', active() && 'core-sidebar-source-link--active')}
                  >
                    <Dynamic component={sourceTypeIcon[source.type]} class="core-sidebar-dedicated-icon core-sidebar-source-link__icon" strokeWidth={1.75} />
                    <span>
                      <span class="verevon-sidebar-row-normal">{source.title}</span>
                      <small>{source.chunks} chunks · {source.coverage || source.status}</small>
                    </span>
                  </button>
                )
              }}
            </For>
          </Show>
        </Show>
      </div>
    </section>
  )
}

function knowledgeModeLabel(id: KnowledgeSidebarModeId, i18n: ReturnType<typeof useI18n>): string {
  if (id === 'folders') return i18n.tr('Kunnskapsbase', 'Knowledge Base')
  if (id === 'sources') return i18n.tr('Kilder', 'Sources')
  return i18n.tr('Etiketter', 'Tags')
}

function KnowledgeSidebarTreeItem(props: {
  activeFolderId: string
  depth: number
  folder: KnowledgeSidebarFolderNode
  onSelect: (folder: KnowledgeSidebarFolderNode) => void
}) {
  const active = () => props.activeFolderId === props.folder.id
  const Icon = () => knowledgeFolderIcon(props.folder, props.depth)

  return (
    <div>
      <button
        type="button"
        aria-pressed={active() ? 'true' : 'false'}
        onClick={() => props.onSelect(props.folder)}
        class={cn('core-sidebar-section-link', active() && 'core-sidebar-source-link--active')}
      >
        <Dynamic component={Icon()} class="core-sidebar-dedicated-icon" strokeWidth={1.75} />
        <span class={props.depth === 0 ? 'verevon-sidebar-row-strong' : 'verevon-sidebar-row-normal'}>{props.folder.label}</span>
        <span class="core-sidebar-count-pill">{props.folder.count}</span>
      </button>
      <Show when={props.folder.children?.length}>
        <div class={cn('core-sidebar-subtree', props.depth > 0 && 'core-sidebar-subtree--nested')}>
          <div class="core-sidebar-subnav__line" />
          <For each={props.folder.children}>
            {(child) => (
              <div class="core-sidebar-subtree__child">
                <KnowledgeSidebarTreeItem
                  activeFolderId={props.activeFolderId}
                  depth={props.depth + 1}
                  folder={child}
                  onSelect={props.onSelect}
                />
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  )
}

function knowledgeFolderIcon(folder: KnowledgeSidebarFolderNode, depth: number) {
  if (depth > 0) return BookOpen
  if (folder.providerKey === 'rag') return ShieldCheck
  if (folder.providerKey === 'web') return Globe2
  if (folder.id === 'all') return Folder
  return BookOpen
}

function folderExists(folders: readonly KnowledgeSidebarFolderNode[], folderId: string): boolean {
  return folders.some((folder) => folder.id === folderId || folderExists(folder.children ?? [], folderId))
}
