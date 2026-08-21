import {
  ChevronRight,
  FileCode2,
  Link2,
  ListChecks,
  MessageSquare,
  MessageSquarePlus,
  RefreshCw,
  Sparkles,
  Square,
} from '@/shared/icons'
import {
  For,
  Show,
  createMemo,
  createSignal,
} from 'solid-js'
import type { JSX } from '@solidjs/web'
import {
  TaskStep,
} from './ChatMessages'
import {
  imageArtifactSrc,
} from './chat-artifacts'
import {
  formatTime,
  groupTaskSteps,
  hostname,
} from './chat-media-markdown'
import {
  type AgentTaskStep,
  type ChatArtifact,
  type ChatGroundingGraph,
  type ChatGroundingSource,
  type ChatKnowledgeGrounding,
  type ChatTab,
  type Citation,
  type EvidenceSource,
  type IconComponent,
} from './chat-types'

export function ChatHeader(props: {
  branchCount: number
  messageCount: number
  title: string
  onNewChat: () => void
  onRegenerate: () => void
}) {
  return (
    <header class="verevon-chat-header">
      <div class="verevon-chat-header__copy">
        <h1>{props.title}</h1>
        <p>{props.messageCount} messages · {props.branchCount} regenerations</p>
      </div>
      <div class="verevon-chat-header__actions">
        <button type="button" class="verevon-chat-header-button" aria-label="Regenerate latest response" onClick={() => props.onRegenerate()}>
          <RefreshCw size={15} />
        </button>
        <button type="button" class="verevon-chat-header-button verevon-chat-header-button--primary" aria-label="New chat" onClick={() => props.onNewChat()}>
          <MessageSquarePlus size={15} />
        </button>
      </div>
    </header>
  )
}

export function ChatTabs(props: {
  active: ChatTab
  artifactCount: number
  sourceCount: number
  stepCount: number
  onChange: (tab: ChatTab) => void
}) {
  const tabs = (): Array<{ id: ChatTab; label: string; icon: IconComponent; count: number }> => [
    { id: 'chat', label: 'Chat', icon: MessageSquare, count: 0 },
    { id: 'sources', label: 'Kilder', icon: Link2, count: props.sourceCount },
    { id: 'artifacts', label: 'Artefakter', icon: FileCode2, count: props.artifactCount },
    { id: 'steps', label: 'Steg', icon: ListChecks, count: props.stepCount },
  ]

  return (
    <div class="verevon-chat-tabs" role="tablist" aria-label="Chat workspace views">
      <For each={tabs()}>
        {(item) => {
          const Icon = item.icon
          const selected = () => props.active === item.id
          return (
            <button
              type="button"
              role="tab"
              aria-selected={selected() ? 'true' : 'false'}
              class={{ 'verevon-chat-tab': true, 'verevon-chat-tab--active': selected() }}
              onClick={() => props.onChange(item.id)}
            >
              <Icon size={14} />
              <span>{item.label}</span>
              <Show when={item.count > 0}>
                <em>{item.count}</em>
              </Show>
            </button>
          )
        }}
      </For>
    </div>
  )
}

export function EmptyPanel(props: { icon: JSX.Element; title: string; subtitle: string }) {
  return (
    <div class="verevon-chat-empty-panel">
      <div>
        <span class="verevon-chat-empty-panel__icon">{props.icon}</span>
        <h2>{props.title}</h2>
        <p>{props.subtitle}</p>
      </div>
    </div>
  )
}

export function SourcesPanel(props: { grounding?: ChatKnowledgeGrounding | null; sources: EvidenceSource[] }) {
  return (
    <Show
      when={props.sources.length > 0 || props.grounding}
      fallback={(
        <EmptyPanel
          icon={<Link2 size={20} />}
          title="Ingen kilder ennå"
          subtitle="Interne kunnskapskilder og websøk dukker opp her når Verevon bruker dem i svaret."
        />
      )}
    >
      <div class="verevon-chat-panel">
        <div class="verevon-chat-panel__inner">
          <Show when={props.grounding}>
            {(grounding) => <GroundingOverviewCard grounding={grounding()} />}
          </Show>
          <For each={props.sources}>
            {(source, index) => (
              source.kind === 'knowledge'
                ? <KnowledgeSourceCard source={source} index={index() + 1} />
                : <WebSourceCard source={source} index={index() + 1} />
            )}
          </For>
        </div>
      </div>
    </Show>
  )
}

export function GroundingOverviewCard(props: { grounding: ChatKnowledgeGrounding }) {
  return (
    <section class="verevon-chat-source-summary">
      <div class="verevon-chat-source-summary__badges">
        <span><Sparkles size={14} /> Internal knowledge grounding</span>
        <Show when={props.grounding.lowConfidence}>
          <em>Low confidence</em>
        </Show>
      </div>
      <div class="verevon-chat-source-summary__metrics">
        <Metric label="Sources" value={String(props.grounding.sourceCount)} />
        <Metric label="Facts" value={String(props.grounding.factCount)} />
        <Metric label="Graph nodes" value={String(props.grounding.graph?.nodes.length ?? 0)} />
      </div>
      <Show when={props.grounding.graph}>
        {(graph) => <GroundingGraphSummary graph={graph()} traceId={props.grounding.traceId} />}
      </Show>
    </section>
  )
}

