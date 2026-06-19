import {
  Bot,
  BookOpen,
  CheckCircle2,
  ClipboardCheck,
  Map,
  RefreshCw,
  ShieldCheck,
  Sparkles,
  Target,
  XCircle,
  type LucideProps,
} from 'lucide-solid'
import { createMemo, createSignal, For, Show, type Component } from 'solid-js'
import type { LiveKnowledgePayload, LiveKnowledgeSource } from '@/shared/api/knowledge-live-client'
import type {
  OperatingMapAgentBlueprint,
  OperatingMapBlueprintSuggestion,
  OperatingMapProposal,
  OperatingMapSnapshot,
  OperatingMapVersion,
  OperatingMapWorkflow,
} from '@/shared/api/operating-map-client'
import { cn } from '@/shared/lib/cn'
import { Button } from '@/shared/ui/Button'

type OperatingMapIcon = Component<LucideProps>

type Props = {
  busy: boolean
  events: readonly string[]
  liveKnowledge: LiveKnowledgePayload
  loading: boolean
  operatingMap: OperatingMapSnapshot | null
  onCreateBlueprint: (blueprint: OperatingMapAgentBlueprint) => void
  onGenerate: () => void
  onReviewProposal: (proposal: OperatingMapProposal, decision: 'accept' | 'reject') => void
}

const phaseIcon: Record<string, OperatingMapIcon> = {
  Assist: Sparkles,
  Ground: Map,
  Act: Bot,
}

export function KnowledgeOperatingMapCanvas(props: Props) {
  const [selectedWorkflowId, setSelectedWorkflowId] = createSignal<string | null>(null)
  const pendingProposal = createMemo(() =>
    props.operatingMap?.proposals.find((proposal) => proposal.status === 'pending') ?? null,
  )
  const activeVersion = createMemo(() =>
    pendingProposal()?.proposedVersion ?? props.operatingMap?.currentVersion ?? null,
  )
  const sourceCount = createMemo(() => props.liveKnowledge.sources.length + props.liveKnowledge.webSources.length)
  const evidenceCount = createMemo(() => {
    const version = activeVersion()
    if (!version) return 0
    let count = 0
    for (const workflow of version.workflows) {
      count += evidenceForWorkflow(workflow, props.liveKnowledge).length
    }
    return count
  })
  const selectedWorkflow = createMemo(() =>
    activeVersion()?.workflows.find((workflow) => workflow.id === selectedWorkflowId()) ?? null,
  )

  return (
    <main class="knowledge-operating-map">
      <section class="velion-panel knowledge-operating-map-hero">
        <div>
          <span class="knowledge-operating-map-eyebrow">
            <Map class="size-4" />
            AI Operating Map
          </span>
          <h2>Evidence-grounded AI rollout map</h2>
          <p>
            Departments, workflows, rollout phases, agent candidates, risk checks, and learning modules tied back to the Knowledge graph, chunks, crawls, and files.
          </p>
        </div>
        <div class="knowledge-operating-map-hero__actions">
          <div class="knowledge-operating-map-stat">
            <strong>{sourceCount()}</strong>
            <span>source signals</span>
          </div>
          <div class="knowledge-operating-map-stat">
            <strong>{evidenceCount()}</strong>
            <span>evidence links</span>
          </div>
          <Button size="md" onClick={props.onGenerate} disabled={props.busy || props.loading}>
            <RefreshCw class={cn('size-4', props.busy && 'knowledge-spin')} />
            Generate map
          </Button>
        </div>
      </section>

      <Show when={props.events.length > 0}>
        <section class="knowledge-operating-map-events">
          <For each={props.events.slice(-3)}>
            {(event) => <span>{event}</span>}
          </For>
        </section>
      </Show>

      <Show when={props.loading}>
        <section class="velion-panel knowledge-loading-panel">Loading Operating Map...</section>
      </Show>

      <Show when={!props.loading && !activeVersion()}>
        <section class="velion-panel knowledge-operating-map-empty">
          <ClipboardCheck class="size-5" />
          <div>
            <h2>No Operating Map yet</h2>
            <p>Generate a first proposal from the current Knowledge evidence. The proposal must be reviewed before it becomes the accepted map.</p>
          </div>
        </section>
      </Show>

      <Show when={activeVersion()}>
        {(version) => (
          <>
            <ProposalReviewPanel
              busy={props.busy}
              proposal={pendingProposal()}
              version={version()}
              onReviewProposal={props.onReviewProposal}
            />
            <PhaseRail version={version()} />
            <WorkflowGrid
              liveKnowledge={props.liveKnowledge}
              onInspectEvidence={(workflow) => setSelectedWorkflowId(workflow.id)}
              version={version()}
            />
            <Show when={selectedWorkflow()}>
              {(workflow) => (
                <EvidenceInspector
                  evidence={evidenceForWorkflow(workflow(), props.liveKnowledge)}
                  liveKnowledge={props.liveKnowledge}
                  onClose={() => setSelectedWorkflowId(null)}
                  workflow={workflow()}
                />
              )}
            </Show>
            <AgentBlueprintPanel
              proposal={pendingProposal()}
              suggestions={props.operatingMap?.blueprintSuggestions ?? []}
              version={version()}
              onCreateBlueprint={props.onCreateBlueprint}
            />
            <RiskLearningGrid version={version()} />
          </>
        )}
      </Show>
    </main>
  )
}

