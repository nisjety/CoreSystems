import { Show, createEffect, lazy } from 'solid-js'
import { createRouter, useNavigate } from '@solidjs/router'
import type { JSX } from '@solidjs/web'
import { AppShell } from '@/app/shell/AppShell'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'
import { getSession } from '@/shared/session/session-store'

const DashboardPage = lazy(() => import('@/features/dashboard/components/DashboardPage'))
const ChatPage = lazy(() => import('@/features/chat/components/ChatPage'))
const StudioPage = lazy(() => import('@/features/studio/components/StudioPage'))
const InboxPage = lazy(() => import('@/features/inbox/components/InboxPage'))
const TicketingPage = lazy(() => import('@/features/tickets/components/TicketingPage'))
const SupportPage = lazy(() => import('@/features/support/components/SupportPage'))
const AgentsPage = lazy(() => import('@/features/agents/components/AgentsPage'))
const AgentRunConsole = lazy(() => import('@/features/agents/components/AgentRunConsole'))
const FleetRunConsole = lazy(() => import('@/features/agents/components/FleetRunConsole'))
const AgentInstallationsPage = lazy(() => import('@/features/agents/components/AgentInstallationsPage'))
const ChiefCoreRoutingPage = lazy(() => import('@/features/agents/components/ChiefCoreRoutingPage'))
const CostDashboardPage = lazy(() => import('@/features/cost/components/CostDashboardPage'))
const OpsQualityPage = lazy(() => import('@/features/quality/components/OpsQualityPage'))
const SocialCalendarPage = lazy(() => import('@/features/social/components/SocialCalendarPage'))
const SocialOperationsPage = lazy(() => import('@/features/social/components/SocialOperationsPage'))
const SocialCommercePage = lazy(() => import('@/features/social/components/SocialCommercePage'))
const InsightsPage = lazy(() => import('@/features/insights/components/InsightsPage'))
const KnowledgePage = lazy(() => import('@/features/knowledge/components/KnowledgePage'))
const SharedWithMePage = lazy(() => import('@/features/knowledge/components/SharedWithMePage'))
const SpacePage = lazy(() => import('@/features/spaces/components/SpacePage'))
const SpacesIndexPage = lazy(() => import('@/features/spaces/components/SpacesIndexPage'))
const LeadsPage = lazy(() => import('@/features/leads/components/LeadsPage'))
const VerevonIngestionsPage = lazy(() => import('@/features/ingestions/components/VerevonIngestionsPage'))
const AuthPage = lazy(() => import('@/features/auth/components/AuthPage'))
const AcceptInvitationPage = lazy(() => import('@/features/auth/components/AcceptInvitationPage'))
const OnboardingPage = lazy(() => import('@/features/onboarding/components/OnboardingPage'))
const AccountSettingsPage = lazy(() => import('@/features/settings/components/AccountSettingsPage'))
const SettingsPage = lazy(() => import('@/features/settings/components/SettingsPage'))
const NotFoundPage = lazy(() => import('@/app/routes/NotFoundPage'))

function SessionLoading() {
  return (
    <div class="session-loading" role="status" aria-live="polite" aria-busy="true">
      <span class="session-loading__spinner" aria-hidden="true" />
      <span class="session-loading__label">Loading…</span>
    </div>
  )
}

/// Gate for authenticated app routes: unauthenticated users go to /login, and
/// authenticated users who have not finished onboarding are sent to /onboarding.
/// The session is loaded once at the shell root; this only reads + reacts to it.
function RequireAuth(props: { children: JSX.Element }) {
  const navigate = useNavigate()
  const session = getSession()

  createEffectRedirect(
    () => ({ status: session.status, onboardingStatus: session.onboardingStatus }),
    (curr) => {
      if (curr.status === 'unauthenticated') {
        navigate('/login', { replace: true })
        return
      }
      if (curr.status === 'authenticated' && curr.onboardingStatus !== 'COMPLETED') {
        navigate('/onboarding', { replace: true })
      }
    },
  )

  return (
    <Show
      when={session.status === 'authenticated' && session.onboardingStatus === 'COMPLETED'}
      fallback={<SessionLoading />}
    >
      {props.children}
    </Show>
  )
}

/// Gate for the onboarding route: unauthenticated users go to /login, and users
/// who have already completed onboarding are sent to /dashboard.
function RequireOnboarding(props: { children: JSX.Element }) {
  const navigate = useNavigate()
  const session = getSession()

  createEffectRedirect(
    () => ({ status: session.status, onboardingStatus: session.onboardingStatus }),
    (curr) => {
      if (curr.status === 'unauthenticated') {
        navigate('/login', { replace: true })
        return
      }
      if (curr.status === 'authenticated' && curr.onboardingStatus === 'COMPLETED') {
        navigate('/dashboard', { replace: true })
      }
    },
  )

  return (
    <Show
      when={session.status === 'authenticated' && session.onboardingStatus !== 'COMPLETED'}
      fallback={<SessionLoading />}
    >
      {props.children}
    </Show>
  )
}

