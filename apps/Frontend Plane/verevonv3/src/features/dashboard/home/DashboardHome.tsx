import { ChevronDown, ChevronUp } from '@/shared/icons'
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
import { useAssistantNavigation } from '@/features/core/assistant-navigation/assistant-navigation-context'
import { useAssistantBrowser } from '@/features/core/assistant-agent/assistant-browser-context'
import { DesktopLocalBrowser } from '@/features/browser/desktop/DesktopLocalBrowser'
import { getDesktopBrowser } from '@/features/browser/desktop/desktop-browser'
import { AssistantLiveBrowser } from '@/features/core/components/AssistantLiveBrowser'
import { AssistantRetainedBrowser } from './AssistantRetainedBrowser'
import { getSession } from '@/shared/session/session-store'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { chatAppearanceActive } from '@/features/chat/lib/chat-appearance-preview'
import { DashboardOverview } from './DashboardOverview'
import { DashboardSectionOverview } from './DashboardSectionOverview'
import { homeOverviewLabel, parseHomeOverview } from '@/shared/actions/home-overview'
import { homeTabActionSchema, homeTabParam, parseHomeTab } from '@/shared/actions/home-tab'
import { resetSearchSession } from './dashboard-search-session'
import { useRetentionVerdictProbe } from './local-retention'
import { homeSectionDescription, type HomeWorkSection } from './home-section-data'
import './dashboard-overview.css'

const cardsPerPage = 3

