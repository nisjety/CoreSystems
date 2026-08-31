import {
  type Approval,
  type ApprovalDecision,
} from '@/shared/api/orchestration-client'
import {
  AlertCircle,
  Brain,
  Check,
  ChevronLeft,
  ChevronRight,
  Copy,
  Download,
  ExternalLink,
  FileCode2,
  FileText,
  GitCompare,
  Globe2,
  Image as ImageIcon,
  Info,
  MessageSquarePlus,
  MoreHorizontal,
  Paperclip,
  Pencil,
  RefreshCw,
  Search,
  Sparkles,
  Square,
  TerminalSquare,
  ThumbsDown,
  ThumbsUp,
  Volume2,
  Wrench,
  X,
} from '@/shared/icons'
import {
  createConversationNodeRegistry,
  deriveConversationNodes,
  parseUnifiedDiff,
  toolPresentation,
  type AnswerState,
  type ConversationNodeContext,
  type DiffResult,
  type PlanApprovalStatus,
  type ToolIntent,
} from '@/shared/chat-nodes'
import type { AutonomyRung, MemoryOrigin, RecalledMemory } from '@/shared/api/chat-client'
import { MIN_PLAN_JUSTIFICATION_CHARS } from '@/shared/api/chat-client'
import {
  For,
  Match,
  Show,
  Switch,
  createEffect,
  createMemo,
  createSignal,
} from 'solid-js'
import type { JSX } from '@solidjs/web'
import {
  dataUriByteSize,
  friendlyMimeLabel,
} from './chat-artifacts'
import {
  domId,
  formatBytes,
  formatDayLabel,
  formatLatency,
  formatRelative,
  formatTime,
  formatToolArgs,
  formatUsd,
  getTaskStepIcon,
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
  type Citation,
  type QueuedInput,
  type ChatTurnAttachment,
  type ComposerToolId,
  type GeneratedFile,
  type GeneratedImagePreview,
  type IconComponent,
  type MarkdownBlock,
  type MarkdownListItem,
  TOOL_LABELS,
} from './chat-types'
import type { VersionBadge } from '@/features/chat/lib/chat-versions'
import { isEffectfulChatTurn } from '@/shared/chat/effect-class'
import { useI18n } from '@/shared/i18n'

export function MessageBlock(props: {
  copied: boolean
  message: ChatTurn
  onBranch: () => void
  onCopy: () => void
  onEdit: (text: string) => void
  onFeedback: (rating: 'positive' | 'negative') => Promise<boolean>
  onRegenerate: () => void
  onRerunAsNewTurn?: () => void
  onApprovalDecision: (approvalId: string, decision: ApprovalDecision) => void
  onApprovePlan: (rung: AutonomyRung, justification: string) => void
  planApproval?: PlanApprovalStatus
  onSelectFollowUp?: (text: string) => void
  onViewSteps: () => void
  onViewAttachments?: (attachmentId: string) => void
  /** n/N badge for this turn's exchange versions — only ever set on the trailing assistant turn. */
  version?: VersionBadge | null
  onSelectVersion?: (target: number) => void
  /** Effectful turns cannot be edited or regenerated in place. */
  editLocked?: boolean
}) {
  // `<For>` invokes its mapper untracked in Solid 2. Read the store-backed
  // role through a memo so the conditional branch is established in a tracked
  // scope before `<Show>` receives its plain boolean value.
  const isAssistant = createMemo(() => props.message.role === 'assistant')
  return (
    <Show when={isAssistant()} fallback={<UserMessage {...props} />}>
      <AssistantMessage {...props} />
    </Show>
  )
}

/**
 * The chat surface's node renderers.
 *
 * Defined once at module scope, not per message: the map is static, and building
 * it inside the component would allocate on every render. Typed as an exhaustive
 * record, so adding a `ConversationNode` kind without a renderer fails the build
 * rather than silently rendering nothing.
 */
const CHAT_NODE_REGISTRY = createConversationNodeRegistry({
  reasoning: {
    kind: 'reasoning',
    render: (node) => (
      <ReasoningTrace text={node.text} streaming={node.streaming} />
    ),
  },
  answer: {
    kind: 'answer',
    render: (node, ctx) => (
      <AnswerRegion answer={node.answer} onRegenerate={ctx.onRegenerate} citations={ctx.citations} />
    ),
  },
  grounding: {
    kind: 'grounding',
    render: (node) => <GroundingInlineSummary grounding={node.grounding} />,
  },
  'low-confidence': {
    kind: 'low-confidence',
    render: (node) => <LowConfidenceNotice confidence={node.confidence} />,
  },
  'memory-recall': {
    kind: 'memory-recall',
    render: (node) => (
      <MemoryRecallNotice count={node.count} memories={node.memories} />
    ),
  },
  truncated: {
    kind: 'truncated',
    render: (node) => <TruncatedAnswerNotice stopReason={node.stopReason} />,
  },
  'tool-chips': {
    kind: 'tool-chips',
    render: (node) => <ToolChips tools={node.tools} />,
  },
  attachments: {
    kind: 'attachments',
    render: (node, ctx) => (
      <AttachmentChips attachments={node.attachments} tone="assistant" onOpen={ctx.onViewAttachments} />
    ),
  },
  steps: {
    kind: 'steps',
    render: (node, ctx) => (
      <StepsPill calls={node.calls} onViewSteps={ctx.onViewSteps} />
    ),
  },
  approvals: {
    kind: 'approvals',
    render: (node, ctx) => (
      <ApprovalRequests
        approvals={node.approvals}
        onDecide={ctx.onApprovalDecision}
      />
    ),
  },
  'plan-approval': {
    kind: 'plan-approval',
    render: (node, ctx) => (
      <PlanApprovalControl
        grantedRung={node.grantedRung}
        pending={node.pending}
        error={node.error}
        onApprove={ctx.onApprovePlan}
      />
    ),
  },
  'image-previews': {
    kind: 'image-previews',
    render: (node) => <GeneratedImagePreviews previews={node.previews} />,
  },
  files: {
    kind: 'files',
    render: (node) => <GeneratedFiles files={node.files} />,
  },
  artifacts: {
    kind: 'artifacts',
    render: (node) => (
      <div class="verevon-chat-artifact-chips">
        <For each={node.artifacts}>
          {(artifact) => (
            <span>
              <FileCode2 size={12} />
              {artifact.title || artifact.kind}
            </span>
          )}
        </For>
      </div>
    ),
  },
  'follow-ups': {
    kind: 'follow-ups',
    render: (node, ctx) => (
      <FollowUpChips
        suggestions={node.suggestions}
        onSelect={ctx.onSelectFollowUp}
      />
    ),
  },
})

