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
import {
  loadInsightsWorkspace,
  type ExternalAnalyticsSlot,
  type InsightsWorkspace,
  type MeasurementState,
} from '@/features/insights/lib/insights-workspace'
import type { InsightConnector, InsightScorecard } from '@/shared/api/insights-client'
import { cn } from '@/shared/lib/cn'

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

const sectionConfigs: SectionConfig[] = [
  {
    id: 'overview',
    label: 'Overview',
    title: 'Overview',
    description: 'The measurement layer surfaces the live connector registry. Metrics stay empty until a connector starts reporting.',
    icon: BarChart3,
  },
  {
    id: 'social',
    label: 'Social',
    title: 'Social measurement',
    description: 'Social connector readiness is shown through the registry. No social metrics are reported yet.',
    icon: TrendingUp,
  },
  {
    id: 'inbox',
    label: 'Inbox',
    title: 'Inbox measurement',
    description: 'Inbox connector readiness is shown through the registry. No conversation metrics are reported yet.',
    icon: Inbox,
  },
  {
    id: 'agents',
    label: 'Agents',
    title: 'Agent measurement',
    description: 'Agent connector readiness is shown through the registry. No run analytics are reported yet.',
    icon: Bot,
  },
  {
    id: 'campaigns',
    label: 'Campaigns',
    title: 'Campaign measurement',
    description: 'Campaign attribution has no backend contract yet, so no campaign metrics are reported.',
    icon: Megaphone,
  },
  {
    id: 'experiments',
    label: 'Experiments',
    title: 'Experiment measurement',
    description: 'No experiment contract is exposed yet, so no experiment metrics are reported.',
    icon: FlaskConical,
  },
]

export default function InsightsPage(props: { section?: InsightsSection }) {
  const [workspace] = createResource(loadInsightsWorkspace)
  const activeSection = createMemo(() => props.section ?? 'overview')
  const config = createMemo(() =>
    sectionConfigs.find((section) => section.id === activeSection()) ?? sectionConfigs[0]!,
  )

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
  return (
    <section class="velion-insights-loading" aria-label="Loading Insights">
      <Gauge size={18} />
      <div>
        <h2>Resolving measurement contracts</h2>
        <p>Checking org scope and the insight-core connector registry.</p>
      </div>
    </section>
  )
}

function ConnectorRegistrySection(props: { registry: InsightsWorkspace['insightConnectors'] }) {
  return (
    <section class="velion-insights-connectors" aria-label="Insight connector registry">
      <div class="velion-insights-section-heading">
        <div>
          <span>
            <PlugZap size={14} />
            Connector registry
          </span>
          <h2>Sources insight-core can report from</h2>
        </div>
        <p>{props.registry.message}</p>
      </div>

      <p class={cn('velion-insights-source', `velion-insights-source--${stateClass(props.registry.state)}`)}>
        <PlugZap size={16} />
        <strong>{stateLabel(props.registry.state)}</strong>
        {props.registry.data.length
          ? `${props.registry.data.length} connector${props.registry.data.length === 1 ? '' : 's'} registered.`
          : 'No connectors are registered for this org yet.'}
      </p>

      <Show
        when={props.registry.data.length}
        fallback={
          <div class="velion-insights-empty" aria-live="polite">
            <CircleDashed size={20} />
            <div>
              <h3>No connectors registered</h3>
              <p>Connect a source to populate the insight-core connector registry.</p>
            </div>
          </div>
        }
      >
        <ul class="velion-insights-connector-grid">
          <For each={props.registry.data}>
            {(connector) => <ConnectorCard connector={connector} />}
          </For>
        </ul>
      </Show>
    </section>
  )
}

function ConnectorCard(props: { connector: InsightConnector }) {
  return (
    <li class={cn('velion-insights-connector-card', `velion-insights-connector-card--${connectorStateClass(props.connector.status)}`)}>
      <div class="velion-insights-connector-card__topline">
        <span>{props.connector.label}</span>
        <small>{props.connector.status}</small>
      </div>
      <dl class="velion-insights-connector-card__meta">
        <div>
          <dt>Kind</dt>
          <dd>{props.connector.kind}</dd>
        </div>
        <div>
          <dt>Id</dt>
          <dd>{props.connector.id}</dd>
        </div>
      </dl>
    </li>
  )
}

function MetricsSection(props: { overview: InsightsWorkspace['overview'] }) {
  const state = () => props.overview.state

  return (
    <section class="velion-insights-metrics" aria-label="Measurement metrics">
      <div class="velion-insights-section-heading">
        <div>
          <span>
            <BarChart3 size={14} />
            Metrics
          </span>
          <h2>Reporting</h2>
        </div>
        <p>{props.overview.message}</p>
      </div>

      <Show
        when={state() === 'live'}
        fallback={<MetricsHonestState state={state()} />}
      >
        <ul class="velion-insights-metrics-grid">
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
  const copy = (): { body: string; heading: string } => {
    switch (props.state) {
      case 'unavailable':
        return {
          heading: 'Metric overview unavailable',
          body: 'The measurement layer did not respond. No numbers are shown rather than placeholders.',
        }
      case 'planned':
        return {
          heading: 'Metric producer not built yet',
          body: 'This measurement source is planned. No numbers are shown until it reports.',
        }
      default:
        return {
          heading: 'Not yet reporting — connect a source',
          body: 'Metrics will appear once a connector starts reporting. The connector registry above is real; no placeholder numbers are shown here.',
        }
    }
  }

  return (
    <div class="velion-insights-empty" aria-live="polite">
      <CircleDashed size={20} />
      <div>
        <h3>{copy().heading}</h3>
        <p>{copy().body}</p>
      </div>
    </div>
  )
}

function ScorecardCard(props: { scorecard: InsightScorecard }) {
  const displayValue = () => {
    const value = props.scorecard.value
    const formatted = Number.isInteger(value) ? value.toString() : value.toFixed(2)
    return props.scorecard.unit ? `${formatted} ${props.scorecard.unit}` : formatted
  }

  return (
    <li class="velion-insights-metric-card">
      <div class="velion-insights-metric-card__topline">
        <span>{props.scorecard.label || props.scorecard.metric}</span>
        <small>{props.scorecard.surface}</small>
      </div>
      <strong class="velion-insights-metric-card__value">{displayValue()}</strong>
      <Show when={props.scorecard.source}>
        <p class="velion-insights-metric-card__source">Source: {props.scorecard.source}</p>
      </Show>
    </li>
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