export function Metric(props: { label: string; value: string }) {
  return (
    <div class="verevon-chat-metric">
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </div>
  )
}

export function GroundingGraphSummary(props: { compact?: boolean; graph: ChatGroundingGraph; traceId?: string }) {
  return (
    <div class={{ 'verevon-chat-graph-summary': true, 'verevon-chat-graph-summary--compact': Boolean(props.compact) }}>
      <div>
        <strong>Graph evidence</strong>
        <Show when={props.traceId}><span>Trace {props.traceId}</span></Show>
      </div>
      <For each={props.graph.communitySummaries}>
        {(summary) => <p>{summary}</p>}
      </For>
      <Show when={props.graph.nodes.length > 0}>
        <div class="verevon-chat-graph-summary__nodes">
          <For each={props.graph.nodes}>
            {(node) => <span>{node.label}</span>}
          </For>
        </div>
      </Show>
    </div>
  )
}

export function KnowledgeSourceCard(props: { source: ChatGroundingSource; index: number }) {
  return (
    <article class="verevon-chat-source-card">
      <div class="verevon-chat-source-card__meta">
        <span>{props.index}</span>
        <em>{props.source.provider} · {props.source.sourceType}</em>
        <strong>Score {props.source.score.toFixed(2)}</strong>
      </div>
      <h2>{props.source.title}</h2>
      <p>{props.source.snippet}</p>
      <a href={props.source.href}>Open knowledge <ChevronRight size={14} /></a>
    </article>
  )
}

export function WebSourceCard(props: { source: Citation & { kind: 'web' }; index: number }) {
  return (
    <a class="verevon-chat-source-card" href={props.source.url} target="_blank" rel="noopener noreferrer">
      <div class="verevon-chat-source-card__meta">
        <span>{props.index}</span>
        <em>{hostname(props.source.url)}</em>
      </div>
      <h2>{props.source.title || props.source.url}</h2>
      <Show when={props.source.snippet}><p>{props.source.snippet}</p></Show>
    </a>
  )
}

export function StepsPanel(props: { steps: AgentTaskStep[]; screen?: ChatArtifact | null; onStopTask: () => void }) {
  const activeTask = () => props.steps.some((step) => step.status === 'active' || step.status === 'waiting')
  const sections = createMemo(() => groupTaskSteps(props.steps))
  const [collapsedSections, setCollapsedSections] = createSignal<Set<string>>(new Set())
  const isCollapsed = (id: string) => collapsedSections().has(id)
  const toggleSection = (id: string) => {
    setCollapsedSections((current) => {
      const next = new Set(current)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  return (
    <Show
      when={props.steps.length > 0 || props.screen}
      fallback={(
        <EmptyPanel
          icon={<ListChecks size={20} />}
          title="Ingen steg ennå"
          subtitle="Agentens arbeidssteg vises her mens en oppgave kjører."
        />
      )}
    >
      <div class="verevon-chat-panel">
        <div class="verevon-chat-panel__inner">
          <div class="verevon-chat-steps-header">
            <div>
              <h2>Agent activity</h2>
              <p>Live oppgavestatus</p>
            </div>
            <button type="button" disabled={!activeTask()} onClick={() => props.onStopTask()}>
              <Square size={12} />
              Stopp
            </button>
          </div>
          <Show when={props.screen}>
            {(screen) => (
              <figure class="verevon-chat-agent-screen">
                <img src={imageArtifactSrc(screen().content)} alt={screen().title || 'Agent screen'} />
                <figcaption>{screen().title || 'Live screen'}</figcaption>
              </figure>
            )}
          </Show>
          <div class="verevon-chat-step-groups">
            <For each={sections()}>
              {(section) => (
                <section class={{ 'verevon-chat-step-group': true, 'is-collapsed': isCollapsed(section.id) }}>
                  <button
                    type="button"
                    class="verevon-chat-step-group__header"
                    aria-expanded={!isCollapsed(section.id) ? 'true' : 'false'}
                    onClick={() => toggleSection(section.id)}
                  >
                    <span class="verevon-chat-step-group__title">
                      <ChevronRight size={14} class={{ 'verevon-chat-rotate': !isCollapsed(section.id) }} />
                      <h3>{section.title}</h3>
                    </span>
                    <span class="verevon-chat-step-group__meta">
                      <em>{section.steps.length}</em>
                      <time>{formatTime(section.createdAt)}</time>
                    </span>
                  </button>
                  <Show when={!isCollapsed(section.id)}>
                    <div class="verevon-chat-step-list">
                      <For each={section.steps}>
                        {(step, index) => <TaskStep step={step} isLast={index() === section.steps.length - 1} />}
                      </For>
                    </div>
                  </Show>
                </section>
              )}
            </For>
          </div>
        </div>
      </div>
    </Show>
  )
}
