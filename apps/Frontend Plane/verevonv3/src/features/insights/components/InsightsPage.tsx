import { A } from '@solidjs/router'
import {
  BarChart3,
  Bot,
  CircleDashed,
  FlaskConical,
  Gauge,
  Inbox,
  LineChart,
  Megaphone,
  PlugZap,
  Search,
  TrendingUp,
  type LucideProps,
} from 'lucide-solid'
import { createMemo, createResource, For, Show, type Component } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import { buildHeadlineMetrics, type HeadlineMetric } from '@/features/insights/lib/insights-headline-metrics'
import {
  loadInsightsWorkspace,
  type ExternalAnalyticsSlot,
  type InsightsWorkspace,
  type MeasurementState,
} from '@/features/insights/lib/insights-workspace'
import type { InsightConnector, InsightScorecard } from '@/shared/api/insights-client'
import { cn } from '@/shared/lib/cn'
import { useI18n } from '@/shared/i18n'

type TrFn = (noText: string, enText: string) => string

// `section` exists only to drive the static header copy and active-tab styling.
// It NEVER changes which numbers render, because there are no live metrics to
// scope per-section — only the real connector registry and an explicit
// "not yet reporting" empty-state.
export type InsightsSection = 'overview' | 'social' | 'inbox' | 'agents' | 'campaigns' | 'experiments'

type SurfaceState = MeasurementState | 'connected_pending_reports'

type SectionConfig = {
  description: string
  icon: Component<LucideProps>
  id: InsightsSection
  label: string
  title: string
}

function sectionConfigs(tr: TrFn): SectionConfig[] {
  return [
    {
      id: 'overview',
      label: tr('Oversikt', 'Overview'),
      title: tr('Oversikt', 'Overview'),
      description: tr('Målelaget viser det live tilkoblingsregisteret. Nøkkeltall forblir tomme til en tilkobling begynner å rapportere.', 'The measurement layer surfaces the live connector registry. Metrics stay empty until a connector starts reporting.'),
      icon: BarChart3,
    },
    {
      id: 'social',
      label: tr('Sosialt', 'Social'),
      title: tr('Sosial måling', 'Social measurement'),
      description: tr('Beredskap for sosiale tilkoblinger vises gjennom registeret. Ingen sosiale nøkkeltall er rapportert ennå.', 'Social connector readiness is shown through the registry. No social metrics are reported yet.'),
      icon: TrendingUp,
    },
    {
      id: 'inbox',
      label: tr('Innboks', 'Inbox'),
      title: tr('Innboksmåling', 'Inbox measurement'),
      description: tr('Beredskap for innbokstilkoblinger vises gjennom registeret. Ingen samtalenøkkeltall er rapportert ennå.', 'Inbox connector readiness is shown through the registry. No conversation metrics are reported yet.'),
      icon: Inbox,
    },
    {
      id: 'agents',
      label: tr('Agenter', 'Agents'),
      title: tr('Agentmåling', 'Agent measurement'),
      description: tr('Beredskap for agenttilkoblinger vises gjennom registeret. Ingen kjøreanalyse er rapportert ennå.', 'Agent connector readiness is shown through the registry. No run analytics are reported yet.'),
      icon: Bot,
    },
    {
      id: 'campaigns',
      label: tr('Kampanjer', 'Campaigns'),
      title: tr('Kampanjemåling', 'Campaign measurement'),
      description: tr('Kampanjeattribusjon har ingen backend-kontrakt ennå, så ingen kampanjenøkkeltall rapporteres.', 'Campaign attribution has no backend contract yet, so no campaign metrics are reported.'),
      icon: Megaphone,
    },
    {
      id: 'experiments',
      label: tr('Eksperimenter', 'Experiments'),
      title: tr('Eksperimentmåling', 'Experiment measurement'),
      description: tr('Ingen eksperimentkontrakt er eksponert ennå, så ingen eksperimentnøkkeltall rapporteres.', 'No experiment contract is exposed yet, so no experiment metrics are reported.'),
      icon: FlaskConical,
    },
  ]
}

