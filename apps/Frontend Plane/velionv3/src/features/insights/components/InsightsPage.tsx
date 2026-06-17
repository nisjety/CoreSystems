import { A } from '@solidjs/router'
import {
  AlertCircle,
  BarChart3,
  Bot,
  CheckCircle2,
  DatabaseZap,
  FlaskConical,
  Gauge,
  Inbox,
  LineChart,
  Megaphone,
  MousePointerClick,
  PlugZap,
  Search,
  TrendingUp,
  type LucideProps,
} from 'lucide-solid'
import { createMemo, createResource, For, Show, type Component } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import {
  loadInsightsWorkspace,
  type ExternalAnalyticsSlot,
  type InsightsSection,
  type InsightsWorkspace,
  type MeasurementState,
} from '@/features/insights/lib/insights-workspace'
import type { LiveTicket } from '@/shared/api/inbox-client'
import type { SocialCalendar } from '@/shared/api/social-client'
import { cn } from '@/shared/lib/cn'

type ConnectorPendingState = 'connected_pending_reports'
type SurfaceState = MeasurementState | ConnectorPendingState

type SectionConfig = {
  description: string
  icon: Component<LucideProps>
  id: InsightsSection
  label: string
  title: string
}

type InsightMetric = {
  detail: string
  icon: Component<LucideProps>
  label: string
  state: SurfaceState
  value: string
}

type InsightSignal = {
  detail: string
  href?: string
  meta: string
  state: SurfaceState
  title: string
}

type SectionView = {
  metrics: InsightMetric[]
  signals: InsightSignal[]
  sourceDetail: string
  sourceState: SurfaceState
  title: string
}

const sectionConfigs: SectionConfig[] = [
  {
    id: 'overview',
    label: 'Overview',
    title: 'Overview',
    description: 'One operational measurement surface across content, conversations, agents, campaigns, tests, and external analytics.',
    icon: BarChart3,
  },
  {
    id: 'social',
    label: 'Social',
    title: 'Social measurement',
    description: 'Channel readiness, calendar health, publishing constraints, approvals, and content outcomes.',
    icon: TrendingUp,
  },
  {
    id: 'inbox',
    label: 'Inbox',
    title: 'Inbox measurement',
    description: 'Conversation volume, queue health, social handoffs, support topics, and response instrumentation.',
    icon: Inbox,
  },
  {
    id: 'agents',
    label: 'Agents',
    title: 'Agent measurement',
    description: 'Runtime readiness, support integration status, macros, groups, approval gaps, and future run analytics.',
    icon: Bot,
  },
  {
    id: 'campaigns',
    label: 'Campaigns',
    title: 'Campaign measurement',
    description: 'Campaign-linked posts, channel coverage, content ownership, launch readiness, and attribution gaps.',
    icon: Megaphone,
  },
  {
    id: 'experiments',
    label: 'Experiments',
    title: 'Experiment measurement',
    description: 'Hypotheses, variants, winning criteria, audience splits, and learning loops once the experiment backend lands.',
    icon: FlaskConical,
  },
]

export type { InsightsSection }

export default function InsightsPage(props: { section?: InsightsSection }) {
  const [workspace] = createResource(loadInsightsWorkspace)
  const activeSection = createMemo(() => props.section ?? 'overview')
  const config = createMemo(() =>
    sectionConfigs.find((section) => section.id === activeSection()) ?? sectionConfigs[0]!,
  )
  const sectionView = createMemo(() => buildSectionView(activeSection(), workspace()))

  return (
    <div class="velion-insights-page">
      <section class="velion-insights-hero">
        <div>
          <span>
            <Dynamic component={config().icon} size={15} />
            {config().title}
          </span>
          <h1>Velion measurement layer</h1>
          <p>{config().description}</p>
        </div>
        <A href="/settings/integrations">Manage connectors</A>
      </section>

      <nav class="velion-insights-tabs" aria-label="Insights sections">
        <For each={sectionConfigs}>
          {(section) => (
            <A
              class={cn('velion-insights-tab', section.id === activeSection() && 'is-active')}
              href={`/insights/${section.id}`}
            >
              <Dynamic component={section.icon} size={14} />
              {section.label}
            </A>
          )}
        </For>
      </nav>

      <Show when={workspace()} fallback={<LoadingState />}>
        {(loadedWorkspace) => (
          <>
            <p class={cn('velion-insights-source', `velion-insights-source--${stateClass(sectionView().sourceState)}`)}>
              <DatabaseZap size={16} />
              <strong>{stateLabel(sectionView().sourceState)}</strong>
              {sectionView().sourceDetail}
            </p>

            <section class="velion-insights-metrics" aria-label={`${sectionView().title} metrics`}>
              <For each={sectionView().metrics}>
                {(metric) => <MetricCard metric={metric} />}
              </For>
            </section>

            <section class="velion-insights-signal-grid" aria-label={`${sectionView().title} signals`}>
              <For each={sectionView().signals}>
                {(signal) => <SignalCard signal={signal} />}
              </For>
            </section>

            <ExternalAnalyticsSection slots={loadedWorkspace().externalAnalytics} />
          </>
        )}
      </Show>
    </div>
  )
}

