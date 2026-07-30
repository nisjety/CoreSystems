import {
  type Approval,
  type ApprovalDecision,
} from '@/shared/api/orchestration-client'
import {
  AlertCircle,
  Brain,
  Check,
  ChevronRight,
  Copy,
  Download,
  ExternalLink,
  FileCode2,
  Globe2,
  Image as ImageIcon,
  Info,
  MessageSquarePlus,
  MoreHorizontal,
  Paperclip,
  Pencil,
  RefreshCw,
  Sparkles,
  Square,
  ThumbsDown,
  ThumbsUp,
  Volume2,
  Wrench,
  X,
} from 'lucide-solid'
import {
  For,
  Match,
  Show,
  Switch,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  type JSX,
} from 'solid-js'
import {
  dataUriByteSize,
  friendlyMimeLabel,
} from './chat-artifacts'
import {
  buildGeneratedImagePreviews,
  domId,
  formatBytes,
  formatDayLabel,
  formatLatency,
  formatRelative,
  formatTime,
  formatToolArgs,
  formatUsd,
  getTaskStepIcon,
  imageGenerationDisplayContent,
  isGeneratedImageFile,
  isImageArtifact,
  parseInline,
  parseMarkdownBlocks,
  prettyModel,
  readAloud,
} from './chat-media-markdown'
import {
  type AgentTaskStep,
  type ChatKnowledgeGrounding,
  type ChatToolCall,
  type ChatTurn,
  type ComposerAttachment,
  type ComposerToolId,
  type GeneratedFile,
  type GeneratedImagePreview,
  type IconComponent,
  LOW_CONFIDENCE_ANSWER_THRESHOLD,
  type MarkdownBlock,
  type MarkdownListItem,
  OVERFLOW_PROMPTS,
  PRIMARY_PROMPTS,
  TOOL_LABELS,
} from './chat-types'

export function MessageBlock(props: {
  copied: boolean
  message: ChatTurn
  onBranch: () => void
  onCopy: () => void
  onEdit: (text: string) => void
  onFeedback: (rating: 'positive' | 'negative') => Promise<boolean>
  onRegenerate: () => void
  onApprovalDecision: (approvalId: string, decision: ApprovalDecision) => void
  onSelectFollowUp?: (text: string) => void
  onViewSteps: () => void
}) {
  return (
    <Show when={props.message.role === 'assistant'} fallback={<UserMessage {...props} />}>
      <AssistantMessage {...props} />
    </Show>
  )
}

