import { useLocation, useNavigate } from '@solidjs/router'
import { createEffect, createMemo, createSignal, onCleanup, Show, untrack } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { createResource } from '@/shared/lib/create-resource-compat'
import { AgentsProvider } from '@/features/agents/lib/use-agent-selection'
import { CoreNavbar } from '@/features/core/components/CoreNavbar'
import { GlobalAssistant, type AssistantAgentView } from '@/features/core/components/GlobalAssistant'
import type { DashboardComposerSubmitPayload } from '@/features/dashboard/home/DashboardComposer'
import { AssistantNavigationCursor, createAssistantNavigationGuide } from '@/features/core/assistant-navigation/assistant-navigation'
import { AssistantNavigationContext } from '@/features/core/assistant-navigation/assistant-navigation-context'
import { createInlineAssistantController, type InlineAssistantState } from '@/features/core/lib/inline-assistant'
import { createAssistantNavigationAgent, type AssistantNavigationAgentState } from '@/features/core/assistant-agent/assistant-navigation-agent'
import { assistantBrowserTarget, createAssistantBrowserAgent } from '@/features/core/assistant-agent/assistant-browser-agent'
import { AssistantBrowserContext, type NettBrowserOwner, type NettTabSelection } from '@/features/core/assistant-agent/assistant-browser-context'
import { isAssistantTaskBusy, submitAssistantTask } from '@/features/core/assistant-agent/assistant-task-routing'
import { maybeRedact } from '@/features/core/assistant-agent/sensitive-redact'
import { clearAssistantChatDraft, queueAssistantChatDraft } from '@/features/chat/lib/assistant-chat-draft'
import { chatThreadHref } from '@/features/chat/lib/chat-thread-deep-link'
import type { ChatSourceScope } from '@/shared/actions/chat-source-scope'
import { CoreSidebar, SIDEBAR_EXPANDED_WIDTH, SIDEBAR_MINIMIZED_WIDTH } from '@/features/core/components/CoreSidebar'
import { FeedbackWidget } from '@/features/core/components/FeedbackWidget'
import { AnnouncerRegions } from '@/shared/a11y/announcer'
import { chatAppearanceActive, chatAppearanceTokens } from '@/features/chat/lib/chat-appearance-preview'
import { OrgDeletionBanner } from '@/features/core/components/OrgDeletionBanner'
import {
  fallbackWorkspaceIdentity,
  formatPlanLabel,
  routeFromPath,
  type WorkspaceIdentity,
} from '@/features/core/lib/shell-data'
import { readThreadDeepLink } from '@/features/chat/lib/chat-thread-deep-link'
import { createAttentionStore, navbarRefreshDelayMs, type AttentionStore, type AttentionStreamHealth } from '@/features/core/lib/attention-store'
import { CoreWorkspaceContext } from '@/features/core/lib/workspace-context'
import { getAttention, type AttentionItem } from '@/shared/api/attention-client'
import { getBrowserSession } from '@/shared/api/browser-client'
import { getNavbarData, type NavbarPayload } from '@/shared/api/navbar-client'
import { getDeletionStatus } from '@/shared/api/org-deletion-client'
import { useI18n } from '@/shared/i18n'
import { getSession, type SessionState } from '@/shared/session/session-store'
import { RegionBoundary } from '@/shared/ui/resilience/RegionBoundary'

/** Extra height the fixed org-deletion banner (Flow C) occupies below the
 * navbar — added to `--dashboard-navbar-height` so `.core-main`'s top
 * padding reflows content below the banner instead of it being covered. */
const DELETION_BANNER_HEIGHT_PX = 40

