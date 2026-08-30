import {
  ChevronRight,
  Link2,
  Layers,
  ListChecks,
  MessageSquarePlus,
  RefreshCw,
  ShieldCheck,
  Sparkles,
  Square,
} from '@/shared/icons'
import type { ThreadContext } from '@/shared/api/chat-client'
import {
  getRunProofBundle,
  type ProofApproval,
  type ProofBundle,
} from '@/shared/api/run-console-client'
import {
  getLineage,
  listPlans,
  listTodos,
  type Plan,
  type Todo,
} from '@/shared/api/orchestration-client'
import { ToolCallCard } from './ChatMessages'
import {
  For,
  Match,
  Show,
  Switch,
  createMemo,
  createSignal,
} from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
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
  type ChatToolCall,
  type ChatGroundingGraph,
  type ChatGroundingSource,
  type ChatKnowledgeGrounding,
  type ChatTab,
  type Citation,
  type EvidenceSource,
  type IconComponent,
} from './chat-types'
import { uiEventLabel, type VerevonUiEvent } from '@/shared/chat/verevon-ui-events'
import { availableChatSurfaces, type ChatSurfaceAvailability } from '../lib/chat-surfaces'

function shortReceipt(value: string) {
  return value.length > 24 ? `${value.slice(0, 12)}…${value.slice(-8)}` : value
}

