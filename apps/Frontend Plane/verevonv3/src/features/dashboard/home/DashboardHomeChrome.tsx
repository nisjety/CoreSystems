
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
  planTrial?: boolean
  title: string
}) {
  return (
    <section class={props.compact ? 'verevon-home-header verevon-home-band verevon-home-band-top verevon-fade-up px-4 items-start pt-8 pb-3' : 'verevon-home-header verevon-home-band verevon-home-band-top verevon-fade-up px-4'}>
      <div class="dashboard-home__center">
        <div class={props.compact ? 'verevon-home-plan w-full max-w-[720px] opacity-55' : 'verevon-home-plan w-full max-w-[720px]'}>
          <DashboardPlanBadge planLabel={props.planLabel} planTrial={props.planTrial} />
        </div>

        <h1 class={props.compact ? 'verevon-home-title verevon-home-title--compact' : 'verevon-home-title'}>
          {props.title}
        </h1>
      </div>
    </section>
  )
}

export function DashboardTabs(props: {
  activeTab: DashboardTab
  /** Fires only when the double-clicked tab was already the active one. */
  onActiveTabDoubleClick?: (tab: DashboardTab) => void
  onTabChange: (tab: DashboardTab) => void
}) {
  const i18n = useI18n()
  const tabLabel = (tab: DashboardTab) => {
    if (tab === 'Søk') return i18n.tr('Nett', 'Web')
    if (tab === 'Crawl') return i18n.tr('Hent ut', 'Extract')
    return i18n.tr('Chat', 'Chat')
  }

  return (
    <div class="verevon-home-tabs sticky top-0 z-10 px-4">
      <div class="dashboard-home-tabs__inner">
        <div class="dashboard-home-tabs__pill">
          <For each={dashboardTabs}>
            {(tab) => {
              // A double-click on an inactive tab switches on its first click, so
              // by the time dblclick fires the tab is already active. Remember
              // whether it was active when the sequence began: that one only switches.
              let activeAtFirstClick: boolean | undefined
              return (
                <button
                  type="button"
                  onClick={(event) => {
                    if (event.detail <= 1) activeAtFirstClick = props.activeTab === tab
                    props.onTabChange(tab)
                  }}
                  onDblClick={() => {
                    const wasActive = activeAtFirstClick ?? props.activeTab === tab
                    activeAtFirstClick = undefined
                    if (wasActive && props.activeTab === tab) props.onActiveTabDoubleClick?.(tab)
                  }}
                  title={i18n.tr(`Vis ${tabLabel(tab).toLowerCase()}`, `Show ${tabLabel(tab).toLowerCase()}`)}
                  class={{ 'dashboard-home-tabs__tab--active': props.activeTab === tab }}
                  aria-pressed={props.activeTab === tab ? 'true' : 'false'}
                >
                  {tabLabel(tab)}
                </button>
              )
            }}
          </For>
        </div>
      </div>
    </div>
  )
}

export function DashboardComposerPanel(props: {
  activeTab: DashboardTab
  persistentChat?: boolean
  compactChat?: boolean
  knowledgeInlinePlanLabel?: string
  knowledgeInlineTitle?: string
  message: string
  onKnowledgePreviewActiveChange: (active: boolean) => void
  onLaunchStart: () => void
  onZeroRetentionChange?: (zeroRetention: boolean | undefined) => void
  onSearchExpandedChange: (expanded: boolean) => void
  onSearchPreviewActiveChange: (active: boolean) => void
  onMessageChange: (value: string) => void
  previewCollapsed: boolean
  /** Whether searches and opened pages may go in the Nett history: on only while the composer says the next turn is not zero-retention. */
  rememberSearches?: boolean
  /** Changing it remounts the search panel, which drops all of its state. */
  searchResetKey?: number
  searchExpanded: boolean
}) {
  return (
    <div class={props.searchExpanded ? 'verevon-home-composer verevon-home-composer-expanded verevon-panel-in' : 'verevon-home-composer verevon-panel-in'}>
      <Show when={props.activeTab !== 'Chat'}>
        {props.activeTab === 'Søk'
          ? (
            // A keyed Show rebuilds its child whenever the key changes.
            <Show when={`search-${props.searchResetKey ?? 0}`} keyed>
              <SearchPanel
                expanded={props.searchExpanded}
                onExpandedChange={props.onSearchExpandedChange}
                onPreviewActiveChange={props.onSearchPreviewActiveChange}
                previewCollapsed={props.previewCollapsed}
                rememberSearches={props.rememberSearches}
              />
            </Show>
          )
          : (
            <KnowledgeComposer
              inlinePlanLabel={props.knowledgeInlinePlanLabel}
              inlineTitle={props.knowledgeInlineTitle}
              onPreviewActiveChange={props.onKnowledgePreviewActiveChange}
              previewCollapsed={props.previewCollapsed}
            />
          )}
      </Show>
      <Show when={props.persistentChat || props.activeTab === 'Chat'}>
        <div hidden={props.activeTab !== 'Chat'}>
          <DashboardComposer
            compact={props.compactChat}
            message={props.message}
            onLaunchStart={props.onLaunchStart}
            onMessageChange={props.onMessageChange}
            onZeroRetentionChange={props.onZeroRetentionChange}
          />
        </div>
      </Show>
    </div>
  )
}
