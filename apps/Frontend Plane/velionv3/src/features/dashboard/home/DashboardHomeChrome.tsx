
import { For, Show } from 'solid-js'
import { DashboardComposer } from '@/features/dashboard/home/DashboardComposer'
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
          <PlanBadge planLabel={props.planLabel} />
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

function PlanBadge(props: { planLabel: string }) {
  const i18n = useI18n()
  // Upgrade is only offered on Trial. On any paid plan we drop the CTA and tint
  // the plan text with the accent (the same color the Upgrade button used).
  const isTrial = () => props.planLabel.trim().toLowerCase() === 'trial'
  return (
    <span
      class="dashboard-home-plan-badge"
      classList={{ 'dashboard-home-plan-badge--paid': !isTrial() }}
    >
      {localPlanLabel(props.planLabel, i18n)} {i18n.tr('plan', 'Plan')}
      <Show when={isTrial()}>
        <span>·</span>
        <button type="button">{i18n.tr('Oppgrader', 'Upgrade')}</button>
      </Show>
    </span>
  )
}

function localPlanLabel(planLabel: string, i18n: ReturnType<typeof useI18n>) {
  const normalized = planLabel.trim().toLowerCase()
  if (normalized === 'advanced') return i18n.tr('Avansert', 'Advanced')
  if (normalized === 'custom') return i18n.tr('Tilpasset', 'Custom')
  if (normalized === 'enterprise') return i18n.tr('Enterprise', 'Enterprise')
  if (normalized === 'essential') return i18n.tr('Essential', 'Essential')
  if (normalized === 'expert') return i18n.tr('Ekspert', 'Expert')
  if (normalized === 'free') return i18n.tr('Gratis', 'Free')
  if (normalized === 'hobby') return i18n.tr('Hobby', 'Hobby')
  if (normalized === 'standard') return i18n.tr('Standard', 'Standard')
  if (normalized === 'trial') return i18n.tr('Prøve', 'Trial')
  return planLabel
}

export function DashboardComposerPanel(props: {
  activeTab: DashboardTab
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
