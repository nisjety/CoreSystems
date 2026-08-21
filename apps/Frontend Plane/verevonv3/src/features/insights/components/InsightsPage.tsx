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
  PlugZap,
  TrendingUp,
  type LucideProps,
} from '@/shared/icons'
import { createMemo, createSignal, For, Show, type Component } from 'solid-js'
import { Dynamic } from '@solidjs/web'
import { createResource } from '@/shared/lib/create-resource-compat'
import { buildHeadlineMetrics, type HeadlineMetric } from '@/features/insights/lib/insights-headline-metrics'
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
import type { InsightConnector, InsightScorecard } from '@/shared/api/insights-client'
import type { ResourceResult } from '@/shared/read-data'
import { cn } from '@/shared/lib/cn'
import { useI18n } from '@/shared/i18n'

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
      description: tr('Se sanntidsregistrerte måltall og tilkoblinger. Verevon viser bare rader som er produsert av en autoritativ kilde.', 'See recorded performance and source readiness. Verevon only shows rows produced by an authoritative source.'),
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
      description: tr('Kampanjemålinger vises når campaign-core publiserer autoritative hendelser til Insight Core.', 'Campaign measurements appear when campaign-core publishes authoritative events to Insight Core.'),
      icon: Megaphone,
      measurementReady: true,
    },
    {
      id: 'external_analytics',
      label: tr('Eksterne kilder', 'External sources'),
      title: tr('Eksterne analysekilder', 'External analytics sources'),
      description: tr('Mål bare provider-rader som Insight Core allerede har registrert fra godkjente integrasjoner.', 'Measure only provider rows that Insight Core has already recorded from authorized integrations.'),
      icon: LineChart,
      measurementReady: true,
    },
    {
      id: 'experiments',
      label: tr('Eksperimenter', 'Experiments'),
      title: tr('Eksperimentinnsikt', 'Experiment insights'),
      description: tr('Eksperimenter er ikke eksponert som en målekontrakt ennå.', 'Experiments are not yet exposed as a measurement contract.'),
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
  const [range, setRange] = createSignal<InsightsRange>('30d')
  const [scope, setScope] = createSignal<InsightsScope>('organization')
  const activeSection = createMemo<InsightsSection>(() => props.section ?? 'overview')
  const sections = createMemo(() => sectionConfigs(i18n.tr))
  const config = createMemo(() =>
    sections().find((section) => section.id === activeSection()) ?? sections()[0]!,
  )
  const visibleSections = createMemo(() => sections().filter((section) => section.measurementReady))
  const query = createMemo(() =>
    config().measurementReady
      ? buildInsightsOverviewQuery(activeSection(), range(), new Date(), scope())
      : null,
  )
  const [workspace] = createResource(query, loadInsightsWorkspace)
  const knowledgeKey = createMemo(() => activeSection() === 'knowledge' ? 'knowledge' : null)
  const [knowledge] = createResource(knowledgeKey, loadKnowledgeInsightsSnapshot)

  return (
    <div class="verevon-insights-page">
      <section class="verevon-insights-hero">
        <div>
          <span>
            <Dynamic component={config().icon} size={15} />
            {config().title}
          </span>
          <h1>{i18n.tr('Verevon Innsikt', 'Verevon Insights')}</h1>
          <p>{config().description}</p>
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
                  <HeadlineMetricsSection overview={loadedWorkspace().overview} cost={loadedWorkspace().cost} />
                </Show>
                <MetricsSection
                  overview={loadedWorkspace().overview}
                  section={activeSection()}
                />
                <ConnectorRegistrySection
                  registry={loadedWorkspace().insightConnectors}
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
              <ConnectorRegistrySection registry={loadedWorkspace().insightConnectors} section="knowledge" />
            </>
          )}
        </Show>
      </Show>
    </div>
  )
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
        <h2>{i18n.tr('Løser målekontrakter', 'Resolving measurement contracts')}</h2>
        <p>{i18n.tr('Sjekker organisasjonsscope og autoritative kilder.', 'Checking organization scope and authoritative sources.')}</p>
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
        <p>{i18n.tr('Ingen Insight Core-kontrakt eller produsent finnes for eksperimenter ennå, så Verevon viser ingen eksempeltall eller plassholderdata.', 'No Insight Core contract or producer exists for experiments yet, so Verevon shows no sample figures or placeholder data.')}</p>
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
        <p>{props.snapshot.message}</p>
      </div>
      <Show when={props.snapshot.state !== 'unavailable'} fallback={<KnowledgeHonestState state={props.snapshot.state} />}>
        <>
          <p class="verevon-insights-knowledge-note">{i18n.tr('Dette er et øyeblikksbilde fra Data Plane med aktiv organisasjon og brukerrettigheter. Historisk kunnskapsaktivitet vises ikke før en egen, innholdsfri hendelseskontrakt er på plass.', 'This is a Data Plane snapshot under the active organization and user permissions. Historical knowledge activity is not shown until a separate content-free event contract exists.')}</p>
          <ul class="verevon-insights-metrics-grid">
            <For each={cards()}>{(card) => (
              <li class="verevon-insights-metric-card">
                <div class="verevon-insights-metric-card__topline"><span>{card.label}</span><small>{i18n.tr('nå', 'now')}</small></div>
                <strong class="verevon-insights-metric-card__value">{card.value.toLocaleString()}</strong>
                <p class="verevon-insights-metric-card__source">{i18n.tr('Kilde: Data Plane, brukersynlighet håndhevet av gatewayen.', 'Source: Data Plane, user visibility enforced by the gateway.')}</p>
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
  const unavailable = props.state === 'unavailable'
  return (
    <div class="verevon-insights-empty" aria-live="polite">
      <CircleDashed size={20} />
      <div>
        <h3>{unavailable ? i18n.tr('Kunnskapstilstand utilgjengelig', 'Knowledge state unavailable') : i18n.tr('Ingen synlig kunnskap ennå', 'No visible knowledge yet')}</h3>
        <p>{unavailable
          ? i18n.tr('Data Plane svarte ikke. Ingen eksempelverdier vises.', 'The Data Plane did not respond. No sample values are shown.')
          : i18n.tr('Data Plane er tilgjengelig, men ingen dokumenter eller kilder er synlige for den aktive organisasjonen og brukeren.', 'The Data Plane is available, but no documents or sources are visible to the active organization and user.')}
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
    <section class="verevon-insights-headline" aria-label={i18n.tr('Pilotens hovedtall', 'Pilot headline metrics')}>
      <div class="verevon-insights-section-heading">
        <div>
          <span><Gauge size={14} />{i18n.tr('Operativt poengkort', 'Operational scorecard')}</span>
          <h2>{i18n.tr('Hva som faktisk ble målt', 'What was actually measured')}</h2>
        </div>
        <p>{i18n.tr('Hvert tall kommer fra en registrert kilde. AI-kostnad er en separat org-ledger og blir ikke presentert som kostnad per sak.', 'Each value comes from a recorded source. AI cost is a separate org ledger and is never presented as cost per case.')}</p>
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
        <small>{stateLabel(props.metric.state, i18n.tr)}</small>
      </div>
      <strong class="verevon-insights-headline-card__value">{props.metric.value}</strong>
      <p class="verevon-insights-headline-card__detail">{props.metric.detail}</p>
    </li>
  )
}

function MetricsSection(props: { overview: InsightsWorkspace['overview']; section: InsightsSection }) {
  const i18n = useI18n()
  const surface = createMemo(() => surfaceForInsightsSection(props.section))
  const title = createMemo(() => surface() ? i18n.tr('Registrerte måltall', 'Recorded measurements') : i18n.tr('Målt på tvers av Verevon', 'Measured across Verevon'))

  return (
    <section class="verevon-insights-metrics" aria-label={i18n.tr('Målenøkkeltall', 'Measurement metrics')}>
      <div class="verevon-insights-section-heading">
        <div>
          <span><BarChart3 size={14} />{i18n.tr('Autoritative rader', 'Authoritative rows')}</span>
          <h2>{title()}</h2>
        </div>
        <p>{props.overview.message}</p>
      </div>
      <Show when={props.overview.state === 'live'} fallback={<MetricsHonestState state={props.overview.state} section={props.section} />}>
        <ul class="verevon-insights-metrics-grid">
          <For each={props.overview.data}>{(scorecard) => <ScorecardCard scorecard={scorecard} />}</For>
        </ul>
      </Show>
    </section>
  )
}

function MetricsHonestState(props: { section: InsightsSection; state: MeasurementState }) {
  const i18n = useI18n()
  const surface = createMemo(() => surfaceForInsightsSection(props.section))
  const label = createMemo(() => surface() ? surface()!.replace(/_/g, ' ') : i18n.tr('valgt visning', 'selected view'))
  const copy = (): { body: string; heading: string } => {
    if (props.state === 'unavailable') {
      return {
        heading: i18n.tr('Målenøkkeltall utilgjengelig', 'Measurements unavailable'),
        body: i18n.tr('Målelaget svarte ikke. Ingen tall vises i stedet for plassholdere.', 'The measurement layer did not respond. No numbers are shown rather than placeholders.'),
      }
    }
    return {
      heading: i18n.tr('Ingen målte hendelser i dette vinduet', 'No measured events in this window'),
      body: i18n.tr(`Insight Core har ikke registrert ${label()}-hendelser for denne visningen og tidsperioden ennå.`, `Insight Core has not recorded ${label()} events for this view and time range yet.`),
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
  const displayValue = () => {
    const value = Number.isInteger(props.scorecard.value) ? props.scorecard.value.toString() : props.scorecard.value.toFixed(2)
    return props.scorecard.unit ? `${value} ${props.scorecard.unit}` : value
  }
  return (
    <li class="verevon-insights-metric-card">
      <div class="verevon-insights-metric-card__topline">
        <span>{props.scorecard.label || props.scorecard.metric}</span>
        <small>{props.scorecard.surface}</small>
      </div>
      <strong class="verevon-insights-metric-card__value">{displayValue()}</strong>
      <Show when={props.scorecard.source}>
        <p class="verevon-insights-metric-card__source">{i18n.tr(`Kilde: ${props.scorecard.source}`, `Source: ${props.scorecard.source}`)}</p>
      </Show>
    </li>
  )
}

function ConnectorRegistrySection(props: { registry: InsightsWorkspace['insightConnectors']; section: InsightsSection }) {
  const i18n = useI18n()
  const surface = createMemo(() => surfaceForInsightsSection(props.section))
  const connectors = createMemo(() => surface()
    ? props.registry.data.filter((connector) => connector.kind === surface())
    : props.registry.data,
  )
  return (
    <section class="verevon-insights-connectors" aria-label={i18n.tr('Innsikt-tilkoblingsregister', 'Insight connector registry')}>
      <div class="verevon-insights-section-heading">
        <div>
          <span><PlugZap size={14} />{i18n.tr('Kildestatus', 'Source status')}</span>
          <h2>{i18n.tr('Tilkoblede målekontrakter', 'Connected measurement contracts')}</h2>
        </div>
        <p>{props.registry.message}</p>
      </div>
      <p class={cn('verevon-insights-source', `verevon-insights-source--${stateClass(props.registry.state)}`)}>
        <PlugZap size={16} />
        <strong>{stateLabel(props.registry.state, i18n.tr)}</strong>
        {connectors().length
          ? i18n.tr(`${connectors().length} kilde${connectors().length === 1 ? '' : 'r'} registrert for denne visningen.`, `${connectors().length} source${connectors().length === 1 ? '' : 's'} registered for this view.`)
          : i18n.tr('Ingen kilde er registrert for denne visningen ennå.', 'No source is registered for this view yet.')}
      </p>
      <Show when={connectors().length} fallback={<div class="verevon-insights-empty"><CircleDashed size={20} /><div><h3>{i18n.tr('Ingen kilde registrert', 'No source registered')}</h3><p>{i18n.tr('Koble til eller aktiver en autoritativ produsent før denne visningen kan rapportere.', 'Connect or enable an authoritative producer before this view can report.')}</p></div></div>}>
        <ul class="verevon-insights-connector-grid">
          <For each={connectors()}>{(connector) => <ConnectorCard connector={connector} />}</For>
        </ul>
      </Show>
    </section>
  )
}

function ConnectorCard(props: { connector: InsightConnector }) {
  const i18n = useI18n()
  return (
    <li class={cn('verevon-insights-connector-card', `verevon-insights-connector-card--${connectorStateClass(props.connector.status)}`)}>
      <div class="verevon-insights-connector-card__topline"><span>{props.connector.label}</span><small>{props.connector.status}</small></div>
      <dl class="verevon-insights-connector-card__meta">
        <div><dt>{i18n.tr('Flate', 'Surface')}</dt><dd>{props.connector.kind}</dd></div>
        <div><dt>{i18n.tr('Kilde', 'Source')}</dt><dd>{props.connector.id}</dd></div>
      </dl>
    </li>
  )
}

function stateLabel(state: MeasurementState, tr: TrFn): string {
  switch (state) {
    case 'live': return tr('Live', 'Live')
    case 'empty': return tr('Ingen rader', 'No rows')
    case 'not_connected': return tr('Ikke tilkoblet', 'Not connected')
    case 'planned': return tr('Avventer produsent', 'Producer pending')
    case 'unavailable': return tr('Utilgjengelig', 'Unavailable')
  }
}

function stateClass(state: MeasurementState): string { return state.replace(/_/g, '-') }

function connectorStateClass(status: string): string {
  switch (status) {
    case 'native':
    case 'connected': return 'connected'
    case 'planned':
    case 'requires_token_lease': return 'planned'
    case 'not_connected':
    case 'needs_oauth': return 'not-connected'
    case 'unavailable': return 'unavailable'
    default: return 'neutral'
  }
}