export function CoreShell(props: { children?: JSX.Element }) {
  const location = useLocation()
  const navigate = useNavigate()
  const session = getSession()
  // One-time peek to seed the initial signal value; the reactive read for
  // ongoing updates is activeRoute (below), a proper createMemo.
  const initialRoute = untrack(() => routeFromPath(location.pathname))
  const [sidebarExpanded, setSidebarExpanded] = createSignal(
    defaultSidebarExpandedForRoute(initialRoute, { pathname: untrack(() => location.pathname), phone: isPhoneViewport() }),
  )
  const [searchOpen, setSearchOpen] = createSignal(false)
  const [assistantOpen, setAssistantOpen] = createSignal(initialRoute === '/dashboard')
  const [assistantForceReveal, setAssistantForceReveal] = createSignal(false)
  // Home shows the assistant by default. A user-opened assistant stays open
  // while navigating; a Home-only default does not follow to other pages.
  let assistantAutomaticallyOpened = initialRoute === '/dashboard'
  const [assistantZeroRetention] = createSignal<boolean | undefined>(undefined)
  const [assistantMessage, setAssistantMessage] = createSignal('')
  const [continueTab, setContinueTab] = createSignal<NettTabSelection | null>(null)
  const [assistantSourceScope, setAssistantSourceScope] = createSignal<ChatSourceScope>('workspace')
  const [assistantState, setAssistantState] = createSignal<InlineAssistantState>({ turns: [], status: 'idle', error: null, activity: null, threadId: null, needsFullChat: false })
  const assistantScope = () => session.user?.id && session.activeOrg?.id ? JSON.stringify([session.user.id, session.activeOrg.id]) : null
  const nettBrowsers = new Set<() => NettBrowserOwner | null>()
  const registerNettBrowser = (readOwner: () => NettBrowserOwner | null) => {
    nettBrowsers.add(readOwner)
    return () => { nettBrowsers.delete(readOwner) }
  }
  const onNett = () => location.pathname === '/dashboard' && new URLSearchParams(location.search).get('tab') === 'web'
  const currentOwnedTab = async (signal: AbortSignal, sessionId?: string) => {
    if (signal.aborted || !onNett()) return null
    const ownedByUser = (owner: NettBrowserOwner | null): owner is NettBrowserOwner =>
      Boolean(owner && owner.userId === session.user?.id && owner.orgId === session.activeOrg?.id)
    for (const readOwner of nettBrowsers) {
      const owner = readOwner()
      if (!ownedByUser(owner) || (sessionId && sessionId !== owner.sessionId)) continue
      try {
        // The owner projection strips query and fragment data and reports
        // whether Quarry still has a live browser.
        const { session: browser } = await getBrowserSession(owner.orgId, owner.sessionId, signal)
        const visibleOwner = readOwner()
        if (signal.aborted || !onNett() || !nettBrowsers.has(readOwner) ||
          !ownedByUser(visibleOwner) || visibleOwner.userId !== owner.userId ||
          visibleOwner.orgId !== owner.orgId || visibleOwner.sessionId !== owner.sessionId ||
          browser.id !== owner.sessionId || browser.status !== 'live' || browser.control?.mode !== 'human_takeover') return null
        const [activeTab, secondActiveTab] = browser.tabs?.filter(tab => tab.active) ?? []
        if (!activeTab?.url || secondActiveTab) return null
        const url = assistantBrowserTarget(activeTab.url)
        return url ? { tab: activeTab, url } : null
      } catch {
        return null
      }
    }
    return null
  }
  const currentBrowserTarget = async (signal: AbortSignal): Promise<string | null> =>
    (await currentOwnedTab(signal))?.url.href ?? null
  onCleanup(clearAssistantChatDraft)
  const assistantController = createInlineAssistantController({ scope: assistantScope, onChange: setAssistantState, tr: (no, en) => i18n.tr(no, en) })
  onCleanup(assistantController.dispose)
  const assistantGuide = createAssistantNavigationGuide({
    scope: assistantScope,
    navigate: (href) => navigate(href),
    tr: (no, en) => i18n.tr(no, en),
    onOpenChat: () => setAssistantOpen(true),
    onFailure: (error) => {
      setNavigationAgentState({ status: 'error', error })
      setAssistantOpen(true)
    },
    onComplete: (suggestion) => {
      setAssistantMessage((current) => current.trim() ? current : suggestion)
      setAssistantOpen(true)
    },
  })
  const startAssistantNavigation = (prompt: string, origin?: HTMLElement | null) => {
    if (isAssistantTaskBusy(browserAgent.state().status, navigationAgent.state().status, Boolean(assistantGuide.destination()))) return false
    if (!assistantGuide.start(prompt, origin)) return false
    setAssistantMessage('')
    assistantAutomaticallyOpened = false
    setAssistantOpen(false)
    return true
  }
  const [navigationAgentState, setNavigationAgentState] = createSignal<AssistantNavigationAgentState>({ status: 'idle', error: null })
  const navigationAgent = createAssistantNavigationAgent({
    scope: assistantScope,
    onChange: setNavigationAgentState,
    tr: (no, en) => i18n.tr(no, en),
    currentBrowserTarget,
    onBrowserTask: (goal, target) => browserAgent.send(goal, target),
    onAction: (action) => {
      if (!assistantGuide.startAction(action, document.querySelector<HTMLElement>('.global-assistant'))) return false
      browserAgent.reset()
      assistantAutomaticallyOpened = false
      setAssistantOpen(false)
      return true
    },
  })
  onCleanup(navigationAgent.dispose)
  const [agentBrowserSurfaceVisible, setAgentBrowserSurfaceVisible] = createSignal(false)
  const [agentBrowserRevealVersion, setAgentBrowserRevealVersion] = createSignal(0)
  const [browserAgentState, setBrowserAgentState] = createSignal<AssistantAgentView>({ available: false, kind: 'browser', status: 'idle', goal: '', error: null, action: null, sessionId: null, runId: null, receipt: null })
  const browserAgent = createAssistantBrowserAgent({
    scope: () => session.user?.id && session.activeOrg?.id ? { userId: session.user.id, orgId: session.activeOrg.id } : null,
    onChange: setBrowserAgentState,
    tr: (no, en) => i18n.tr(no, en),
  })
  onCleanup(browserAgent.dispose)
  const assistantTaskBusy = () => isAssistantTaskBusy(browserAgent.state().status, navigationAgent.state().status, Boolean(assistantGuide.destination()))
  const prepareContinuation = (tab: NettTabSelection) => {
    if (!onNett() || assistantTaskBusy()) return
    setContinueTab(tab)
    assistantAutomaticallyOpened = false
    setAssistantForceReveal(true)
    setAssistantOpen(true)
  }
  createEffect(onNett, (visible) => { if (!visible) setContinueTab(null) })
  const revealAgentBrowser = () => {
    setAgentBrowserRevealVersion((version) => version + 1)
    const params = new URLSearchParams(location.pathname === '/dashboard' ? location.search : '')
    params.delete('overview')
    params.set('tab', 'web')
    const search = `?${params}`
    if (location.pathname !== '/dashboard' || location.search !== search) navigate(`/dashboard${search}`)
  }
  let lastRevealedRunId: string | null = null
  createEffect(() => {
    const task = browserAgentState()
    return task.kind === 'browser' && task.sessionId && ['starting', 'running', 'disconnected', 'completed'].includes(task.status)
      ? task.runId : null
  }, (runId) => {
    if (!runId || runId === lastRevealedRunId) return
    lastRevealedRunId = runId
    revealAgentBrowser()
  })
  const submitOrdinaryAssistant = (payload: DashboardComposerSubmitPayload | string) => submitAssistantTask(payload, {
    browserStatus: () => browserAgent.state().status,
    navigationStatus: () => navigationAgent.state().status,
    guiding: () => Boolean(assistantGuide.destination()),
    resetNavigation: navigationAgent.reset,
    sendBrowser: browserAgent.send,
    sendNavigation: navigationAgent.send,
    openChatDraft: (action) => {
      if (!queueAssistantChatDraft(action)) return false
      assistantAutomaticallyOpened = false
      setAssistantOpen(false)
      if (location.pathname !== action.href || (action.newChat && location.search)) navigate(action.href)
      return true
    },
    tr: (no, en) => i18n.tr(no, en),
  })
  const submitAssistant = async (goal: string) => {
    const selection = continueTab()
    if (!selection) return submitOrdinaryAssistant(maybeRedact(goal))
    const redactedGoal = maybeRedact(goal)
    const current = await currentOwnedTab(AbortSignal.timeout(5000), selection.sessionId)
    if (continueTab() !== selection) throw new Error(i18n.tr('Fanevalget er avbrutt. Utkastet er beholdt.', 'The tab selection was cancelled. Your draft has been kept.'))
    if (assistantTaskBusy()) throw new Error(i18n.tr('En oppgave er allerede i gang.', 'A task is already running.'))
    if (current?.tab.tabId !== selection.tabId || current.tab.tabEpoch !== selection.tabEpoch ||
        current.tab.holder !== 'human' || current.url.origin !== selection.origin) {
      throw new Error(i18n.tr('Fanen er endret. Velg fanen på nytt for å fortsette.', 'The tab has changed. Select it again to continue.'))
    }
    navigationAgent.reset()
    await browserAgent.send(redactedGoal, selection.origin, {
      sessionId: selection.sessionId,
      tabId: selection.tabId,
      expectedEpoch: selection.tabEpoch,
      expectedOrigin: selection.origin,
    })
    if (continueTab() === selection) setContinueTab(null)
  }
  const [navbarData, { refetch: refetchNavbar }] = createResource(fetchNavbarData)
  const orgId = createMemo(() => session.activeOrg?.id ?? null)
  let assistantOrgId = untrack(assistantScope)
  createEffect(assistantScope, (currentOrgId) => {
    if (currentOrgId === assistantOrgId) return
    assistantOrgId = currentOrgId
    clearAssistantChatDraft()
    assistantGuide.stop()
    navigationAgent.reset()
    browserAgent.reset()
    assistantController.reset()
    setContinueTab(null)
    setAssistantMessage('')
    setAssistantSourceScope('workspace')
    assistantAutomaticallyOpened = routeFromPath(location.pathname) === '/dashboard'
    setAssistantOpen(assistantAutomaticallyOpened)
  })
  const [deletionStatus, { refetch: refetchDeletionStatus }] = createResource(
    orgId,
    fetchDeletionStatus,
  )
  const deletionBannerVisible = createMemo(() => Boolean(deletionStatus.latest?.pending ?? deletionStatus()?.pending))
  const activeRoute = createMemo(() => routeFromPath(location.pathname))
  let lastAssistantRoute = initialRoute
  createEffect(activeRoute, (route) => {
    if (route === lastAssistantRoute) return
    lastAssistantRoute = route
    if (route === '/dashboard') {
      if (!assistantOpen()) {
        assistantAutomaticallyOpened = true
        setAssistantForceReveal(false)
        setAssistantOpen(true)
      }
    } else if (assistantAutomaticallyOpened) {
      assistantAutomaticallyOpened = false
      setAssistantOpen(false)
    }
  })
  // Support uses a compact navigation column so conversation and ticket work
  // retain the horizontal space they need.
  const expandedSidebarWidth = () => activeRoute() === '/support' ? 303 : SIDEBAR_EXPANDED_WIDTH
  const sidebarWidth = () => (sidebarExpanded() ? expandedSidebarWidth() : SIDEBAR_MINIMIZED_WIDTH)
  const workspace = createMemo(() => resolveWorkspaceIdentity(session, navbarData.latest ?? navbarData()))

  // The attention stream (WS16 S5) is read by a store that lives as long as the shell. The navbar keeps the
  // cadence it always had until the server says it serves the stream.
  const i18n = useI18n()
  const [streamSupported, setStreamSupported] = createSignal(false)
  const [streamHealth, setStreamHealth] = createSignal<AttentionStreamHealth>('connecting')
  const [waitingNotice, setWaitingNotice] = createSignal('')
  let attentionStream: AttentionStore | undefined
  let refreshTimer: number | undefined
  let waitingRefetch: number | undefined
  const scheduleRefresh = () => {
    window.clearTimeout(refreshTimer)
    refreshTimer = window.setTimeout(() => {
      void refetchNavbar()
      scheduleRefresh()
    }, navbarRefreshDelayMs({
      streamSupported: streamSupported(),
      streamLive: streamHealth() === 'live',
      hidden: document.hidden,
    }))
  }
  // However many threads start waiting together, the navbar refreshes once. The notice is only for a visible
  // tab that is not already showing that thread.
  const announceWaiting = (item: AttentionItem) => {
    if (waitingRefetch === undefined) {
      waitingRefetch = window.setTimeout(() => {
        waitingRefetch = undefined
        void refetchNavbar()
      }, 0)
    }
    if (document.visibilityState === 'visible' && readThreadDeepLink(window.location.search) !== item.threadId) {
      setWaitingNotice(i18n.tr('En samtale venter på svaret ditt.', 'A conversation is waiting for your reply.'))
    }
  }
  // At phone width the expanded sidebar is a drawer laid over the page. Picking a room or a
  // conversation from it (or from the rooms index) navigates to an open room; the drawer has done
  // its job then, and left open it hides the room the person just chose. Only a change of
  // location counts (the first run is the page the person arrived on, whose default is decided
  // above), only a destination that is an open room, and only at phone width: desktop keeps its
  // column, and a rail tap to another section keeps the drawer open for that section's list.
  let lastNavigationKey = untrack(() => `${location.pathname}${location.search}`)
  createEffect(
    () => `${location.pathname}${location.search}`,
    (key) => {
      if (key === lastNavigationKey) return
      lastNavigationKey = key
      if (isPhoneViewport() && isOpenRoomPath(location.pathname)) setSidebarExpanded(false)
    },
  )

  createEffect(
    () => {
      if (navbarData.latest?.theme?.configured === false) return undefined
      return navbarData.latest?.theme?.theme
    },
    (theme) => {
      if (theme) applyThemePreference(theme)
    },
  )

  // No tracked reads here — this effect only sets up an imperative interval/
  // visibility listener once on mount, so compute is a constant and all the
  // side-effect work lives in the effect function.
  createEffect(
    () => undefined,
    () => {
      const handleVisibilityChange = () => {
        if (!document.hidden) void refetchNavbar()
      }

      scheduleRefresh()
      document.addEventListener('visibilitychange', handleVisibilityChange)
      return () => {
        window.clearTimeout(refreshTimer)
        document.removeEventListener('visibilitychange', handleVisibilityChange)
      }
    },
  )

  // The next navbar refresh is scheduled again whenever the stream's health changes, so it uses the new cadence.
  createEffect(streamHealth, () => scheduleRefresh())

  // The stream is opened only once the server says it serves one; until then the shell makes no stream request.
  createEffect(
    () => undefined,
    () => {
      let disposed = false
      void getAttention()
        .then((listing) => {
          if (disposed || !listing.capabilities.stream) return
          attentionStream = createAttentionStore({ onWaiting: announceWaiting })
          attentionStream.subscribe(setStreamHealth)
          setStreamSupported(true)
          attentionStream.start()
        })
        .catch(() => undefined)
      return () => {
        disposed = true
        window.clearTimeout(waitingRefetch)
        attentionStream?.stop()
        attentionStream = undefined
      }
    },
  )

  return (
    <AgentsProvider routeLocation={location}>
      <AssistantNavigationContext value={{ start: startAssistantNavigation, canStart: assistantGuide.canStart, submit: submitOrdinaryAssistant, zeroRetention: assistantZeroRetention, open: () => { assistantAutomaticallyOpened = false; setAssistantForceReveal(true); setAssistantOpen(true) }, openWithPrompt: (prompt) => { setContinueTab(null); assistantAutomaticallyOpened = false; setAssistantMessage(prompt); setAssistantOpen(true) }, isAssistantVisible: () => assistantOpen() || Boolean(assistantGuide.destination()) }}>
      <div
        class="core-product-shell"
        data-chat-appearance={activeRoute() === '/chat' ? chatAppearanceTokens() : chatAppearanceActive('navigation') ? 'navigation' : undefined}
        data-sidebar-expanded={sidebarExpanded() ? 'true' : 'false'}
        style={{
          '--dashboard-navbar-height': deletionBannerVisible()
            ? `${56 + DELETION_BANNER_HEIGHT_PX}px`
            : '56px',
          '--dashboard-rail-width': `${sidebarWidth()}px`,
          // Only override when the workspace has its own accent; otherwise fall
          // through to the :root default (--verevon-accent, warm) to match v2.
          '--verevon-sidebar-accent': workspace().accentColor || undefined,
        }}
      >
        <RegionBoundary region="navbar" compact>
          <CoreNavbar
            activeRoute={activeRoute()}
            navbarData={navbarData.latest ?? navbarData()}
            onNavbarRefresh={() => void refetchNavbar()}
            assistantOpen={assistantOpen()}
            onAssistantToggle={() => { assistantAutomaticallyOpened = false; setAssistantForceReveal(!assistantOpen()); setAssistantOpen((open) => !open) }}
            searchOpen={searchOpen()}
            onSearchOpenChange={setSearchOpen}
            workspace={workspace()}
            sidebarExpanded={sidebarExpanded()}
            onSidebarToggle={() => setSidebarExpanded((expanded) => !expanded)}
          />
        </RegionBoundary>
        <Show when={deletionBannerVisible()}>
          <OrgDeletionBanner
            status={deletionStatus.latest ?? deletionStatus()}
            onRefetch={() => void refetchDeletionStatus()}
          />
        </Show>
        <div
          role="status"
          aria-live="polite"
          style={{ position: 'absolute', width: '1px', height: '1px', overflow: 'hidden', clip: 'rect(0 0 0 0)', 'white-space': 'nowrap' }}
        >
          {waitingNotice()}
        </div>
        <Show when={chatAppearanceActive('navigation') && sidebarExpanded()}>
          <button type="button" class="core-sidebar-mobile-scrim" aria-label="Close navigation" onClick={() => setSidebarExpanded(false)} />
        </Show>
        <RegionBoundary region="sidebar" compact>
          <CoreSidebar
            activeRoute={activeRoute()}
            expanded={sidebarExpanded()}
            expandedWidth={expandedSidebarWidth()}
            onExpandedChange={setSidebarExpanded}
            onOpenSearch={() => setSearchOpen(true)}
          />
        </RegionBoundary>
        <main class="core-main">
          <div class="verevon-workspace-panel dashboard-main-panel core-main__panel">
            <CoreWorkspaceContext value={workspace}>
              <AssistantBrowserContext value={{ view: browserAgentState, stop: browserAgent.stop, claimRetained: browserAgent.claimRetained, setSurfaceVisible: setAgentBrowserSurfaceVisible, revealVersion: agentBrowserRevealVersion, registerNettBrowser, prepareContinuation, taskBusy: assistantTaskBusy }}>
              <RegionBoundary region="outlet" resetKey={() => location.pathname}>
                {props.children}
              </RegionBoundary>
              </AssistantBrowserContext>
            </CoreWorkspaceContext>
          </div>
        </main>
        <AssistantNavigationCursor guide={assistantGuide} />
        <Show when={assistantOpen()}>
          <RegionBoundary region="panel" compact>
            <GlobalAssistant
              forceReveal={assistantForceReveal()}
              message={assistantMessage()}
              onMessageChange={setAssistantMessage}
              onBeforeSubmit={() => false}
              canHandleBeforeSubmit={assistantGuide.canStart}
              onClose={() => { setContinueTab(null); setAssistantOpen(false) }}
              continuation={continueTab()}
              onCancelContinuation={() => setContinueTab(null)}
              state={assistantState()}
              agentView={browserAgentState()}
              browserPresentedInWorkspace={agentBrowserSurfaceVisible()}
              onStopAgent={browserAgent.stop}
              onOpenAgentBrowser={revealAgentBrowser}
              onApprovalDecision={(decision) => { void browserAgent.decide(decision) }}
              onRetryAgent={() => {
                const selection = continueTab()
                const draft = assistantMessage()
                void browserAgent.retry().then(() => {
                  if (!selection || continueTab() !== selection) return
                  setContinueTab(null)
                  if (assistantMessage() === draft && browserAgent.state().goal === draft.trim()) setAssistantMessage('')
                }).catch(() => undefined)
              }}
              onAllowSiteAccess={browserAgent.allowSiteAccess}
              onDeclineSiteAccess={browserAgent.declineSiteAccess}
              navigationAgent={navigationAgentState()}
              onStopNavigation={navigationAgent.stop}
              onStop={assistantController.stop}
              sourceScope={assistantSourceScope()}
              onSourceScopeChange={setAssistantSourceScope}
              onNewChat={() => { navigationAgent.reset(); browserAgent.reset(); assistantGuide.stop(); assistantController.reset(); setContinueTab(null); setAssistantMessage(''); setAssistantSourceScope('workspace') }}
              onOpenChat={() => { const threadId = assistantState().threadId; if (threadId) { setAssistantOpen(false); navigate(chatThreadHref(threadId)) } }}
              onSubmit={submitAssistant}
            />
          </RegionBoundary>
        </Show>
        <FeedbackWidget />
        <AnnouncerRegions />
      </div>
      </AssistantNavigationContext>
    </AgentsProvider>
  )
}