/**
 * The answer region: thinking indicator, error, or content.
 *
 * One component for all three because they share the streaming-class wrapper and
 * were previously chosen by a nested `Show`/fallback pair — a structure that made
 * rendering two of them at once impossible. Keeping that guarantee is why
 * `AnswerState` is a discriminated union rather than three separate nodes.
 */
function AnswerRegion(props: {
  answer: AnswerState
  onRegenerate: () => void
  citations?: readonly Citation[]
}) {
  return (
    <Switch>
      <Match when={props.answer.state === 'pending'}>
        <ThinkingDots />
      </Match>
      <Match when={props.answer.state === 'failed' ? props.answer : null}>
        {(failed) => (
          <ErrorNotice
            message={failed().message}
            onRetry={props.onRegenerate}
          />
        )}
      </Match>
      <Match when={props.answer.state === 'content' ? props.answer : null}>
        {(content) => (
          <div class={{ 'verevon-chat-streaming': content().streaming }}>
            <Show when={content().content}>
              {(text) => <ChatMarkdown content={text()} citations={props.citations} />}
            </Show>
            <Show when={content().stopped}>
              <span class="verevon-chat-status-chip">
                <Square size={12} /> Stoppet
              </span>
            </Show>
          </div>
        )}
      </Match>
    </Switch>
  )
}