export default function DashboardHome(props: { workspace?: WorkspaceIdentity } = {}) {
  const i18n = useI18n()
  const shellWorkspace = useCoreWorkspace()
  const assistantNavigation = useAssistantNavigation()
  const assistantBrowser = useAssistantBrowser()
  const desktopBrowser = getDesktopBrowser()
  const [desktopPageOpen, setDesktopPageOpen] = createSignal(false)
  const [desktopNavigationError, setDesktopNavigationError] = createSignal<string | null>(null)
  let desktopNavigation = 0
  const clearDesktopNavigationError = () => {
    desktopNavigation += 1
    setDesktopNavigationError(null)
  }
  const [showAgentBrowser, setShowAgentBrowser] = createSignal(true)
  const localAgentView = () => {
    const view = assistantBrowser.view()
    return desktopBrowser && view?.localBrowser && view.status !== 'idle' ? view : null
  }
  const localAgentBusy = () => ['starting', 'running', 'stopping'].includes(localAgentView()?.status ?? '')
  const desktopWorkspace = () => Boolean(desktopBrowser && (desktopPageOpen() || localAgentView()))
  const browserView = () => {
    const view = assistantBrowser.view()
    return view?.kind === 'browser' && !view.localBrowser && view.sessionId && view.runId &&
      (['starting', 'running', 'stopping', 'disconnected'].includes(view.status) ||
        (!view.usesExistingTab && (view.status === 'completed' || view.siteHandoffReady))) ? view : null
  }
  createEffect(() => browserView()?.runId, (runId) => {
    if (runId) {
      setDesktopPageOpen(false)
      setShowAgentBrowser(true)
    }
  })
  createEffect(() => localAgentView()?.runId, (runId) => {
    if (runId) {
      setDesktopPageOpen(true)
      clearDesktopNavigationError()
      setShowAgentBrowser(true)
    }
  })
  createEffect(() => assistantBrowser.revealVersion?.() ?? 0, (version) => { if (version > 0) setShowAgentBrowser(true) })
  const [searchParams, setSearchParams] = useSearchParams()
  const [isLaunching, setIsLaunching] = createSignal(false)
  const [message, setMessage] = createSignal('')
  const [cardPage, setCardPage] = createSignal(0)
  const [searchExpanded, setSearchExpanded] = createSignal(false)
  const [searchPreviewActive, setSearchPreviewActive] = createSignal(false)
  const [webBrowserActive, setWebBrowserActive] = createSignal(false)
  const [knowledgePreviewActive, setKnowledgePreviewActive] = createSignal(false)
  const [cardsRevealed, setCardsRevealed] = createSignal(false)
  // Bumped to remount the search panel, which drops its ~30 local signals at once.
  const [searchResetKey, setSearchResetKey] = createSignal(0)
  const overview = () => chatAppearanceActive('dashboard')
  const section = () => overview() ? parseHomeOverview(searchParams.overview) : 'home'
  const workSection = () => section() !== 'home' ? section() as HomeWorkSection : null
  // The URL owns the tab (?tab=web|extract, none is Chat), so Back/Forward and
  // shared links land on the same tool. ?overview= sections still win.
  const homeTool = () => parseHomeTab(searchParams.tab)
  const activeTab = () => workSection() ? 'Chat' : homeTool()
  const agentBrowserVisible = (): boolean => !workSection() && activeTab() === 'Søk' && (desktopWorkspace() || Boolean(browserView())) && showAgentBrowser()
  createEffect(agentBrowserVisible, (visible) => {
    assistantBrowser.setSurfaceVisible?.(visible)
    return () => assistantBrowser.setSurfaceVisible?.(false)
  })
  // The draft, the saved search and the Nett history obey the workspace's retention
  // verdict, which only a threads listing states. The recent-work card lists them on
  // the Chat overview; on every other tab, and without the overview, Home asks itself.
  useRetentionVerdictProbe(() => overview() && !workSection() && activeTab() === 'Chat')
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
    !workSection() && (!searchExpanded() || webBrowserActive())
    && ((activeTab() === 'Søk' && (searchPreviewActive() || webBrowserActive()))
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
  // A tab's leftovers must not outlive it. This follows the visible tab rather
  // than the click, because Back/Forward and direct links change ?tab= without
  // passing through changeActiveTab.
  createEffect(
    () => activeTab(),
    (tab) => {
      setCardsRevealed(false)
      if (tab !== 'Søk') {
        setWebBrowserActive(false)
        setSearchExpanded(false)
        setSearchPreviewActive(false)
      }
      if (tab !== 'Crawl') setKnowledgePreviewActive(false)
    },
    { defer: true },
  )

  const bandsClass = () => {
    if (!workSection() && searchExpanded() && !webBrowserActive()) return 'verevon-home-bands verevon-home-bands--expanded min-h-0 flex-1'
    if (resultsView()) return 'verevon-home-bands verevon-home-bands--results min-h-0 flex-1'
    return 'verevon-home-bands min-h-0 flex-1'
  }
  const composerSectionClass = () => {
    if (!workSection() && searchExpanded() && !webBrowserActive()) return 'verevon-home-composer-section verevon-home-composer-section--expanded'
    if (knowledgeBrowserHeaderInline()) return 'verevon-home-composer-section verevon-home-composer-section--results verevon-home-composer-section--knowledge-browser'
    if (resultsView()) return 'verevon-home-composer-section verevon-home-composer-section--results'
    return 'verevon-home-composer-section verevon-home-band verevon-home-band-middle px-4 transition-[padding] duration-500'
  }

  // A new tab is a new history entry; unrelated params (e.g. ?overview=) stay.
  // Chat has no param, so choosing it removes ?tab=.
  const selectTab = (tab: DashboardTab) => {
    if (tab === homeTool()) return
    const action = homeTabActionSchema.parse({ type: 'home.tab.select', tab })
    setSearchParams({ tab: homeTabParam(action.tab) })
  }

  const applyCardPrompt = (card: DashboardCard) => {
    selectTab('Chat')
    assistantNavigation.openWithPrompt?.(i18n.tr(card.prompt, card.promptEn))
    // The URL settles a navigation after this click, so a timeout (not a
    // microtask) lets the composer show again before it takes focus.
    setTimeout(() => document.getElementById('verevon-assistant-input')?.focus(), 0)
  }

  const openDesktopBrowser = (url: string) => {
    if (!desktopBrowser) return
    setDesktopPageOpen(true)
    setShowAgentBrowser(true)
    if (localAgentBusy()) return
    clearDesktopNavigationError()
    const attempt = desktopNavigation
    void desktopBrowser.navigate(url).catch(() => {
      if (attempt === desktopNavigation) {
        setDesktopNavigationError(i18n.tr('Kunne ikke åpne nettadressen.', 'Could not open the web address.'))
      }
    })
  }

  const changeActiveTab = (tab: DashboardTab) => {
    if (tab === 'Chat') assistantNavigation.open?.()
    // Re-clicking the active tab changes no URL but still returns to the result.
    setCardsRevealed(false)
    selectTab(tab)
  }

  // Double-clicking the active Nett tab starts the web search over: the query,
  // the saved snapshot and anything in flight go, and the panel remounts empty.
  // The chat draft and the shared 4h web-search cache are not touched.
  const resetSearch = () => {
    resetSearchSession()
    setSearchExpanded(false)
    setSearchPreviewActive(false)
    setSearchResetKey((key) => key + 1)
  }

  const handleActiveTabDoubleClick = (tab: DashboardTab) => {
    if (tab === 'Søk') {
      if (desktopWorkspace()) setShowAgentBrowser(false)
      resetSearch()
    }
  }

  const showNextCards = () => {
    setCardPage((current) => (current + 1) % pageCount())
  }

  return (
    <div class={cn('verevon-dashboard-surface dashboard-home', overview() && 'dashboard-home--overview', (webBrowserActive() || agentBrowserVisible()) && 'dashboard-home--web-browser')} data-home-tab={activeTab()} data-home-section={section()}>
      <div class="pointer-events-none absolute inset-0 dashboard-home-grid" aria-hidden="true" />

      <div class={cn('verevon-home-stage relative flex h-full min-h-0 flex-col overflow-hidden', isLaunching() && 'verevon-home-launching')}>
        <Show when={overview()}>
          <header class="home-overview-heading">
            <div><span class="home-eyebrow">{workspace().name} <span aria-hidden="true">/</span> {i18n.tr('Oversikt', 'Overview')}</span><h1>{homeTitle()}</h1><p>{workSection() ? homeSectionDescription(workSection()!, i18n.tr) : activeTab() === 'Chat' ? i18n.tr('Det siste arbeidet ditt og dagens oppdateringer.', 'Your recent work and today’s updates.') : activeTab() === 'Søk' ? i18n.tr('Finn svar i arbeidsområdet og på nettet.', 'Find answers across your workspace and the web.') : i18n.tr('Gjør nettsider og dokumenter til kunnskap.', 'Turn websites and documents into knowledge.')}</p></div>
            <time datetime={new Date().toISOString()}>{new Date().toLocaleDateString(i18n.locale() === 'no' ? 'nb-NO' : 'en-GB', { weekday: 'long', day: 'numeric', month: 'long' })}</time>
          </header>
        </Show>
        <Show when={!workSection()}><DashboardTabs activeTab={activeTab()} onActiveTabDoubleClick={handleActiveTabDoubleClick} onTabChange={changeActiveTab} /></Show>

        <div class={bandsClass()}>
          <Show when={!overview() && !searchExpanded() && !knowledgeBrowserHeaderInline()}>
            <DashboardHomeHeader compact={resultsView()} planLabel={planLabel()} planTrial={workspace().planTrial} title={homeTitle()} />
          </Show>

          <Show when={activeTab() === 'Søk' && !workSection() && (desktopWorkspace() || (browserView() && !browserView()?.usesExistingTab))}>
            <div class="home-agent-browser-switch" role="group" aria-label={i18n.tr('Nettvisning', 'Web view')}>
              <button type="button" aria-pressed={showAgentBrowser() ? 'true' : 'false'} onClick={() => setShowAgentBrowser(true)}>{desktopWorkspace()
                ? i18n.tr('Lokal nettleser', 'Local browser')
                : browserView()?.browserClaimed ? i18n.tr('Nettleser fra oppgaven', 'Task browser') : i18n.tr('Agentens nettleser', 'Agent browser')}</button>
              <button type="button" aria-pressed={showAgentBrowser() ? 'false' : 'true'} onClick={() => setShowAgentBrowser(false)}>{i18n.tr('Ditt nettsøk', 'Your web search')}</button>
            </div>
          </Show>
          <Show when={agentBrowserVisible() && desktopWorkspace() && desktopBrowser}>{bridge => <div class="home-agent-browser-view">
            <DesktopLocalBrowser
              bridge={bridge()}
              agentBusy={localAgentBusy()}
              stopping={localAgentView()?.status === 'stopping'}
              navigationError={desktopNavigationError()}
              onManualCommand={clearDesktopNavigationError}
              onStop={assistantBrowser.stop}
            />
          </div>}</Show>
          <Show when={agentBrowserVisible() && !desktopWorkspace() && browserView()}>{view => <div class="home-agent-browser-view">
            <Show when={(view().status === 'completed' || view().siteHandoffReady) && view().receipt && getSession().activeOrg?.id}>{orgId => <AssistantRetainedBrowser orgId={orgId()} sessionId={view().sessionId!} modelRunId={view().runId!} receipt={view().receipt} claimed={view().browserClaimed} requiresManualAccess={view().siteHandoffReady} onClaim={id => assistantBrowser.claimRetained?.(id) ?? false} />}</Show>
            <Show when={!view().browserClaimed}><AssistantLiveBrowser surface="workspace" sessionId={view().sessionId!} modelRunId={view().runId!} receipt={view().receipt} action={view().action} finished={view().status === 'completed' || view().siteHandoffReady === true} onStop={assistantBrowser.stop} stopping={view().status === 'stopping'} stopLabel={view().usesExistingTab ? i18n.tr('Stopp og ta over fanen', 'Stop and take over tab') : undefined} /></Show>
          </div>}</Show>
          <Show when={!agentBrowserVisible() && overview() && !workSection() && (activeTab() === 'Chat' || (!searchExpanded() && !resultPresent()))}><DashboardOverview tab={activeTab()} onPrompt={applyCardPrompt} /></Show>
          <Show when={workSection()}>{(selected) => <DashboardSectionOverview section={selected()} />}</Show>

          <Show when={activeTab() !== 'Chat'}>
          <section class={composerSectionClass()} hidden={agentBrowserVisible()}>
            <DashboardComposerPanel
              activeTab={activeTab()}
              browserContextVisible={!agentBrowserVisible()}
              knowledgeInlinePlanLabel={knowledgeBrowserHeaderInline() ? formatPlanBadge(workspace().plan, workspace().planTrial, i18n.tr) : undefined}
              knowledgeInlineTitle={knowledgeBrowserHeaderInline() ? homeTitle() : undefined}
              message={message()}
              onKnowledgePreviewActiveChange={setKnowledgePreviewActive}
              onLaunchStart={() => {
                setIsLaunching(true)
              }}
              onMessageChange={setMessage}
              onSearchExpandedChange={setSearchExpanded}
              onSearchPreviewActiveChange={setSearchPreviewActive}
              onWebBrowserActiveChange={setWebBrowserActive}
              onOpenDesktopBrowser={desktopBrowser ? openDesktopBrowser : undefined}
              previewCollapsed={cardsRevealed()}
              rememberSearches={assistantNavigation.zeroRetention?.() === false}
              searchResetKey={searchResetKey()}
              searchExpanded={!workSection() && searchExpanded() && !webBrowserActive()}
            />
          </section>
          </Show>

          <Show when={!agentBrowserVisible() && !overview() && !searchExpanded()}>
            <DashboardCardsRail
              hidden={resultsView()}
              hideNext={resultPresent()}
              onNextPage={showNextCards}
              onPrompt={applyCardPrompt}
              visibleCards={visibleCards()}
            />
          </Show>
        </div>

        <Show when={!agentBrowserVisible() && !overview() && resultPresent()}>
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