export function AssistantMessage(props: {
  copied: boolean
  message: ChatTurn
  onBranch: () => void
  onCopy: () => void
  onFeedback: (rating: 'positive' | 'negative') => Promise<boolean>
  onRegenerate: () => void
  onApprovalDecision: (approvalId: string, decision: ApprovalDecision) => void
  onSelectFollowUp?: (text: string) => void
  onViewSteps: () => void
}) {
  const [reaction, setReaction] = createSignal<'up' | 'down' | null>(null)

  /**
   * Light the thumb immediately, then keep it only if the rating persisted.
   * Clicking the already-active thumb clears it, matching the previous toggle.
   */
  const rate = async (next: 'up' | 'down', wire: 'positive' | 'negative') => {
    const previous = reaction()
    setReaction(previous === next ? null : next)
    if (!(await props.onFeedback(wire))) setReaction(previous)
  }

  const waiting = () => props.message.status === 'waiting'
  const errored = () => props.message.status === 'error'
  const stopped = () => props.message.status === 'stopped'
  const emptyWaiting = () => waiting() && !props.message.content && !props.message.reasoning
  const files = () => props.message.files ?? []
  const artifacts = () => props.message.artifacts ?? []
  const imagePreviews = createMemo(() => buildGeneratedImagePreviews(files(), artifacts(), props.message.content))
  const displayContent = createMemo(() => (
    imagePreviews().length > 0 ? imageGenerationDisplayContent(props.message.content) : props.message.content
  ))
  const visibleFiles = createMemo(() => files().filter((file) => !isGeneratedImageFile(file)))
  const visibleArtifacts = createMemo(() => artifacts().filter((artifact) => !isImageArtifact(artifact)))
  const artifactCount = () => visibleArtifacts().length

  return (
    <article class="velion-chat-message velion-chat-message--assistant">
      <div class="velion-chat-message__avatar">
        <Sparkles size={14} strokeWidth={1.8} />
      </div>
      <div class="velion-chat-message__body">
        <div class="velion-chat-message__heading">
          <span>Velion</span>
          <time>{formatRelative(props.message.createdAt)}</time>
        </div>
        <Show when={props.message.reasoning}>
          {(reasoning) => <ReasoningTrace text={reasoning()} streaming={waiting()} />}
        </Show>
        <Show
          when={!emptyWaiting()}
          fallback={<ThinkingDots />}
        >
          <Show
            when={!errored()}
            fallback={<ErrorNotice message={props.message.content || 'Stream error'} onRetry={props.onRegenerate} />}
          >
            <div classList={{ 'velion-chat-streaming': waiting() }}>
              <Show when={displayContent()}>
                {(content) => <ChatMarkdown content={content()} />}
              </Show>
              <Show when={stopped()}>
                <span class="velion-chat-status-chip"><Square size={12} /> Stoppet</span>
              </Show>
            </div>
          </Show>
        </Show>
        <Show when={props.message.grounding}>
          {(grounding) => <GroundingInlineSummary grounding={grounding()} />}
        </Show>
        <Show when={!waiting() && !errored() && props.message.confidence != null && (props.message.confidence ?? 1) < LOW_CONFIDENCE_ANSWER_THRESHOLD}>
          <LowConfidenceNotice confidence={props.message.confidence ?? 0} />
        </Show>
        <ToolChips tools={props.message.tools} />
        <AttachmentChips attachments={props.message.attachments} tone="assistant" />
        <Show when={(props.message.toolCalls?.length ?? 0) > 0}>
          <StepsPill calls={props.message.toolCalls ?? []} onViewSteps={props.onViewSteps} />
        </Show>
        <Show when={(props.message.pendingApprovals?.length ?? 0) > 0}>
          <ApprovalRequests
            approvals={props.message.pendingApprovals ?? []}
            onDecide={props.onApprovalDecision}
          />
        </Show>
        <Show when={imagePreviews().length > 0}>
          <GeneratedImagePreviews previews={imagePreviews()} />
        </Show>
        <Show when={visibleFiles().length > 0}>
          <GeneratedFiles files={visibleFiles()} />
        </Show>
        <Show when={artifactCount() > 0}>
          <div class="velion-chat-artifact-chips">
            <For each={visibleArtifacts()}>
              {(artifact) => (
                <span>
                  <FileCode2 size={12} />
                  {artifact.title || artifact.kind}
                </span>
              )}
            </For>
          </div>
        </Show>
        <Show when={!waiting() && !errored() && (props.message.followUps?.length ?? 0) > 0}>
          <FollowUpChips suggestions={props.message.followUps ?? []} onSelect={props.onSelectFollowUp} />
        </Show>
        <Show when={!waiting() && !errored()}>
          <div class="velion-chat-message-actions">
            <MessageAction label={props.copied ? 'Copied' : 'Copy'} onClick={props.onCopy}>
              {props.copied ? <Check size={14} /> : <Copy size={14} />}
            </MessageAction>
            {/*
              The highlight is optimistic so the click feels instant, but it is
              rolled back if the rating did not persist -- otherwise the thumb
              sits lit right next to a "not saved" notice, which reads as though
              the rating was recorded.
            */}
            <MessageAction
              active={reaction() === 'up'}
              label="Good response"
              onClick={() => void rate('up', 'positive')}
            >
              <ThumbsUp size={14} />
            </MessageAction>
            <MessageAction
              active={reaction() === 'down'}
              label="Bad response"
              onClick={() => void rate('down', 'negative')}
            >
              <ThumbsDown size={14} />
            </MessageAction>
            <MessageAction label="Regenerate" onClick={props.onRegenerate}>
              <RefreshCw size={14} />
            </MessageAction>
            <MessageMenu
              items={[
                { label: 'Fortsett i ny chat', icon: <MessageSquarePlus size={16} />, onClick: props.onBranch },
                { label: 'Les høyt', icon: <Volume2 size={16} />, onClick: () => readAloud(props.message.content) },
              ]}
            />
            <MessageMetricsBadge message={props.message} />
            <ReasoningPopover message={props.message} />
          </div>
        </Show>
      </div>
    </article>
  )
}

export function UserMessage(props: {
  copied: boolean
  message: ChatTurn
  onBranch: () => void
  onCopy: () => void
  onEdit: (text: string) => void
}) {
  const [editing, setEditing] = createSignal(false)
  const [draft, setDraft] = createSignal('')

  createEffect(() => {
    if (!editing()) setDraft(props.message.content)
  })

  const startEditing = () => {
    setDraft(props.message.content)
    setEditing(true)
  }

  const submitEdit = () => {
    const next = draft().trim()
    if (!next) return
    setEditing(false)
    props.onEdit(next)
  }

  return (
    <article class="velion-chat-message velion-chat-message--user">
      <div class="velion-chat-user-meta">
        <span>Meg</span>
        <time>{formatRelative(props.message.createdAt)}</time>
      </div>
      <Show
        when={!editing()}
        fallback={(
          <div class="velion-chat-edit-box">
            <textarea
              autofocus
              value={draft()}
              rows={Math.min(10, Math.max(2, draft().split('\n').length))}
              onInput={(event) => setDraft(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
                  event.preventDefault()
                  submitEdit()
                }
                if (event.key === 'Escape') {
                  setEditing(false)
                  setDraft(props.message.content)
                }
              }}
            />
            <div>
              <button type="button" onClick={() => setEditing(false)}>Avbryt</button>
              <button type="button" disabled={!draft().trim()} onClick={submitEdit}>Send på nytt</button>
            </div>
          </div>
        )}
      >
        <>
          <div class="velion-chat-bubble">
            <span class="velion-chat-bubble__text">{props.message.content}</span>
            <ToolChips tools={props.message.tools} />
            <AttachmentChips attachments={props.message.attachments} tone="user" />
          </div>
          <div class="velion-chat-message-actions velion-chat-message-actions--user">
            <MessageAction label="Rediger" onClick={startEditing}>
              <Pencil size={14} />
            </MessageAction>
            <MessageAction label={props.copied ? 'Copied' : 'Copy'} onClick={props.onCopy}>
              {props.copied ? <Check size={14} /> : <Copy size={14} />}
            </MessageAction>
            <MessageMenu
              align="end"
              items={[{ label: 'Fortsett i ny chat', icon: <MessageSquarePlus size={16} />, onClick: props.onBranch }]}
            />
          </div>
        </>
      </Show>
    </article>
  )
}

