import { ChevronDown, ChevronUp } from 'lucide-solid'
import { createEffect, createMemo, createSignal, onMount, Show } from 'solid-js'
import { DashboardCardsRail } from '@/features/dashboard/home/DashboardCards'
import { DashboardComposerPanel, DashboardHomeHeader, DashboardTabs } from '@/features/dashboard/home/DashboardHomeChrome'
import { dashboardCards, type DashboardCard } from '@/features/dashboard/home/dashboard-cards'
import type { DashboardTab } from '@/features/dashboard/home/dashboard-home-types'
import {
  fallbackWorkspaceIdentity,
  formatPlanLabel,
  type WorkspaceIdentity,
} from '@/features/core/lib/shell-data'
import { useCoreWorkspace } from '@/features/core/lib/workspace-context'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'

const cardsPerPage = 3

export default function DashboardHome(props: { workspace?: WorkspaceIdentity } = {}) {
  const i18n = useI18n()
  const shellWorkspace = useCoreWorkspace()
  const [activeTab, setActiveTab] = createSignal<DashboardTab>('Chat')
  const [isLaunching, setIsLaunching] = createSignal(false)
  const [message, setMessage] = createSignal('')
  const [cardPage, setCardPage] = createSignal(0)
  const [searchExpanded, setSearchExpanded] = createSignal(false)
  const [searchPreviewActive, setSearchPreviewActive] = createSignal(false)
  const [knowledgePreviewActive, setKnowledgePreviewActive] = createSignal(false)
  const [cardsRevealed, setCardsRevealed] = createSignal(false)
  // Warm the chat route chunk so the first composer→chat View Transition morph
  // finds its target already mounted (no lazy/Suspense gap on the first launch).
  onMount(() => void import('@/features/chat/components/ChatPage'))
  const workspace = () => props.workspace ?? shellWorkspace() ?? fallbackWorkspaceIdentity
  const displayName = () => firstName(workspace().userName ?? workspace().userEmail)
  const planLabel = () => formatPlanLabel(workspace().plan)
  const homeTitle = () => {
    if (activeTab() === 'Søk') return i18n.tr('Søk på nett og i Verevon', 'Search the web and Verevon')
    if (activeTab() === 'Crawl') return i18n.tr('Hent inn kunnskap', 'Bring in knowledge')

    const greeting = getGreeting(i18n)
    const name = displayName()
    return name ? `${greeting}, ${name}` : greeting
  }
  const pageCount = () => Math.max(1, Math.ceil(dashboardCards.length / cardsPerPage))
  const visibleCards = createMemo(() => {
    const start = cardPage() * cardsPerPage
    return dashboardCards.slice(start, start + cardsPerPage)
  })

  // A result is "present" when the active tab shows a preview/result that should
  // claim the info-cards' space: the Crawl scrape preview or the Søk inline
  // preview. The expanded Søk results page is excluded — it takes over the stage
  // on its own.
  const resultPresent = () =>
    !searchExpanded()
    && ((activeTab() === 'Søk' && searchPreviewActive())
      || (activeTab() === 'Crawl' && knowledgePreviewActive()))
  // Default when a result appears: show the result, hide the cards. The chevron
  // flips `cardsRevealed` → cards come back and the result is tucked away.
  const resultsView = () => resultPresent() && !cardsRevealed()
  const knowledgeBrowserHeaderInline = () => activeTab() === 'Crawl' && resultsView()

  // Once the result clears, drop any manual card reveal so the next result
  // starts from the default (results shown).
  createEffect(() => {
    if (!resultPresent()) setCardsRevealed(false)
  })

  const bandsClass = () => {
    if (searchExpanded()) return 'verevon-home-bands verevon-home-bands--expanded min-h-0 flex-1'
    if (resultsView()) return 'verevon-home-bands verevon-home-bands--results min-h-0 flex-1'
    return 'verevon-home-bands min-h-0 flex-1'
  }
  const composerSectionClass = () => {
    if (searchExpanded()) return 'verevon-home-composer-section verevon-home-composer-section--expanded'
    if (knowledgeBrowserHeaderInline()) return 'verevon-home-composer-section verevon-home-composer-section--results verevon-home-composer-section--knowledge-browser'
    if (resultsView()) return 'verevon-home-composer-section verevon-home-composer-section--results'
    return 'verevon-home-composer-section verevon-home-band verevon-home-band-middle px-4 transition-[padding] duration-500'
  }

  const applyCardPrompt = (card: DashboardCard) => {
    setActiveTab('Chat')
    setMessage(i18n.tr(card.prompt, card.promptEn))
  }

  const changeActiveTab = (tab: DashboardTab) => {
    setActiveTab(tab)
    setCardsRevealed(false)
    if (tab !== 'Søk') {
      setSearchExpanded(false)
      setSearchPreviewActive(false)
    }
    if (tab !== 'Crawl') setKnowledgePreviewActive(false)
  }

  const showNextCards = () => {
    setCardPage((current) => (current + 1) % pageCount())
  }

  return (
    <div class="verevon-dashboard-surface dashboard-home">
      <div class="pointer-events-none absolute inset-0 dashboard-home-grid" aria-hidden="true" />

      <div class={cn('verevon-home-stage relative flex h-full min-h-0 flex-col overflow-hidden', isLaunching() && 'verevon-home-launching')}>
        <DashboardTabs activeTab={activeTab()} onTabChange={changeActiveTab} />

        <div class={bandsClass()}>
          <Show when={!searchExpanded() && !knowledgeBrowserHeaderInline()}>
            <DashboardHomeHeader compact={resultsView()} planLabel={planLabel()} title={homeTitle()} />
          </Show>

          <section class={composerSectionClass()}>
            <DashboardComposerPanel
              activeTab={activeTab()}
              knowledgeInlinePlanLabel={knowledgeBrowserHeaderInline() ? planLabel() : undefined}
              knowledgeInlineTitle={knowledgeBrowserHeaderInline() ? homeTitle() : undefined}
              message={message()}
              onKnowledgePreviewActiveChange={setKnowledgePreviewActive}
              onLaunchStart={() => setIsLaunching(true)}
              onMessageChange={setMessage}
              onSearchExpandedChange={setSearchExpanded}
              onSearchPreviewActiveChange={setSearchPreviewActive}
              previewCollapsed={cardsRevealed()}
              searchExpanded={searchExpanded()}
            />
          </section>

          <Show when={!searchExpanded()}>
            <DashboardCardsRail
              hidden={resultsView()}
              hideNext={resultPresent()}
              onNextPage={showNextCards}
              onPrompt={applyCardPrompt}
              visibleCards={visibleCards()}
            />
          </Show>
        </div>

        <Show when={resultPresent()}>
          <button
            type="button"
            class="verevon-home-results-toggle"
            onClick={() => setCardsRevealed((value) => !value)}
            aria-label={cardsRevealed() ? i18n.tr('Vis resultatet igjen', 'Show the result again') : i18n.tr('Vis kortene', 'Show the cards')}
            title={cardsRevealed() ? i18n.tr('Vis resultatet igjen', 'Show the result again') : i18n.tr('Vis kortene', 'Show the cards')}
          >
            <Show when={cardsRevealed()} fallback={<ChevronDown class="size-4" />}>
              <ChevronUp class="size-4" />
            </Show>
            <span>{cardsRevealed() ? i18n.tr('Resultat', 'Result') : i18n.tr('Kort', 'Cards')}</span>
          </button>
        </Show>
      </div>
    </div>
  )
}

function getGreeting(i18n: ReturnType<typeof useI18n>) {
  const hour = new Date().getHours()

  if (hour < 11) {
    return i18n.tr('God morgen', 'Good morning')
  }

  if (hour < 17) {
    return i18n.tr('God ettermiddag', 'Good afternoon')
  }

  return i18n.tr('God kveld', 'Good evening')
}

function firstName(value?: string | null) {
  const trimmed = value?.trim()
  if (!trimmed) return ''
  if (trimmed.includes('@')) return trimmed.split('@')[0] || ''
  return trimmed.split(/\s+/)[0] || ''
}