function RequireWorkspaceAdmin(props: { children: JSX.Element }) {
  const navigate = useNavigate()
  const session = getSession()

  createEffectRedirect(
    () => ({ status: session.status, onboardingStatus: session.onboardingStatus, session }),
    (curr) => {
      if (curr.status === 'unauthenticated') {
        navigate('/login', { replace: true })
        return
      }
      if (curr.status === 'authenticated' && curr.onboardingStatus !== 'COMPLETED') {
        navigate('/onboarding', { replace: true })
        return
      }
      if (curr.status === 'authenticated' && !hasWorkspaceAdminAccess(curr.session)) {
        navigate('/dashboard', { replace: true })
      }
    },
  )

  return (
    <Show
      when={
        session.status === 'authenticated' &&
        session.onboardingStatus === 'COMPLETED' &&
        hasWorkspaceAdminAccess(session)
      }
      fallback={<SessionLoading />}
    >
      {props.children}
    </Show>
  )
}

/// Solid 2's createEffect requires two separate functions: `compute` (tracked
/// reads only) and `effect` (untracked imperative work, e.g. navigate()).
/// Mixing a tracked read with an imperative write in one callback — the
/// Solid 1 pattern this file used — is a compile error in v2
/// (MISSING_EFFECT_FN), since the single-argument overload types to `never`.
function createEffectRedirect<T>(compute: () => T, effect: (value: T) => void): void {
  createEffect(compute, effect)
}

export const Router = createRouter({
  routes: [
    { path: '/auth', component: AuthPage },
    { path: '/login', component: AuthPage },
    { path: '/reset-password', component: AuthPage },
    { path: '/accept-invitation/:invitationId', component: AcceptInvitationPage },
    {
      path: '/onboarding',
      component: () => (
        <RequireOnboarding>
          <OnboardingPage />
        </RequireOnboarding>
      ),
    },
    {
      path: '/',
      component: (props: { children?: JSX.Element }) => <RequireAuth>{props.children}</RequireAuth>,
      children: [
        { path: '/', component: DashboardPage },
        { path: '/dashboard', component: DashboardPage },
        { path: '/chat', component: ChatPage },
        { path: '/spaces', component: SpacesIndexPage },
        { path: '/spaces/:spaceId', component: SpacePage },
        { path: '/studio', component: () => <StudioPage section="canvas" /> },
        { path: '/studio/canvas', component: () => <StudioPage section="canvas" /> },
        { path: '/studio/campaigns', component: () => <StudioPage section="campaigns" /> },
        { path: '/studio/templates', component: () => <StudioPage section="templates" /> },
        { path: '/inbox', component: InboxPage },
        { path: '/tickets', component: TicketingPage },
        { path: '/support', component: SupportPage },
        { path: '/social', component: SocialCalendarPage },
        { path: '/social/accounts', component: () => <SocialOperationsPage section="accounts" /> },
        { path: '/social/calendar', component: SocialCalendarPage },
        { path: '/social/drafts', component: () => <SocialOperationsPage section="drafts" /> },
        { path: '/social/approvals', component: () => <SocialOperationsPage section="approvals" /> },
        { path: '/social/campaigns', component: () => <SocialOperationsPage section="campaigns" /> },
        { path: '/social/competitors', component: () => <SocialOperationsPage section="competitors" /> },
        { path: '/social/trends', component: () => <SocialOperationsPage section="trends" /> },
        { path: '/social/evergreen', component: () => <SocialOperationsPage section="evergreen" /> },
        { path: '/social/commerce', component: SocialCommercePage },
        { path: '/insights', component: () => <InsightsPage section="overview" /> },
        { path: '/insights/overview', component: () => <InsightsPage section="overview" /> },
        { path: '/insights/social', component: () => <InsightsPage section="social" /> },
        { path: '/insights/inbox', component: () => <InsightsPage section="inbox" /> },
        { path: '/insights/agents', component: () => <InsightsPage section="agents" /> },
        { path: '/insights/chat', component: () => <InsightsPage section="chat" /> },
        { path: '/insights/knowledge', component: () => <InsightsPage section="knowledge" /> },
        { path: '/insights/ingestion', component: () => <InsightsPage section="ingestion" /> },
        { path: '/insights/campaigns', component: () => <InsightsPage section="campaigns" /> },
        { path: '/insights/external', component: () => <InsightsPage section="external_analytics" /> },
        { path: '/insights/external_analytics', component: () => <InsightsPage section="external_analytics" /> },
        { path: '/insights/experiments', component: () => <InsightsPage section="experiments" /> },
        { path: '/agents', component: AgentsPage },
        { path: '/agents/installations', component: AgentInstallationsPage },
        { path: '/agents/chief-core', component: ChiefCoreRoutingPage },
        { path: '/agents/runs', component: AgentRunConsole },
        { path: '/agents/fleets/:fleetId', component: () => <FleetRunConsole fleetId={window.location.pathname.split('/').pop()!} /> },
        { path: '/agents/cost', component: CostDashboardPage },
        { path: '/agents/quality', component: OpsQualityPage },
        { path: '/ingestions', component: VerevonIngestionsPage },
        { path: '/knowledge', component: KnowledgePage },
        { path: '/knowledge/shared', component: SharedWithMePage },
        { path: '/leads', component: LeadsPage },
        { path: '/account', component: AccountSettingsPage },
        {
          path: '/settings',
          component: () => (
            <RequireWorkspaceAdmin>
              <SettingsPage />
            </RequireWorkspaceAdmin>
          ),
        },
        {
          path: '/settings/:section',
          component: () => (
            <RequireWorkspaceAdmin>
              <SettingsPage />
            </RequireWorkspaceAdmin>
          ),
        },
      ],
    },
    { path: '*404', component: NotFoundPage },
  ],
  explicitLinks: true,
})

export default function App() {
  return <Router>{(props) => <AppShell {...props} />}</Router>
}