export function ChatMarkdown(props: { content: string }) {
  return (
    <div class="velion-chat-markdown">
      <For each={parseMarkdownBlocks(props.content)}>
        {(block) => <MarkdownBlockView block={block} />}
      </For>
    </div>
  )
}

export function MarkdownBlockView(props: { block: MarkdownBlock }) {
  return (
    <Switch>
      <Match when={props.block.kind === 'heading'}>
        <DynamicHeading block={props.block as Extract<MarkdownBlock, { kind: 'heading' }>} />
      </Match>
      <Match when={props.block.kind === 'code'}>
        <MarkdownCodeBlock block={props.block as Extract<MarkdownBlock, { kind: 'code' }>} />
      </Match>
      <Match when={props.block.kind === 'table'}>
        <MarkdownTable block={props.block as Extract<MarkdownBlock, { kind: 'table' }>} />
      </Match>
      <Match when={props.block.kind === 'list'}>
        <MarkdownList block={props.block as Extract<MarkdownBlock, { kind: 'list' }>} />
      </Match>
      <Match when={props.block.kind === 'quote'}>
        <blockquote>{parseInline((props.block as Extract<MarkdownBlock, { kind: 'quote' }>).text)}</blockquote>
      </Match>
      <Match when={props.block.kind === 'hr'}>
        <hr />
      </Match>
      <Match when={props.block.kind === 'paragraph'}>
        <p>{parseInline((props.block as Extract<MarkdownBlock, { kind: 'paragraph' }>).text)}</p>
      </Match>
    </Switch>
  )
}

export function DynamicHeading(props: { block: Extract<MarkdownBlock, { kind: 'heading' }> }) {
  return (
    <Switch fallback={<h3>{parseInline(props.block.text)}</h3>}>
      <Match when={props.block.level === 1}>
        <h1>{parseInline(props.block.text)}</h1>
      </Match>
      <Match when={props.block.level === 2}>
        <h2>{parseInline(props.block.text)}</h2>
      </Match>
    </Switch>
  )
}

export function MarkdownCodeBlock(props: { block: Extract<MarkdownBlock, { kind: 'code' }> }) {
  return (
    <div class="velion-chat-codeblock">
      <Show when={props.block.lang}>
        <div class="velion-chat-codeblock__label">{props.block.lang}</div>
      </Show>
      <pre><code>{props.block.text}</code></pre>
    </div>
  )
}

export function MarkdownTable(props: { block: Extract<MarkdownBlock, { kind: 'table' }> }) {
  const alignStyle = (column: number): JSX.CSSProperties | undefined => {
    const align = props.block.align[column]
    return align ? { 'text-align': align } : undefined
  }
  return (
    <div class="velion-chat-table-wrap">
      <table class="velion-chat-table">
        <thead>
          <tr>
            <For each={props.block.header}>
              {(cell, column) => <th style={alignStyle(column())}>{parseInline(cell)}</th>}
            </For>
          </tr>
        </thead>
        <tbody>
          <For each={props.block.rows}>
            {(row) => (
              <tr>
                <For each={row}>
                  {(cell, column) => <td style={alignStyle(column())}>{parseInline(cell)}</td>}
                </For>
              </tr>
            )}
          </For>
        </tbody>
      </table>
    </div>
  )
}

export function MarkdownList(props: { block: Extract<MarkdownBlock, { kind: 'list' }> }) {
  return <>{renderMarkdownListLevel(props.block.items, 0, props.block.items.length, props.block.ordered)}</>
}

/**
 * Renders one nesting level of a flat, depth-annotated item list as a real
 * <ul>/<ol>, recursing for runs of deeper items so nested bullets indent the
 * way GFM renders them. Marker family per level follows the first item of
 * that level, so numbered children under bullets (and vice versa) work.
 */
function renderMarkdownListLevel(items: MarkdownListItem[], start: number, end: number, ordered: boolean): JSX.Element {
  const levelDepth = items[start]?.depth ?? 0
  const nodes: JSX.Element[] = []
  let index = start
  while (index < end) {
    const item = items[index]
    if (!item) break
    let childEnd = index + 1
    while (childEnd < end && (items[childEnd]?.depth ?? 0) > levelDepth) childEnd += 1
    nodes.push(
      <li>
        {parseInline(item.text)}
        {childEnd > index + 1
          ? renderMarkdownListLevel(items, index + 1, childEnd, items[index + 1]?.ordered ?? false)
          : null}
      </li>,
    )
    index = childEnd
  }
  return ordered ? <ol>{nodes}</ol> : <ul>{nodes}</ul>
}

