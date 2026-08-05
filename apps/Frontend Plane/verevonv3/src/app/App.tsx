import { Route, Router, useNavigate } from '@solidjs/router'
import { createEffect, lazy, Show, type JSX } from 'solid-js'
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
const CostDashboardPage = lazy(() => import('@/features/cost/components/CostDashboardPage'))
const OpsQualityPage = lazy(() => import('@/features/quality/components/OpsQualityPage'))
const SocialCalendarPage = lazy(() => import('@/features/social/components/SocialCalendarPage'))
const SocialOperationsPage = lazy(() => import('@/features/social/components/SocialOperationsPage'))
const SocialCommercePage = lazy(() => import('@/features/social/components/SocialCommercePage'))
const InsightsPage = lazy(() => import('@/features/insights/components/InsightsPage'))
const KnowledgePage = lazy(() => import('@/features/knowledge/components/KnowledgePage'))
const SharedWithMePage = lazy(() => import('@/features/knowledge/components/SharedWithMePage'))
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

  createEffect(() => {
    if (session.status === 'unauthenticated') {
      navigate('/login', { replace: true })
      return
    }
    if (session.status === 'authenticated' && session.onboardingStatus !== 'COMPLETED') {
      navigate('/onboarding', { replace: true })
    }
  })

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

  createEffect(() => {
    if (session.status === 'unauthenticated') {
      navigate('/login', { replace: true })
      return
    }
    if (session.status === 'authenticated' && session.onboardingStatus === 'COMPLETED') {
      navigate('/dashboard', { replace: true })
    }
  })

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

  createEffect(() => {
    if (session.status === 'unauthenticated') {
      navigate('/login', { replace: true })
      return
    }
    if (session.status === 'authenticated' && session.onboardingStatus !== 'COMPLETED') {
      navigate('/onboarding', { replace: true })
      return
    }
    if (session.status === 'authenticated' && !hasWorkspaceAdminAccess(session)) {
      navigate('/dashboard', { replace: true })
    }
  })

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

export default function App() {
  return (
    <Router root={AppShell}>
      <Route path="/auth" component={AuthPage} />
      <Route path="/login" component={AuthPage} />
      <Route path="/reset-password" component={AuthPage} />
      <Route path="/accept-invitation/:invitationId" component={AcceptInvitationPage} />
      <Route
        path="/onboarding"
        component={() => (
          <RequireOnboarding>
            <OnboardingPage />
          </RequireOnboarding>
        )}
      />
      <Route
        path="/"
        component={(props) => <RequireAuth>{props.children}</RequireAuth>}
      >
        <Route path="/" component={DashboardPage} />
        <Route path="/dashboard" component={DashboardPage} />
        <Route path="/chat" component={ChatPage} />
        <Route path="/studio" component={() => <StudioPage section="canvas" />} />
        <Route path="/studio/canvas" component={() => <StudioPage section="canvas" />} />
        <Route path="/studio/campaigns" component={() => <StudioPage section="campaigns" />} />
        <Route path="/studio/templates" component={() => <StudioPage section="templates" />} />
        <Route path="/inbox" component={InboxPage} />
        <Route path="/tickets" component={TicketingPage} />
        <Route path="/support" component={SupportPage} />
        <Route path="/social" component={SocialCalendarPage} />
        <Route path="/social/accounts" component={() => <SocialOperationsPage section="accounts" />} />
        <Route path="/social/calendar" component={SocialCalendarPage} />
        <Route path="/social/drafts" component={() => <SocialOperationsPage section="drafts" />} />
        <Route path="/social/approvals" component={() => <SocialOperationsPage section="approvals" />} />
        <Route path="/social/campaigns" component={() => <SocialOperationsPage section="campaigns" />} />
        <Route path="/social/competitors" component={() => <SocialOperationsPage section="competitors" />} />
        <Route path="/social/trends" component={() => <SocialOperationsPage section="trends" />} />
        <Route path="/social/evergreen" component={() => <SocialOperationsPage section="evergreen" />} />
        <Route path="/social/commerce" component={SocialCommercePage} />
        <Route path="/insights" component={() => <InsightsPage section="overview" />} />
        <Route path="/insights/overview" component={() => <InsightsPage section="overview" />} />
        <Route path="/insights/social" component={() => <InsightsPage section="social" />} />
        <Route path="/insights/inbox" component={() => <InsightsPage section="inbox" />} />
        <Route path="/insights/agents" component={() => <InsightsPage section="agents" />} />
        <Route path="/insights/campaigns" component={() => <InsightsPage section="campaigns" />} />
        <Route path="/insights/experiments" component={() => <InsightsPage section="experiments" />} />
        <Route path="/agents" component={AgentsPage} />
        <Route path="/agents/runs" component={AgentRunConsole} />
        <Route path="/agents/cost" component={CostDashboardPage} />
        <Route path="/agents/quality" component={OpsQualityPage} />
        <Route path="/ingestions" component={VerevonIngestionsPage} />
        <Route path="/knowledge" component={KnowledgePage} />
        <Route path="/knowledge/shared" component={SharedWithMePage} />
        <Route path="/leads" component={LeadsPage} />
        <Route path="/account" component={AccountSettingsPage} />
        <Route path="/settings" component={() => <RequireWorkspaceAdmin><SettingsPage /></RequireWorkspaceAdmin>} />
        <Route path="/settings/:section" component={() => <RequireWorkspaceAdmin><SettingsPage /></RequireWorkspaceAdmin>} />
      </Route>
      <Route path="*404" component={NotFoundPage} />
    </Router>
  )
}