/** `null` orgId (no active org yet) or a transient org-core failure both
 * resolve to `null` — the banner must never block the shell from rendering. */
async function fetchDeletionStatus(orgId: string | null) {
  if (!orgId) return null
  try {
    return await getDeletionStatus(orgId)
  } catch {
    return null
  }
}

async function fetchNavbarData(): Promise<NavbarPayload | null> {
  try {
    return await getNavbarData()
  } catch {
    // Keep shell rendering even when optional navbar integrations are offline.
    return null
  }
}

function resolveWorkspaceIdentity(
  session: SessionState,
  navbarData: NavbarPayload | null | undefined,
): WorkspaceIdentity {
  const organization = session.activeOrg
  const profile = navbarData?.profile ?? null
  const userName = profile?.name ?? session.user?.name ?? null
  const userEmail = profile?.email ?? session.user?.email ?? null
  const personalName = resolvePersonalName(userName, organization?.name, userEmail)
  // An org-style account name (e.g. "AQUATIQ AS") is the workspace label, not a
  // person — surface it as the workspace name when no explicit org name exists.
  const orgStyleName = userName && isOrgStyleName(userName.trim()) ? userName.trim() : null
  // Use `||` (not `??`) so an empty-string org name falls through to the
  // org-style account name instead of collapsing to the "Workspace" fallback.
  const name = organization?.name || orgStyleName || personalName || fallbackWorkspaceIdentity.name
  const plan = formatPlanLabel(navbarData?.plan ?? fallbackWorkspaceIdentity.plan)

  return {
    accentColor: navbarData?.theme?.colorScheme ?? null,
    initial: firstInitial(organization?.name, orgStyleName, personalName, userEmail),
    name: cleanOrganizationName(name),
    plan,
    planTrial: navbarData?.planTrial === true,
    role: organization?.role ?? null,
    userAvatar: profile?.avatar ?? session.user?.image ?? null,
    userEmail,
    userName: personalName,
  }
}

