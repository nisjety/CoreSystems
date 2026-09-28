import { ChevronDown, ChevronUp, MessageCircleMore, Minus, MoreHorizontal, Plus } from '@/shared/icons'
import { createEffect, createMemo, createSignal, Show } from 'solid-js'
import { useSearchParams } from '@solidjs/router'
import { DashboardCardsRail } from '@/features/dashboard/home/DashboardCards'
import { DashboardComposerPanel, DashboardHomeHeader, DashboardTabs } from '@/features/dashboard/home/DashboardHomeChrome'
import { dashboardCards, type DashboardCard } from '@/features/dashboard/home/dashboard-cards'
import type { DashboardTab } from '@/features/dashboard/home/dashboard-home-types'
import {
  fallbackWorkspaceIdentity,
  formatPlanBadge,
  formatPlanLabel,
  type WorkspaceIdentity,
} from '@/features/core/lib/shell-data'
import { useCoreWorkspace } from '@/features/core/lib/workspace-context'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { chatAppearanceActive } from '@/features/chat/lib/chat-appearance-preview'
import { DashboardOverview } from './DashboardOverview'
import { DashboardSectionOverview } from './DashboardSectionOverview'
import { homeOverviewLabel, parseHomeOverview } from '@/shared/actions/home-overview'
import { homeSectionDescription, type HomeWorkSection } from './home-section-data'
import './dashboard-overview.css'

const cardsPerPage = 3