function LoadingState() {
  return (
    <section class="velion-insights-loading" aria-label="Loading Insights">
      <Gauge size={18} />
      <div>
        <h2>Resolving measurement contracts</h2>
        <p>Checking org scope, Social, Inbox, Agents, and integration connectors.</p>
      </div>
    </section>
  )
}

function MetricCard(props: { metric: InsightMetric }) {
  return (
    <article class={cn('velion-insights-metric', `velion-insights-card--${stateClass(props.metric.state)}`)}>
      <div class="velion-insights-metric__topline">
        <span>
          <Dynamic component={props.metric.icon} size={15} />
          {props.metric.label}
        </span>
        <small>{stateLabel(props.metric.state)}</small>
      </div>
      <strong>{props.metric.value}</strong>
      <p>{props.metric.detail}</p>
    </article>
  )
}

function SignalCard(props: { signal: InsightSignal }) {
  return (
    <article class={cn('velion-insights-card', `velion-insights-card--${stateClass(props.signal.state)}`)}>
      <div class="velion-insights-card__meta">
        <span>{props.signal.meta}</span>
        <small>{stateLabel(props.signal.state)}</small>
      </div>
      <h2>{props.signal.title}</h2>
      <p>{props.signal.detail}</p>
      <Show when={props.signal.href}>
        {(href) => <A href={href()}>Open source</A>}
      </Show>
    </article>
  )
}

function ExternalAnalyticsSection(props: { slots: ExternalAnalyticsSlot[] }) {
  return (
    <section class="velion-insights-external" aria-label="External analytics connectors">
      <div class="velion-insights-section-heading">
        <div>
          <span>
            <PlugZap size={14} />
            External analytics
          </span>
          <h2>Google Analytics and SEO slots</h2>
        </div>
        <p>Connector state is shown separately from report availability so the UI never presents placeholder analytics as live rows.</p>
      </div>

      <div class="velion-insights-external-grid">
        <For each={props.slots}>
          {(slot) => <ExternalSlotCard slot={slot} />}
        </For>
      </div>
    </section>
  )
}

function ExternalSlotCard(props: { slot: ExternalAnalyticsSlot }) {
  const Icon = () => props.slot.connectorId === 'ga4' ? LineChart : Search
  const conceptSentence = () => props.slot.connectorId === 'ga4'
    ? 'Maps to GA4 Data API report concepts: dimensions, metrics, date ranges, and rows.'
    : 'Maps to Search Console concepts: queries, pages, clicks, impressions, CTR, and position.'

  return (
    <article class={cn('velion-insights-external-card', `velion-insights-card--${stateClass(props.slot.status)}`)}>
      <header>
        <span class="velion-insights-external-card__icon">
          <Dynamic component={Icon()} size={17} />
        </span>
        <div>
          <h3>{props.slot.title}</h3>
          <small>{props.slot.statusLabel}</small>
        </div>
      </header>
      <p>{props.slot.detail}</p>
      <p>{conceptSentence()}</p>

      <div class="velion-insights-report-shape">
        <ReportConcepts title="Request" items={props.slot.reportConcepts} />
        <ReportConcepts title="Response" items={props.slot.responseConcepts} />
        <ReportConcepts title="Metrics" items={props.slot.metricConcepts} />
      </div>

      <a href={props.slot.docsHref} target="_blank" rel="noreferrer">Open official API reference</a>
    </article>
  )
}

function ReportConcepts(props: { items: string[]; title: string }) {
  return (
    <div>
      <h4>{props.title}</h4>
      <ul>
        <For each={props.items}>
          {(item) => <li>{item}</li>}
        </For>
      </ul>
    </div>
  )
}