function ProposalReviewPanel(props: {
  busy: boolean
  proposal: OperatingMapProposal | null
  version: OperatingMapVersion
  onReviewProposal: (proposal: OperatingMapProposal, decision: 'accept' | 'reject') => void
}) {
  return (
    <section class="velion-panel knowledge-operating-map-review">
      <div>
        <h2>{props.proposal ? 'Proposal awaiting review' : 'Accepted Operating Map'}</h2>
        <p>
          Confidence {Math.round(props.version.confidence * 100)}% · {props.version.departments.length} departments · {props.version.workflows.length} workflows · {props.version.agentBlueprints.length} agent candidates
        </p>
      </div>
      <Show when={props.proposal}>
        {(proposal) => (
          <div class="knowledge-operating-map-review__actions">
            <Button size="sm" variant="secondary" disabled={props.busy} onClick={() => props.onReviewProposal(proposal(), 'reject')}>
              <XCircle class="size-4" />
              Reject
            </Button>
            <Button size="sm" variant="primary" disabled={props.busy} onClick={() => props.onReviewProposal(proposal(), 'accept')}>
              <CheckCircle2 class="size-4" />
              Accept
            </Button>
          </div>
        )}
      </Show>
    </section>
  )
}

function PhaseRail(props: { version: OperatingMapVersion }) {
  return (
    <section class="knowledge-operating-map-phase-grid">
      <For each={props.version.rolloutPhases}>
        {(phase) => {
          const Icon = phaseIcon[phase.name] ?? Target
          const workflowCount = () => props.version.workflows.filter((workflow) => workflow.phase === phase.name).length
          return (
            <article class="velion-panel knowledge-operating-map-phase-card">
              <div>
                <Icon class="size-4" />
                <span>{phase.name}</span>
              </div>
              <strong>{workflowCount()}</strong>
              <p>{phase.description}</p>
            </article>
          )
        }}
      </For>
    </section>
  )
}

function WorkflowGrid(props: {
  liveKnowledge: LiveKnowledgePayload
  onInspectEvidence: (workflow: OperatingMapWorkflow) => void
  version: OperatingMapVersion
}) {
  const departmentName = (departmentId: string) =>
    props.version.departments.find((department) => department.id === departmentId)?.name ?? 'Shared'

  return (
    <section>
      <SectionHeader title="Workflow opportunities" description="Each opportunity carries source evidence from the Knowledge workspace." />
      <div class="knowledge-operating-map-workflow-grid">
        <For each={props.version.workflows}>
          {(workflow) => (
            <WorkflowCard
              departmentName={departmentName(workflow.departmentId)}
              evidence={evidenceForWorkflow(workflow, props.liveKnowledge)}
              onInspectEvidence={() => props.onInspectEvidence(workflow)}
              workflow={workflow}
            />
          )}
        </For>
      </div>
    </section>
  )
}

