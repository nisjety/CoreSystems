import {
  BarChart3,
  BookOpen,
  Bot,
  CalendarRange,
  CircleDashed,
  Database,
  FlaskConical,
  Gauge,
  Inbox,
  LineChart,
  Megaphone,
  RefreshCw,
  TrendingUp,
  type LucideProps,
} from '@/shared/icons'
import { createMemo, createSignal, For, Show, onCleanup, untrack, type Component } from 'solid-js'
import { Dynamic } from '@solidjs/web'
import { createResource } from '@/shared/lib/create-resource-compat'
import { buildHeadlineMetrics, type HeadlineMetric } from '@/features/insights/lib/insights-headline-metrics'
import { UserUsageSection } from './UserUsageSection'
import { UsageAnalyticsSection } from './UsageAnalyticsSection'
import { BillingAllowancesSection } from './BillingAllowancesSection'
import {
  loadKnowledgeInsightsSnapshot,
  type KnowledgeInsightsSnapshot,
} from '@/features/insights/lib/knowledge-insights'
import {
  buildInsightsOverviewQuery,
  surfaceForInsightsSection,
  type InsightsRange,
  type InsightsScope,
  type InsightsSection,
} from '@/features/insights/lib/insights-view'
import {
  loadInsightsWorkspace,
  type InsightsWorkspace,
  type MeasurementState,
} from '@/features/insights/lib/insights-workspace'
import type { InsightScorecard } from '@/shared/api/insights-client'
import type { ResourceResult } from '@/shared/read-data'
import { cn } from '@/shared/lib/cn'
import { useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'
import { getChatGptSubscriptionUsage, isChatGptSubscriptionConnected, listChatGptSubscriptions, type ChatGptRateLimitReport } from '@/shared/api/chatgpt-subscription-client'
import { insightsRefreshActionSchema, type InsightsRefreshAction } from '@/shared/actions/insights-refresh'
import { startInsightsAutoRefresh } from '@/features/insights/lib/insights-auto-refresh'
import './insights-refresh.css'

type TrFn = (noText: string, enText: string) => string

type SectionConfig = {
  description: string
  icon: Component<LucideProps>
  id: InsightsSection
  label: string
  measurementReady: boolean
  title: string
}

function sectionConfigs(tr: TrFn): SectionConfig[] {
  return [
    {
      id: 'overview',
      label: tr('Oversikt', 'Overview'),
      title: tr('Innsikt', 'Insights'),
      description: tr('Se målte resultater fra valgte kilder og tidsperioder.', 'See measured outcomes from connected sources over the selected period.'),
      icon: BarChart3,
      measurementReady: true,
    },
    {
      id: 'inbox',
      label: tr('Support', 'Support'),
      title: tr('Supportinnsikt', 'Support insights'),
      description: tr('Mål samtale- og ticketutfall fra conversation-core, inkludert godkjente og avviste AI-forslag.', 'Measure conversation and ticket outcomes from conversation-core, including reviewed AI suggestions.'),
      icon: Inbox,
      measurementReady: true,
    },
    {
      id: 'social',
      label: tr('Sosialt', 'Social'),
      title: tr('Sosial innsikt', 'Social insights'),
      description: tr('Mål publiseringsaktivitet og utførte sosiale arbeidsflyter fra social-core.', 'Measure publishing activity and completed social workflows from social-core.'),
      icon: TrendingUp,
      measurementReady: true,
    },
    {
      id: 'agents',
      label: tr('Agenter', 'Agents'),
      title: tr('Agentinnsikt', 'Agent insights'),
      description: tr('Mål faktiske agentkjøringer, verktøyhandlinger og godkjenninger fra Model Plane.', 'Measure actual agent runs, tool actions, and approvals from the Model Plane.'),
      icon: Bot,
      measurementReady: true,
    },
    {
      id: 'chat',
      label: tr('Chat', 'Chat'),
      title: tr('Chatinnsikt', 'Chat insights'),
      description: tr('Mål bare nye globale chat-økter med en bekreftet bruker og organisasjon fra Model Plane.', 'Measure only new global Chat turns with a verified user and organization from the Model Plane.'),
      icon: Bot,
      measurementReady: true,
    },
    {
      id: 'knowledge',
      label: tr('Kunnskap', 'Knowledge'),
      title: tr('Kunnskapsinnsikt', 'Knowledge insights'),
      description: tr('Se en direkte, tillatelsesstyrt tilstand for kunnskap som er synlig for deg i den aktive organisasjonen.', 'See a live, permission-scoped state of knowledge visible to you in the active organization.'),
      icon: BookOpen,
      measurementReady: true,
    },
    {
      id: 'ingestion',
      label: tr('Inntak', 'Ingestion'),
      title: tr('Inntaksinnsikt', 'Ingestion insights'),
      description: tr('Mål faktiske import- og crawl-livssyklus-hendelser fra Ingestion Plane uten å kopiere innhold.', 'Measure real import and crawl lifecycle events from the Ingestion Plane without copying content.'),
      icon: Database,
      measurementReady: true,
    },
    {
      id: 'campaigns',
      label: tr('Kampanjer', 'Campaigns'),
      title: tr('Kampanjeinnsikt', 'Campaign insights'),
      description: tr('Følg aktiviteten og resultatene fra kampanjene dine.', 'Track activity and results from your campaigns.'),
      icon: Megaphone,
      measurementReady: true,
    },
    {
      id: 'external_analytics',
      label: tr('Eksterne kilder', 'External sources'),
      title: tr('Eksterne analysekilder', 'External analytics sources'),
      description: tr('Resultater fra dine tilkoblede tjenester.', 'Results from your connected services.'),
      icon: LineChart,
      measurementReady: true,
    },
    {
      id: 'experiments',
      label: tr('Eksperimenter', 'Experiments'),
      title: tr('Eksperimentinnsikt', 'Experiment insights'),
      description: tr('Eksperimentrapporter er ikke tilgjengelige ennå.', 'Experiment reports are not available yet.'),
      icon: FlaskConical,
      measurementReady: false,
    },
  ]
}

const RANGE_OPTIONS: Array<{ id: InsightsRange; label: string }> = [
  { id: '7d', label: '7d' },
  { id: '30d', label: '30d' },
  { id: '90d', label: '90d' },
  { id: 'all', label: 'All' },
]

function insightsHref(section: InsightsSection): string {
  if (section === 'overview') return '/insights'
  if (section === 'external_analytics') return '/insights/external'
  return `/insights/${section}`
}

export default function InsightsPage(props: { section?: InsightsSection }) {
  const i18n = useI18n()
  const session = getSession()
  const [range, setRange] = createSignal<InsightsRange>('30d')
  const [scope, setScope] = createSignal<InsightsScope>('organization')
  const [refreshTime, setRefreshTime] = createSignal(new Date())
  const identityKey = createMemo(() => session.user?.id && session.activeOrg?.id
    ? `${session.user.id}:${session.activeOrg.id}:${session.activeOrg.role}` : null)
  const organizationCostAccess = createMemo(() => ['owner', 'admin'].includes(session.activeOrg?.role.trim().toLowerCase() ?? ''))
  const activeSection = createMemo<InsightsSection>(() => props.section ?? 'overview')
  const sections = createMemo(() => sectionConfigs(i18n.tr))
  const config = createMemo(() =>
    sections().find((section) => section.id === activeSection()) ?? sections()[0]!,
  )
  const visibleSections = createMemo(() => sections().filter((section) => section.measurementReady))
  const query = createMemo(() =>
    config().measurementReady
      ? buildInsightsOverviewQuery(activeSection(), range(), refreshTime(), scope())
      : null,
  )
  const workspaceKey = createMemo(() => identityKey() && query()
    ? { identity: identityKey()!, view: `${activeSection()}:${range()}:${scope()}`, query: query()!, organizationCostAccess: organizationCostAccess() } : null)
  const [workspaceRead] = createResource(workspaceKey, async (key) => ({
    identity: key.identity,
    view: key.view,
    data: await loadInsightsWorkspace(key.query, { organizationCostAccess: key.organizationCostAccess }),
    checkedAt: new Date(),
  }))
  // Resource refreshes preserve previous values. Never render those values
  // after the active tenant or user changes, even while a new read is pending.
  const workspace = createMemo(() => workspaceRead()?.identity === identityKey() && workspaceRead()?.view === workspaceKey()?.view ? workspaceRead()?.data : undefined)
  const codexKey = createMemo(() => identityKey() && activeSection() === 'overview'
    ? { identity: identityKey()!, org: session.activeOrg!.id, revision: refreshTime() } : null)
  const [codexRead] = createResource(codexKey, async (key) => ({ identity: key.identity, data: await loadCodexUsage(key.org) }))
  const codexUsage = createMemo(() => codexRead()?.identity === identityKey() ? codexRead()?.data : undefined)
  const knowledgeKey = createMemo(() => identityKey() && activeSection() === 'knowledge'
    ? { identity: identityKey()!, revision: refreshTime() } : null)
  const [knowledgeRead] = createResource(knowledgeKey, async (key) => ({ identity: key.identity, data: await loadKnowledgeInsightsSnapshot() }))
  const knowledge = createMemo(() => knowledgeRead()?.identity === identityKey() ? knowledgeRead()?.data : undefined)
  const refreshing = () => workspaceRead.loading || (activeSection() === 'overview' && codexRead.loading) || (activeSection() === 'knowledge' && knowledgeRead.loading)
  const checkedAt = createMemo(() => workspaceRead()?.identity === identityKey() ? workspaceRead()?.checkedAt : undefined)
  const refresh = (reason: InsightsRefreshAction['reason']) => {
    insightsRefreshActionSchema.parse({ type: 'insights.refresh', reason })
    untrack(() => {
      if (identityKey() && !refreshing()) setRefreshTime(new Date())
    })
  }
  const stopAutoRefresh = startInsightsAutoRefresh(() => refresh('automatic'), () => untrack(() => Boolean(identityKey()) && !refreshing()))
  onCleanup(stopAutoRefresh)

  return (
    <div class="verevon-insights-page">
      <section class="verevon-insights-hero">
        <div>
          <span><Dynamic component={config().icon} size={15} />{i18n.tr('Innsikt', 'Insights')}</span>
          <h1>{i18n.tr('Verevon Innsikt', 'Verevon Insights')}</h1>
          <p>{activeSection() === 'overview'
            ? i18n.tr('Oversikt over teamets aktivitet og AI-bruk.', 'Your team’s activity and AI usage at a glance.')
            : config().description}</p>
        </div>
        <a href="/settings/integrations" link>{i18n.tr('Administrer tilkoblinger', 'Manage connectors')}</a>
      </section>

      <nav class="verevon-insights-tabs" aria-label={i18n.tr('Innsikt-seksjoner', 'Insights sections')}>
        <For each={visibleSections()}>
          {(section) => (
            <a
              class={cn('verevon-insights-tab', section.id === activeSection() && 'is-active')}
              href={insightsHref(section.id)}
              link
            >
              <Dynamic component={section.icon} size={14} />
              {section.label}
            </a>
          )}
        </For>
      </nav>

      <div class="verevon-insights-refresh" aria-live="polite">
        <span>{i18n.tr('Oppdateres hvert minutt', 'Updates every minute')}
          <Show when={checkedAt()}>{(date) => <time data-testid="insights-checked-at" datetime={date().toISOString()}> · {i18n.tr('Sist sjekket', 'Last checked')} {date().toLocaleTimeString()}</time>}</Show>
        </span>
        <button type="button" onClick={() => refresh('manual')} disabled={refreshing()} aria-label={i18n.tr('Oppdater innsikt', 'Refresh insights')}>
          <RefreshCw size={14} />{refreshing() ? i18n.tr('Oppdaterer…', 'Refreshing…') : i18n.tr('Oppdater nå', 'Refresh now')}
        </button>
      </div>

      <Show when={activeSection() === 'knowledge'} fallback={
        <Show when={config().measurementReady} fallback={<NoExperimentContract />}>
          <div class="verevon-insights-controls">
            <RangePicker range={range()} onChange={setRange} />
            <ScopePicker scope={scope()} onChange={setScope} />
          </div>
          <Show when={workspace()} fallback={<LoadingState />}>
            {(loadedWorkspace) => (
              <>
                <Show when={activeSection() === 'overview'}>
                  <>
                    <HeadlineMetricsSection overview={loadedWorkspace().overview} cost={loadedWorkspace().cost} />
                    <UsageAnalyticsSection identity={identityKey()!} scope={organizationCostAccess() && scope() === 'organization' ? 'organization' : 'me'} from={query()?.from} to={query()?.to} refreshKey={refreshTime().getTime()} canManage={organizationCostAccess()} />
                    <Show when={organizationCostAccess() && scope() === 'organization'}><BillingAllowancesSection identity={identityKey()!} refreshKey={refreshTime().getTime()} /></Show>
                    <CodexUsageSection result={codexUsage()} />
                    <Show when={organizationCostAccess() && scope() === 'organization'}>
                      <AzureActualCostSection result={loadedWorkspace().azureActualCost} />
                      <UserUsageSection identity={identityKey()!} orgId={session.activeOrg!.id} from={query()?.from} to={query()?.to} refreshKey={refreshTime().getTime()} viewer={session.user} />
                    </Show>
                  </>
                </Show>
                <MetricsSection
                  overview={loadedWorkspace().overview}
                  section={activeSection()}
                />
              </>
            )}
          </Show>
        </Show>
      }>
        <div class="verevon-insights-controls">
          <RangePicker range={range()} onChange={setRange} />
          <ScopePicker scope={scope()} onChange={setScope} />
        </div>
        <Show when={knowledge()} fallback={<LoadingState />}>
          {(snapshot) => <KnowledgeSnapshotSection snapshot={snapshot()} />}
        </Show>
        <Show when={workspace()} fallback={<LoadingState />}>
          {(loadedWorkspace) => (
            <>
              <MetricsSection overview={loadedWorkspace().overview} section="knowledge" />
            </>
          )}
        </Show>
      </Show>
    </div>
  )
}

type CodexUsageResult = { report: ChatGptRateLimitReport; state: 'measured' } | { state: 'unavailable' | 'not_connected' }

async function loadCodexUsage(orgId: string | null): Promise<CodexUsageResult> {
  if (!orgId) return { state: 'not_connected' }
  try {
    const connection = (await listChatGptSubscriptions(orgId)).find((item) => isChatGptSubscriptionConnected(item.status))
    if (!connection) return { state: 'not_connected' }
    const result = await getChatGptSubscriptionUsage(orgId, connection.id)
    return { report: result.usage, state: 'measured' }
  } catch {
    return { state: 'unavailable' }
  }
}

function ScopePicker(props: { onChange: (scope: InsightsScope) => void; scope: InsightsScope }) {
  const i18n = useI18n()
  const options: Array<{ id: InsightsScope; label: string }> = [
    { id: 'organization', label: i18n.tr('Organisasjon', 'Organization') },
    { id: 'me', label: i18n.tr('Min aktivitet', 'My activity') },
  ]
  return (
    <div class="verevon-insights-scope" aria-label={i18n.tr('Rapporteringsomfang', 'Reporting scope')}>
      <span><CircleDashed size={14} />{i18n.tr('Omfang', 'Scope')}</span>
      <div role="group" aria-label={i18n.tr('Velg rapporteringsomfang', 'Choose reporting scope')}>
        <For each={options}>
          {(option) => (
            <button
              type="button"
              class={cn(props.scope === option.id && 'is-active')}
              aria-pressed={props.scope === option.id ? 'true' : 'false'}
              onClick={() => props.onChange(option.id)}
            >
              {option.label}
            </button>
          )}
        </For>
      </div>
    </div>
  )
}

function RangePicker(props: { onChange: (range: InsightsRange) => void; range: InsightsRange }) {
  const i18n = useI18n()
  return (
    <div class="verevon-insights-range" aria-label={i18n.tr('Tidsrom for målinger', 'Measurement time range')}>
      <span><CalendarRange size={14} />{i18n.tr('Hendelsesvindu', 'Event window')}</span>
      <div role="group" aria-label={i18n.tr('Velg tidsrom', 'Choose time range')}>
        <For each={RANGE_OPTIONS}>
          {(option) => (
            <button
              type="button"
              class={cn(props.range === option.id && 'is-active')}
              aria-pressed={props.range === option.id ? 'true' : 'false'}
              onClick={() => props.onChange(option.id)}
            >
              {option.id === 'all' ? i18n.tr('Alle', 'All') : option.label}
            </button>
          )}
        </For>
      </div>
    </div>
  )
}

function LoadingState() {
  const i18n = useI18n()
  return (
    <section class="verevon-insights-loading" aria-label={i18n.tr('Laster Innsikt', 'Loading Insights')}>
      <Gauge size={18} />
      <div>
        <h2>{i18n.tr('Laster innsikt', 'Loading insights')}</h2>
        <p>{i18n.tr('Henter den nyeste aktiviteten.', 'Getting your latest activity.')}</p>
      </div>
    </section>
  )
}

function NoExperimentContract() {
  const i18n = useI18n()
  return (
    <section class="verevon-insights-empty" aria-live="polite">
      <FlaskConical size={20} />
      <div>
        <h2>{i18n.tr('Eksperimentmåling er ikke tilgjengelig', 'Experiment measurement is not available')}</h2>
        <p>{i18n.tr('Denne rapporten er ikke tilgjengelig ennå.', 'This report is not available yet.')}</p>
      </div>
    </section>
  )
}

function KnowledgeSnapshotSection(props: { snapshot: ResourceResult<KnowledgeInsightsSnapshot> }) {
  const i18n = useI18n()
  const cards = createMemo(() => [
    { label: i18n.tr('Synlige dokumenter', 'Visible documents'), value: props.snapshot.data.documentCount },
    { label: i18n.tr('Indekserte dokumenter', 'Indexed documents'), value: props.snapshot.data.indexedCount },
    { label: i18n.tr('Kunnskapskilder', 'Knowledge sources'), value: props.snapshot.data.sourceCount },
  ])

  return (
    <section class="verevon-insights-metrics" aria-label={i18n.tr('Tillatelsesstyrt kunnskapsøyeblikksbilde', 'Permission-scoped knowledge snapshot')}>
      <div class="verevon-insights-section-heading">
        <div>
          <span><BookOpen size={14} />{i18n.tr('Direkte tilstand', 'Live state')}</span>
          <h2>{i18n.tr('Kunnskap du har tilgang til', 'Knowledge you can access')}</h2>
        </div>
        <p>{i18n.tr('Dokumenter og kilder du har tilgang til.', 'Documents and sources you can access.')}</p>
      </div>
      <Show when={props.snapshot.state !== 'unavailable'} fallback={<KnowledgeHonestState state={props.snapshot.state} />}>
        <>
          <ul class="verevon-insights-metrics-grid">
            <For each={cards()}>{(card) => (
              <li class="verevon-insights-metric-card">
                <div class="verevon-insights-metric-card__topline"><span>{card.label}</span><small>{i18n.tr('nå', 'now')}</small></div>
                <strong class="verevon-insights-metric-card__value">{card.value.toLocaleString()}</strong>
              </li>
            )}</For>
          </ul>
        </>
      </Show>
    </section>
  )
}

function KnowledgeHonestState(props: { state: MeasurementState }) {
  const i18n = useI18n()
  const unavailable = () => props.state === 'unavailable'
  return (
    <div class="verevon-insights-empty" aria-live="polite">
      <CircleDashed size={20} />
      <div>
        <h3>{unavailable() ? i18n.tr('Kunnskapstilstand utilgjengelig', 'Knowledge state unavailable') : i18n.tr('Ingen synlig kunnskap ennå', 'No visible knowledge yet')}</h3>
        <p>{unavailable()
          ? i18n.tr('Kunne ikke hente dokumentene. Prøv å oppdatere.', 'Unable to load your documents. Try refreshing.')
          : i18n.tr('Legg til dokumenter eller koble til en kilde for å komme i gang.', 'Add documents or connect a source to get started.')}
        </p>
      </div>
    </div>
  )
}

function HeadlineMetricsSection(props: {
  cost: InsightsWorkspace['cost']
  overview: InsightsWorkspace['overview']
}) {
  const i18n = useI18n()
  const metrics = createMemo(() => buildHeadlineMetrics(props.overview, props.cost))

  return (
    <section class="verevon-insights-headline" aria-label={i18n.tr('Aktivitetsoversikt', 'Activity overview')}>
      <div class="verevon-insights-section-heading">
        <div>
          <span><Gauge size={14} />{i18n.tr('Denne perioden', 'This period')}</span>
          <h2>{i18n.tr('Teamaktivitet', 'Your activity')}</h2>
        </div>
        <Show when={props.cost.data.reportingStartedAt}>
          {(start) => <p>{i18n.tr('AI-bruk registreres fra', 'AI usage tracked from')} {new Date(start()).toLocaleString()}</p>}
        </Show>
      </div>
      <ul class="verevon-insights-headline-grid">
        <For each={metrics()}>{(metric) => <HeadlineMetricCard metric={metric} />}</For>
      </ul>
    </section>
  )
}

function HeadlineMetricCard(props: { metric: HeadlineMetric }) {
  const i18n = useI18n()
  return (
    <li class={cn('verevon-insights-headline-card', `verevon-insights-headline-card--${props.metric.state}`)}>
      <div class="verevon-insights-headline-card__topline">
        <span>{props.metric.label}</span>
        <Show when={props.metric.state === 'partial' || props.metric.state === 'unavailable'}>
          <small>{props.metric.state === 'partial'
            ? props.metric.id === 'ai_usage_cost' ? i18n.tr('Begrensede kostnadsdata', 'Limited cost data') : i18n.tr('Begrensede data', 'Limited data')
            : i18n.tr('Utilgjengelig', 'Unavailable')}</small>
        </Show>
      </div>
      <strong class="verevon-insights-headline-card__value">{props.metric.value}</strong>
      <p class="verevon-insights-headline-card__detail">{props.metric.state === 'unavailable' ? props.metric.detail : props.metric.summary || props.metric.detail}</p>
    </li>
  )
}

function CodexUsageSection(props: { result: CodexUsageResult | undefined }) {
  const i18n = useI18n()
  const result = () => props.result
  const windows = () => [report()?.primary, report()?.secondary].filter((window) => Boolean(window))
  const report = () => {
    const current = result()
    return current?.state === 'measured' ? current.report : undefined
  }
  return <Show when={result()?.state !== 'not_connected' && result()}>
    {(value) => <section class="verevon-insights-codex-usage" aria-label={i18n.tr('Codex-abonnementsbruk', 'Codex subscription usage')}>
      <div>
        <span>{i18n.tr('Abonnementsbruk', 'Subscription usage')}</span>
        <h3>{i18n.tr('Codex-kvote fra ChatGPT', 'Codex allowance from ChatGPT')}</h3>
        <p>{value().state === 'unavailable'
          ? i18n.tr('Leverandørens kvotedata er utilgjengelig akkurat nå.', 'The provider allowance snapshot is unavailable right now.')
          : i18n.tr('Kvoten for din tilkoblede ChatGPT-konto.', 'Allowance for your connected ChatGPT account.')}</p>
      </div>
      <Show when={value().state === 'measured'}>
        <Show when={windows().length > 0} fallback={<p>{i18n.tr('ChatGPT returnerte ingen aktive kvotevinduer. Dette er ikke en forbruksprosent.', 'ChatGPT returned no current allowance windows. No usage percentage is available.')}</p>}>
          <div class="verevon-insights-codex-usage__windows">
          <For each={windows()}>
            {(window) => <div><strong>{Math.round(window!.usedPercent)}%</strong><span>{i18n.tr('brukt', 'used')}{window!.windowMinutes === 10080 ? i18n.tr(' denne uken', ' this week') : window!.windowMinutes === 300 ? i18n.tr(' · 5 timer', ' · 5 hours') : ''}</span>{window!.resetsAt && <small>{i18n.tr('Tilbakestilles', 'Resets')} {new Date(window!.resetsAt).toLocaleString()}</small>}</div>}
          </For>
          </div>
        </Show>
      </Show>
    </section>}
  </Show>
}

function AzureActualCostSection(props: { result: InsightsWorkspace['azureActualCost'] }) {
  const i18n = useI18n()
  const amount = (value: number, currency: string) => new Intl.NumberFormat(undefined, {
    style: 'currency', currency, maximumFractionDigits: 2,
  }).format(value)
  return <section class="verevon-insights-codex-usage" aria-label={i18n.tr('Azure faktiske kostnader', 'Azure actual charges')}>
    <div>
      <span>{i18n.tr('Faktisk Azure-forbruk', 'Azure actual cost')}</span>
      <h3>{i18n.tr('Azure-kostnader', 'Azure charges')}</h3>
      <p>{props.result.state === 'unavailable'
        ? i18n.tr('Kostnader er midlertidig utilgjengelige. Prøv å oppdatere.', 'Charges are temporarily unavailable. Try refreshing.')
        : props.result.data.byCurrency.length > 0
          ? i18n.tr('Rapporterte kostnader for organisasjonen i valgt periode.', 'Reported charges for your organization in this period.')
          : i18n.tr('Ingen kostnadsrapport er tilgjengelig for organisasjonen ennå.', 'A cost report for your organization is not available yet.')}</p>
      <Show when={props.result.data.latestUsageDate}>
        {(date) => <small>{i18n.tr('Siste rapporterte bruksdato', 'Latest reported usage date')}: {new Date(date()).toLocaleDateString()}</small>}
      </Show>
    </div>
    <Show when={props.result.data.byCurrency.length > 0}>
      <div class="verevon-insights-codex-usage__windows">
        <For each={props.result.data.byCurrency}>{(total) => <div><strong>{amount(total.amount, total.currency)}</strong><span>{total.currency} · {props.result.data.rows} {i18n.tr('målerader', 'meter rows')}</span></div>}</For>
      </div>
    </Show>
  </section>
}

function MetricsSection(props: { overview: InsightsWorkspace['overview']; section: InsightsSection }) {
  const i18n = useI18n()
  const surface = createMemo(() => surfaceForInsightsSection(props.section))
  const title = createMemo(() => surface() ? i18n.tr('Registrerte måltall', 'Recorded measurements') : i18n.tr('Målt på tvers av Verevon', 'Measured across Verevon'))

  return (
    <section class="verevon-insights-metrics" aria-label={i18n.tr('Målenøkkeltall', 'Measurement metrics')}>
      <div class="verevon-insights-section-heading">
        <div>
          <span><BarChart3 size={14} />{i18n.tr('Kildedata', 'Source data')}</span>
          <h2>{title()}</h2>
        </div>
      </div>
      <Show when={props.overview.state === 'live' || (props.overview.state === 'partial' && props.overview.data.length > 0)} fallback={<MetricsHonestState state={props.overview.state} section={props.section} message={props.overview.message} />}>
        <Show when={props.overview.state === 'partial'}>
          <details class="verevon-insights-coverage-note"><summary>{i18n.tr('Om datadekningen', 'About data coverage')}</summary><p>{i18n.tr('Noe aktivitet kan mangle. Tall merket bekreftet er kontrollert mot lagrede oppføringer.', 'Some activity may be missing. Figures marked verified have been checked against saved records.')}</p></details>
        </Show>
        <ul class="verevon-insights-metrics-grid">
          <For each={props.overview.data}>{(scorecard) => <ScorecardCard scorecard={scorecard} />}</For>
        </ul>
      </Show>
    </section>
  )
}

function MetricsHonestState(props: { section: InsightsSection; state: MeasurementState; message: string }) {
  const i18n = useI18n()
  const surface = createMemo(() => surfaceForInsightsSection(props.section))
  const label = createMemo(() => surface() ? surface()!.replace(/_/g, ' ') : i18n.tr('valgt visning', 'selected view'))
  const copy = (): { body: string; heading: string } => {
    if (props.state === 'unavailable') {
      return {
        heading: i18n.tr('Målenøkkeltall utilgjengelig', 'Measurements unavailable'),
        body: i18n.tr('Kunne ikke hente aktiviteten. Prøv å oppdatere.', 'Unable to load activity. Try refreshing.'),
      }
    }
    if (props.state === 'partial') {
      return {
        heading: i18n.tr('Ingen aktivitet å vise ennå', 'No activity to show yet'),
        body: i18n.tr('Data for denne perioden er ikke tilgjengelige ennå.', 'Data for this period is not available yet.'),
      }
    }
    return {
      heading: i18n.tr('Ingen målte hendelser i dette vinduet', 'No measured events in this window'),
      body: i18n.tr(`Ingen registrert aktivitet for ${label()} i denne perioden.`, `No recorded activity for ${label()} in this period.`),
    }
  }
  return (
    <div class="verevon-insights-empty" aria-live="polite">
      <CircleDashed size={20} />
      <div><h3>{copy().heading}</h3><p>{copy().body}</p></div>
    </div>
  )
}

function ScorecardCard(props: { scorecard: InsightScorecard }) {
  const i18n = useI18n()
  const label = () => ({
    'agents.agent_runs_completed': i18n.tr('Fullførte agentkjøringer', 'Completed agent runs'),
    'agents.agent_runs_started': i18n.tr('Startede agentkjøringer', 'Started agent runs'),
    'agents.agent_runs_failed': i18n.tr('Mislykkede agentkjøringer', 'Failed agent runs'),
    'inbox.messages_received': i18n.tr('Mottatte meldinger', 'Messages received'),
    'inbox.messages_sent': i18n.tr('Sendte meldinger', 'Messages sent'),
    'chat.chat_turns_started': i18n.tr('Chatforespørsler', 'Chat requests'),
  } as Record<string, string>)[props.scorecard.id] || props.scorecard.label || props.scorecard.metric.replace(/_/g, ' ')
  const displayValue = () => {
    if (props.scorecard.aggregation === 'unverified') return i18n.tr('Ubekreftet verdi', 'Unverified value')
    const value = Number.isInteger(props.scorecard.value) ? props.scorecard.value.toString() : props.scorecard.value.toFixed(2)
    return props.scorecard.unit && props.scorecard.unit !== 'count' ? `${value} ${props.scorecard.unit}` : value
  }
  return (
    <li class="verevon-insights-metric-card">
      <div class="verevon-insights-metric-card__topline">
        <span>{label()}</span>
        <small>{props.scorecard.coverage === 'measured' ? i18n.tr('Bekreftet', 'Verified') : i18n.tr('Observert', 'Observed')}</small>
      </div>
      <strong class="verevon-insights-metric-card__value">{displayValue()}</strong>
      <p class="verevon-insights-metric-card__source">{props.scorecard.coverage === 'measured'
        ? i18n.tr(`${props.scorecard.eventCount} kilderegistreringer i valgt periode`, `${props.scorecard.eventCount} source records in the selected period`)
        : i18n.tr(`${props.scorecard.eventCount} hendelser i valgt periode`, `${props.scorecard.eventCount} events in the selected period`)}</p>
      <details class="verevon-insights-metric-card__details">
        <summary>{i18n.tr('Om dette tallet', 'About this number')}</summary>
        <p>{props.scorecard.definition}</p>
        <Show when={props.scorecard.checkedAt}><p>{i18n.tr('Bekreftet', 'Verified')} {new Date(props.scorecard.checkedAt!).toLocaleString()}</p></Show>
      </details>
    </li>
  )
}