export function ReasoningTrace(props: { text: string; streaming: boolean }) {
  const [open, setOpen] = createSignal(false)
  const expanded = () => open() || props.streaming
  const trimmed = () => props.text.trim()
  return (
    <Show when={trimmed()}>
      <div class="velion-chat-reasoning">
        <button type="button" aria-expanded={expanded()} onClick={() => setOpen((value) => !value)}>
          <Brain size={14} />
          {props.streaming ? 'Tenker ...' : 'Tenkte'}
          <ChevronRight size={14} classList={{ 'velion-chat-rotate': expanded() }} />
        </button>
        <Show when={expanded()}>
          <p>{trimmed()}</p>
        </Show>
      </div>
    </Show>
  )
}

export function ReasoningPopover(props: { message: ChatTurn }) {
  const [open, setOpen] = createSignal(false)
  const [tab, setTab] = createSignal('general')
  let ref!: HTMLDivElement
  const model = () => props.message.modelUsed ?? props.message.model
  const hasMetrics = () => Boolean(
    model()
    || props.message.inputTokens != null
    || props.message.outputTokens != null
    || props.message.latencyMs != null
    || props.message.confidence != null
    || props.message.costUsd != null
    || props.message.reasoning
    || props.message.grounding
    || (props.message.citations?.length ?? 0) > 0
    || (props.message.toolCalls?.length ?? 0) > 0,
  )
  const tabs = () => [
    { id: 'general', label: 'Oversikt' },
    ...(props.message.reasoning ? [{ id: 'insight', label: 'Innsikt' }] : []),
    ...((props.message.toolCalls?.length ?? 0) > 0 ? [{ id: 'tools', label: 'Verktøy' }] : []),
    ...(props.message.grounding || (props.message.citations?.length ?? 0) > 0 ? [{ id: 'sources', label: 'Kilder' }] : []),
  ]

  createEffect(() => {
    if (!open()) return
    const onPointer = (event: PointerEvent) => {
      if (ref && !ref.contains(event.target as Node)) setOpen(false)
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false)
    }
    document.addEventListener('pointerdown', onPointer)
    document.addEventListener('keydown', onKey)
    onCleanup(() => {
      document.removeEventListener('pointerdown', onPointer)
      document.removeEventListener('keydown', onKey)
    })
  })

  return (
    <Show when={hasMetrics()}>
      <div ref={ref} class="velion-chat-reasoning-popover">
        <button type="button" aria-expanded={open()} onClick={() => setOpen((value) => !value)}>
          <Sparkles size={12} />
          <Show when={model()}><span>{prettyModel(model() ?? '')}</span></Show>
          <Show when={props.message.outputTokens}><em>{props.message.outputTokens} tokens</em></Show>
        </button>
        <Show when={open()}>
          <div class="velion-chat-reasoning-popover__panel">
            <div class="velion-chat-reasoning-popover__head">
              <strong>Reasoning</strong>
              <button type="button" aria-label="Lukk" onClick={() => setOpen(false)}>
                <X size={14} />
              </button>
            </div>
            <Show when={tabs().length > 1}>
              <div class="velion-chat-reasoning-popover__tabs">
                <For each={tabs()}>
                  {(item) => (
                    <button type="button" classList={{ 'is-active': tab() === item.id }} onClick={() => setTab(item.id)}>
                      {item.label}
                    </button>
                  )}
                </For>
              </div>
            </Show>
            <Switch>
              <Match when={tab() === 'insight'}>
                <p class="velion-chat-reasoning-popover__copy">{props.message.reasoning}</p>
              </Match>
              <Match when={tab() === 'tools'}>
                <div class="velion-chat-reasoning-popover__stack">
                  <For each={props.message.toolCalls ?? []}>
                    {(call) => <span><Wrench size={13} /> {call.name}</span>}
                  </For>
                </div>
              </Match>
              <Match when={tab() === 'sources'}>
                <div class="velion-chat-reasoning-popover__stack">
                  <For each={props.message.grounding?.sources ?? []}>
                    {(source) => <span>{source.title}</span>}
                  </For>
                  <For each={props.message.citations ?? []}>
                    {(citation) => <a href={citation.url} target="_blank" rel="noopener noreferrer">{citation.title || citation.url}</a>}
                  </For>
                </div>
              </Match>
              <Match when={true}>
                <dl>
                  <Show when={model()}><MetricRow label="Modell" value={prettyModel(model() ?? '')} /></Show>
                  <Show when={props.message.inputTokens != null}><MetricRow label="Input" value={`${props.message.inputTokens} tokens`} /></Show>
                  <Show when={props.message.outputTokens != null}><MetricRow label="Output" value={`${props.message.outputTokens} tokens`} /></Show>
                  <Show when={props.message.latencyMs != null}><MetricRow label="Total tid" value={formatLatency(props.message.latencyMs ?? 0)} /></Show>
                  <Show when={props.message.confidence != null}><MetricRow label="Sikkerhet" value={`${Math.round((props.message.confidence ?? 0) * 100)}%`} /></Show>
                  <Show when={props.message.costUsd != null}><MetricRow label="Kostnad" value={`$${(props.message.costUsd ?? 0).toFixed(4)}`} /></Show>
                </dl>
              </Match>
            </Switch>
          </div>
        </Show>
      </div>
    </Show>
  )
}