function applyThemePreference(theme: NonNullable<NavbarPayload['theme']>['theme']) {
  const prefersDark = window.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false
  const dark = theme === 'dark' || (theme === 'system' && prefersDark)
  document.documentElement.classList.toggle('dark', dark)
}

function cleanOrganizationName(name: string): string {
  const trimmed = name.trim()
  if (!trimmed) return fallbackWorkspaceIdentity.name
  if (!/^[A-ZÆØÅ0-9 .&-]+$/.test(trimmed)) return trimmed

  return trimmed
    .toLowerCase()
    .split(' ')
    .map((word) => {
      if (['as', 'asa', 'ab', 'sa', 'ba', 'llc', 'inc'].includes(word)) return word.toUpperCase()
      return word.charAt(0).toUpperCase() + word.slice(1)
    })
    .join(' ')
}

function firstInitial(...values: Array<string | null | undefined>): string {
  const first = values.find((value) => value?.trim())
  return (first?.trim().charAt(0) || fallbackWorkspaceIdentity.initial).toUpperCase()
}

function normalizeIdentityName(value: string | null | undefined): string {
  return (value ?? '')
    .trim()
    .toLowerCase()
    .replace(/\b(as|asa|ab|sa|ba|llc|inc|ltd)\b/g, '')
    .replace(/[^a-z0-9æøå]+/g, '')
}