export default function InsightsPage(props: { section?: InsightsSection }) {
  const i18n = useI18n()
  const [workspace] = createResource(loadInsightsWorkspace)
  const activeSection = createMemo(() => props.section ?? 'overview')
  const sections = createMemo(() => sectionConfigs(i18n.tr))
  const config = createMemo(() =>
    sections().find((section) => section.id === activeSection()) ?? sections()[0]!,
  )

  return (
    <div class="verevon-insights-page">
      <section class="verevon-insights-hero">
        <div>
          <span>
            <Dynamic component={config().icon} size={15} />
            {config().title}
          </span>
          <h1>{i18n.tr('Verevon målelag', 'Verevon measurement layer')}</h1>
          <p>{config().description}</p>
        </div>
        <A href="/settings/integrations">{i18n.tr('Administrer tilkoblinger', 'Manage connectors')}</A>
      </section>

      <nav class="verevon-insights-tabs" aria-label={i18n.tr('Innsikt-seksjoner', 'Insights sections')}>
        <For each={sections()}>
          {(section) => (
            <A
              class={cn('verevon-insights-tab', section.id === activeSection() && 'is-active')}
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
            <Show when={activeSection() === 'overview'}>
              <HeadlineMetricsSection overview={loadedWorkspace().overview} />
            </Show>
            <ConnectorRegistrySection registry={loadedWorkspace().insightConnectors} />
            <MetricsSection overview={loadedWorkspace().overview} />
            <ExternalAnalyticsSection slots={loadedWorkspace().externalAnalytics} />
          </>
        )}
      </Show>
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
        <p>{i18n.tr('Sjekker organisasjonsscope og insight-core-tilkoblingsregisteret.', 'Checking org scope and the insight-core connector registry.')}</p>
      </div>
    </section>
  )
}

// The pilot's "three honest numbers on one page" (see verevon-feature-map.md
// 1.9's done-enough gate): conversations handled, AI draft acceptance %, and
// cost per resolution. Each tile resolves its OWN state from real recorded
// data — `live` never attaches to an unproduced or estimated value, and a
// metric with no backend data path (cost per resolution) renders its honest
// `planned` empty state rather than a guess.
function HeadlineMetricsSection(props: { overview: InsightsWorkspace['overview'] }) {
  const i18n = useI18n()
  const metrics = createMemo(() => buildHeadlineMetrics(props.overview))

  return (
    <section class="verevon-insights-headline" aria-label={i18n.tr('Pilotens hovedtall', 'Pilot headline metrics')}>
      <div class="verevon-insights-section-heading">
        <div>
          <span>
            <Gauge size={14} />
            {i18n.tr('Pilot-poengkort', 'Pilot scorecard')}
          </span>
          <h2>{i18n.tr('De tre ærlige tallene', 'The three honest numbers')}</h2>
        </div>
        <p>{i18n.tr('Hvert tall beregnes fra ekte registrerte rader eller viser en eksplisitt ærlig-tom tilstand — aldri en plassholder.', 'Each number is computed from real recorded rows or shows an explicit honest-empty state — never a placeholder.')}</p>
      </div>

      <ul class="verevon-insights-headline-grid">
        <For each={metrics()}>
          {(metric) => <HeadlineMetricCard metric={metric} />}
        </For>
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
        <small>{headlineStateLabel(props.metric.state, i18n.tr)}</small>
      </div>
      <strong class="verevon-insights-headline-card__value">{props.metric.value}</strong>
      <p class="verevon-insights-headline-card__detail">{props.metric.detail}</p>
    </li>
  )
}

function headlineStateLabel(state: MeasurementState, tr: TrFn): string {
  switch (state) {
    case 'live':
      return tr('Live', 'Live')
    case 'empty':
      return tr('Rapporterer ikke ennå', 'Not yet reporting')
    case 'planned':
      return tr('Ikke bygget ennå', 'Not built yet')
    case 'not_connected':
      return tr('Ikke tilkoblet', 'Not connected')
    case 'unavailable':
      return tr('Utilgjengelig', 'Unavailable')
  }
}