export function MetricRow(props: { label: string; value: string }) {
  return (
    <div>
      <dt>{props.label}</dt>
      <dd>{props.value}</dd>
    </div>
  )
}

/**
 * Quiet per-turn cost/token badge (t3.chat/ChatGPT-style, but with real
 * dollar cost since that is already computed server-side). Deliberately
 * separate from `ReasoningPopover`: this is a single-purpose usage readout,
 * not the reasoning/tools/sources drill-down. Renders nothing at all when
 * every one of tokens/latency/cost is missing (an older cached turn from
 * before these fields existed) rather than an empty affordance. A missing
 * `costUsd` (server could not attribute cost to this turn) omits the cost
 * segment instead of ever showing "$0" or "$NaN" for it.
 */
export function MessageMetricsBadge(props: { message: ChatTurn }) {
  const [open, setOpen] = createSignal(false)
  let ref!: HTMLDivElement

  const hasAnyMetric = () => (
    props.message.inputTokens != null
    || props.message.outputTokens != null
    || props.message.latencyMs != null
    || props.message.costUsd != null
  )

  const summary = () => {
    const parts: string[] = []
    if (props.message.inputTokens != null) parts.push(`${props.message.inputTokens} in`)
    if (props.message.outputTokens != null) parts.push(`${props.message.outputTokens} out`)
    if (props.message.latencyMs != null) parts.push(formatLatency(props.message.latencyMs))
    if (props.message.costUsd != null) parts.push(formatUsd(props.message.costUsd))
    return parts.join(' · ')
  }

  createEffect(() => {
    if (!open()) return
    const onPointer = (event: PointerEvent) => {
      if (ref && !ref.contains(event.target as Node)) setOpen(false)
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false)
    }
    document.addEventListener('pointerdown', onPointer)
    document.addEventListener('keydown', onKey)
    onCleanup(() => {
      document.removeEventListener('pointerdown', onPointer)
      document.removeEventListener('keydown', onKey)
    })
  })

  return (
    <Show when={hasAnyMetric()}>
      <div ref={ref} class="velion-chat-metrics-badge">
        <MessageAction label={open() ? summary() : 'Cost & usage'} onClick={() => setOpen((value) => !value)}>
          <Info size={14} />
        </MessageAction>
        <Show when={open()}>
          <div class="velion-chat-metrics-badge__panel" role="note">
            {summary()}
          </div>
        </Show>
      </div>
    </Show>
  )
}

/**
 * A single compact chip under the answer that summarizes tool activity and jumps
 * to the Steps tab, instead of stacking one expandable card per tool call inline.
 * A multi-step ERP turn otherwise buried the answer under a dozen
 * `mcp__…__execute_query` cards; the full detail still lives in the Steps tab.
 */
export function StepsPill(props: { calls: ChatToolCall[]; onViewSteps: () => void }) {
  const total = () => props.calls.length
  const failed = () => props.calls.filter((call) => Boolean(call.error) || call.status === 'error').length
  const running = () => props.calls.some((call) => !call.status || call.status === 'running')
  const label = () => {
    if (running()) return `Bruker verktøy … (${total()})`
    const plural = total() === 1 ? 'steg' : 'steg'
    return `${total()} ${plural}`
  }

  return (
    <button type="button" class="velion-chat-steps-pill" onClick={props.onViewSteps}>
      <Wrench size={13} />
      <span>{label()}</span>
      <Show when={failed() > 0}>
        <em class="velion-chat-steps-pill__failed">{failed()} feilet</em>
      </Show>
      <ChevronRight size={13} />
    </button>
  )
}

/**
 * AI-generated "what to ask next" suggestions (ChatGPT-style), rendered below
 * a completed answer. Clicking a chip only POPULATES the composer — it does
 * not auto-send — matching the existing click-to-populate pattern used by
 * `PRIMARY_PROMPTS`/`OVERFLOW_PROMPTS` in `EmptyChatState` (`onSelectPrompt`
 * there, `onSelectFollowUp` here, both ultimately wired to `setInput`).
 */
export function FollowUpChips(props: { suggestions: string[]; onSelect?: (text: string) => void }) {
  return (
    <div class="velion-chat-followups" role="group" aria-label="Forslag til oppfølgingsspørsmål">
      <For each={props.suggestions}>
        {(suggestion) => (
          <button
            type="button"
            class="velion-chat-followup-chip"
            onClick={() => props.onSelect?.(suggestion)}
          >
            {suggestion}
          </button>
        )}
      </For>
    </div>
  )
}