function buildSectionView(section: InsightsSection, workspace: InsightsWorkspace | undefined): SectionView {
  if (!workspace) return loadingView(section)

  switch (section) {
    case 'social':
      return socialView(workspace)
    case 'inbox':
      return inboxView(workspace)
    case 'agents':
      return agentsView(workspace)
    case 'campaigns':
      return campaignsView(workspace)
    case 'experiments':
      return experimentsView()
    case 'overview':
    default:
      return overviewView(workspace)
  }
}

function overviewView(workspace: InsightsWorkspace): SectionView {
  const liveContracts = [
    workspace.insightCore.state,
    workspace.social.state,
    workspace.inbox.state,
    workspace.agents.state,
    workspace.integrations.state,
  ].filter((state) => state === 'live').length
  const connectedExternal = workspace.externalAnalytics.filter((slot) => slot.status === 'connected_pending_reports').length
  const plannedSurfaces =
    2 +
    (workspace.insightCore.state === 'live' ? 0 : 1) +
    workspace.externalAnalytics.filter((slot) => slot.status !== 'connected_pending_reports').length

  return {
    title: 'Overview',
    sourceState: liveContracts > 0 ? 'live' : 'unavailable',
    sourceDetail: liveContracts > 0
      ? `${liveContracts} org-scoped contracts responded. External analytics remains separated from live report rows.`
      : 'No org-scoped measurement contracts responded yet.',
    metrics: [
      {
        label: 'Live contracts',
        value: `${liveContracts} live`,
        detail: 'Social, Inbox, Agents, and Integrations are checked independently.',
        icon: DatabaseZap,
        state: liveContracts > 0 ? 'live' : 'unavailable',
      },
      {
        label: 'Insight core',
        value: workspace.insightCore.state === 'live' ? 'Live' : 'Proxy pending',
        detail: workspace.insightCore.message,
        icon: DatabaseZap,
        state: workspace.insightCore.state === 'live' ? 'live' : 'planned',
      },
      {
        label: 'External analytics',
        value: `${connectedExternal} connected`,
        detail: 'GA4 and Search Console connector state, not synthetic report rows.',
        icon: PlugZap,
        state: connectedExternal > 0 ? 'connected_pending_reports' : 'not_connected',
      },
      {
        label: 'Inbox measurement',
        value: `${workspace.inbox.data.length} conversations`,
        detail: workspace.inbox.message,
        icon: Inbox,
        state: workspace.inbox.state,
      },
      {
        label: 'Backend gaps',
        value: `${plannedSurfaces} planned`,
        detail: 'Campaign, experiment, insight-core gateway proxy, and external report table wiring are tracked separately.',
        icon: AlertCircle,
        state: 'planned',
      },
    ],
    signals: [
      {
        title: 'Social and Inbox can already feed measurement',
        detail: `${workspace.social.message} ${workspace.inbox.message}`,
        meta: 'Operational contracts',
        state: strongestState(workspace.social.state, workspace.inbox.state),
        href: '/insights/social',
      },
      {
        title: 'Agent runtime is separate from run analytics',
        detail: workspace.agents.message,
        meta: 'Agent surface',
        state: workspace.agents.state,
        href: '/insights/agents',
      },
      {
        title: workspace.insightCore.state === 'live' ? 'Insight core proxy is live' : 'Insight core proxy is pending',
        detail: workspace.insightCore.message,
        meta: 'Application Plane insight-core',
        state: workspace.insightCore.state === 'live' ? 'live' : 'planned',
      },
      {
        title: 'External analytics slots are mapped, not faked',
        detail: 'GA4 and Search Console cards expose the report concepts Velion must proxy before showing rows.',
        meta: 'Connector slots',
        state: connectedExternal > 0 ? 'connected_pending_reports' : 'not_connected',
      },
    ],
  }
}