function ConnectorRegistrySection(props: { registry: InsightsWorkspace['insightConnectors'] }) {
  const i18n = useI18n()
  return (
    <section class="verevon-insights-connectors" aria-label={i18n.tr('Innsikt-tilkoblingsregister', 'Insight connector registry')}>
      <div class="verevon-insights-section-heading">
        <div>
          <span>
            <PlugZap size={14} />
            {i18n.tr('Tilkoblingsregister', 'Connector registry')}
          </span>
          <h2>{i18n.tr('Kilder insight-core kan rapportere fra', 'Sources insight-core can report from')}</h2>
        </div>
        <p>{props.registry.message}</p>
      </div>

      <p class={cn('verevon-insights-source', `verevon-insights-source--${stateClass(props.registry.state)}`)}>
        <PlugZap size={16} />
        <strong>{stateLabel(props.registry.state, i18n.tr)}</strong>
        {props.registry.data.length
          ? i18n.tr(`${props.registry.data.length} tilkobling${props.registry.data.length === 1 ? '' : 'er'} registrert.`, `${props.registry.data.length} connector${props.registry.data.length === 1 ? '' : 's'} registered.`)
          : i18n.tr('Ingen tilkoblinger er registrert for denne organisasjonen ennå.', 'No connectors are registered for this org yet.')}
      </p>

      <Show
        when={props.registry.data.length}
        fallback={
          <div class="verevon-insights-empty" aria-live="polite">
            <CircleDashed size={20} />
            <div>
              <h3>{i18n.tr('Ingen tilkoblinger registrert', 'No connectors registered')}</h3>
              <p>{i18n.tr('Koble til en kilde for å fylle insight-core-tilkoblingsregisteret.', 'Connect a source to populate the insight-core connector registry.')}</p>
            </div>
          </div>
        }
      >
        <ul class="verevon-insights-connector-grid">
          <For each={props.registry.data}>
            {(connector) => <ConnectorCard connector={connector} />}
          </For>
        </ul>
      </Show>
    </section>
  )
}

function ConnectorCard(props: { connector: InsightConnector }) {
  const i18n = useI18n()
  return (
    <li class={cn('verevon-insights-connector-card', `verevon-insights-connector-card--${connectorStateClass(props.connector.status)}`)}>
      <div class="verevon-insights-connector-card__topline">
        <span>{props.connector.label}</span>
        <small>{props.connector.status}</small>
      </div>
      <dl class="verevon-insights-connector-card__meta">
        <div>
          <dt>{i18n.tr('Type', 'Kind')}</dt>
          <dd>{props.connector.kind}</dd>
        </div>
        <div>
          <dt>{i18n.tr('Id', 'ID')}</dt>
          <dd>{props.connector.id}</dd>
        </div>
      </dl>
    </li>
  )
}

function MetricsSection(props: { overview: InsightsWorkspace['overview'] }) {
  const i18n = useI18n()
  const state = () => props.overview.state

  return (
    <section class="verevon-insights-metrics" aria-label={i18n.tr('Målenøkkeltall', 'Measurement metrics')}>
      <div class="verevon-insights-section-heading">
        <div>
          <span>
            <BarChart3 size={14} />
            {i18n.tr('Nøkkeltall', 'Metrics')}
          </span>
          <h2>{i18n.tr('Rapportering', 'Reporting')}</h2>
        </div>
        <p>{props.overview.message}</p>
      </div>

      <Show
        when={state() === 'live'}
        fallback={<MetricsHonestState state={state()} />}
      >
        <ul class="verevon-insights-metrics-grid">
          <For each={props.overview.data}>
            {(scorecard) => <ScorecardCard scorecard={scorecard} />}
          </For>
        </ul>
      </Show>
    </section>
  )
}

// The honest non-live states. Each renders an explicit empty/unavailable card —
// NEVER a placeholder number. `empty` means the measurement layer is live but no
// connector has reported yet; `unavailable` means the contract did not respond.
function MetricsHonestState(props: { state: MeasurementState }) {
  const i18n = useI18n()
  const copy = (): { body: string; heading: string } => {
    switch (props.state) {
      case 'unavailable':
        return {
          heading: i18n.tr('Nøkkeltalloversikt utilgjengelig', 'Metric overview unavailable'),
          body: i18n.tr('Målelaget svarte ikke. Ingen tall vises i stedet for plassholdere.', 'The measurement layer did not respond. No numbers are shown rather than placeholders.'),
        }
      case 'planned':
        return {
          heading: i18n.tr('Nøkkeltallprodusent ikke bygget ennå', 'Metric producer not built yet'),
          body: i18n.tr('Denne målekilden er planlagt. Ingen tall vises før den rapporterer.', 'This measurement source is planned. No numbers are shown until it reports.'),
        }
      default:
        return {
          heading: i18n.tr('Rapporterer ikke ennå — koble til en kilde', 'Not yet reporting — connect a source'),
          body: i18n.tr('Nøkkeltall vises når en tilkobling begynner å rapportere. Tilkoblingsregisteret ovenfor er ekte; ingen plassholdertall vises her.', 'Metrics will appear once a connector starts reporting. The connector registry above is real; no placeholder numbers are shown here.'),
        }
    }
  }

  return (
    <div class="verevon-insights-empty" aria-live="polite">
      <CircleDashed size={20} />
      <div>
        <h3>{copy().heading}</h3>
        <p>{copy().body}</p>
      </div>
    </div>
  )
}