function WorkflowCard(props: {
  departmentName: string
  evidence: LiveKnowledgeSource[]
  onInspectEvidence: () => void
  workflow: OperatingMapWorkflow
}) {
  return (
    <article class="velion-panel knowledge-operating-map-workflow-card">
      <div class="knowledge-operating-map-workflow-card__top">
        <div>
          <span>{props.departmentName}</span>
          <h3>{props.workflow.name}</h3>
        </div>
        <small class={cn('knowledge-status-pill', phaseStatusClass(props.workflow.phase))}>{props.workflow.phase}</small>
      </div>
      <p>Risk: {props.workflow.risk}</p>
      <Button size="sm" variant="secondary" onClick={props.onInspectEvidence}>
        <BookOpen class="size-4" />
        View evidence
      </Button>
      <div class="knowledge-operating-map-evidence-list">
        <For each={props.evidence}>
          {(source) => (
            <div class="knowledge-operating-map-evidence">
              <strong>{source.title}</strong>
              <span>{source.provider} · {source.updated}</span>
              <Show when={source.chunksPreview[0]}>
                {(chunk) => <p>{chunk().text}</p>}
              </Show>
            </div>
          )}
        </For>
      </div>
    </article>
  )
}

function EvidenceInspector(props: {
  evidence: LiveKnowledgeSource[]
  liveKnowledge: LiveKnowledgePayload
  onClose: () => void
  workflow: OperatingMapWorkflow
}) {
  const graphEntities = (source: LiveKnowledgeSource) =>
    props.liveKnowledge.graph.nodes.filter((node) =>
      node.sourceIds.includes(source.id) ||
      source.chunksPreview.some((chunk) => node.sourceRefs.includes(chunk.id)),
    )

  return (
    <section class="velion-panel knowledge-operating-map-evidence-drawer" aria-label={`Evidence for ${props.workflow.name}`}>
      <div class="knowledge-operating-map-evidence-drawer__top">
        <div>
          <span class="knowledge-operating-map-eyebrow">
            <BookOpen class="size-4" />
            Evidence
          </span>
          <h2>Evidence for {props.workflow.name}</h2>
        </div>
        <Button size="sm" variant="secondary" onClick={props.onClose}>
          <XCircle class="size-4" />
          Close
        </Button>
      </div>
      <div class="knowledge-operating-map-evidence-drawer__grid">
        <For each={props.evidence}>
          {(source) => (
            <article class="knowledge-operating-map-evidence-detail">
              <div>
                <strong>{source.title}</strong>
                <span>{source.provider} · {source.category} · {source.updated}</span>
              </div>
              <dl>
                <div>
                  <dt>Status</dt>
                  <dd>{source.status}</dd>
                </div>
                <div>
                  <dt>Freshness</dt>
                  <dd>{source.coverage}</dd>
                </div>
                <div>
                  <dt>Confidence</dt>
                  <dd>{source.similarity}</dd>
                </div>
              </dl>
              <Show when={graphEntities(source).length > 0}>
                <p>Graph: {graphEntities(source).map((entity) => entity.label).join(', ')}</p>
              </Show>
              <For each={source.chunksPreview.slice(0, 2)}>
                {(chunk) => (
                  <blockquote>
                    <span>{chunk.title} · {chunk.score}</span>
                    <p>{chunk.text}</p>
                  </blockquote>
                )}
              </For>
            </article>
          )}
        </For>
      </div>
    </section>
  )
}

