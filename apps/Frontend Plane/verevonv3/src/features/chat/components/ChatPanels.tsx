import {
  ChevronRight,
  Link2,
  Layers,
  ListChecks,
  MessageSquarePlus,
  MoreHorizontal,
  PanelRightOpen,
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
import { createEffect, createMemo, createSignal, For, Match, onCleanup, Show, Switch } from 'solid-js'
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
import { isWorkStep } from './chat-normalizers'
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
import { useI18n } from '@/shared/i18n'

function shortReceipt(value: string) {
  return value.length > 24 ? `${value.slice(0, 12)}…${value.slice(-8)}` : value
}

function cancellationReceipt(event: VerevonUiEvent) {
  return event.type === 'run.cancelled' ? event.receiptId : undefined
}

export function ChatHeader(props: {
  active: ChatTab
  artifactCount: number
  /** Attachments on this conversation. See `availability()` below. */
  attachmentCount?: number
  branchCount: number
  messageCount: number
  runAvailable?: boolean
  sourceCount: number
  stepCount: number
  /** Steps that are work rather than bookkeeping; gates the Work destination. */
  workStepCount?: number
  /** Tool calls the model actually made; gates the Work destination. */
  toolCallCount?: number
  title: string
  traceAvailable?: boolean
  onChange: (tab: ChatTab) => void
  onNewChat: () => void
  onRegenerate: () => void
}) {
  const i18n = useI18n()
  const [openMenu, setOpenMenu] = createSignal<'workspace' | 'more' | null>(null)
  const availability = (): ChatSurfaceAvailability => ({
    sourceCount: props.sourceCount,
    hasGrounding: props.sourceCount > 0,
    artifactCount: props.artifactCount,
    // Was hardcoded to 0, which made this header disagree with the tab strip:
    // `chat-surfaces.ts` gates Output on `artifactCount > 0 ||
    // attachmentCount > 0`, so a conversation whose only output was an
    // attachment offered Output in the tabs but never in this dropdown.
    attachmentCount: props.attachmentCount ?? 0,
    stepCount: props.stepCount,
    workStepCount: props.workStepCount,
    toolCallCount: props.toolCallCount,
    hasRun: Boolean(props.runAvailable || props.traceAvailable),
  })
  const workSurfaces = () => availableChatSurfaces(availability()).filter((surface) => surface.id !== 'chat')
  const surfaceLabel = (tab: ChatTab) => {
    if (tab === 'steps') return i18n.tr('Arbeid', 'Work')
    if (tab === 'artifacts') return i18n.tr('Resultat', 'Output')
    if (tab === 'sources') return i18n.tr('Kilder', 'Sources')
    if (tab === 'trace') return i18n.tr('Spor', 'Trace')
    return i18n.tr('Samtale', 'Chat')
  }
  const surfaceCount = (tab: ChatTab) => (
    tab === 'sources'
      ? props.sourceCount
      : tab === 'artifacts'
        ? props.artifactCount
        : tab === 'steps' ? props.stepCount : 0
  )
  const selectSurface = (tab: ChatTab) => {
    setOpenMenu(null)
    props.onChange(tab)
  }
  const closeOnFocusOut = (event: FocusEvent) => {
    const next = event.relatedTarget
    if (!(event.currentTarget instanceof HTMLElement) || !(next instanceof Node) || !event.currentTarget.contains(next)) {
      setOpenMenu(null)
    }
  }

  return (
    <header class="verevon-chat-header">
      <div class="verevon-chat-header__copy">
        <h1>{props.title}</h1>
        <p>
          {i18n.tr(`${props.messageCount} meldinger`, `${props.messageCount} messages`)}
          <Show when={props.branchCount > 0}>
            {' · '}{i18n.tr(`${props.branchCount} versjoner`, `${props.branchCount} versions`)}
          </Show>
        </p>
      </div>
      <div class="verevon-chat-header__actions">
        <Show when={workSurfaces().length > 0}>
          <div class="verevon-chat-header-menu" onFocusOut={closeOnFocusOut}>
            <button
              type="button"
              class={{
                'verevon-chat-workspace-trigger': true,
                'verevon-chat-workspace-trigger--active': props.active !== 'chat',
              }}
              aria-haspopup="menu"
              aria-expanded={openMenu() === 'workspace' ? 'true' : 'false'}
              onClick={() => setOpenMenu(openMenu() === 'workspace' ? null : 'workspace')}
            >
              <PanelRightOpen size={14} />
              <span>{props.active === 'chat' ? i18n.tr('Arbeidsflate', 'Workspace') : surfaceLabel(props.active)}</span>
              <ChevronRight size={13} class="verevon-chat-workspace-trigger__chevron" />
            </button>
            <Show when={openMenu() === 'workspace'}>
              <div class="verevon-chat-header-menu__popover verevon-chat-header-menu__popover--workspace" role="menu" aria-label={i18n.tr('Åpne arbeidsflate', 'Open workspace')}>
                <For each={workSurfaces()}>
                  {(surface) => {
                    const Icon = surface.icon
                    return (
                      <button
                        type="button"
                        role="menuitem"
                        class={{ 'is-selected': props.active === surface.id }}
                        onClick={() => selectSurface(surface.id)}
                      >
                        <Icon size={14} />
                        <span>{surfaceLabel(surface.id)}</span>
                        <Show when={surfaceCount(surface.id) > 0}><em>{surfaceCount(surface.id)}</em></Show>
                      </button>
                    )
                  }}
                </For>
              </div>
            </Show>
          </div>
        </Show>
        <div class="verevon-chat-header__utility-actions">
          <div class="verevon-chat-header-menu" role="presentation" onFocusOut={closeOnFocusOut}>
            <button
              type="button"
              class="verevon-chat-header-button"
              aria-haspopup="menu"
              aria-expanded={openMenu() === 'more' ? 'true' : 'false'}
              onClick={() => setOpenMenu(openMenu() === 'more' ? null : 'more')}
            >
              <MoreHorizontal size={14} />
              <span class="sr-only">{i18n.tr('Flere handlinger', 'More actions')}</span>
            </button>
            <Show when={openMenu() === 'more'}>
              <div class="verevon-chat-header-menu__popover" role="menu" aria-label={i18n.tr('Samtalehandlinger', 'Conversation actions')}>
                <button type="button" role="menuitem" onClick={() => { setOpenMenu(null); props.onRegenerate() }}>
                  <RefreshCw size={14} />
                  <span>{i18n.tr('Generer siste svar på nytt', 'Regenerate latest response')}</span>
                </button>
              </div>
            </Show>
          </div>
          <button type="button" class="verevon-chat-header-button verevon-chat-header-button--primary" aria-label={i18n.tr('Ny samtale', 'New chat')} onClick={() => props.onNewChat()}>
            <MessageSquarePlus size={15} />
            <span>{i18n.tr('Ny', 'New')}</span>
          </button>
        </div>
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
  /** Steps that are work rather than bookkeeping; gates the Work destination. */
  workStepCount?: number
  /** Tool calls the model actually made; gates the Work destination. */
  toolCallCount?: number
  traceAvailable?: boolean
  includeChat?: boolean
  onChange: (tab: ChatTab) => void
}) {
  const i18n = useI18n()
  const availability = (): ChatSurfaceAvailability => ({
    sourceCount: props.sourceCount,
    hasGrounding: props.sourceCount > 0,
    artifactCount: props.artifactCount,
    attachmentCount: 0,
    stepCount: props.stepCount,
    workStepCount: props.workStepCount,
    toolCallCount: props.toolCallCount,
    hasRun: Boolean(props.runAvailable || props.traceAvailable),
  })
  const tabs = (): Array<{ id: ChatTab; label: string; icon: IconComponent; count: number }> => (
    availableChatSurfaces(availability()).filter((surface) => props.includeChat !== false || surface.id !== 'chat').map((surface) => ({
      id: surface.id,
      label: surface.id === 'steps'
        ? i18n.tr('Arbeid', 'Work')
        : surface.id === 'artifacts'
          ? i18n.tr('Resultat', 'Output')
          : surface.id === 'sources'
            ? i18n.tr('Kilder', 'Sources')
            : surface.id === 'trace'
              ? i18n.tr('Spor', 'Trace')
              : i18n.tr('Samtale', 'Chat'),
      icon: surface.icon,
      // The Work badge counts work, not the lifecycle rows that sit behind the
      // technical-activity disclosure (audit items 19 and 27).
      count: surface.id === 'sources'
        ? props.sourceCount
        : surface.id === 'artifacts'
          ? props.artifactCount
          : surface.id === 'steps' ? (props.workStepCount ?? props.stepCount) : 0,
    }))
  )

  return (
    <Show when={tabs().length > (props.includeChat === false ? 0 : 1)}>
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
  const i18n = useI18n()
  return (
    <Show
      when={props.sources.length > 0 || props.grounding}
      fallback={(
        <EmptyPanel
          icon={<Link2 size={20} />}
          title={i18n.tr('Ingen kilder ennå', 'No sources yet')}
          subtitle={i18n.tr('Interne kunnskapskilder og websøk dukker opp her når Verevon bruker dem i svaret.', 'Internal knowledge sources and web results appear here once Verevon uses them in an answer.')}
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
  const i18n = useI18n()
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
        aria-label={i18n.tr(`Åpne ${props.source.title} i Kunnskap`, `Open ${props.source.title} in Knowledge`)}
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
  const i18n = useI18n()
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
          <Match when={!props.context}>
            {/* Three outcomes used to collapse into one message. An unresolved
                resource leaves `context` undefined with loading AND failed
                both false — for a temporary chat, or before a thread exists —
                and that fell through to "no segments reported", which reads as
                "the server says your context is empty" rather than "nothing
                was asked for". Say which one it is. Kept inside the Match
                rather than between siblings, so Switch only ever sees Match
                children. */}
            <p class="verevon-chat-context-window__note">
              Kontekstvinduet er ikke hentet for denne samtalen ennå.
            </p>
          </Match>
          <Match when={props.context?.segments.length === 0}>
            <p class="verevon-chat-context-window__note">{i18n.tr('Ingen segmenter rapportert.', 'No segments reported.')}</p>
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
  const i18n = useI18n()
  const activeTask = () => props.steps.some((step) => step.status === 'active' || step.status === 'waiting')
  const sections = createMemo(() => groupTaskSteps(props.steps))
  // UX spec section 7, question 1: what is Verevon doing now, in plain language.
  const waitingStep = () => props.steps.find((step) => step.status === 'waiting')
  const activeStep = () => props.steps.find((step) => step.status === 'active')
  const failedStep = () => [...props.steps].reverse().find((step) => step.status === 'error')
  const statusLine = () => {
    if (waitingStep()) return i18n.tr('Venter på deg', 'Waiting for you')
    const active = activeStep()
    if (active) return active.title
    if (failedStep()) return i18n.tr('Siste steg feilet', 'The last step failed')
    if (props.steps.length > 0) return i18n.tr('Ferdig', 'Finished')
    return i18n.tr('Ingen aktivitet nå', 'Nothing running')
  }
  // The timeline shows work; lifecycle rows go behind the disclosure. Sections
  // with nothing but bookkeeping disappear rather than render an empty group.
  const workSections = createMemo(() => sections()
    .map((section) => ({ ...section, steps: section.steps.filter(isWorkStep) }))
    .filter((section) => section.steps.length > 0))
  const bookkeepingSteps = createMemo(() => props.steps.filter((step) => !isWorkStep(step)))
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
          title={i18n.tr('Ingen steg ennå', 'No steps yet')}
          subtitle={i18n.tr(
            'Agentens arbeidssteg vises her mens en oppgave kjører.',
            "The agent's work steps appear here while a task runs.",
          )}
        />
      )}
    >
      <div class="verevon-chat-panel">
        <div class="verevon-chat-panel__inner">
          {/* UX spec section 7 order: what is happening now, what is left, does
              Verevon need me, what happened underneath. The header used to read
              "Agent activity / Live oppgavestatus" -- an English heading over a
              Norwegian subtitle -- above run telemetry and a flat event list. */}
          <div class="verevon-chat-steps-header">
            <div>
              <h2>{i18n.tr('Arbeid', 'Work')}</h2>
              <p>{statusLine()}</p>
            </div>
            <button type="button" disabled={!activeTask()} onClick={() => props.onStopTask()}>
              <Square size={12} />
              {i18n.tr('Stopp', 'Stop')}
            </button>
          </div>
          {/* Step count drives the plan refetch: session-core mirrors each
              completed execution step into the run's plan, so a new task step
              is the cheapest available signal that the durable plan has more
              rows than the panel last read. */}
          <RunPlanPanel
            runId={props.runId}
            threadId={props.threadId}
            progressKey={`${props.steps.length}:${props.steps.filter((step) => step.status !== 'active' && step.status !== 'waiting').length}`}
          />
          {/* Question 3: does Verevon need me. A waiting step is the typed pause
              the run reported; the panel never invents one. The proof bundle
              (receipts, approvals) stays in Trace, which is the audit record. */}
          <Show when={waitingStep()}>
            {(step) => (
              <section class="verevon-chat-steps-attention" role="note">
                <strong>{i18n.tr('Verevon venter på deg', 'Verevon needs you')}</strong>
                <p>{step().title}</p>
                <Show when={step().detail}><p>{step().detail}</p></Show>
              </section>
            )}
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
            <For each={workSections()}>
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
                <h3>{i18n.tr('Verktøykall', 'Tool calls')}</h3>
                <For each={props.toolCalls ?? []}>
                  {(call) => <ToolCallCard call={call} />}
                </For>
              </section>
            </Show>
            {/* Question 4: what happened underneath. Run telemetry, the lifecycle
                rows and the raw event stream in one collapsed disclosure, the
                same shape the live rail uses -- not four peers of the work
                timeline. Closed by default; the reader opts in. */}
            <Show when={bookkeepingSteps().length > 0 || latestUsage() || (props.events?.length ?? 0) > 0}>
              <details class="verevon-chat-run-activity-disclosure">
                <summary>
                  <span>{i18n.tr('Teknisk aktivitet', 'Technical activity')}</span>
                  <em>{bookkeepingSteps().length + (props.events?.length ?? 0)}</em>
                </summary>
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
                      <div class="verevon-chat-telemetry" aria-label={i18n.tr('Kjøringsmålinger', 'Run metrics')}>
                        <Metric label="Output" value={`${current.outputTokens ?? '—'} tokens`} />
                        <Metric label="Tokens/sec" value={tokensPerSecond() != null ? String(tokensPerSecond()) : '—'} />
                        <Show when={cacheTotal() > 0}>
                          <Metric label="Prompt cache" value={`${current.cacheReadTokens ?? 0} read`} />
                        </Show>
                      </div>
                    )
                  }}
                </Show>
                <Show when={bookkeepingSteps().length > 0}>
                  <section
                    class="verevon-chat-run-activity"
                    aria-label={i18n.tr('Livsløpssteg', 'Lifecycle steps')}
                  >
                    <For each={bookkeepingSteps()}>
                      {(step) => (
                        <div class="verevon-chat-run-activity__row" data-status={step.status}>
                          <strong>{step.title}</strong>
                          <Show when={step.detail}><span>{step.detail}</span></Show>
                        </div>
                      )}
                    </For>
                  </section>
                </Show>
                <Show when={(props.events?.length ?? 0) > 0}>
                  {/* `.verevon-chat-event-log ol` is scoped to that ancestor class,
                      so the wrapper stays and the list keeps its existing styling. */}
                  <div class="verevon-chat-event-log">
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
                  </div>
                </Show>
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
/**
 * Steps carry no stored title, so their `operation` is all the plan API sends
 * (`tool_execution`). Render it as a phrase rather than an identifier.
 */
function humanizePlanStepOperation(operation?: string) {
  const value = operation?.trim()
  if (!value) return 'Steg'
  if (value === 'tool_execution') return 'Verktøykjøring'
  return value
    .replaceAll('_', ' ')
    .replace(/^./, (character) => character.toLocaleUpperCase('nb-NO'))
}

export function RunPlanPanel(props: {
  runId?: string | null
  threadId?: string | null
  /**
   * Changes as the run records steps: new steps and status changes alike. It
   * used to be part of the resource key, so every step restarted the fetch. A
   * deep-research run emits a step every ~400ms while the three plan requests
   * take ~450ms, so almost no fetch survived long enough to resolve: `value`
   * never populated, `loading` stayed true, the panel read "Laster plan og
   * oppgaver …" for the entire run, and it fired three requests per step (about
   * 165 in a 24s run). It now schedules a throttled, coalesced refetch (one per
   * `refreshIntervalMs`, never while a fetch is in flight), and the last
   * successful snapshot stays on screen while a refresh runs.
   */
  progressKey?: number | string
  /** Minimum gap between progress-driven refetches. Tests shorten it. */
  refreshIntervalMs?: number
}) {
  const i18n = useI18n()
  const keySeparator = '\u0000'
  // Keyed on the ids only. A stable key means progress never supersedes the
  // in-flight fetch, so the initial load always lands.
  const source = () => {
    const runId = props.runId?.trim()
    const threadId = props.threadId?.trim()
    if (!runId || !threadId) return null
    return `${runId}${keySeparator}${threadId}`
  }
  const [snapshot, { refetch }] = createResource(source, async (key: string): Promise<RunPlanSnapshot> => {
    const [runId = '', threadId = ''] = key.split(keySeparator)
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
  /** Best available data: the last successful snapshot while a refresh runs. */
  const data = () => snapshot.latest

  // Progress-driven refresh: at most one refetch per interval, never two in
  // flight at once, and a change that arrives mid-flight is not lost — it
  // queues exactly one more refresh after the current one settles.
  const refreshInterval = () => props.refreshIntervalMs ?? 4000
  let refreshTimer: ReturnType<typeof setTimeout> | undefined
  let refreshQueued = false
  // Start time of the most recent fetch, initial load included; stamped by the
  // loading effect below so the interval is always measured from a real start.
  let lastRefreshAt = Date.now()
  let progressSeen = false
  const runRefresh = () => {
    refreshTimer = undefined
    // A timer armed from a stale timestamp must not fire early: wait out the
    // rest of the interval measured from the latest fetch start. Without this,
    // refetches ran back-to-back at fetch-duration cadence under load.
    const remaining = lastRefreshAt + refreshInterval() - Date.now()
    if (remaining > 0) {
      refreshTimer = setTimeout(runRefresh, remaining)
      return
    }
    // Never restart a fetch that is still in flight -- that is exactly how the
    // old per-step key starved the panel. Queue one refresh; the loading effect
    // releases it once the current fetch settles.
    if (snapshot.loading) {
      refreshQueued = true
      return
    }
    // A failed refresh is recorded in `snapshot.error`; the panel keeps its
    // last data rather than blanking.
    void refetch().catch(() => {})
  }
  const scheduleRefresh = () => {
    if (refreshTimer !== undefined) return
    const wait = Math.max(0, lastRefreshAt + refreshInterval() - Date.now())
    refreshTimer = setTimeout(runRefresh, wait)
  }
  createEffect(
    () => props.progressKey ?? 0,
    () => {
      // The first run is mount; the source-driven fetch covers the initial load.
      if (!progressSeen) {
        progressSeen = true
        return
      }
      if (!source()) return
      scheduleRefresh()
    },
  )
  createEffect(
    () => snapshot.loading,
    (loading) => {
      if (loading) {
        lastRefreshAt = Date.now()
        return
      }
      // Release a refresh that arrived while a fetch was in flight.
      if (!refreshQueued) return
      refreshQueued = false
      scheduleRefresh()
    },
  )
  onCleanup(() => {
    if (refreshTimer !== undefined) clearTimeout(refreshTimer)
  })

  const lineageEdges = createMemo(() => {
    const value = data()?.lineage
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
      // The API relays prost's enum spelling verbatim
      // (`PLAN_STEP_STATE_RUNNING`), which rendered as the sentence "Plan step
      // state running" beside every step. Only the last segment is the state.
      .replace(/^PLAN_(?:STEP_)?STATE_/, '')
      .toLocaleLowerCase('nb-NO')
      .replaceAll('_', ' ')
      .replace(/^./, (character) => character.toLocaleUpperCase('nb-NO'))
  }

  return (
    <Show when={source()}>
      <section class="verevon-chat-run-plan" aria-label={i18n.tr('Kjøringsplan', 'Run plan')}>
        <div class="verevon-chat-run-plan__header">
          <div>
            <h3>Plan</h3>
            <p>{i18n.tr('Planen som er lagret av arbeidskjøringen.', 'The plan stored by the work run.')}</p>
          </div>
          <Show when={data()?.plans.length}>
            <span class="verevon-chat-run-plan__count">{data()?.plans.length}</span>
          </Show>
        </div>

        <Show when={snapshot.loading && !data()}>
          <p class="verevon-chat-run-plan__note">Laster plan og oppgaver …</p>
        </Show>

        <Show when={!snapshot.loading && snapshot.error && !data()}>
          <p class="verevon-chat-run-plan__note">{i18n.tr('Klarte ikke å hente kjøringsplanen.', 'Could not load the run plan.')}</p>
        </Show>

        <Show when={!snapshot.loading && data() && data()!.plans.length === 0 && data()!.todos.length === 0 && lineageEdges().length === 0}>
          <p class="verevon-chat-run-plan__note">
            Ingen varige plandetaljer er rapportert ennå.
          </p>
        </Show>

        <For each={data()?.plans ?? []}>
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
                          {/* session-core stores no per-step title, so the API
                              sends `title: ""` and `normalizePlan` already
                              falls back to `operation`. Both fields therefore
                              arrive identical and the step rendered the same
                              token twice ("tool_execution tool_execution").
                              Show the operation underneath only when it really
                              adds something. */}
                          <strong>{humanizePlanStepOperation(step.title || step.operation)}</strong>
                          {/* Prefer the outcome over the operation slug: "permission
                              denied by policy" is what a reader of a failed plan
                              needs, and repeating `tool_execution` is not. */}
                          <Show
                            when={step.detail?.trim()}
                            fallback={(
                              <Show when={step.operation && step.operation !== step.title}>
                                <small>{step.operation}</small>
                              </Show>
                            )}
                          >
                            {(detail) => <small>{detail()}</small>}
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

        <Show when={(data()?.todos.length ?? 0) > 0}>
          <div class="verevon-chat-run-plan__todos">
            <h4>Oppgaver</h4>
            <For each={data()?.todos ?? []}>
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

        <Show when={(data()?.failed ?? 0) > 0}>
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
  const i18n = useI18n()
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
      <section class="verevon-run-panel verevon-run-proof verevon-chat-run-proof" aria-label={i18n.tr('Kjøringskvittering', 'Run receipt')}>
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
          <p class="verevon-run-proof__note">{i18n.tr('Henter kjøringskvittering …', 'Loading the run receipt …')}</p>
        </Show>
        <Show when={proof.error != null}>
          <p class="verevon-run-proof__note">{i18n.tr('Kvitteringen kunne ikke hentes akkurat nå.', 'The receipt could not be loaded right now.')}</p>
        </Show>
        <Show when={!proof.loading && proof.error == null && !proof()}>
          <p class="verevon-run-proof__note">{i18n.tr('Ingen varig kvittering er registrert ennå.', 'No durable receipt has been recorded yet.')}</p>
        </Show>
        <Show when={proof()}>
          {(bundle) => (
            <>
              <Show when={bundle().run}>
                {(run) => <p class="verevon-run-proof__goal">{run().goal || 'Arbeidskjøring'}<Show when={run().status}><span class="verevon-run-proof__agent"> · {run().status}</span></Show></p>}
              </Show>
              <div class="verevon-chat-run-proof__stats">
                <Metric label={i18n.tr('Godkjenninger', 'Approvals')} value={String(bundle().approvals.length)} />
                <Metric label="Verifisert" value={String(verified())} />
                <Show when={effectClass()}>
                  <Metric label="Effektklasse" value={effectClassLabel()} />
                </Show>
                <Show when={outcome()}>
                  {(status) => <Metric label="Utfall" value={status()} />}
                </Show>
              </div>
              <Show when={bundle().approvals.length > 0}>
                <div class="verevon-run-proof__approvals verevon-chat-run-proof__approvals" aria-label={i18n.tr('Godkjenninger og kvitteringer', 'Approvals and receipts')}>
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
  const i18n = useI18n()
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
                <dt>{i18n.tr('Kvittering', 'Receipt')}</dt>
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
  const i18n = useI18n()
  return (
    <div class="verevon-chat-trace-panel">
      <RunProofPanel runId={props.runId} />
      <section class="verevon-chat-trace-log" aria-label={i18n.tr('Trace-hendelser', 'Trace events')}>
        <header>
          <div>
            <h2>Trace</h2>
            <p>
              Revisjonssporet for denne kjøringen: hva som skjedde, i rekkefølge,
              med kvitteringer. Hentes fra den varige hendelsesloggen og er bare
              synlig for deg som eier kjøringen — dette er ikke en delingsflate.
            </p>
          </div>
          <span>{props.events.length} hendelser</span>
        </header>
        <Show when={props.replayTruncated}>
          {/* An audit record that is incomplete must say so. This fires both
              when the durable replay hits its page ceiling and when a long
              live run reaches the same cap in memory. */}
          <p class="verevon-chat-trace-log__notice" role="status">
            Visningen er avkortet ved 5&nbsp;000 hendelser — de eldste vises ikke
            her. Den varige loggen er fortsatt komplett.
          </p>
        </Show>
        <Show
          when={props.events.length > 0}
          fallback={<p class="verevon-chat-trace-log__empty">{i18n.tr('Ingen hendelser er registrert ennå.', 'No events have been recorded yet.')}</p>}
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