function cancellationReceipt(event: VerevonUiEvent) {
  return event.type === 'run.cancelled' ? event.receiptId : undefined
}

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
  runAvailable?: boolean
  sourceCount: number
  stepCount: number
  traceAvailable?: boolean
  onChange: (tab: ChatTab) => void
}) {
  const availability = (): ChatSurfaceAvailability => ({
    sourceCount: props.sourceCount,
    hasGrounding: props.sourceCount > 0,
    artifactCount: props.artifactCount,
    attachmentCount: 0,
    stepCount: props.stepCount,
    hasRun: Boolean(props.runAvailable || props.traceAvailable),
  })
  const tabs = (): Array<{ id: ChatTab; label: string; icon: IconComponent; count: number }> => (
    availableChatSurfaces(availability()).map((surface) => ({
      id: surface.id,
      label: surface.label,
      icon: surface.icon,
      count: surface.id === 'sources'
        ? props.sourceCount
        : surface.id === 'artifacts'
          ? props.artifactCount
          : surface.id === 'steps' ? props.stepCount : 0,
    }))
  )

  return (
    <Show when={tabs().length > 1}>
      <div class="verevon-chat-tabs" role="tablist" aria-label="Chat workspace views">
        <For each={tabs()}>
          {(item, index) => {
            const Icon = item.icon
            const selected = () => props.active === item.id
            const focusRelativeTab = (event: KeyboardEvent, offset: number) => {
              const buttons = Array.from(
                event.currentTarget instanceof HTMLElement
                  ? event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="tab"]') ?? []
                  : [],
              )
              if (buttons.length === 0) return
              event.preventDefault()
              const nextIndex = (index() + offset + buttons.length) % buttons.length
              buttons[nextIndex]?.focus()
              const next = tabs()[nextIndex]
              if (next) props.onChange(next.id)
            }
            return (
              <button
                type="button"
                role="tab"
                aria-selected={selected() ? 'true' : 'false'}
                aria-controls={`verevon-chat-tabpanel-${item.id}`}
                tabindex={selected() ? 0 : -1}
                class={{ 'verevon-chat-tab': true, 'verevon-chat-tab--active': selected() }}
                onClick={() => props.onChange(item.id)}
                onKeyDown={(event) => {
                  if (event.key === 'ArrowRight' || event.key === 'ArrowDown') focusRelativeTab(event, 1)
                  else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') focusRelativeTab(event, -1)
                  else if (event.key === 'Home') focusRelativeTab(event, -index())
                  else if (event.key === 'End') focusRelativeTab(event, tabs().length - 1 - index())
                }}
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
    </Show>
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
      <a
        href={props.source.href}
        link={props.source.href.startsWith('/') ? true : undefined}
        aria-label={`Åpne ${props.source.title} i Kunnskap`}
      >
        Open knowledge <ChevronRight size={14} />
      </a>
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

/**
 * Context inspector: what is actually in the model's window for this thread,
 * itemized by segment with per-segment token estimates.
 *
 * The data (`GetContextAssembly`) has existed all along and the gateway already
 * called it to BUILD prompts — it was simply never exposed, so the one surface
 * that could answer "why did it answer from *that*?" was unreachable. Collapsed
 * by default: it is diagnostic, not part of reading an answer.
 *
 * Segment content is shown, not just sizes. "1,200 tokens of grounding" does not
 * answer the question the inspector exists for.
 */
export function ContextWindowPanel(props: {
  context: ThreadContext | undefined
  loading: boolean
  failed: boolean
}) {
  const [open, setOpen] = createSignal(false)
  const used = () => props.context?.estimatedTokens ?? 0
  const budget = () => props.context?.budgetTokens ?? 0
  // Guard the divide: a zero budget would render NaN%, which reads as broken
  // rather than as unknown.
  const fill = () => (budget() > 0 ? Math.round((used() / budget()) * 100) : null)

  return (
    <section class="verevon-chat-context-window">
      <button
        type="button"
        class="verevon-chat-context-window__toggle"
        aria-expanded={open() ? 'true' : 'false'}
        onClick={() => setOpen((value) => !value)}
      >
        <Layers size={14} />
        <span>Kontekstvindu</span>
        <Show when={props.context}>
          <em>
            {used().toLocaleString('nb-NO')}
            <Show when={budget() > 0}>{` / ${budget().toLocaleString('nb-NO')}`}</Show>
            {' tokens'}
            <Show when={fill() != null}>{` (${fill()}%)`}</Show>
          </em>
        </Show>
        <ChevronRight size={14} class={{ 'verevon-chat-rotate': open() }} />
      </button>
      <Show when={open()}>
        <Switch>
          <Match when={props.loading}>
            <p class="verevon-chat-context-window__note">Laster …</p>
          </Match>
          <Match when={props.failed}>
            {/* Named, not blank: a failed read and an empty window look the
                same otherwise, and only one of them is a problem. */}
            <p class="verevon-chat-context-window__note">
              Kunne ikke hente kontekstvinduet.
            </p>
          </Match>
          <Match when={(props.context?.segments.length ?? 0) === 0}>
            <p class="verevon-chat-context-window__note">Ingen segmenter rapportert.</p>
          </Match>
          <Match when={props.context}>
            {(context) => (
              <ul class="verevon-chat-context-window__list">
                <For each={context().segments}>
                  {(segment) => (
                    <li class="verevon-chat-context-window__item">
                      <div class="verevon-chat-context-window__head">
                        <span class="verevon-chat-context-window__kind">{segment.kind}</span>
                        <span class="verevon-chat-context-window__tokens">
                          {segment.estimatedTokens.toLocaleString('nb-NO')} tokens
                        </span>
                      </div>
                      <Show when={segment.content.trim()}>
                        {(content) => (
                          <pre class="verevon-chat-context-window__content">{content()}</pre>
                        )}
                      </Show>
                    </li>
                  )}
                </For>
              </ul>
            )}
          </Match>
        </Switch>
      </Show>
    </section>
  )
}

export function StepsPanel(props: {
  events?: readonly VerevonUiEvent[]
  runId?: string | null
  steps: AgentTaskStep[]
  threadId?: string | null
  /**
   * The turn's tool calls, with their real arguments and output.
   *
   * These were collected on every turn and rendered NOWHERE: `ToolCallCard`
   * had no caller, and the "view steps" pill pointed here, which showed task
   * steps (title/detail/status) instead. So the evidence a step was built on —
   * what was searched, what came back, what failed — was unreachable in the UI.
   */
  toolCalls?: ChatToolCall[]
  screen?: ChatArtifact | null
  onStopTask: () => void
}) {
  const activeTask = () => props.steps.some((step) => step.status === 'active' || step.status === 'waiting')
  const sections = createMemo(() => groupTaskSteps(props.steps))
  const latestUsage = createMemo(() => {
    const events = props.events ?? []
    for (let index = events.length - 1; index >= 0; index -= 1) {
      const event = events[index]
      if (event?.type === 'usage.recorded') return event
    }
    return null
  })
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
      when={props.steps.length > 0 || props.screen || props.runId}
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
          <RunPlanPanel runId={props.runId} threadId={props.threadId} />
          <RunProofPanel runId={props.runId} />
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
          <Show when={latestUsage()}>
            {(usage) => {
              const current = usage()
              if (!current) return null
              const tokensPerSecond = () => {
                const outputTokens = current.outputTokens
                const latencyMs = current.latencyMs
                return outputTokens != null && latencyMs != null && latencyMs > 0
                  ? Math.round((outputTokens / latencyMs) * 1000)
                  : null
              }
              const cacheTotal = () => {
                return (current.cacheReadTokens ?? 0) + (current.cacheWriteTokens ?? 0)
              }
              return (
                <div class="verevon-chat-telemetry" aria-label="Kjøringsmålinger">
                  <Metric label="Output" value={`${current.outputTokens ?? '—'} tokens`} />
                  <Metric label="Tokens/sec" value={tokensPerSecond() != null ? String(tokensPerSecond()) : '—'} />
                  <Show when={cacheTotal() > 0}>
                    <Metric label="Prompt cache" value={`${current.cacheReadTokens ?? 0} read`} />
                  </Show>
                </div>
              )
            }}
          </Show>
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
            <Show when={(props.toolCalls?.length ?? 0) > 0}>
              <section class="verevon-chat-steps-tools">
                <h3>{'Verktøykall'}</h3>
                <For each={props.toolCalls ?? []}>
                  {(call) => <ToolCallCard call={call} />}
                </For>
              </section>
            </Show>
            <Show when={(props.events?.length ?? 0) > 0}>
              <details class="verevon-chat-event-log">
                <summary>Hendelsesstrøm · {props.events?.length ?? 0}</summary>
                <ol>
                  <For each={props.events ?? []}>
                    {(event) => (
                      <li>
                        <span>{uiEventLabel(event)}</span>
                        <time>{formatTime(event.at)}</time>
                      </li>
                    )}
                  </For>
                </ol>
              </details>
            </Show>
          </div>
        </div>
      </div>
    </Show>
  )
}

type RunPlanSnapshot = {
  plans: Plan[]
  todos: Todo[]
  lineage: unknown
  failed: number
}

/**
 * Hydrates the durable orchestration records that back the local step stream.
 * The gateway already owns authorization and proxies these reads to the Model
 * Plane; this component only presents the returned records and never invents
 * plan steps from the chat text.
 */
function RunPlanPanel(props: { runId?: string | null; threadId?: string | null }) {
  const keySeparator = '\u0000'
  const source = () => {
    const runId = props.runId?.trim()
    const threadId = props.threadId?.trim()
    return runId && threadId ? `${runId}${keySeparator}${threadId}` : null
  }
  const [snapshot] = createResource(source, async (key: string): Promise<RunPlanSnapshot> => {
    const separatorIndex = key.indexOf(keySeparator)
    const runId = separatorIndex >= 0 ? key.slice(0, separatorIndex) : key
    const threadId = separatorIndex >= 0 ? key.slice(separatorIndex + keySeparator.length) : ''
    const [plans, todos, lineage] = await Promise.allSettled([
      listPlans(runId),
      listTodos(threadId, { runId }),
      getLineage(threadId),
    ])
    return {
      plans: plans.status === 'fulfilled' ? plans.value : [],
      todos: todos.status === 'fulfilled' ? todos.value : [],
      lineage: lineage.status === 'fulfilled' ? lineage.value : undefined,
      failed: [plans, todos, lineage].filter((result) => result.status === 'rejected').length,
    }
  })

  const lineageEdges = createMemo(() => {
    const value = snapshot()?.lineage
    if (!value || typeof value !== 'object') return [] as Array<Record<string, unknown>>
    const root = value as Record<string, unknown>
    const candidate = root.lineage && typeof root.lineage === 'object'
      ? root.lineage as Record<string, unknown>
      : root
    return Array.isArray(candidate.edges)
      ? candidate.edges.filter((edge): edge is Record<string, unknown> => Boolean(edge && typeof edge === 'object' && !Array.isArray(edge)))
      : []
  })

  const stateLabel = (value?: string) => {
    if (!value) return 'Ukjent'
    return value
      .toLocaleLowerCase('nb-NO')
      .replaceAll('_', ' ')
      .replace(/^./, (character) => character.toLocaleUpperCase('nb-NO'))
  }

  return (
    <Show when={source()}>
      <section class="verevon-chat-run-plan" aria-label="Kjøringsplan">
        <div class="verevon-chat-run-plan__header">
          <div>
            <h3>Plan</h3>
            <p>Planen som er lagret av arbeidskjøringen.</p>
          </div>
          <Show when={snapshot()?.plans.length}>
            <span class="verevon-chat-run-plan__count">{snapshot()?.plans.length}</span>
          </Show>
        </div>

        <Show when={snapshot.loading}>
          <p class="verevon-chat-run-plan__note">Laster plan og oppgaver …</p>
        </Show>

        <Show when={!snapshot.loading && snapshot.error}>
          <p class="verevon-chat-run-plan__note">Klarte ikke å hente kjøringsplanen.</p>
        </Show>

        <Show when={!snapshot.loading && snapshot() && snapshot()!.plans.length === 0 && snapshot()!.todos.length === 0 && lineageEdges().length === 0}>
          <p class="verevon-chat-run-plan__note">
            Ingen varige plandetaljer er rapportert ennå.
          </p>
        </Show>

        <For each={snapshot()?.plans ?? []}>
          {(plan) => (
            <article class="verevon-chat-run-plan__card">
              <div class="verevon-chat-run-plan__card-head">
                <strong>{plan.summary || 'Arbeidsplan'}</strong>
                <span data-state={plan.state}>{stateLabel(plan.state)}</span>
              </div>
              <Show when={plan.author || plan.supersedes}>
                <p class="verevon-chat-run-plan__meta">
                  <Show when={plan.author}>Opprettet av {plan.author}</Show>
                  <Show when={plan.author && plan.supersedes}> · </Show>
                  <Show when={plan.supersedes}>erstatter {plan.supersedes}</Show>
                </p>
              </Show>
              <Show when={(plan.steps?.length ?? 0) > 0}>
                <ol class="verevon-chat-run-plan__steps">
                  <For each={plan.steps}>
                    {(step) => (
                      <li data-state={step.state}>
                        <span class="verevon-chat-run-plan__step-state" data-state={step.state}>{stateLabel(step.state)}</span>
                        <span>
                          <strong>{step.title}</strong>
                          <Show when={step.operation}>
                            <small>{step.operation}</small>
                          </Show>
                        </span>
                      </li>
                    )}
                  </For>
                </ol>
              </Show>
            </article>
          )}
        </For>

        <Show when={(snapshot()?.todos.length ?? 0) > 0}>
          <div class="verevon-chat-run-plan__todos">
            <h4>Oppgaver</h4>
            <For each={snapshot()?.todos ?? []}>
              {(todo) => (
                <div class="verevon-chat-run-plan__todo" data-state={todo.state}>
                  <span class="verevon-chat-run-plan__step-state" data-state={todo.state}>{stateLabel(todo.state)}</span>
                  <span>
                    <strong>{todo.title || 'Uten tittel'}</strong>
                    <Show when={todo.description}><small>{todo.description}</small></Show>
                  </span>
                </div>
              )}
            </For>
          </div>
        </Show>

        <Show when={lineageEdges().length > 0}>
          <details class="verevon-chat-run-plan__lineage">
            <summary>Delegert arbeid · {lineageEdges().length}</summary>
            <ul>
              <For each={lineageEdges()}>
                {(edge) => (
                  <li>
                    <code>{String(edge.parent_run_id ?? edge.parentRunId ?? '—')}</code>
                    <ChevronRight size={12} />
                    <code>{String(edge.child_run_id ?? edge.childRunId ?? '—')}</code>
                    <span>{String(edge.role ?? '')}</span>
                  </li>
                )}
              </For>
            </ul>
          </details>
        </Show>

        <Show when={(snapshot()?.failed ?? 0) > 0}>
          <p class="verevon-chat-run-plan__partial" role="status">
            Noen kjøringsdetaljer kunne ikke hentes akkurat nå.
          </p>
        </Show>
      </section>
    </Show>
  )
}

/**
 * Compact trust surface for the chat workspace. The Agent console owns the
 * full proof-bundle renderer; chat only needs a calm summary and an honest
 * distinction between unavailable, unproven, and verified execution.
 */
export function RunProofPanel(props: { runId?: string | null }) {
  const runId = () => props.runId?.trim() || null
  const [proof, proofActions] = createResource(runId, async (id: string): Promise<ProofBundle | null> =>
    getRunProofBundle(id),
  )
  const outcome = () => proof()?.run?.status ?? null
  const verified = () => proof()?.approvals.filter((approval) => {
    const status = approval.execution?.outcome?.verification?.status
    return status === 'verified_success' || status === 'partially_verified'
  }).length ?? 0
  const effectClass = () => proof()?.effectClass
  const effectClassLabel = () => {
    switch (effectClass()) {
      case 'read_only': return 'Kun lesing'
      case 'proposed_effect': return 'Foreslått effekt'
      case 'effectful': return 'Effekt forsøkt'
      case 'external_receipt': return 'Ekstern kvittering'
      default: return 'Ikke fastslått'
    }
  }

  const refresh = () => {
    void proofActions.refetch().catch(() => undefined)
  }

  return (
    <Show when={runId()}>
      <section class="verevon-run-panel verevon-run-proof verevon-chat-run-proof" aria-label="Kjøringskvittering">
        <div class="verevon-run-panel__head">
          <span class="verevon-run-panel__eyebrow">
            <ShieldCheck size={14} strokeWidth={2.1} /> Kvittering
          </span>
          <button type="button" class="verevon-chat-run-proof__refresh" onClick={refresh} disabled={proof.loading}>
            <RefreshCw size={12} class={{ 'verevon-run-spin': proof.loading }} />
            Oppdater
          </button>
        </div>

        <Show when={proof.loading}>
          <p class="verevon-run-proof__note">Henter kjøringskvittering …</p>
        </Show>
        <Show when={proof.error != null}>
          <p class="verevon-run-proof__note">Kvitteringen kunne ikke hentes akkurat nå.</p>
        </Show>
        <Show when={!proof.loading && proof.error == null && !proof()}>
          <p class="verevon-run-proof__note">Ingen varig kvittering er registrert ennå.</p>
        </Show>
        <Show when={proof()}>
          {(bundle) => (
            <>
              <Show when={bundle().run}>
                {(run) => <p class="verevon-run-proof__goal">{run().goal || 'Arbeidskjøring'}<Show when={run().status}><span class="verevon-run-proof__agent"> · {run().status}</span></Show></p>}
              </Show>
              <div class="verevon-chat-run-proof__stats">
                <Metric label="Godkjenninger" value={String(bundle().approvals.length)} />
                <Metric label="Verifisert" value={String(verified())} />
                <Show when={effectClass()}>
                  <Metric label="Effektklasse" value={effectClassLabel()} />
                </Show>
                <Show when={outcome()}>
                  {(status) => <Metric label="Utfall" value={status()} />}
                </Show>
              </div>
              <Show when={bundle().approvals.length > 0}>
                <div class="verevon-run-proof__approvals verevon-chat-run-proof__approvals" aria-label="Godkjenninger og kvitteringer">
                  <For each={bundle().approvals}>
                    {(approval) => <ChatProofApproval approval={approval} />}
                  </For>
                </div>
              </Show>
              <Show when={bundle().unavailable.length > 0}>
                <p class="verevon-run-proof__note">Noen deler av utfallet er ikke dekket av denne pakken.</p>
              </Show>
            </>
          )}
        </Show>
      </section>
    </Show>
  )
}

function ChatProofApproval(props: { approval: ProofApproval }) {
  const execution = () => props.approval.execution
  const outcome = () => execution()?.outcome
  const verification = () => outcome()?.verification
  const status = () => props.approval.status || 'registrert'
  const statusTone = () => {
    const value = status().toLowerCase()
    if (value.includes('deny') || value.includes('reject') || value.includes('fail')) return 'error'
    if (value.includes('wait') || value.includes('pending') || value.includes('pause')) return 'warn'
    if (value.includes('approv') || value.includes('grant') || value.includes('complete')) return 'ok'
    return 'neutral'
  }
  return (
    <article class="verevon-run-proof__approval verevon-chat-run-proof__approval">
      <div class="verevon-run-proof__approval-head">
        <span class="verevon-run-proof__kind">{props.approval.kind || 'Handling'}</span>
        <span class={`verevon-run-proof__pill verevon-run-proof__pill--${statusTone()}`}>{status()}</span>
      </div>
      <Show when={execution()?.receiptId || outcome()?.outcome || verification()?.status}>
        <dl class="verevon-run-proof__receipt">
          <Show when={execution()?.receiptId}>
            {(receipt) => (
              <div class="verevon-run-proof__receipt-row">
                <dt>Kvittering</dt>
                <dd><code title={receipt()}>{shortReceipt(receipt())}</code></dd>
              </div>
            )}
          </Show>
          <Show when={outcome()?.outcome}>
            {(value) => (
              <div class="verevon-run-proof__receipt-row">
                <dt>Utfallsstatus</dt>
                <dd>{value()}</dd>
              </div>
            )}
          </Show>
          <Show when={verification()?.status}>
            {(value) => (
              <div class="verevon-run-proof__receipt-row">
                <dt>Verifisering</dt>
                <dd>{value()}</dd>
              </div>
            )}
          </Show>
        </dl>
      </Show>
    </article>
  )
}

/**
 * A focused audit surface for a completed or running Do turn. The proof bundle
 * remains the authority for approvals and verified effects; the event list is
 * only the user-safe projection already reduced by the chat controller (no
 * hidden chain-of-thought or provider credentials are exposed here).
 */
export function TracePanel(props: {
  events: readonly VerevonUiEvent[]
  replayTruncated?: boolean
  runId?: string | null
}) {
  return (
    <div class="verevon-chat-trace-panel">
      <RunProofPanel runId={props.runId} />
      <section class="verevon-chat-trace-log" aria-label="Trace-hendelser">
        <header>
          <div>
            <h2>Trace</h2>
            <p>En lesbar oversikt over hva som skjedde i denne kjøringen.</p>
          </div>
          <span>{props.events.length} hendelser</span>
        </header>
        <Show when={props.replayTruncated}>
          <p class="verevon-chat-trace-log__notice" role="status">
            Trace-visningen er avkortet etter 5&nbsp;000 hendelser.
          </p>
        </Show>
        <Show
          when={props.events.length > 0}
          fallback={<p class="verevon-chat-trace-log__empty">Ingen hendelser er registrert ennå.</p>}
        >
          <ol>
            <For each={props.events}>
              {(event) => (
                <li>
                  <span class="verevon-chat-trace-log__marker" aria-hidden="true" />
                  <div>
                    <span class="verevon-chat-trace-log__event-copy">
                      <strong>{uiEventLabel(event)}</strong>
                      <Show when={cancellationReceipt(event)}>
                        {(receipt) => <code title={receipt()}>kvittering {shortReceipt(receipt())}</code>}
                      </Show>
                    </span>
                    <time>{formatTime(event.at)}</time>
                  </div>
                </li>
              )}
            </For>
          </ol>
        </Show>
      </section>
    </div>
  )
}