function sameIdentityName(first: string | null | undefined, second: string | null | undefined): boolean {
  const normalizedFirst = normalizeIdentityName(first)
  const normalizedSecond = normalizeIdentityName(second)
  return normalizedFirst.length > 0 && normalizedFirst === normalizedSecond
}

function displayNameFromEmail(email: string | null | undefined): string | null {
  const local = email?.split('@')[0]?.trim()
  if (!local) return null
  return local
    .split(/[._-]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1).toLowerCase())
    .join(' ')
}

function resolvePersonalName(
  userName: string | null | undefined,
  organizationName: string | null | undefined,
  userEmail: string | null | undefined,
): string | null {
  const trimmed = userName?.trim()
  if (trimmed && !sameIdentityName(trimmed, organizationName) && !isOrgStyleName(trimmed)) return trimmed
  return displayNameFromEmail(userEmail)
}

// All-uppercase single word (no lowercase letters) = org identifier pattern, not a personal name.
// Matches "AQUATIQ", "VEREVON", "MICROSOFT" but not "Ima", "John Smith", "João".
function isOrgStyleName(name: string): boolean {
  return /^[A-ZÆØÅ0-9 ]{2,40}$/.test(name) && !/[a-zæøå]/.test(name)
}

/** Below this width the expanded sidebar is a drawer laid over the page, not a column beside it. */
const PHONE_MEDIA_QUERY = '(max-width: 767px)'

