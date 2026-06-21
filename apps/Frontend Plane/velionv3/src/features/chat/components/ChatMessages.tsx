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
  buildGeneratedImagePreviews,
  domId,
  formatBytes,
  formatDayLabel,
  formatLatency,
  formatRelative,
  formatTime,
  formatToolArgs,
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
  type MarkdownBlock,
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
  onFeedback: (rating: 'positive' | 'negative') => void
  onRegenerate: () => void
  onApprovalDecision: (approvalId: string, decision: ApprovalDecision) => void
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
  onFeedback: (rating: 'positive' | 'negative') => void
  onRegenerate: () => void
  onApprovalDecision: (approvalId: string, decision: ApprovalDecision) => void
}) {
  const [reaction, setReaction] = createSignal<'up' | 'down' | null>(null)
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
        <ToolChips tools={props.message.tools} />
        <AttachmentChips attachments={props.message.attachments} tone="assistant" />
        <Show when={(props.message.toolCalls?.length ?? 0) > 0}>
          <ToolCallList calls={props.message.toolCalls ?? []} />
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
        <Show when={!waiting() && !errored()}>
          <div class="velion-chat-message-actions">
            <MessageAction label={props.copied ? 'Copied' : 'Copy'} onClick={props.onCopy}>
              {props.copied ? <Check size={14} /> : <Copy size={14} />}
            </MessageAction>
            <MessageAction
              active={reaction() === 'up'}
              label="Good response"
              onClick={() => {
                setReaction((current) => current === 'up' ? null : 'up')
                props.onFeedback('positive')
              }}
            >
              <ThumbsUp size={14} />
            </MessageAction>
            <MessageAction
              active={reaction() === 'down'}
              label="Bad response"
              onClick={() => {
                setReaction((current) => current === 'down' ? null : 'down')
                props.onFeedback('negative')
              }}
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
        <pre><code>{(props.block as Extract<MarkdownBlock, { kind: 'code' }>).text}</code></pre>
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

export function MarkdownList(props: { block: Extract<MarkdownBlock, { kind: 'list' }> }) {
  return (
    <Show
      when={props.block.ordered}
      fallback={<ul><For each={props.block.items}>{(item) => <li>{parseInline(item)}</li>}</For></ul>}
    >
      <ol><For each={props.block.items}>{(item) => <li>{parseInline(item)}</li>}</For></ol>
    </Show>
  )
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

export function ToolCallList(props: { calls: ChatToolCall[] }) {
  return (
    <div class="velion-chat-tool-list">
      <For each={props.calls}>
        {(call) => <ToolCallCard call={call} />}
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

export function GeneratedFiles(props: { files: GeneratedFile[] }) {
  return (
    <div class="velion-chat-generated-files">
      <For each={props.files}>
        {(file) => (
          <a href={file.url} target="_blank" rel="noopener noreferrer">
            <Paperclip size={12} />
            {file.name}
            <Show when={file.size > 0}><span>{formatBytes(file.size)}</span></Show>
          </a>
        )}
      </For>
    </div>
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