function socialView(workspace: InsightsWorkspace): SectionView {
  const calendar = workspace.social.data.calendar
  const connectedAccounts = calendar.accounts.filter((account) => account.status === 'connected')
  const scheduledPosts = calendar.posts.filter((post) => post.status === 'scheduled')
  const analyticsReady = connectedAccounts.filter((account) => account.capabilities.includes('social.analytics.read'))
  const waitingApprovals = calendar.posts.filter((post) => post.approval.required && post.approval.state !== 'approved')

  return {
    title: 'Social measurement',
    sourceState: workspace.social.state,
    sourceDetail: workspace.social.message,
    metrics: [
      {
        label: 'Connected accounts',
        value: String(connectedAccounts.length),
        detail: `${calendar.accounts.length} total account records from Social.`,
        icon: CheckCircle2,
        state: stateForCount(workspace.social.state, connectedAccounts.length),
      },
      {
        label: 'Scheduled posts',
        value: String(scheduledPosts.length),
        detail: 'Calendar-backed posts with scheduled publish windows.',
        icon: TrendingUp,
        state: stateForCount(workspace.social.state, scheduledPosts.length),
      },
      {
        label: 'Analytics capable',
        value: String(analyticsReady.length),
        detail: 'Accounts declaring social.analytics.read capability.',
        icon: LineChart,
        state: stateForCount(workspace.social.state, analyticsReady.length),
      },
      {
        label: 'Approval queue',
        value: String(waitingApprovals.length),
        detail: 'Posts still requiring approval before measurement can close the loop.',
        icon: AlertCircle,
        state: stateForCount(workspace.social.state, waitingApprovals.length),
      },
    ],
    signals: socialSignals(calendar, workspace.social.state),
  }
}

function inboxView(workspace: InsightsWorkspace): SectionView {
  const tickets = workspace.inbox.data
  const open = tickets.filter((ticket) => ticketStateName(ticket) === 'open')
  const social = tickets.filter((ticket) => ticket.channel === 'social' || ticket.tags?.some((tag) => tag.includes('social')))
  const routed = tickets.filter((ticket) => Boolean(ticket.owner))

  return {
    title: 'Inbox measurement',
    sourceState: workspace.inbox.state,
    sourceDetail: workspace.inbox.message,
    metrics: [
      {
        label: 'Conversations',
        value: String(tickets.length),
        detail: 'Conversation summaries returned from the inbox gateway.',
        icon: Inbox,
        state: stateForCount(workspace.inbox.state, tickets.length),
      },
      {
        label: 'Open queue',
        value: String(open.length),
        detail: 'Open tickets that still affect response and routing measurements.',
        icon: AlertCircle,
        state: stateForCount(workspace.inbox.state, open.length),
      },
      {
        label: 'Social handoffs',
        value: String(social.length),
        detail: 'Conversations tagged or routed from social channels.',
        icon: MousePointerClick,
        state: stateForCount(workspace.inbox.state, social.length),
      },
      {
        label: 'Assigned',
        value: String(routed.length),
        detail: 'Conversations with an owner available for routing quality measurement.',
        icon: CheckCircle2,
        state: stateForCount(workspace.inbox.state, routed.length),
      },
    ],
    signals: inboxSignals(tickets, workspace.inbox.state),
  }
}

function agentsView(workspace: InsightsWorkspace): SectionView {
  const status = workspace.agents.data
  const connected = status?.status === 'connected'

  return {
    title: 'Agent measurement',
    sourceState: workspace.agents.state,
    sourceDetail: workspace.agents.message,
    metrics: [
      {
        label: 'Runtime status',
        value: connected ? 'Connected' : 'Unavailable',
        detail: status?.message ?? workspace.agents.message,
        icon: Bot,
        state: workspace.agents.state,
      },
      {
        label: 'Support groups',
        value: String(status?.groups ?? 0),
        detail: 'Groups visible to the chatbot support runtime.',
        icon: Inbox,
        state: stateForCount(workspace.agents.state, status?.groups ?? 0),
      },
      {
        label: 'Macros',
        value: String(status?.macros ?? 0),
        detail: 'Approved support macros available to guarded agent actions.',
        icon: CheckCircle2,
        state: stateForCount(workspace.agents.state, status?.macros ?? 0),
      },
      {
        label: 'Run analytics',
        value: 'Pending',
        detail: 'Workflow run, cost, approval, and policy metrics need a dedicated backend contract.',
        icon: AlertCircle,
        state: 'planned',
      },
    ],
    signals: [
      {
        title: connected ? 'Chatbot support runtime is connected' : 'Agent runtime is not measurable yet',
        detail: status?.message ?? workspace.agents.message,
        meta: 'Runtime',
        state: workspace.agents.state,
        href: '/agents?agent=chatbot&view=analytics',
      },
      {
        title: 'Workflow analytics backend dependency',
        detail: 'Insights needs run summaries, failure reasons, approval decisions, tool costs, and policy block counts.',
        meta: 'Backend gap',
        state: 'planned',
        href: '/agents?agent=workflow',
      },
      {
        title: 'Support action quality can be linked later',
        detail: 'Once run events are exposed, Velion can join agent outcomes with inbox resolution and campaign content loops.',
        meta: 'Cross-surface',
        state: 'planned',
      },
    ],
  }
}