export function AssistantMessage(props: {
  copied: boolean
  message: ChatTurn
  onBranch: () => void
  onCopy: () => void
  onFeedback: (rating: 'positive' | 'negative') => Promise<boolean>
  onRegenerate: () => void
  onRerunAsNewTurn?: () => void
  onApprovalDecision: (approvalId: string, decision: ApprovalDecision) => void
  onApprovePlan: (rung: AutonomyRung, justification: string) => void
  planApproval?: PlanApprovalStatus
  onSelectFollowUp?: (text: string) => void
  onViewSteps: () => void
  onViewAttachments?: (attachmentId: string) => void
  version?: VersionBadge | null
  onSelectVersion?: (target: number) => void
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

  const waiting = createMemo(() => props.message.status === 'waiting')
  const errored = createMemo(() => props.message.status === 'error')
  const effectful = createMemo(() => isEffectfulChatTurn(props.message.effectClass))

  // What this turn renders, as data. The conditions and order that used to live
  // inline as fourteen nested `<Show>` blocks are now one reviewable function
  // (`deriveConversationNodes`) with its own tests — including the three-path
  // equivalence gate, which caught `stopReason`/`memoryRecallCount` being
  // dropped on reload.
  const nodes = createMemo(() => deriveConversationNodes(props.message, props.planApproval))
  // `<For>` executes its mapper untracked in Solid 2. Build each render entry
  // in a memo so the mapper receives plain node/context data and never reads a
  // live prop (notably `message.citations`) outside a tracking scope.
  const nodeEntries = createMemo(() => {
    const context: ConversationNodeContext = {
      onViewSteps: props.onViewSteps,
      onViewAttachments: props.onViewAttachments,
      onApprovalDecision: props.onApprovalDecision,
      onApprovePlan: props.onApprovePlan,
      onSelectFollowUp: props.onSelectFollowUp,
      onRegenerate: props.onRegenerate,
      citations: props.message.citations,
    }
    return nodes().map((node) => ({ node, context }))
  })

  return (
    <article class="verevon-chat-message verevon-chat-message--assistant">
      <div class="verevon-chat-message__avatar">
        <span class="verevon-chat-message__logo" aria-hidden="true" />
      </div>
      <div class="verevon-chat-message__body">
        <div class="verevon-chat-message__heading">
          <span>Verevon</span>
          <time>{formatRelative(props.message.createdAt)}</time>
        </div>
        {/*
          Rendered from the derived node list through the keyed registry. Adding
          a node kind means adding a renderer to CHAT_NODE_REGISTRY and a case to
          `deriveConversationNodes` — not editing this component. `keyed` is off
          deliberately: nodes are recreated each derivation, and keying on
          identity would remount every node on every token during streaming.
        */}
        <For each={nodeEntries()}>
          {(entry) => CHAT_NODE_REGISTRY.render(entry.node, entry.context)}
        </For>
        <Show when={!waiting() && !errored()}>
          <div class="verevon-chat-message-actions">
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
            <Show when={props.version}>
              {(version) => (
                <div class="verevon-chat-version-switcher" role="group" aria-label="Answer version">
                  <button
                    type="button"
                    class="verevon-chat-version-switcher__arrow"
                    disabled={version().current <= 1}
                    aria-label="Previous version"
                    onClick={() => props.onSelectVersion?.(version().current - 2)}
                  >
                    <ChevronLeft size={14} />
                  </button>
                  <span class="verevon-chat-version-switcher__label">
                    {version().current}/{version().total}
                  </span>
                  <button
                    type="button"
                    class="verevon-chat-version-switcher__arrow"
                    disabled={version().current >= version().total}
                    aria-label="Next version"
                    onClick={() => props.onSelectVersion?.(version().current)}
                  >
                    <ChevronRight size={14} />
                  </button>
                </div>
              )}
            </Show>
            <Show
              when={!effectful()}
              fallback={(
                <>
                  <span class="verevon-chat-action-note" role="note">
                    Effekt registrert — kan ikke endres
                  </span>
                  <Show when={props.onRerunAsNewTurn}>
                    <MessageAction label="Kjør som ny tur" onClick={() => props.onRerunAsNewTurn?.()}>
                      <RefreshCw size={14} />
                    </MessageAction>
                  </Show>
                </>
              )}
            >
              <MessageAction label="Regenerate" onClick={props.onRegenerate}>
                <RefreshCw size={14} />
              </MessageAction>
            </Show>
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
  onViewAttachments?: (attachmentId: string) => void
  editLocked?: boolean
}) {
  const [editing, setEditing] = createSignal(false)
  const [draft, setDraft] = createSignal('')

  createEffect(
    () => ({ editing: editing(), content: props.message.content }),
    ({ editing, content }) => {
      if (!editing) setDraft(content)
    },
  )

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
    <article class="verevon-chat-message verevon-chat-message--user">
      <div class="verevon-chat-user-meta">
        <span>Meg</span>
        <time>{formatRelative(props.message.createdAt)}</time>
      </div>
      <Show
        when={!editing()}
        fallback={(
          <div class="verevon-chat-edit-box">
            <textarea
              // Focus imperatively after the branch mounts rather than relying
              // on the `autofocus` attribute, which is unreliable for a
              // dynamically-inserted element (fires only for the first candidate
              // and only when nothing else is focused). Caret to the end so the
              // agent can adjust the message immediately.
              ref={(el) => queueMicrotask(() => {
                el.focus()
                el.setSelectionRange(el.value.length, el.value.length)
              })}
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
          <div class="verevon-chat-bubble">
            <span class="verevon-chat-bubble__text">{props.message.content}</span>
            <ToolChips tools={props.message.tools} />
            <AttachmentChips attachments={props.message.attachments} tone="user" onOpen={props.onViewAttachments} />
          </div>
          <div class="verevon-chat-message-actions verevon-chat-message-actions--user">
            <Show
              when={!props.editLocked}
              fallback={(
                <span class="verevon-chat-action-note" role="note">
                  Effektiv tur — redigering lager ny tur
                </span>
              )}
            >
              <MessageAction label="Rediger" onClick={startEditing}>
                <Pencil size={14} />
              </MessageAction>
            </Show>
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

export function ChatMarkdown(props: { content: string; citations?: readonly Citation[] }) {
  return (
    <div class="verevon-chat-markdown">
      <For each={parseMarkdownBlocks(props.content)}>
        {(block) => <MarkdownBlockView block={block} citations={props.citations} />}
      </For>
    </div>
  )
}

export function MarkdownBlockView(props: { block: MarkdownBlock; citations?: readonly Citation[] }) {
  return (
    <Switch>
      <Match when={props.block.kind === 'heading'}>
        <DynamicHeading block={props.block as Extract<MarkdownBlock, { kind: 'heading' }>} citations={props.citations} />
      </Match>
      <Match when={props.block.kind === 'code'}>
        <MarkdownCodeBlock block={props.block as Extract<MarkdownBlock, { kind: 'code' }>} />
      </Match>
      <Match when={props.block.kind === 'table'}>
        <MarkdownTable block={props.block as Extract<MarkdownBlock, { kind: 'table' }>} citations={props.citations} />
      </Match>
      <Match when={props.block.kind === 'list'}>
        <MarkdownList block={props.block as Extract<MarkdownBlock, { kind: 'list' }>} citations={props.citations} />
      </Match>
      <Match when={props.block.kind === 'quote'}>
        <blockquote>{parseInline((props.block as Extract<MarkdownBlock, { kind: 'quote' }>).text, props.citations)}</blockquote>
      </Match>
      <Match when={props.block.kind === 'hr'}>
        <hr />
      </Match>
      <Match when={props.block.kind === 'paragraph'}>
        <p>{parseInline((props.block as Extract<MarkdownBlock, { kind: 'paragraph' }>).text, props.citations)}</p>
      </Match>
    </Switch>
  )
}

export function DynamicHeading(props: { block: Extract<MarkdownBlock, { kind: 'heading' }>; citations?: readonly Citation[] }) {
  return (
    <Switch fallback={<h3>{parseInline(props.block.text, props.citations)}</h3>}>
      <Match when={props.block.level === 1}>
        <h1>{parseInline(props.block.text, props.citations)}</h1>
      </Match>
      <Match when={props.block.level === 2}>
        <h2>{parseInline(props.block.text, props.citations)}</h2>
      </Match>
    </Switch>
  )
}

export function MarkdownCodeBlock(props: { block: Extract<MarkdownBlock, { kind: 'code' }> }) {
  return (
    <div class="verevon-chat-codeblock">
      <Show when={props.block.lang}>
        <div class="verevon-chat-codeblock__label">{props.block.lang}</div>
      </Show>
      <pre><code>{props.block.text}</code></pre>
    </div>
  )
}

export function MarkdownTable(props: { block: Extract<MarkdownBlock, { kind: 'table' }>; citations?: readonly Citation[] }) {
  const alignStyle = (column: number): JSX.CSSProperties | undefined => {
    const align = props.block.align[column]
    return align ? { 'text-align': align } : undefined
  }
  return (
    <div class="verevon-chat-table-wrap">
      <table class="verevon-chat-table">
        <thead>
          <tr>
            <For each={props.block.header}>
              {(cell, column) => <th style={alignStyle(column())}>{parseInline(cell, props.citations)}</th>}
            </For>
          </tr>
        </thead>
        <tbody>
          <For each={props.block.rows}>
            {(row) => (
              <tr>
                <For each={row}>
                  {(cell, column) => <td style={alignStyle(column())}>{parseInline(cell, props.citations)}</td>}
                </For>
              </tr>
            )}
          </For>
        </tbody>
      </table>
    </div>
  )
}

export function MarkdownList(props: { block: Extract<MarkdownBlock, { kind: 'list' }>; citations?: readonly Citation[] }) {
  return <>{renderMarkdownListLevel(props.block.items, 0, props.block.items.length, props.block.ordered, props.citations)}</>
}

/**
 * Renders one nesting level of a flat, depth-annotated item list as a real
 * <ul>/<ol>, recursing for runs of deeper items so nested bullets indent the
 * way GFM renders them. Marker family per level follows the first item of
 * that level, so numbered children under bullets (and vice versa) work.
 */
function renderMarkdownListLevel(items: MarkdownListItem[], start: number, end: number, ordered: boolean, citations?: readonly Citation[]): JSX.Element {
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
        {parseInline(item.text, citations)}
        {childEnd > index + 1
          ? renderMarkdownListLevel(items, index + 1, childEnd, items[index + 1]?.ordered ?? false, citations)
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
      <div class="verevon-chat-reasoning">
        <button type="button" aria-expanded={expanded() ? 'true' : 'false'} onClick={() => setOpen((value) => !value)}>
          <Brain size={14} />
          {props.streaming ? 'Tenker ...' : 'Tenkte'}
          <ChevronRight size={14} class={{ 'verevon-chat-rotate': expanded() }} />
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
  const model = createMemo(() => props.message.modelUsed ?? props.message.model)
  const hasMetrics = createMemo(() => Boolean(
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
  ))
  const tabs = createMemo(() => [
    { id: 'general', label: 'Oversikt' },
    ...(props.message.reasoning ? [{ id: 'insight', label: 'Innsikt' }] : []),
    ...((props.message.toolCalls?.length ?? 0) > 0 ? [{ id: 'tools', label: 'Verktøy' }] : []),
    ...(props.message.grounding || (props.message.citations?.length ?? 0) > 0 ? [{ id: 'sources', label: 'Kilder' }] : []),
  ])

  createEffect(
    () => open(),
    (isOpen) => {
      if (!isOpen) return
      const onPointer = (event: PointerEvent) => {
        if (ref && !ref.contains(event.target as Node)) setOpen(false)
      }
      const onKey = (event: KeyboardEvent) => {
        if (event.key === 'Escape') setOpen(false)
      }
      document.addEventListener('pointerdown', onPointer)
      document.addEventListener('keydown', onKey)
      return () => {
        document.removeEventListener('pointerdown', onPointer)
        document.removeEventListener('keydown', onKey)
      }
    },
  )

  return (
    <Show when={hasMetrics()}>
      <div ref={ref} class="verevon-chat-reasoning-popover">
        <button type="button" aria-expanded={open() ? 'true' : 'false'} onClick={() => setOpen((value) => !value)}>
          <Sparkles size={12} />
          <Show when={model()}><span>{prettyModel(model() ?? '')}</span></Show>
          <Show when={props.message.outputTokens}><em>{props.message.outputTokens} tokens</em></Show>
        </button>
        <Show when={open()}>
          <div class="verevon-chat-reasoning-popover__panel">
            <div class="verevon-chat-reasoning-popover__head">
              <strong>Reasoning</strong>
              <button type="button" aria-label="Lukk" onClick={() => setOpen(false)}>
                <X size={14} />
              </button>
            </div>
            <Show when={tabs().length > 1}>
              <div class="verevon-chat-reasoning-popover__tabs">
                <For each={tabs()}>
                  {(item) => (
                    <button type="button" class={{ 'is-active': tab() === item.id }} onClick={() => setTab(item.id)}>
                      {item.label}
                    </button>
                  )}
                </For>
              </div>
            </Show>
            <Switch>
              <Match when={tab() === 'insight'}>
                <p class="verevon-chat-reasoning-popover__copy">{props.message.reasoning}</p>
              </Match>
              <Match when={tab() === 'tools'}>
                <div class="verevon-chat-reasoning-popover__stack">
                  <For each={props.message.toolCalls ?? []}>
                    {(call) => <span><Wrench size={13} /> {call.name}</span>}
                  </For>
                </div>
              </Match>
              <Match when={tab() === 'sources'}>
                <div class="verevon-chat-reasoning-popover__stack">
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

  const hasAnyMetric = createMemo(() => (
    props.message.inputTokens != null
    || props.message.outputTokens != null
    || props.message.latencyMs != null
    || props.message.costUsd != null
  ))

  const summary = createMemo(() => {
    const parts: string[] = []
    if (props.message.inputTokens != null) parts.push(`${props.message.inputTokens} in`)
    if (props.message.outputTokens != null) parts.push(`${props.message.outputTokens} out`)
    if (props.message.latencyMs != null) parts.push(formatLatency(props.message.latencyMs))
    if (props.message.costUsd != null) parts.push(formatUsd(props.message.costUsd))
    return parts.join(' · ')
  })

  createEffect(
    () => open(),
    (isOpen) => {
      if (!isOpen) return
      const onPointer = (event: PointerEvent) => {
        if (ref && !ref.contains(event.target as Node)) setOpen(false)
      }
      const onKey = (event: KeyboardEvent) => {
        if (event.key === 'Escape') setOpen(false)
      }
      document.addEventListener('pointerdown', onPointer)
      document.addEventListener('keydown', onKey)
      return () => {
        document.removeEventListener('pointerdown', onPointer)
        document.removeEventListener('keydown', onKey)
      }
    },
  )

  return (
    <Show when={hasAnyMetric()}>
      <div ref={ref} class="verevon-chat-metrics-badge">
        <MessageAction label={open() ? summary() : 'Cost & usage'} onClick={() => setOpen((value) => !value)}>
          <Info size={14} />
        </MessageAction>
        <Show when={open()}>
          <div class="verevon-chat-metrics-badge__panel" role="note">
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
    <button type="button" class="verevon-chat-steps-pill" onClick={props.onViewSteps}>
      <Wrench size={13} />
      <span>{label()}</span>
      <Show when={failed() > 0}>
        <em class="verevon-chat-steps-pill__failed">{failed()} feilet</em>
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
    <div class="verevon-chat-followups" role="group" aria-label="Forslag til oppfølgingsspørsmål">
      <For each={props.suggestions}>
        {(suggestion) => (
          <button
            type="button"
            class="verevon-chat-followup-chip"
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
    <div class="verevon-chat-approvals" role="group" aria-label="Godkjenninger">
      <For each={props.approvals}>
        {(approval) => (
          <div class="verevon-chat-approval">
            <div class="verevon-chat-approval__head">
              <span class="verevon-chat-approval__badge">Godkjenning</span>
              <span class="verevon-chat-approval__kind">
                {approval.kind ?? 'Agenten venter på godkjenning før neste steg'}
              </span>
            </div>
            <Show when={approval.detail}>
              <p class="verevon-chat-approval__detail">{approval.detail}</p>
            </Show>
            <div class="verevon-chat-approval__actions">
              <button
                type="button"
                class="verevon-chat-approval__approve"
                onClick={() => props.onDecide(approval.id, 'approve')}
              >
                Godkjenn
              </button>
              <button
                type="button"
                class="verevon-chat-approval__reject"
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

/**
 * A unified diff, rendered as hunks.
 *
 * Shared by both patch producers — a tool that returned a patch, and an artifact
 * revision diffed client-side — because a diff is a diff and two renderers would
 * drift. Line numbers are deliberately omitted: the hunk header carries the
 * position, and per-line numbers in a 320px card cost more width than they earn.
 */
export function DiffView(props: { result: DiffResult }) {
  return (
    <div class="verevon-chat-diff">
      <div class="verevon-chat-diff__stat">
        <span class="verevon-chat-diff__added">+{props.result.stat.added}</span>
        <span class="verevon-chat-diff__removed">
          −{props.result.stat.removed}
        </span>
        <Show when={props.result.truncated}>
          {/* An empty hunk list would otherwise read as "no changes". */}
          {/* Norwegian inline, matching this file's convention (no i18n hook
              here — see the sibling notices). */}
          <span class="verevon-chat-diff__truncated">
            for stor til å vises linje for linje
          </span>
        </Show>
      </div>
      <Show when={props.result.hunks.length > 0}>
        <For each={props.result.hunks}>
          {(hunk) => (
            <div class="verevon-chat-diff__hunk">
              <div class="verevon-chat-diff__hunk-head">
                @@ -{hunk.oldStart},{hunk.oldLines} +{hunk.newStart},
                {hunk.newLines} @@
              </div>
              <For each={hunk.lines}>
                {(line) => (
                  <div
                    class={`verevon-chat-diff__line verevon-chat-diff__line--${line.kind}`}
                  >
                    <span class="verevon-chat-diff__sigil" aria-hidden="true">
                      {line.kind === 'added'
                        ? '+'
                        : line.kind === 'removed'
                          ? '-'
                          : ' '}
                    </span>
                    <span class="verevon-chat-diff__text">
                      {line.text || "\u00a0"}
                    </span>
                  </div>
                )}
              </For>
            </div>
          )}
        </For>
      </Show>
    </div>
  )
}

/** Per-intent icon. The glyph is the fastest signal of what a step did. */
function ToolIntentIcon(props: { intent: ToolIntent }) {
  return (
    <Switch fallback={<Wrench size={14} />}>
      <Match when={props.intent === 'terminal'}>
        <TerminalSquare size={14} />
      </Match>
      <Match when={props.intent === 'search'}>
        <Search size={14} />
      </Match>
      <Match when={props.intent === 'read'}>
        <FileText size={14} />
      </Match>
      <Match when={props.intent === 'diff'}>
        <GitCompare size={14} />
      </Match>
    </Switch>
  )
}

/**
 * A tool call, presented according to its recomputed intent
 * (`shared/chat-nodes/tool-presentation`).
 *
 * Every call used to render identically: a wrench, the name, and raw `<pre>`
 * blocks. The header now carries what the intent makes meaningful — a result
 * count for a search, a truncation warning for a read — so the collapsed row is
 * informative without expanding it. The body is still the raw payload, because
 * the payload is the evidence and reshaping it would hide what the model saw.
 */
export function ToolCallCard(props: { call: ChatToolCall }) {
  const [open, setOpen] = createSignal(false)
  const failed = () =>
    Boolean(props.call.error) || props.call.status === 'error'
  const running = () => !props.call.status || props.call.status === 'running'
  const statusLabel = () =>
    failed() ? 'feilet' : running() ? 'kjører ...' : 'fullført'
  const args = () => formatToolArgs(props.call.args)
  const presentation = createMemo(() => toolPresentation(props.call))

  return (
    <div class="verevon-chat-tool-call" data-intent={presentation().intent}>
      <button
        type="button"
        aria-expanded={open() ? 'true' : 'false'}
        onClick={() => setOpen((value) => !value)}
      >
        <ToolIntentIcon intent={presentation().intent} />
        <span>{props.call.name}</span>
        {/* A search that returned nothing is a real outcome, so 0 must render —
            hence an explicit null check rather than a truthiness test. */}
        <Show when={presentation().count != null && !running()}>
          <em class="verevon-chat-tool-call__count">
            {presentation().count}{' '}
            {presentation().count === 1 ? 'treff' : 'treff'}
          </em>
        </Show>
        <Show when={presentation().intent === 'diff'}>
          <em class="verevon-chat-tool-call__count">
            +{presentation().added ?? 0} −{presentation().removed ?? 0}
          </em>
        </Show>
        <Show when={presentation().truncated}>
          {/* Not the same as "no results": there IS more and the model did not
              see it, which matters for any answer built on this. */}
          <em
            class="verevon-chat-tool-call__truncated"
            title="Resultatet ble avkortet"
          >
            avkortet
          </em>
        </Show>
        <em class={{ 'is-error': failed(), 'is-running': running() }}>
          {statusLabel()}
        </em>
        <ChevronRight size={14} class={{ 'verevon-chat-rotate': open() }} />
      </button>
      <Show when={open() && (args() || props.call.output || props.call.error)}>
        <div>
          <Show when={args()}>
            <pre>{args()}</pre>
          </Show>
          <Show
            when={presentation().intent === 'diff' && props.call.output}
            fallback={
              <Show when={props.call.output}>
                <pre>{props.call.output}</pre>
              </Show>
            }
          >
            {/* A patch rendered as a patch. The raw text is still what the model
                saw — this only changes how a reader reads it. */}
            {(output) => <DiffView result={parseUnifiedDiff(output())} />}
          </Show>
          <Show when={props.call.error}>
            <p>{props.call.error}</p>
          </Show>
        </div>
      </Show>
    </div>
  )
}

/**
 * Messages the user sent WHILE the agent was working.
 *
 * Rendered under the in-progress answer, where the newest thing the user did
 * belongs, and every state is visible: waiting, handed to the agent, or refused
 * with the reason. Silence here is the bug this replaces — the send path used to
 * discard mid-run input with no trace at all.
 */
export function QueuedInputStrip(props: { entries: QueuedInput[] }) {
  const label = (entry: QueuedInput) => {
    if (entry.state === 'delivered') return 'levert til agenten'
    if (entry.state === 'refused') return 'ikke levert'
    return 'venter'
  }
  return (
    <Show when={props.entries.length > 0}>
      <ul class="verevon-chat-queued" aria-label="Meldinger sendt underveis">
        <For each={props.entries}>
          {(entry) => (
            <li class="verevon-chat-queued-item" data-state={entry.state}>
              <p class="verevon-chat-queued-text">{entry.content}</p>
              <p class="verevon-chat-queued-state">
                <span>{label(entry)}</span>
                <Show when={entry.note}>
                  {(note) => (
                    <span class="verevon-chat-queued-note">{note()}</span>
                  )}
                </Show>
              </p>
            </li>
          )}
        </For>
      </ul>
    </Show>
  )
}

/**
 * The plan-approval control: the only thing that grants a planning run the
 * authority to execute.
 *
 * A plan-mode run described what it would do. Letting it act is a decision a
 * person makes, and the Model Plane requires that decision to name TWO things —
 * how much authority is granted, and why. So this asks for both, and refuses to
 * submit until the reason is a reason: an approval with no stated ground is a
 * rubber stamp with extra steps, and the whole point of the ladder is that the
 * grant is reviewable afterwards.
 *
 * Before this existed, plan mode could be entered from the composer and never
 * left — the control that decides how much a run may do had no way to be used.
 */
export function PlanApprovalControl(props: {
  grantedRung?: AutonomyRung
  pending: boolean
  error?: string
  onApprove: (rung: AutonomyRung, justification: string) => void
}) {
  const [rung, setRung] = createSignal<AutonomyRung>('workspace_write')
  const [reason, setReason] = createSignal('')
  const tooShort = () => reason().trim().length < MIN_PLAN_JUSTIFICATION_CHARS

  return (
    <Show
      when={!props.grantedRung}
      fallback={
        <p class="verevon-chat-plan-granted">
          Godkjent: agenten kan nå {RUNG_LABEL[props.grantedRung ?? 'read_only']}.
        </p>
      }
    >
      <section class="verevon-chat-plan" aria-label="Godkjenn planen">
        <p class="verevon-chat-plan__lead">
          Dette var en plan – ingenting er utført. Velg hvor mye agenten får gjøre, og skriv
          hvorfor.
        </p>
        <div class="verevon-chat-plan__rungs" role="radiogroup" aria-label="Fullmakt">
          <For each={GRANTABLE_RUNGS}>
            {(option) => (
              <button
                type="button"
                role="radio"
                aria-checked={rung() === option ? 'true' : 'false'}
                class="verevon-chat-plan__rung"
                data-selected={rung() === option}
                onClick={() => setRung(option)}
              >
                {RUNG_LABEL[option]}
              </button>
            )}
          </For>
        </div>
        <textarea
          class="verevon-chat-plan__reason"
          rows={2}
          placeholder="Hvorfor trenger agenten denne fullmakten?"
          value={reason()}
          onInput={(event) => setReason(event.currentTarget.value)}
        />
        <div class="verevon-chat-plan__actions">
          {/* The minimum is stated, not enforced silently: a disabled button
              with no reason given is indistinguishable from a broken one. */}
          <Show when={tooShort()}>
            <span class="verevon-chat-plan__hint">
              Minst {MIN_PLAN_JUSTIFICATION_CHARS} tegn – en begrunnelse noen kan vurdere.
            </span>
          </Show>
          <Show when={props.error}>
            {(message) => (
              <span class="verevon-chat-plan__error" role="alert">
                {message()}
              </span>
            )}
          </Show>
          <button
            type="button"
            class="verevon-chat-plan__approve"
            disabled={tooShort() || props.pending}
            onClick={() => props.onApprove(rung(), reason().trim())}
          >
            {props.pending ? 'Godkjenner…' : 'Godkjenn'}
          </button>
        </div>
      </section>
    </Show>
  )
}

/**
 * `read_only` is deliberately absent: it is what a plan-mode run already has, so
 * offering it as a grant would be an approval that changes nothing while still
 * taking the run out of plan mode.
 */
const GRANTABLE_RUNGS: AutonomyRung[] = ['workspace_write', 'danger_full_access']

const RUNG_LABEL: Record<AutonomyRung, string> = {
  read_only: 'bare undersøke',
  workspace_write: 'skrive i arbeidsområdet',
  danger_full_access: 'utføre alt, også utgående handlinger',
}

export function ToolChips(props: { tools: ComposerToolId[] }) {
  return (
    <Show when={props.tools.length > 0}>
      <div class="verevon-chat-tool-chips">
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

export function AttachmentChips(props: {
  attachments: ChatTurnAttachment[]
  tone: 'assistant' | 'user'
  onOpen?: (attachmentId: string) => void
}) {
  return (
    <Show when={props.attachments.length > 0}>
      <div class="verevon-chat-attachments">
        <For each={props.attachments}>
          {(attachment) => <AttachmentItem attachment={attachment} tone={props.tone} onOpen={props.onOpen} />}
        </For>
      </div>
    </Show>
  )
}

export function AttachmentItem(props: {
  attachment: ChatTurnAttachment
  tone: 'assistant' | 'user'
  onOpen?: (attachmentId: string) => void
}) {
  const [failed, setFailed] = createSignal(false)
  const previewUrl = () => props.attachment.previewUrl || props.attachment.url
  const isImage = () => Boolean(previewUrl()) && props.attachment.type.startsWith('image/') && !failed()
  const open = () => props.onOpen?.(props.attachment.id)
  const handleKeyDown = (event: KeyboardEvent) => {
    if ((event.key === 'Enter' || event.key === ' ') && props.onOpen) {
      event.preventDefault()
      open()
    }
  }
  const interaction = () => props.onOpen

  return (
    <Show
      when={isImage() && previewUrl()}
      fallback={(
        <span
          class={`verevon-chat-attachment verevon-chat-attachment--${props.tone}`}
          role={interaction() ? 'button' : undefined}
          tabindex={interaction() ? 0 : undefined}
          onClick={open}
          onKeyDown={handleKeyDown}
          title={interaction() ? 'Åpne i arbeidsflate' : undefined}
        >
          {props.attachment.name}
        </span>
      )}
    >
      {(url) => (
        <span
          class="verevon-chat-attachment-image"
          role={interaction() ? 'button' : undefined}
          tabindex={interaction() ? 0 : undefined}
          onClick={open}
          onKeyDown={handleKeyDown}
          title={interaction() ? 'Åpne i arbeidsflate' : undefined}
        >
          <img src={url()} alt={props.attachment.name} onError={() => setFailed(true)} />
        </span>
      )}
    </Show>
  )
}

export function GeneratedImagePreviews(props: { previews: GeneratedImagePreview[] }) {
  return (
    <div class="verevon-chat-image-previews">
      <For each={props.previews}>
        {(preview) => (
          <figure class="verevon-chat-image-preview">
            <figcaption>
              <span class="verevon-chat-image-preview__title">
                <ImageIcon size={14} />
                <span>{preview.title}</span>
              </span>
              <Show when={preview.size > 0}>
                <small>{formatBytes(preview.size)}</small>
              </Show>
              <div class="verevon-chat-image-preview__actions">
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
              class="verevon-chat-image-preview__media"
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
    <div class="verevon-chat-generated-files">
      <For each={props.files}>
        {(file) => {
          const bytes = () => (file.size > 0 ? file.size : dataUriByteSize(file.url))
          const remote = () => /^https?:\/\//i.test(file.url)
          return (
            <span class="verevon-chat-generated-file">
              <a href={file.url} download={file.name} aria-label={`Last ned ${file.name}`}>
                <Paperclip size={12} />
                {file.name}
                <em>{friendlyMimeLabel(file.mime)}</em>
                <Show when={bytes() > 0}><span>{formatBytes(bytes())}</span></Show>
                <Download size={12} />
              </a>
              <Show when={remote()}>
                <a
                  class="verevon-chat-generated-file__open"
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
    <p class="verevon-chat-low-confidence-notice" role="note">
      <AlertCircle size={12} />
      Usikkert svar ({Math.round(props.confidence * 100)}% sikkerhet) — sjekk kilder før du stoler på dette.
    </p>
  )
}

/**
 * Deterministic "memory was used" indicator (Model Plane's `memory_recall`
 * event). Shown because a user cannot otherwise distinguish an answer that
 * drew on remembered context from one that guessed — and a wrong remembered
 * fact is only correctable if you know it was in play. The backend emits the
 * event only when memory genuinely contributed, so there is no zero state.
 */
/**
 * Norwegian label per origin.
 *
 * `unrecorded` is deliberately NOT phrased as something the user said. The
 * backend contract is explicit that rows predating provenance must never render
 * as `stated`, and "du sa dette" about a row that does not record it would
 * manufacture consent. It reads as an unknown source instead.
 */
const MEMORY_ORIGIN_LABEL: Record<MemoryOrigin, string> = {
  stated: 'du ba meg huske',
  inferred: 'utledet',
  unrecorded: 'ukjent kilde',
}

/**
 * "Memory was used", and — when the backend said which — what was used.
 *
 * Collapsed to a single line by default: the count is the signal, the contents
 * are the follow-up. Expanding is the point of the whole feature, though. A
 * remembered fact that is wrong is only correctable if you can find it, and
 * seeing the list is also how you tell whether the agent's memory needs
 * updating at all.
 */
export function MemoryRecallNotice(props: {
  count: number
  memories: RecalledMemory[]
}) {
  const [open, setOpen] = createSignal(false)
  const listed = () => props.memories.length > 0
  const summary = () =>
    `Brukte ${props.count} ${props.count === 1 ? 'minne' : 'minner'} fra tidligere samtaler.`

  return (
    <div class="verevon-chat-memory-recall">
      <Show
        when={listed()}
        fallback={
          // No list to show — an older backend, or entries that carried nothing
          // readable. The count still stands on its own.
          <p class="verevon-chat-memory-recall-notice" role="note">
            <Sparkles size={12} />
            {summary()}
          </p>
        }
      >
        <button
          type="button"
          class="verevon-chat-memory-recall-notice verevon-chat-memory-recall__toggle"
          aria-expanded={open() ? 'true' : 'false'}
          onClick={() => setOpen((value) => !value)}
        >
          <Sparkles size={12} />
          <span>{summary()}</span>
          <span class="verevon-chat-memory-recall__hint">
            {open() ? 'Skjul' : 'Vis hva jeg husker'}
          </span>
          <ChevronRight
            size={12}
            class={{ 'verevon-chat-rotate': open() }}
          />
        </button>
        <Show when={open()}>
          <ul class="verevon-chat-memory-recall__list">
            <For each={props.memories}>
              {(memory) => (
                <li class="verevon-chat-memory-recall__item">
                  <div class="verevon-chat-memory-recall__head">
                    <span class="verevon-chat-memory-recall__label">
                      {memory.label}
                    </span>
                    {/* Both axes are shown: WHERE it came from and HOW it came
                        to exist. They answer different questions, and the
                        second is the one that says whether to trust it. */}
                    <span
                      class="verevon-chat-memory-recall__badge"
                      data-origin={memory.origin}
                    >
                      {MEMORY_ORIGIN_LABEL[memory.origin]}
                    </span>
                    <Show when={memory.role === 'inject'}>
                      <span class="verevon-chat-memory-recall__scope">
                        organisasjon
                      </span>
                    </Show>
                  </div>
                  <p class="verevon-chat-memory-recall__preview">
                    {memory.preview}
                  </p>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Show>
    </div>
  )
}

/**
 * The answer may be incomplete. `stream_incomplete` means the provider
 * connection broke before any proper termination signal; the token-ceiling
 * reasons mean the model ran out of room mid-thought. Either way the reply
 * looked finished and was not — the exact case honesty requires naming.
 */
export function TruncatedAnswerNotice(props: { stopReason: string }) {
  const cutOff = () =>
    props.stopReason === 'max_tokens' || props.stopReason === 'length'
  const incomplete = () => props.stopReason === 'stream_incomplete'
  return (
    <Show when={cutOff() || incomplete()}>
      <p class="verevon-chat-truncated-notice" role="note">
        <AlertCircle size={12} />
        {cutOff()
          ? 'Svaret nådde lengdegrensen og kan være avkuttet — be om fortsettelsen.'
          : 'Forbindelsen brøt før svaret var fullført — svaret kan være avkuttet.'}
      </p>
    </Show>
  )
}

export function GroundingInlineSummary(props: {
  grounding: ChatKnowledgeGrounding
}) {
  return (
    <div class="verevon-chat-grounding-inline">
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
    <div class="verevon-chat-error-notice">
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
      aria-pressed={props.active === undefined ? undefined : (props.active ? 'true' : 'false')}
      class={{ 'verevon-chat-action-button': true, 'verevon-chat-action-button--active': Boolean(props.active) }}
      onClick={() => props.onClick()}
    >
      {props.children}
    </button>
  )
}

export function MessageMenu(props: { align?: 'start' | 'end'; items: Array<{ label: string; icon: JSX.Element; onClick: () => void }> }) {
  const [open, setOpen] = createSignal(false)
  let ref!: HTMLDivElement

  createEffect(
    () => open(),
    (isOpen) => {
      if (!isOpen) return
      const onPointer = (event: PointerEvent) => {
        if (ref && !ref.contains(event.target as Node)) setOpen(false)
      }
      const onKey = (event: KeyboardEvent) => {
        if (event.key === 'Escape') setOpen(false)
      }
      document.addEventListener('pointerdown', onPointer)
      document.addEventListener('keydown', onKey)
      return () => {
        document.removeEventListener('pointerdown', onPointer)
        document.removeEventListener('keydown', onKey)
      }
    },
  )

  return (
    <div ref={ref} class="verevon-chat-menu">
      <button type="button" aria-label="Flere handlinger" aria-expanded={open() ? 'true' : 'false'} onClick={() => setOpen((value) => !value)}>
        <MoreHorizontal size={14} />
      </button>
      <Show when={open()}>
        <div class={{ 'verevon-chat-menu__panel': true, 'verevon-chat-menu__panel--end': props.align === 'end' }}>
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
    <div class="verevon-chat-divider">
      <span />
      <time>{formatDayLabel(props.value)}</time>
    </div>
  )
}

export function ThinkingDots() {
  return (
    <span class="verevon-chat-thinking">
      <span class="verevon-thinking-dots" aria-hidden="true">
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
    <div class={{ 'verevon-chat-step': true, 'verevon-chat-step--expandable': true, 'is-open': open() }}>
      <Show when={!props.isLast}>
        <span class="verevon-chat-step__line" />
      </Show>
      <span class={`verevon-chat-step__icon ${icon().className}`}>{icon().node}</span>
      <div>
        <button
          type="button"
          class="verevon-chat-step__heading"
          aria-expanded={open() ? 'true' : 'false'}
          aria-controls={detailsId()}
          onClick={() => setOpen((value) => !value)}
        >
          <span>
            <strong>{props.step.title}</strong>
            <time>{formatTime(props.step.createdAt)}</time>
          </span>
          <ChevronRight size={13} class={{ 'verevon-chat-rotate': open() }} />
        </button>
        <span class="verevon-chat-step__summary">{props.step.detail}</span>
        <Show when={open()}>
          <div id={detailsId()} class="verevon-chat-step__details">
            <Show when={(props.step.evidence?.length ?? 0) > 0}>
              <div class="verevon-chat-step__evidence">
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
              <p class="verevon-chat-step__detail-copy">{props.step.detail || 'No additional detail captured.'}</p>
            </Show>
          </div>
        </Show>
      </div>
    </div>
  )
}

// ── Empty state ───────────────────────────────────────────────────────────────

export function EmptyChatState(props: { children: JSX.Element; onSelectPrompt: (prompt: string) => void }) {
  const i18n = useI18n()
  const starterPrompts = () => [
    {
      description: i18n.tr('Trekk ut beslutninger, risiko og neste steg.', 'Extract decisions, risks, and next steps.'),
      icon: FileText,
      label: i18n.tr('Oppsummer et dokument', 'Summarize a document'),
      prompt: i18n.tr('Oppsummer dokumentet jeg legger ved. Fremhev beslutninger, risiko og neste steg.', 'Summarize the document I attach. Highlight decisions, risks, and next steps.'),
    },
    {
      description: i18n.tr('Finn et pålitelig svar med sporbare kilder.', 'Find a reliable answer with traceable sources.'),
      icon: Search,
      label: i18n.tr('Undersøk med kilder', 'Research with sources'),
      prompt: i18n.tr('Undersøk dette spørsmålet grundig og vis hvilke kilder som støtter konklusjonen.', 'Research this question thoroughly and show which sources support the conclusion.'),
    },
    {
      description: i18n.tr('Gå fra idé til et nyttig førsteutkast.', 'Turn an idea into a useful first draft.'),
      icon: Pencil,
      label: i18n.tr('Lag et førsteutkast', 'Create a first draft'),
      prompt: i18n.tr('Lag et tydelig førsteutkast som jeg kan gjennomgå og forbedre.', 'Create a clear first draft that I can review and improve.'),
    },
  ]

  return (
    <div class="verevon-chat-empty">
      <div class="verevon-chat-empty__inner">
        <div class="verevon-chat-empty__heading">
          <div class="verevon-chat-empty__brand" aria-hidden="true">
            <span class="verevon-chat-empty__mark"><span /></span>
            <span>Verevon</span>
          </div>
          <h1>{i18n.tr('Hva vil du få gjort?', 'What would you like to get done?')}</h1>
          <p>{i18n.tr('Start med et spørsmål. Arbeidsflaten åpnes først når du har noe å undersøke, følge eller gjennomgå.', 'Start with a question. The workspace opens only when there is something to research, follow, or review.')}</p>
        </div>
        <div class="verevon-chat-empty__composer">{props.children}</div>
        <div class="verevon-chat-empty__prompts">
          <span class="verevon-chat-empty__prompt-label">{i18n.tr('Prøv for eksempel', 'Try one of these')}</span>
          <For each={starterPrompts()}>
            {({ label, description, prompt, icon: Icon }) => (
              <button
                type="button"
                class="verevon-chat-starter"
                aria-label={label}
                onClick={() => props.onSelectPrompt(prompt)}
              >
                <span class="verevon-chat-starter__icon"><Icon size={15} strokeWidth={1.9} /></span>
                <span class="verevon-chat-starter__copy"><strong>{label}</strong><small>{description}</small></span>
                <ChevronRight size={15} aria-hidden="true" />
              </button>
            )}
          </For>
        </div>
        <p class="verevon-chat-empty__trust">{i18n.tr('Organisasjonens kunnskap er standard. Nettbruk er alltid synlig og valgfri.', 'Organisation knowledge is the default. Web access is always visible and optional.')}</p>
      </div>
    </div>
  )
}