function isPhoneViewport(): boolean {
  try {
    return typeof window !== 'undefined' && window.matchMedia?.(PHONE_MEDIA_QUERY).matches === true
  } catch {
    return false
  }
}

/** An open room (`/spaces/:id`), as opposed to the rooms index at `/spaces`. */
function isOpenRoomPath(pathname: string): boolean {
  return /^\/spaces\/[^/]+/.test(pathname)
}

/**
 * Whether the sidebar starts expanded on a first visit. On a phone the expanded sidebar covers the
 * whole page, so a room opened by link or reload starts with the drawer closed: the person came for
 * the room, and the menu button is one tap away. The rooms index keeps its list open.
 */
export function defaultSidebarExpandedForRoute(
  route: ReturnType<typeof routeFromPath>,
  options: { readonly pathname?: string; readonly phone?: boolean } = {},
) {
  if (options.phone === true && isOpenRoomPath(options.pathname ?? '')) return false
  return route === '/spaces' ||
    route === '/support' ||
    route === '/social' ||
    route.startsWith('/studio') ||
    route.startsWith('/social') ||
    route.startsWith('/insights') ||
    route === '/social/calendar' ||
    route === '/agents' ||
    route === '/ingestions' ||
    route === '/knowledge' ||
    route === '/account' ||
    route === '/settings' ||
    route.startsWith('/settings/')
}