export function ApprovalRequests(props: {
  approvals: Approval[]
  onDecide: (approvalId: string, decision: ApprovalDecision) => void
}) {
  return (
    <div class="velion-chat-approvals" role="group" aria-label="Godkjenninger">
      <For each={props.approvals}>
        {(approval) => (
          <div class="velion-chat-approval">
            <div class="velion-chat-approval__head">
              <span class="velion-chat-approval__badge">Godkjenning</span>
              <span class="velion-chat-approval__kind">
                {approval.kind ?? 'Agenten venter på godkjenning før neste steg'}
              </span>
            </div>
            <Show when={approval.detail}>
              <p class="velion-chat-approval__detail">{approval.detail}</p>
            </Show>
            <div class="velion-chat-approval__actions">
              <button
                type="button"
                class="velion-chat-approval__approve"
                onClick={() => props.onDecide(approval.id, 'approve')}
              >
                Godkjenn
              </button>
              <button
                type="button"
                class="velion-chat-approval__reject"
                onClick={() => props.onDecide(approval.id, 'reject')}
              >
                Avvis
              </button>
            </div>
          </div>
        )}
      </For>
    </div>
  )
}

export function ToolCallCard(props: { call: ChatToolCall }) {
  const [open, setOpen] = createSignal(false)
  const failed = () => Boolean(props.call.error) || props.call.status === 'error'
  const running = () => !props.call.status || props.call.status === 'running'
  const statusLabel = () => failed() ? 'feilet' : running() ? 'kjører ...' : 'fullført'
  const args = () => formatToolArgs(props.call.args)

  return (
    <div class="velion-chat-tool-call">
      <button type="button" aria-expanded={open()} onClick={() => setOpen((value) => !value)}>
        <Wrench size={14} />
        <span>{props.call.name}</span>
        <em classList={{ 'is-error': failed(), 'is-running': running() }}>{statusLabel()}</em>
        <ChevronRight size={14} classList={{ 'velion-chat-rotate': open() }} />
      </button>
      <Show when={open() && (args() || props.call.output || props.call.error)}>
        <div>
          <Show when={args()}><pre>{args()}</pre></Show>
          <Show when={props.call.output}><pre>{props.call.output}</pre></Show>
          <Show when={props.call.error}><p>{props.call.error}</p></Show>
        </div>
      </Show>
    </div>
  )
}

export function ToolChips(props: { tools: ComposerToolId[] }) {
  return (
    <Show when={props.tools.length > 0}>
      <div class="velion-chat-tool-chips">
        <For each={props.tools}>
          {(tool) => {
            const Icon = toolChipIcon(tool)
            return (
              <span role="img" aria-label={TOOL_LABELS[tool]} title={TOOL_LABELS[tool]}>
                <Icon size={13} aria-hidden="true" />
              </span>
            )
          }}
        </For>
      </div>
    </Show>
  )
}

export function toolChipIcon(tool: ComposerToolId): IconComponent {
  if (tool === 'search') return Globe2
  if (tool === 'reason') return Brain
  if (tool === 'research') return Sparkles
  if (tool === 'image') return FileCode2
  return Wrench
}

export function AttachmentChips(props: { attachments: ComposerAttachment[]; tone: 'assistant' | 'user' }) {
  return (
    <Show when={props.attachments.length > 0}>
      <div class="velion-chat-attachments">
        <For each={props.attachments}>
          {(attachment) => <AttachmentItem attachment={attachment} tone={props.tone} />}
        </For>
      </div>
    </Show>
  )
}

export function AttachmentItem(props: { attachment: ComposerAttachment; tone: 'assistant' | 'user' }) {
  const [failed, setFailed] = createSignal(false)
  const isImage = () => Boolean(props.attachment.url) && props.attachment.type.startsWith('image/') && !failed()

  return (
    <Show
      when={isImage() && props.attachment.url}
      fallback={<span class={`velion-chat-attachment velion-chat-attachment--${props.tone}`}>{props.attachment.name}</span>}
    >
      {(url) => (
        <span class="velion-chat-attachment-image">
          <img src={url()} alt={props.attachment.name} onError={() => setFailed(true)} />
        </span>
      )}
    </Show>
  )
}

export function GeneratedImagePreviews(props: { previews: GeneratedImagePreview[] }) {
  return (
    <div class="velion-chat-image-previews">
      <For each={props.previews}>
        {(preview) => (
          <figure class="velion-chat-image-preview">
            <figcaption>
              <span class="velion-chat-image-preview__title">
                <ImageIcon size={14} />
                <span>{preview.title}</span>
              </span>
              <Show when={preview.size > 0}>
                <small>{formatBytes(preview.size)}</small>
              </Show>
              <div class="velion-chat-image-preview__actions">
                <a
                  href={preview.src}
                  target="_blank"
                  rel="noopener noreferrer"
                  aria-label={`Open ${preview.title}`}
                  title="Open image"
                >
                  <ExternalLink size={14} />
                </a>
                <a
                  href={preview.src}
                  download={preview.downloadName}
                  aria-label={`Download ${preview.title}`}
                  title="Download image"
                >
                  <Download size={14} />
                </a>
              </div>
            </figcaption>
            <a
              class="velion-chat-image-preview__media"
              href={preview.src}
              target="_blank"
              rel="noopener noreferrer"
              aria-label={`Open ${preview.title}`}
            >
              <img src={preview.src} alt={preview.title} loading="lazy" />
            </a>
          </figure>
        )}
      </For>
    </div>
  )
}