function AgentBlueprintPanel(props: {
  proposal: OperatingMapProposal | null
  suggestions: readonly OperatingMapBlueprintSuggestion[]
  version: OperatingMapVersion
  onCreateBlueprint: (blueprint: OperatingMapAgentBlueprint) => void
}) {
  const suggestedIds = createMemo(() =>
    new Set(
      props.suggestions
        .filter((suggestion) => suggestion.versionId === props.version.id)
        .map((suggestion) => suggestion.blueprintId),
    ),
  )

  return (
    <section>
      <SectionHeader title="Agent blueprint candidates" description="Approved workflows can become Velion agent blueprints for service, sales, ecommerce, chatbot, or workflow roles." />
      <div class="knowledge-operating-map-blueprint-grid">
        <For each={props.version.agentBlueprints}>
          {(blueprint) => {
            const alreadySuggested = () => suggestedIds().has(blueprint.id)
            const disabled = () => Boolean(props.proposal) || alreadySuggested()
            return (
              <article class="velion-panel knowledge-operating-map-blueprint-card">
                <div>
                  <Bot class="size-4" />
                  <span>{blueprint.role}</span>
                </div>
                <h3>{blueprint.name}</h3>
                <p>
                  {props.proposal
                    ? 'Accept the Operating Map before creating this blueprint suggestion'
                    : alreadySuggested()
                      ? 'Already suggested for Agents review'
                      : blueprint.requiresApproval
                        ? 'Approval-gated before deploy'
                        : 'Ready for low-risk queueing'}
                </p>
                <Button size="sm" variant="secondary" disabled={disabled()} onClick={() => props.onCreateBlueprint(blueprint)}>
                  <Sparkles class="size-4" />
                  {alreadySuggested() ? 'Suggested' : 'Create blueprint'}
                </Button>
              </article>
            )
          }}
        </For>
      </div>
    </section>
  )
}

function RiskLearningGrid(props: { version: OperatingMapVersion }) {
  return (
    <section class="knowledge-operating-map-risk-learning">
      <div>
        <SectionHeader title="Risk overlay" description="Controls to validate before rollout." />
        <div class="knowledge-operating-map-list">
          <For each={props.version.riskOverlays}>
            {(risk) => (
              <article class="velion-panel knowledge-operating-map-list-card">
                <ShieldCheck class="size-4" />
                <div>
                  <h3>{risk.label}</h3>
                  <p>{risk.severity}</p>
                </div>
              </article>
            )}
          </For>
        </div>
      </div>
      <div>
        <SectionHeader title="Team learning" description="Short enablement modules linked to the rollout." />
        <div class="knowledge-operating-map-list">
          <For each={props.version.learningModules}>
            {(module) => (
              <article class="velion-panel knowledge-operating-map-list-card">
                <BookOpen class="size-4" />
                <div>
                  <h3>{module.title}</h3>
                  <p>{module.audience}</p>
                </div>
              </article>
            )}
          </For>
        </div>
      </div>
    </section>
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

function evidenceForWorkflow(workflow: OperatingMapWorkflow, liveKnowledge: LiveKnowledgePayload): LiveKnowledgeSource[] {
  const refs = new Set(workflow.evidenceRefs)
  const direct = liveKnowledge.sources.filter((source) =>
    refs.has(source.id) ||
    source.chunksPreview.some((chunk) => refs.has(chunk.id)),
  )
  if (direct.length > 0) return direct.slice(0, 2)

  const normalized = `${workflow.name} ${workflow.departmentId}`.toLowerCase()
  const matched = liveKnowledge.sources.filter((source) =>
    [source.title, source.category, ...source.tags, ...source.related]
      .some((value) => normalized.includes(value.toLowerCase()) || value.toLowerCase().includes(workflow.departmentId.toLowerCase())),
  )
  return (matched.length > 0 ? matched : liveKnowledge.sources).slice(0, 2)
}

function phaseStatusClass(phase: string) {
  if (phase === 'Act') return 'knowledge-status--review'
  if (phase === 'Ground') return 'knowledge-status--syncing'
  return 'knowledge-status--connected'
}