function campaignsView(workspace: InsightsWorkspace): SectionView {
  const campaignPosts = workspace.social.data.calendar.posts.filter((post) => post.source.kind === 'campaign')
  const scheduled = campaignPosts.filter((post) => post.status === 'scheduled')
  const platformCount = new Set(campaignPosts.flatMap((post) => post.platforms)).size

  return {
    title: 'Campaign measurement',
    sourceState: campaignPosts.length ? 'live' : 'planned',
    sourceDetail: campaignPosts.length
      ? 'Campaign-linked social posts are available, but campaign attribution still needs a backend aggregate.'
      : 'No campaign measurement contract exists yet; only campaign-linked social posts can be derived.',
    metrics: [
      {
        label: 'Campaign posts',
        value: String(campaignPosts.length),
        detail: 'Posts whose Social source is campaign-linked.',
        icon: Megaphone,
        state: stateForCount(workspace.social.state, campaignPosts.length),
      },
      {
        label: 'Scheduled assets',
        value: String(scheduled.length),
        detail: 'Campaign posts that have a scheduled publish state.',
        icon: TrendingUp,
        state: stateForCount(workspace.social.state, scheduled.length),
      },
      {
        label: 'Channel coverage',
        value: String(platformCount),
        detail: 'Unique social platforms covered by campaign-linked posts.',
        icon: BarChart3,
        state: stateForCount(workspace.social.state, platformCount),
      },
      {
        label: 'Attribution',
        value: 'Pending',
        detail: 'Revenue, GA4 conversion, and SEO lift attribution require backend joins.',
        icon: AlertCircle,
        state: 'planned',
      },
    ],
    signals: campaignPosts.length
      ? campaignPosts.slice(0, 3).map((post) => ({
        title: post.title,
        detail: `${post.source.label} is ${post.status} for ${post.platforms.join(', ')}.`,
        meta: 'Social campaign source',
        state: 'live' as SurfaceState,
        href: post.source.href ?? '/social/campaigns',
      }))
      : plannedSignals('Campaign backend', 'Campaign health, attribution, asset ownership, and launch readiness need a live campaign aggregate.'),
  }
}

function experimentsView(): SectionView {
  return {
    title: 'Experiment measurement',
    sourceState: 'planned',
    sourceDetail: 'No live experiment contract is exposed to Velion v3 yet.',
    metrics: [
      {
        label: 'Running tests',
        value: 'Pending',
        detail: 'Experiment records need hypothesis, owner, start/end dates, and active state.',
        icon: FlaskConical,
        state: 'planned',
      },
      {
        label: 'Variants',
        value: 'Pending',
        detail: 'Variant definitions should include channel, audience, creative, and prompt deltas.',
        icon: BarChart3,
        state: 'planned',
      },
      {
        label: 'Winners',
        value: 'Pending',
        detail: 'Winner criteria should be measured against GA4, Search Console, Social, and Inbox outcomes.',
        icon: CheckCircle2,
        state: 'planned',
      },
      {
        label: 'Learning velocity',
        value: 'Pending',
        detail: 'Insights needs completed test cadence and confidence metadata.',
        icon: TrendingUp,
        state: 'planned',
      },
    ],
    signals: plannedSignals(
      'Experiment backend',
      'Add a contract for hypotheses, variants, assignments, guardrails, results, confidence, and archived learnings.',
    ),
  }
}