export default function DashboardHome(props: { workspace?: WorkspaceIdentity } = {}) {
  const i18n = useI18n()
  const shellWorkspace = useCoreWorkspace()
  const [searchParams] = useSearchParams()
  const [homeTool, setActiveTab] = createSignal<DashboardTab>('Chat')
  const [isLaunching, setIsLaunching] = createSignal(false)
  const [message, setMessage] = createSignal('')
  const [cardPage, setCardPage] = createSignal(0)
  const [searchExpanded, setSearchExpanded] = createSignal(false)
  const [searchPreviewActive, setSearchPreviewActive] = createSignal(false)
  const [knowledgePreviewActive, setKnowledgePreviewActive] = createSignal(false)
  const [cardsRevealed, setCardsRevealed] = createSignal(false)
  const [dockExpanded, setDockExpanded] = createSignal(false)
  const [dockMinimized, setDockMinimized] = createSignal(false)
  const overview = () => chatAppearanceActive('dashboard')
  const section = () => overview() ? parseHomeOverview(searchParams.overview) : 'home'
  const workSection = () => section() !== 'home' ? section() as HomeWorkSection : null
  const activeTab = () => workSection() ? 'Chat' : homeTool()
  // Warm the chat route chunk so the first composer→chat View Transition morph
  // finds its target already mounted (no lazy/Suspense gap on the first launch).
  createEffect(
    () => undefined,
    () => {
      void import('@/features/chat/components/ChatPage')
    },
  )
  const workspace = () => props.workspace ?? shellWorkspace() ?? fallbackWorkspaceIdentity
  const displayName = () => firstName(workspace().userName ?? workspace().userEmail)
  const planLabel = () => formatPlanLabel(workspace().plan)
  const homeTitle = () => {
    if (workSection()) return homeOverviewLabel(section(), i18n.tr)
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
    !workSection() && !searchExpanded()
    && ((activeTab() === 'Søk' && searchPreviewActive())
      || (activeTab() === 'Crawl' && knowledgePreviewActive()))
  // Default when a result appears: show the result, hide the cards. The chevron
  // flips `cardsRevealed` → cards come back and the result is tucked away.
  const resultsView = () => resultPresent() && !cardsRevealed()
  const knowledgeBrowserHeaderInline = () => activeTab() === 'Crawl' && resultsView()

  // Once the result clears, drop any manual card reveal so the next result
  // starts from the default (results shown).
  createEffect(
    () => resultPresent(),
    (present) => {
      if (!present) setCardsRevealed(false)
    },
  )

  const bandsClass = () => {
    if (!workSection() && searchExpanded()) return 'verevon-home-bands verevon-home-bands--expanded min-h-0 flex-1'
    if (resultsView()) return 'verevon-home-bands verevon-home-bands--results min-h-0 flex-1'
    return 'verevon-home-bands min-h-0 flex-1'
  }
  const composerSectionClass = () => {
    if (!workSection() && searchExpanded()) return 'verevon-home-composer-section verevon-home-composer-section--expanded'
    if (knowledgeBrowserHeaderInline()) return 'verevon-home-composer-section verevon-home-composer-section--results verevon-home-composer-section--knowledge-browser'
    if (resultsView()) return 'verevon-home-composer-section verevon-home-composer-section--results'
    return 'verevon-home-composer-section verevon-home-band verevon-home-band-middle px-4 transition-[padding] duration-500'
  }

  const applyCardPrompt = (card: DashboardCard) => {
    setActiveTab('Chat')
    setMessage(i18n.tr(card.prompt, card.promptEn))
    setDockMinimized(false)
    setDockExpanded(true)
    queueMicrotask(() => document.getElementById('verevon-dashboard-input')?.focus())
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
    <div class={cn('verevon-dashboard-surface dashboard-home', overview() && 'dashboard-home--overview')} data-home-tab={activeTab()} data-home-section={section()}>
      <div class="pointer-events-none absolute inset-0 dashboard-home-grid" aria-hidden="true" />

      <div class={cn('verevon-home-stage relative flex h-full min-h-0 flex-col overflow-hidden', isLaunching() && 'verevon-home-launching')}>
        <Show when={overview()}>
          <header class="home-overview-heading">
            <div><span class="home-eyebrow">{workspace().name} <span aria-hidden="true">/</span> {i18n.tr('Oversikt', 'Overview')}</span><h1>{homeTitle()}</h1><p>{workSection() ? homeSectionDescription(workSection()!, i18n.tr) : activeTab() === 'Chat' ? i18n.tr('Det siste arbeidet ditt og dagens oppdateringer.', 'Your recent work and today’s updates.') : activeTab() === 'Søk' ? i18n.tr('Finn svar i arbeidsområdet og på nettet.', 'Find answers across your workspace and the web.') : i18n.tr('Gjør nettsider og dokumenter til kunnskap.', 'Turn websites and documents into knowledge.')}</p></div>
            <time datetime={new Date().toISOString()}>{new Date().toLocaleDateString(i18n.locale() === 'no' ? 'nb-NO' : 'en-GB', { weekday: 'long', day: 'numeric', month: 'long' })}</time>
          </header>
        </Show>
        <Show when={!workSection()}><DashboardTabs activeTab={activeTab()} onTabChange={changeActiveTab} /></Show>

        <div class={bandsClass()}>
          <Show when={!overview() && !searchExpanded() && !knowledgeBrowserHeaderInline()}>
            <DashboardHomeHeader compact={resultsView()} planLabel={planLabel()} planTrial={workspace().planTrial} title={homeTitle()} />
          </Show>

          <Show when={overview() && !workSection() && (activeTab() === 'Chat' || (!searchExpanded() && !resultPresent()))}><DashboardOverview onPrompt={applyCardPrompt} /></Show>
          <Show when={workSection()}>{(selected) => <DashboardSectionOverview section={selected()} />}</Show>

          <section class={cn(composerSectionClass(), overview() && activeTab() === 'Chat' && 'home-chat-dock')} data-dock-expanded={dockExpanded() ? 'true' : 'false'} data-dock-minimized={dockMinimized() ? 'true' : 'false'} aria-label={overview() && activeTab() === 'Chat' ? i18n.tr('Spør Verevon', 'Ask Verevon') : undefined}>
            <Show when={overview() && activeTab() === 'Chat'}>
              <div class="home-chat-dock__header">
                <button type="button" aria-label={dockMinimized() ? i18n.tr('Åpne skrivefelt', 'Expand chat bar') : i18n.tr('Minimer skrivefelt', 'Minimize chat bar')} aria-expanded={dockMinimized() ? 'false' : 'true'} onClick={() => setDockMinimized((value) => !value)}><Show when={dockMinimized()} fallback={<Minus size={16} />}><Plus size={16} /></Show></button>
                <MessageCircleMore size={16} /><strong>{i18n.tr('Spør Verevon', 'Ask Verevon')}</strong><span>{i18n.tr('Ny samtale', 'New conversation')}</span>
                <button type="button" aria-label={i18n.tr('Flere chattevalg', 'More chat options')} title={i18n.tr('Modell, kontekst og verktøy', 'Model, context and tools')} aria-expanded={dockExpanded() ? 'true' : 'false'} onClick={() => { setDockMinimized(false); setDockExpanded((value) => !value) }}><MoreHorizontal size={18} /></button>
              </div>
            </Show>
            <DashboardComposerPanel
              persistentChat={overview()}
              compactChat={overview() && !dockExpanded()}
              activeTab={activeTab()}
              knowledgeInlinePlanLabel={knowledgeBrowserHeaderInline() ? formatPlanBadge(workspace().plan, workspace().planTrial, i18n.tr) : undefined}
              knowledgeInlineTitle={knowledgeBrowserHeaderInline() ? homeTitle() : undefined}
              message={message()}
              onKnowledgePreviewActiveChange={setKnowledgePreviewActive}
              onLaunchStart={() => setIsLaunching(true)}
              onMessageChange={setMessage}
              onSearchExpandedChange={setSearchExpanded}
              onSearchPreviewActiveChange={setSearchPreviewActive}
              previewCollapsed={cardsRevealed()}
              searchExpanded={!workSection() && searchExpanded()}
            />
          </section>

          <Show when={!overview() && !searchExpanded()}>
            <DashboardCardsRail
              hidden={resultsView()}
              hideNext={resultPresent()}
              onNextPage={showNextCards}
              onPrompt={applyCardPrompt}
              visibleCards={visibleCards()}
            />
          </Show>
        </div>

        <Show when={!overview() && resultPresent()}>
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