/**
 * Non-image generated files (xlsx/docx/pdf/…) attached to a message.
 *
 * These must be downloadable from the message itself, not only from the
 * Artefakter panel. The previous `target="_blank"` link was dead for generated
 * files: `attachment.url` is a `data:` URI, and browsers block top-level
 * navigation to `data:` URLs — clicking did nothing. An `<a download>` is the
 * working path for a data URI, so that is the primary control here; the
 * open-in-tab affordance survives only for real http(s) URLs, where it works.
 */
export function GeneratedFiles(props: { files: GeneratedFile[] }) {
  return (
    <div class="velion-chat-generated-files">
      <For each={props.files}>
        {(file) => {
          const bytes = () => (file.size > 0 ? file.size : dataUriByteSize(file.url))
          const remote = () => /^https?:\/\//i.test(file.url)
          return (
            <span class="velion-chat-generated-file">
              <a href={file.url} download={file.name} aria-label={`Last ned ${file.name}`}>
                <Paperclip size={12} />
                {file.name}
                <em>{friendlyMimeLabel(file.mime)}</em>
                <Show when={bytes() > 0}><span>{formatBytes(bytes())}</span></Show>
                <Download size={12} />
              </a>
              <Show when={remote()}>
                <a
                  class="velion-chat-generated-file__open"
                  href={file.url}
                  target="_blank"
                  rel="noopener noreferrer"
                  aria-label={`Åpne ${file.name}`}
                >
                  <ExternalLink size={12} />
                </a>
              </Show>
            </span>
          )
        }}
      </For>
    </div>
  )
}

/**
 * Visible, inline caveat for an answer scored below
 * `LOW_CONFIDENCE_ANSWER_THRESHOLD` (see chat-types.ts for the threshold
 * rationale). Rendered directly on the message bubble — unlike the
 * `ReasoningPopover`'s "Sikkerhet" metric, this does not require the user to
 * open anything to see it. Independent of `message.grounding`: an answer can
 * be low-confidence with no grounding object at all (an ungrounded guess),
 * which is exactly the case this notice exists to catch.
 */
export function LowConfidenceNotice(props: { confidence: number }) {
  return (
    <p class="velion-chat-low-confidence-notice" role="note">
      <AlertCircle size={12} />
      Usikkert svar ({Math.round(props.confidence * 100)}% sikkerhet) — sjekk kilder før du stoler på dette.
    </p>
  )
}

export function GroundingInlineSummary(props: { grounding: ChatKnowledgeGrounding }) {
  return (
    <div class="velion-chat-grounding-inline">
      <span><Sparkles size={12} /> {props.grounding.sourceCount} internal source{props.grounding.sourceCount === 1 ? '' : 's'}</span>
      <span>{props.grounding.factCount} fact{props.grounding.factCount === 1 ? '' : 's'}</span>
      <Show when={props.grounding.graph?.nodes.length}>
        {(count) => <span>{count()} graph node{count() === 1 ? '' : 's'}</span>}
      </Show>
      <Show when={props.grounding.lowConfidence}>
        <em>Low confidence</em>
      </Show>
    </div>
  )
}

export function ErrorNotice(props: { message: string; onRetry: () => void }) {
  return (
    <div class="velion-chat-error-notice">
      <p><AlertCircle size={16} /> {props.message}</p>
      <button type="button" onClick={() => props.onRetry()}>
        <RefreshCw size={14} />
        Prøv igjen
      </button>
    </div>
  )
}

export function MessageAction(props: { active?: boolean; children: JSX.Element; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      title={props.label}
      aria-label={props.label}
      aria-pressed={props.active}
      classList={{ 'velion-chat-action-button': true, 'velion-chat-action-button--active': Boolean(props.active) }}
      onClick={() => props.onClick()}
    >
      {props.children}
    </button>
  )
}

export function MessageMenu(props: { align?: 'start' | 'end'; items: Array<{ label: string; icon: JSX.Element; onClick: () => void }> }) {
  const [open, setOpen] = createSignal(false)
  let ref!: HTMLDivElement

  createEffect(() => {
    if (!open()) return
    const onPointer = (event: PointerEvent) => {
      if (ref && !ref.contains(event.target as Node)) setOpen(false)
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false)
    }
    document.addEventListener('pointerdown', onPointer)
    document.addEventListener('keydown', onKey)
    onCleanup(() => {
      document.removeEventListener('pointerdown', onPointer)
      document.removeEventListener('keydown', onKey)
    })
  })

  return (
    <div ref={ref} class="velion-chat-menu">
      <button type="button" aria-label="Flere handlinger" aria-expanded={open()} onClick={() => setOpen((value) => !value)}>
        <MoreHorizontal size={14} />
      </button>
      <Show when={open()}>
        <div classList={{ 'velion-chat-menu__panel': true, 'velion-chat-menu__panel--end': props.align === 'end' }}>
          <For each={props.items}>
            {(item) => (
              <button
                type="button"
                onClick={() => {
                  item.onClick()
                  setOpen(false)
                }}
              >
                {item.icon}
                {item.label}
              </button>
            )}
          </For>
        </div>
      </Show>
    </div>
  )
}