function ScorecardCard(props: { scorecard: InsightScorecard }) {
  const i18n = useI18n()
  const displayValue = () => {
    const value = props.scorecard.value
    const formatted = Number.isInteger(value) ? value.toString() : value.toFixed(2)
    return props.scorecard.unit ? `${formatted} ${props.scorecard.unit}` : formatted
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

function ExternalAnalyticsSection(props: { slots: ExternalAnalyticsSlot[] }) {
  const i18n = useI18n()
  return (
    <section class="verevon-insights-external" aria-label={i18n.tr('Eksterne analysetilkoblinger', 'External analytics connectors')}>
      <div class="verevon-insights-section-heading">
        <div>
          <span>
            <PlugZap size={14} />
            {i18n.tr('Ekstern analyse', 'External analytics')}
          </span>
          <h2>{i18n.tr('Google Analytics- og SEO-plasser', 'Google Analytics and SEO slots')}</h2>
        </div>
        <p>{i18n.tr('Tilkoblingsstatus vises separat fra rapporttilgjengelighet, slik at grensesnittet aldri viser plassholderanalyse som live rader.', 'Connector state is shown separately from report availability so the UI never presents placeholder analytics as live rows.')}</p>
      </div>

      <div class="verevon-insights-external-grid">
        <For each={props.slots}>
          {(slot) => <ExternalSlotCard slot={slot} />}
        </For>
      </div>
    </section>
  )
}

function ExternalSlotCard(props: { slot: ExternalAnalyticsSlot }) {
  const i18n = useI18n()
  const Icon = () => props.slot.connectorId === 'ga4' ? LineChart : Search
  const conceptSentence = () => props.slot.connectorId === 'ga4'
    ? i18n.tr('Tilsvarer GA4 Data API-rapportbegreper: dimensjoner, nøkkeltall, datointervaller og rader.', 'Maps to GA4 Data API report concepts: dimensions, metrics, date ranges, and rows.')
    : i18n.tr('Tilsvarer Search Console-begreper: søk, sider, klikk, visninger, CTR og posisjon.', 'Maps to Search Console concepts: queries, pages, clicks, impressions, CTR, and position.')

  return (
    <article class={cn('verevon-insights-external-card', `verevon-insights-card--${stateClass(props.slot.status)}`)}>
      <header>
        <span class="verevon-insights-external-card__icon">
          <Dynamic component={Icon()} size={17} />
        </span>
        <div>
          <h3>{props.slot.title}</h3>
          <small>{props.slot.statusLabel}</small>
        </div>
      </header>
      <p>{props.slot.detail}</p>
      <p>{conceptSentence()}</p>

      <div class="verevon-insights-report-shape">
        <ReportConcepts title={i18n.tr('Forespørsel', 'Request')} items={props.slot.reportConcepts} />
        <ReportConcepts title={i18n.tr('Svar', 'Response')} items={props.slot.responseConcepts} />
        <ReportConcepts title={i18n.tr('Nøkkeltall', 'Metrics')} items={props.slot.metricConcepts} />
      </div>

      <a href={props.slot.docsHref} target="_blank" rel="noreferrer">{i18n.tr('Åpne offisiell API-referanse', 'Open official API reference')}</a>
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

function stateLabel(state: SurfaceState, tr: TrFn): string {
  switch (state) {
    case 'connected_pending_reports':
      return tr('Tilkoblet, rapporter venter', 'Connected, reports pending')
    case 'empty':
      return tr('Tom', 'Empty')
    case 'live':
      return tr('Live', 'Live')
    case 'not_connected':
      return tr('Ikke tilkoblet', 'Not connected')
    case 'planned':
      return tr('Backend-avhengighet', 'Backend dependency')
    case 'unavailable':
      return tr('Utilgjengelig', 'Unavailable')
  }
}

function stateClass(state: SurfaceState): string {
  return state.replace(/_/g, '-')
}

// Connector `status` is a free-form upstream string. Map the few states with a
// dedicated style; everything else falls back to a neutral class.
function connectorStateClass(status: string): string {
  switch (status) {
    case 'native':
    case 'connected':
      return 'connected'
    case 'planned':
      return 'planned'
    case 'not_connected':
    case 'needs_oauth':
      return 'not-connected'
    case 'unavailable':
      return 'unavailable'
    default:
      return 'neutral'
  }
}