function socialSignals(calendar: SocialCalendar, state: MeasurementState): InsightSignal[] {
  if (state !== 'live' || (!calendar.accounts.length && !calendar.posts.length)) {
    return unavailableSignals('Social measurement', 'Connect social accounts and expose the social calendar contract to populate this section.', state)
  }

  const connectedAccounts = calendar.accounts.filter((account) => account.status === 'connected')
  const upcomingPost = calendar.posts.find((post) => post.status === 'scheduled') ?? calendar.posts[0] ?? null

  return [
    {
      title: connectedAccounts.length ? 'Connected social accounts found' : 'No connected account yet',
      detail: connectedAccounts.length
        ? connectedAccounts.map((account) => `${account.label} ${account.handle}`).join(', ')
        : 'Accounts exist, but none are connected for direct publishing.',
      meta: 'Accounts',
      state: connectedAccounts.length ? 'live' : 'not_connected',
      href: '/social/accounts',
    },
    {
      title: upcomingPost ? upcomingPost.title : 'No social posts yet',
      detail: upcomingPost
        ? `${upcomingPost.source.label} is ${upcomingPost.status} for ${upcomingPost.platforms.join(', ')}.`
        : 'Social outcomes will appear after drafts or scheduled posts exist.',
      meta: 'Calendar',
      state: upcomingPost ? 'live' : 'empty',
      href: '/social/calendar',
    },
    {
      title: 'Adapter rules available',
      detail: 'Insights reads social readiness separately from external GA4/SEO analytics.',
      meta: 'Measurement boundary',
      state: 'live',
    },
  ]
}

function inboxSignals(tickets: readonly LiveTicket[], state: MeasurementState): InsightSignal[] {
  if (state !== 'live' || !tickets.length) {
    return unavailableSignals('Inbox measurement', 'Conversation summaries will appear after the inbox contract returns org-scoped rows.', state)
  }

  return tickets.slice(0, 3).map((ticket) => ({
    title: ticket.title,
    detail: `${ticketStateName(ticket)} ${ticketPriorityName(ticket)} conversation from ${ticket.channel ?? ticket.group?.name ?? 'Inbox'}.`,
    meta: ticket.tags?.length ? ticket.tags.join(', ') : 'Conversation',
    state: 'live' as SurfaceState,
    href: '/inbox',
  }))
}

function ticketStateName(ticket: LiveTicket): string {
  return ticket.state?.name ?? 'open'
}

function ticketPriorityName(ticket: LiveTicket): string {
  return ticket.priority?.name ?? 'normal'
}

function unavailableSignals(title: string, detail: string, state: MeasurementState): InsightSignal[] {
  return [
    {
      title,
      detail,
      meta: 'Unavailable',
      state,
    },
    {
      title: 'No placeholder analytics are shown',
      detail: 'This section stays empty until an org-scoped live contract responds.',
      meta: 'Measurement policy',
      state,
    },
    {
      title: 'Connector work can proceed safely',
      detail: 'External analytics slots below remain visible even while this internal surface is unavailable.',
      meta: 'External slots',
      state: 'planned',
    },
  ]
}

function plannedSignals(title: string, detail: string): InsightSignal[] {
  return [
    {
      title,
      detail,
      meta: 'Backend dependency',
      state: 'planned',
    },
    {
      title: 'No synthetic data',
      detail: 'Insights will render empty and unavailable states until the backing API exists.',
      meta: 'Measurement policy',
      state: 'planned',
    },
    {
      title: 'External report joins are separate',
      detail: 'GA4 and Search Console rows should land through connector-specific proxies, not hardcoded UI samples.',
      meta: 'Connector dependency',
      state: 'planned',
    },
  ]
}

function loadingView(section: InsightsSection): SectionView {
  const config = sectionConfigs.find((item) => item.id === section) ?? sectionConfigs[0]!
  return {
    title: config.title,
    sourceState: 'planned',
    sourceDetail: 'Loading measurement contracts.',
    metrics: [],
    signals: [],
  }
}

function stateForCount(parentState: MeasurementState, count: number): SurfaceState {
  if (parentState === 'unavailable') return 'unavailable'
  if (parentState === 'not_connected') return 'not_connected'
  if (count > 0) return 'live'
  return parentState === 'live' ? 'empty' : parentState
}

function strongestState(first: MeasurementState, second: MeasurementState): SurfaceState {
  if (first === 'live' || second === 'live') return 'live'
  if (first === 'empty' || second === 'empty') return 'empty'
  if (first === 'not_connected' || second === 'not_connected') return 'not_connected'
  return 'unavailable'
}

function stateLabel(state: SurfaceState): string {
  switch (state) {
    case 'connected_pending_reports':
      return 'Connected, reports pending'
    case 'empty':
      return 'Empty'
    case 'live':
      return 'Live'
    case 'not_connected':
      return 'Not connected'
    case 'planned':
      return 'Backend dependency'
    case 'unavailable':
      return 'Unavailable'
  }
}

function stateClass(state: SurfaceState): string {
  return state.replace(/_/g, '-')
}
