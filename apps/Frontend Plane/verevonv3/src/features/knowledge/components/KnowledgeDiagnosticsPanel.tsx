import { For, Show } from 'solid-js'
import {
  type LiveKnowledgeDiagnosticItem,
  type LiveKnowledgeDiagnosticTone,
  type LiveKnowledgeDiagnostics,
  type LiveKnowledgePayload,
} from '@/shared/api/knowledge-live-client'
import { cn } from '@/shared/lib/cn'

const toneBadgeClass: Record<LiveKnowledgeDiagnosticTone, string> = {
  bad: 'knowledge-diagnostic-badge--bad',
  good: 'knowledge-diagnostic-badge--good',
  neutral: 'knowledge-diagnostic-badge--neutral',
  warn: 'knowledge-diagnostic-badge--warn',
}

const toneBorderClass: Record<LiveKnowledgeDiagnosticTone, string> = {
  bad: 'knowledge-diagnostic-card--bad',
  good: 'knowledge-diagnostic-card--good',
  neutral: 'knowledge-diagnostic-card--neutral',
  warn: 'knowledge-diagnostic-card--warn',
}

export function KnowledgeDiagnosticsPanel(props: {
  dataPlane: LiveKnowledgePayload['dataPlane']
  diagnostics?: LiveKnowledgeDiagnostics | null
}) {
  const safeDiagnostics = () => props.diagnostics ?? {
    available: false,
    sparseBackend: null,
    vectorCollections: [],
    quickwitIndexes: [],
    services: [],
    storage: [],
    capabilities: [],
  }

  return (
    <section>
      <SectionHeader
        title="Data Plane status"
        description="Live storage, retrieval, embedding, graph, and wiki runtime state from Data Plane v2."
      />

      <div class="verevon-panel knowledge-diagnostics-panel">
        <div class="knowledge-summary-chip-row">
          <SummaryChip label="Documents" value={`${formatCount(props.dataPlane.documentCount)} live`} />
          <SummaryChip label="Indexed" value={`${formatCount(props.dataPlane.indexedCount)} ready`} />
          <SummaryChip label="Sparse backend" value={safeDiagnostics().sparseBackend || 'unknown'} />
          <SummaryChip label="Vectors" value={`${formatCount(safeDiagnostics().vectorCollections.length)} collections`} />
          <SummaryChip label="Quickwit" value={`${formatCount(safeDiagnostics().quickwitIndexes.length)} indexes`} />
        </div>

        <div class="knowledge-diagnostics-grid">
          <DiagnosticsGroup title="Runtime services" items={safeDiagnostics().services} />
          <DiagnosticsGroup title="Storage + retrieval backends" items={safeDiagnostics().storage} />
        </div>

        <div class="knowledge-diagnostics-capabilities">
          <h3>Capabilities</h3>
          <p>
            This distinguishes what is live now from what is only defined in code or still absent in Data Plane v2.
          </p>
          <div class="knowledge-capability-grid">
            <For each={safeDiagnostics().capabilities}>
              {(item) => <DiagnosticCard item={item} compact />}
            </For>
          </div>
        </div>
      </div>
    </section>
  )
}

function DiagnosticsGroup(props: {
  items: LiveKnowledgeDiagnosticItem[]
  title: string
}) {
  return (
    <div class="knowledge-diagnostics-group">
      <h3>{props.title}</h3>
      <Show
        when={props.items.length > 0}
        fallback={<p class="knowledge-muted-copy">No live diagnostics were returned for this group yet.</p>}
      >
        <div class="knowledge-diagnostic-card-list">
          <For each={props.items}>
            {(item) => <DiagnosticCard item={item} />}
          </For>
        </div>
      </Show>
    </div>
  )
}

function DiagnosticCard(props: {
  compact?: boolean
  item: LiveKnowledgeDiagnosticItem
}) {
  return (
    <article class={cn('knowledge-diagnostic-card', toneBorderClass[props.item.tone], props.compact && 'knowledge-diagnostic-card--compact')}>
      <div class="knowledge-diagnostic-card__heading">
        <div>
          <h4>{props.item.label}</h4>
          <Show when={props.item.meta}>
            <p>{props.item.meta}</p>
          </Show>
        </div>
        <span class={cn('knowledge-diagnostic-badge', toneBadgeClass[props.item.tone])}>
          {props.item.status}
        </span>
      </div>
      <p class="knowledge-diagnostic-detail">{props.item.detail}</p>
    </article>
  )
}

function SectionHeader(props: { title: string; description: string }) {
  return (
    <div class="knowledge-section-header">
      <h2>{props.title}</h2>
      <p>{props.description}</p>
    </div>
  )
}

function SummaryChip(props: { label: string; value: string }) {
  return (
    <span class="knowledge-summary-chip">
      {props.label}: {props.value}
    </span>
  )
}

function formatCount(value: number): string {
  return new Intl.NumberFormat('en-US').format(Math.max(0, Math.trunc(value)))
}