export function DateDivider(props: { value: string }) {
  return (
    <div class="velion-chat-divider">
      <span />
      <time>{formatDayLabel(props.value)}</time>
    </div>
  )
}

export function ThinkingDots() {
  return (
    <span class="velion-chat-thinking">
      <span class="velion-thinking-dots" aria-hidden="true">
        <span />
        <span />
        <span />
      </span>
      Tenker
    </span>
  )
}

export function TaskStep(props: { isLast: boolean; step: AgentTaskStep }) {
  const icon = () => getTaskStepIcon(props.step.status)
  const [open, setOpen] = createSignal(false)
  const hasRichDetail = () => Boolean(props.step.expandedDetail?.trim()) || (props.step.evidence?.length ?? 0) > 0
  const detailsId = () => domId(`step-details-${props.step.id}`)
  return (
    <div classList={{ 'velion-chat-step': true, 'velion-chat-step--expandable': true, 'is-open': open() }}>
      <Show when={!props.isLast}>
        <span class="velion-chat-step__line" />
      </Show>
      <span class={`velion-chat-step__icon ${icon().className}`}>{icon().node}</span>
      <div>
        <button
          type="button"
          class="velion-chat-step__heading"
          aria-expanded={open()}
          aria-controls={detailsId()}
          onClick={() => setOpen((value) => !value)}
        >
          <span>
            <strong>{props.step.title}</strong>
            <time>{formatTime(props.step.createdAt)}</time>
          </span>
          <ChevronRight size={13} classList={{ 'velion-chat-rotate': open() }} />
        </button>
        <span class="velion-chat-step__summary">{props.step.detail}</span>
        <Show when={open()}>
          <div id={detailsId()} class="velion-chat-step__details">
            <Show when={(props.step.evidence?.length ?? 0) > 0}>
              <div class="velion-chat-step__evidence">
                <For each={props.step.evidence ?? []}>
                  {(item) => (
                    <Show
                      when={item.href}
                      fallback={<span><strong>{item.label}</strong><em>{item.value}</em></span>}
                    >
                      {(href) => (
                        <a href={href()} target="_blank" rel="noopener noreferrer">
                          <strong>{item.label}</strong>
                          <em>{item.value}</em>
                        </a>
                      )}
                    </Show>
                  )}
                </For>
              </div>
            </Show>
            <Show when={props.step.expandedDetail?.trim()}>
              {(detail) => <pre>{detail()}</pre>}
            </Show>
            <Show when={!hasRichDetail()}>
              <p class="velion-chat-step__detail-copy">{props.step.detail || 'No additional detail captured.'}</p>
            </Show>
          </div>
        </Show>
      </div>
    </div>
  )
}

// ── Empty state ───────────────────────────────────────────────────────────────

export function EmptyChatState(props: { children: JSX.Element; onSelectPrompt: (prompt: string) => void }) {
  const [moreOpen, setMoreOpen] = createSignal(false)
  let moreRef!: HTMLDivElement

  createEffect(() => {
    if (!moreOpen()) return
    const onPointer = (e: PointerEvent) => { if (!moreRef?.contains(e.target as Node)) setMoreOpen(false) }
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') setMoreOpen(false) }
    document.addEventListener('pointerdown', onPointer)
    document.addEventListener('keydown', onKey)
    onCleanup(() => {
      document.removeEventListener('pointerdown', onPointer)
      document.removeEventListener('keydown', onKey)
    })
  })

  const select = (prompt: string) => {
    props.onSelectPrompt(prompt)
    setMoreOpen(false)
  }

  return (
    <div class="velion-chat-empty">
      <div class="velion-chat-empty__inner">
        <div class="velion-chat-empty__heading">
          <Sparkles size={28} strokeWidth={1.75} />
          <h1>Hva kan jeg hjelpe med?</h1>
        </div>
        <div class="velion-chat-empty__composer">{props.children}</div>
        <div class="velion-chat-empty__prompts">
          <For each={PRIMARY_PROMPTS}>
            {({ label, prompt, icon: Icon }) => (
              <button
                type="button"
                class="velion-quick-chip"
                aria-label={`Use quick prompt: ${label}`}
                onClick={() => select(prompt)}
              >
                <Icon size={16} strokeWidth={1.9} />
                <span>{label}</span>
              </button>
            )}
          </For>
          <div ref={moreRef} class="velion-quick-chip-more">
            <button
              type="button"
              class="velion-quick-chip"
              aria-expanded={moreOpen()}
              onClick={() => setMoreOpen((o) => !o)}
            >
              <MoreHorizontal size={16} strokeWidth={1.9} />
              <span>More</span>
            </button>
            <Show when={moreOpen()}>
              <div class="velion-popover velion-quick-chip-menu" role="menu">
                <For each={OVERFLOW_PROMPTS}>
                  {({ label, prompt, icon: Icon }) => (
                    <button type="button" role="menuitem" class="velion-menu-item" onClick={() => select(prompt)}>
                      <Icon size={16} strokeWidth={1.9} />
                      <span>{label}</span>
                    </button>
                  )}
                </For>
              </div>
            </Show>
          </div>
        </div>
      </div>
    </div>
  )
}
