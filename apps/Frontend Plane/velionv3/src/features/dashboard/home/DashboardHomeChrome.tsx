
import { For, Show } from 'solid-js'
import { DashboardComposer } from '@/features/dashboard/home/DashboardComposer'
import { DashboardPlanBadge } from '@/features/dashboard/home/DashboardPlanBadge'
import { KnowledgeComposer } from '@/features/dashboard/home/KnowledgeComposer'
import { SearchPanel } from '@/features/dashboard/home/DashboardSearchPanel'
import { dashboardTabs, type DashboardTab } from '@/features/dashboard/home/dashboard-home-types'
import { useI18n } from '@/shared/i18n'

export function DashboardHomeHeader(props: {
  compact: boolean
  planLabel: string
  title: string
}) {
  return (
    <section class={props.compact ? 'velion-home-header velion-home-band velion-home-band-top velion-fade-up px-4 items-start pt-8 pb-3' : 'velion-home-header velion-home-band velion-home-band-top velion-fade-up px-4'}>
      <div class="dashboard-home__center">
        <div class={props.compact ? 'velion-home-plan w-full max-w-[720px] opacity-55' : 'velion-home-plan w-full max-w-[720px]'}>
          <DashboardPlanBadge planLabel={props.planLabel} />
        </div>

        <h1 class={props.compact ? 'velion-home-title velion-home-title--compact' : 'velion-home-title'}>
          {props.title}
        </h1>
      </div>
    </section>
  )
}

export function DashboardTabs(props: {
  activeTab: DashboardTab
  onTabChange: (tab: DashboardTab) => void
}) {
  const i18n = useI18n()
  const tabLabel = (tab: DashboardTab) => {
    if (tab === 'Søk') return i18n.tr('Søk', 'Search')
    if (tab === 'Crawl') return i18n.tr('Innhent', 'Crawl')
    return i18n.tr('Chat', 'Chat')
  }

  return (
    <div class="velion-home-tabs sticky top-0 z-10 px-4">
      <div class="dashboard-home-tabs__inner">
        <div class="dashboard-home-tabs__pill">
          <For each={dashboardTabs}>
            {(tab) => (
              <button
                type="button"
                onClick={() => props.onTabChange(tab)}
                title={i18n.tr(`Vis ${tabLabel(tab).toLowerCase()}`, `Show ${tabLabel(tab).toLowerCase()}`)}
                classList={{ 'dashboard-home-tabs__tab--active': props.activeTab === tab }}
                aria-pressed={props.activeTab === tab}
              >
                {tabLabel(tab)}
              </button>
            )}
          </For>
        </div>
      </div>
    </div>
  )
}

export function DashboardComposerPanel(props: {
  activeTab: DashboardTab
  knowledgeInlinePlanLabel?: string
  knowledgeInlineTitle?: string
  message: string
  onKnowledgePreviewActiveChange: (active: boolean) => void
  onLaunchStart: () => void
  onSearchExpandedChange: (expanded: boolean) => void
  onSearchPreviewActiveChange: (active: boolean) => void
  onMessageChange: (value: string) => void
  previewCollapsed: boolean
  searchExpanded: boolean
}) {
  return (
    <div class={props.searchExpanded ? 'velion-home-composer velion-home-composer-expanded velion-panel-in' : 'velion-home-composer velion-panel-in'}>
      <Show
        when={props.activeTab === 'Chat'}
        fallback={props.activeTab === 'Søk'
          ? (
            <SearchPanel
              expanded={props.searchExpanded}
              onExpandedChange={props.onSearchExpandedChange}
              onPreviewActiveChange={props.onSearchPreviewActiveChange}
              previewCollapsed={props.previewCollapsed}
            />
          )
          : (
            <KnowledgeComposer
              inlinePlanLabel={props.knowledgeInlinePlanLabel}
              inlineTitle={props.knowledgeInlineTitle}
              onPreviewActiveChange={props.onKnowledgePreviewActiveChange}
              previewCollapsed={props.previewCollapsed}
            />
          )}
      >
        <DashboardComposer
          message={props.message}
          onLaunchStart={props.onLaunchStart}
          onMessageChange={props.onMessageChange}
        />
      </Show>
    </div>
  )
}
