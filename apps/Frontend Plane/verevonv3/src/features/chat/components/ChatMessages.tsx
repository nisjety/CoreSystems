import { ConnectorSuggestionCard } from './ConnectorSuggestionCard'
import { ResearchPlanCard, researchPlanTurnEnabled } from './ResearchPlanCard'
import type { ConnectIntent } from '@/shared/integrations/connect-intent'
import { ForkAction, threadForkEnabled } from './ForkAction'
import { ExportConversationAction, threadExportEnabled } from './ExportConversationAction'
import { ShareSnapshotDialog, shareSnapshotEnabled } from './ShareSnapshotDialog'
import { ImportConversationAction, threadImportEnabled } from './ImportConversationAction'
import { formatResetTime, humanizeToolName, refreshResetTimeWording, workStepLabel } from './chat-normalizers'
import { RunNotifyButton } from './RunNotifyButton'
import {
  WritePreviewDisclosure,
  approvalWritePreviewEnabled,
  isMailSendApproval,
  mailEditProblemText,
  type MailEditDraft,
} from './WritePreview'
import {
  approvalEditRefusalOf,
  type Approval,
  type ApprovalDecision,
  type ApprovalEdit,
  type ApprovalScope,
  type PlanStepEdit,
} from '@/shared/api/orchestration-client'
import {
  AlertCircle,
  ArrowRight,
  Brain,
  CalendarClock,
  Check,
  ChevronDown,
  ChevronLeft,
  Pin,
  RotateCcw,
  ChevronRight,
  ChevronUp,
  Copy,
  Download,
  ExternalLink,
  FileCode2,
  FileSpreadsheet,
  FileText,
  GitCompare,
  Globe2,
  Image as ImageIcon,
  Info,
  Loader2,
  MessageSquarePlus,
  MoreHorizontal,
  Paperclip,
  Pencil,
  Plug,
  Presentation,
  RefreshCw,
  Search,
  Sparkles,
  Square,
  TerminalSquare,
  ThumbsDown,
  ThumbsUp,
  Trash2,
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
  type ConversationNode,
  type ConversationNodeContext,
  type ConversationNodeEntry,
  type DiffResult,
  type PlanApprovalStatus,
  type ToolIntent,
} from '@/shared/chat-nodes'
import type { DockedNodes } from '@/shared/chat-nodes/types'
import type { ChatContextRef } from '@/shared/actions/chat-context-refs'
// The barrel does not re-export it yet; one more line there when it next changes.
import { nodeKey } from '@/shared/chat-nodes/derive'
import type {
  AutonomyRung,
  ChatKnowledgeReceipt,
  ChatPlanStep,
  ChatRoutingDecision,
  ChatSearchProviderId,
  ChatSearchProvenanceEvent,
  ChatSearchRoute,
  MemoryOrigin,
  RecalledMemory,
} from '@/shared/api/chat-client'
import { isSafeLinkHref } from '../lib/safe-href'
import { isWidgetFence } from '../lib/inline-widget'
import { feedbackReasonLabels } from '../lib/feedback-reasons'
import type { SettledFold } from '../lib/settled-fold'
import type { SettledFoldChoices } from '../lib/settled-fold-choices'
import { readChatThreadHistory } from '../lib/chat-thread-history'
import { buildPromptSuggestions, type PromptSuggestion } from '../lib/prompt-suggestions'
import { readerPolishOn } from '../lib/reader-polish'
import { listConversations, type LiveTicket } from '@/shared/api/inbox-client'
import { listNotifications, type Notification } from '@/shared/api/notifications-client'
import { MIN_PLAN_JUSTIFICATION_CHARS, translateText } from '@/shared/api/chat-client'
import {
  REASONING_TRANSLATE_MAX_CHARS,
  reasoningTranslateChars,
  reasoningTranslateLocaleOf,
  type ReasoningTranslateLocale,
} from '@/shared/actions/chat-reasoning-translate'
import { answerProcessingNote } from '@/shared/api/privacy-tier'
import { correctMemory, deleteMemory, listMemories, rememberMemory } from '@/shared/api/memory-client'
import { memberSkillAuthoring } from '@/shared/api/skills-client'
import { SaveAsSkillDialog } from '@/features/skills/components/SaveAsSkillDialog'
import { createResource } from '@/shared/lib/create-resource-compat'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'
import { getSession } from '@/shared/session/session-store'
import { ApiError } from '@/shared/api/http'
import {
  For,
  Match,
  Show,
  Switch,
  createEffect,
  createMemo,
  createSignal,
  createUniqueId,
  onCleanup,
  untrack,
} from 'solid-js'
import { render, type JSX } from '@solidjs/web'
import {
  artifactRenderKind,
  dataUriByteSize,
  friendlyMimeLabel,
} from './chat-artifacts'
import {
  MAX_HIGHLIGHT_CHARS,
  highlightCode,
} from './chat-code-highlight'
import { ChangedFilesCard, chatChangedFilesEnabled } from './ChangedFilesCard'
import { ChatImageStrip, chatImageStripEnabled, webSearchQueryOf } from './ChatImageStrip'
import { ChartFenceBlock } from './ChatChart'
import { InlineWidget, InlineWidgetAnswer, InlineWidgetSurface, inlineWidgetsEnabled, useInlineWidgetsLive } from './InlineWidget'
import { McpAppsForAnswer, mcpAppsEnabled } from './McpAppFrame'
import { ChatTableTools, TableChart, createTableChartState } from './ChatTableTools'
import { SettledTurnContent } from './SettledTurnFold'
import { StepStrip, stepStripEnabled } from './StepStrip'
import {
  domId,
  formatBytes,
  formatDayLabel,
  formatElapsedWait,
  formatLatency,
  formatRelative,
  formatTime,
  formatToolArgs,
  formatUsd,
  getTaskStepIcon,
  hostname,
  parseInline,
  parseMarkdownBlocks,
  parseMarkdownIncremental,
  prettyModel,
  renderBlockMath,
  truncateText,
  type ParsedMarkdown,
} from './chat-media-markdown'
import { answerExportSize, EXPORT_BUTTON_MIN_CHARS, exportChatAnswer, type ExportFormat } from '../lib/chat-export'
import { changedFiles } from '../lib/changed-files'
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
  type LifecycleNotice,
  type MarkdownBlock,
  type MarkdownListItem,
  type MarkdownQuoteLine,
  type StreamFailureAction,
  TOOL_LABELS,
} from './chat-types'
import type { VersionBadge } from '@/features/chat/lib/chat-versions'
import type { HiddenTurn, RewindPlan, ServerVersionBadge } from '@/features/chat/lib/chat-versions-server'
import './rewind-branch.css'
import './reasoning-translate.css'
import { streamActivityLabel, type StreamActivity, type Translator } from '@/features/chat/lib/stream-activity'
import { imageRefusalText, narrate } from '@/shared/activity/narration'
import { isEffectfulChatTurn } from '@/shared/chat/effect-class'
import { translateApiError, useI18n } from '@/shared/i18n'
import {
  approvalHeadline,
  approvalReason,
  browserAssistActionLabel,
  browserAssistKindOf,
  browserAssistNoTakeOverLine,
} from '@/shared/approvals/approval-copy'
import type { ChatRunBrowserControl } from '@/features/chat/lib/chat-run-watch'
import { BrowserApprovalDecisionCard } from '@/features/browser/approval/BrowserApprovalDecisionCard'
import {
  APPROVAL_REDIRECT_ENABLED,
  browserApprovalCardEnabled,
  runApprovalFitsCard,
  runApprovalToCardModel,
  type ApprovalCardModel,
} from '@/features/browser/approval/approval-model'
import { confirmStepUp } from '@/shared/approvals/step-up'
import { createPlayingSpeechId, speechPlayer } from '@/features/voice/speech-playback'
import { readAloudBlockedMessage, readAloudUnavailableMessage } from '@/features/voice/voice-mode-state'
import { ComputerViewer, chatComputerPanelEnabled } from '@/features/chat/components/computer/ComputerViewer'
import { pickComputerViewer } from '@/shared/computer/viewer-registry'

/**
 * What an assistant turn's connected-app suggestion card needs from the page (EP-10 S8, S9). Absent where the
 * surface offers no card.
 */
export type ConnectorSuggestionWiring = {
  orgId?: string
  /** "Use it for this question": select the account for the turn and send the question again. */
  onUse: (suggestion: { suggestionId: string; provider: string }, connectionId: string) => void
  connectTurn?: () => { threadId: string; prompt: string } | null
  pendingConnectIntent?: () => ConnectIntent | null
  resumeConnectIntent?: (intent: ConnectIntent, connectionId: string, providerLabel: string) => boolean
  dismissConnectIntent?: () => void
}

/** The turns a rewind would hide, or an undo would bring back: each one's role and the start of its text. */
function HiddenTurnList(props: { turns: HiddenTurn[] }) {
  const i18n = useI18n()
  return (
    <ol class="verevon-chat-rewind-confirm__hidden">
      <For each={props.turns}>
        {(turn) => (
          <li>
            <span class="verevon-chat-rewind-confirm__role">
              {turn.role === 'user' ? i18n.tr('Du', 'You') : i18n.tr('Svar', 'Answer')}
            </span>{' '}
            {turn.excerpt || i18n.tr('(ingen tekst)', '(no text)')}
          </li>
        )}
      </For>
    </ol>
  )
}

export function MessageBlock(props: {
  copied: boolean
  message: ChatTurn
  /**
   * Message pinning. Absent when the surface does not offer it (a foreign-origin
   * or read-only thread), which is what hides the control entirely rather than
   * showing a dead one.
   */
  pinned?: boolean
  /** The thread already holds the maximum number of pins. */
  pinDisabled?: boolean
  onTogglePin?: () => void
  onBranch: () => void
  onCopy: () => void
  onEdit: (text: string) => void
  onFeedback: (rating: 'positive' | 'negative', note?: string) => Promise<boolean>
  onRegenerate: () => void
  onRerunAsNewTurn?: () => void
  /**
   * "Fortsett" — continue a STOPPED turn's partial answer from exactly where
   * it left off, rather than discarding it. Absent on a surface that does not
   * offer it (mirrors `onTogglePin`'s convention); `AssistantMessage` itself
   * gates visibility on the turn actually being stopped with partial content,
   * so a caller need not repeat that check.
   */
  onContinue?: () => void
  /**
   * "Planlegg dette" (chat-wave2 "schedule" spec §2.1) — turns this turn's
   * original request into a recurring job. Absent on a surface that does not
   * offer scheduling (mirrors `onTogglePin`/`onRerunAsNewTurn`'s convention).
   */
  onScheduleThis?: () => void
  onApprovalDecision: (approvalId: string, decision: ApprovalDecision) => void
  /** Sends an approval with the person's edits (EP-10-S6); see `ApprovalRequests`. */
  onApprovalEditedDecision?: (approvalId: string, edit: ApprovalEdit) => Promise<void>
  onApprovePlan: (rung: AutonomyRung, justification: string, steps?: PlanStepEdit[]) => void
  onRejectPlan?: (planId: string, reason: string) => void
  planApproval?: PlanApprovalStatus
  onSelectFollowUp?: (text: string) => void
  /** Puts a widget's quoted passage in front of the composer's draft (EP-4 W4-IO-2). Absent, a widget quotes nothing. */
  onQuoteSelection?: (text: string) => void
  onViewSteps: () => void
  onViewAttachments?: (attachmentId: string) => void
  /** n/N badge for this turn's exchange versions — only ever set on the trailing assistant turn. */
  version?: VersionBadge | null
  onSelectVersion?: (target: number) => void
  /** The server's version switcher for this turn (WS17 S9); see `AssistantMessage`. Absent on an unversioned thread. */
  serverVersion?: ServerVersionBadge | null
  /** Moves the thread to the sibling version the switcher named. */
  onSwitchServerVersion?: (messageId: string) => void
  /** The rewind this turn offers, and the turns it would hide (WS17 S9). Absent on a surface with no version tree. */
  rewindPlan?: RewindPlan | null
  /** Rewinds the thread to this turn, once the person has confirmed it. Nothing is deleted. */
  onRewind?: () => void
  /** The turns an undo of the rewind that landed on this turn would bring back. Set only under that turn. */
  rewindUndo?: HiddenTurn[] | null
  /** Undoes that rewind: the leaf returns to the turn it left. */
  onUndoRewind?: () => void
  /**
   * This is the latest assistant turn. Rendered as `data-latest` so its action
   * row stays visible at rest on hover-capable devices (older turns reveal
   * theirs on hover or focus).
   */
  isLatest?: boolean
  /** Effectful turns cannot be edited or regenerated in place. */
  editLocked?: boolean
  /** Forwarded into `ConversationNodeContext` — see its own doc for why. */
  threadId?: string | null
  /**
   * Temporary chat: "Les høyt" is disabled, because governed speech cannot yet
   * attest zero data retention.
   */
  temporaryChat?: boolean
  /**
   * The thread is scoped to a Space. Its history has its own surface and authority, so the conversation export
   * is not offered there (EP-12 D8).
   */
  spaceScoped?: boolean
  /** Which of this turn's controls the composer dock holds; see `DockedNodes`. */
  docked?: DockedNodes
  /** This turn's settled-turn fold (EP-14 S1), from the page. Absent when the switch is off or nothing folds. */
  settledFold?: SettledFold
  /** The page's fold choices for this view (EP-14 S1). Absent, the fold keeps its choice for this mount only. */
  settledFoldChoices?: SettledFoldChoices
  /** The connected-app suggestion card's wiring (EP-10 S8); see `ConnectorSuggestionWiring`. */
  connectorSuggestion?: ConnectorSuggestionWiring
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
 * The per-token render path of a streaming answer is incremental: Markdown blocks
 * and node rows keep their DOM from one token to the next. `VITE_CHAT_RENDER_LEGACY=true`
 * restores the old path, which rebuilt every node row and every Markdown block per
 * token, for one release. Exact string, as for `VITE_ALLOW_DEV_AUTH_BYPASS`.
 */
const chatRenderLegacy = () => import.meta.env.VITE_CHAT_RENDER_LEGACY === 'true'

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
      <AnswerRegion
        answer={node.answer}
        onRegenerate={ctx.onRegenerate}
        citations={ctx.citations}
        temporaryChat={ctx.temporaryChat}
      />
    ),
  },
  grounding: {
    kind: 'grounding',
    render: (node) => <GroundingInlineSummary grounding={node.grounding} />,
  },
  confidence: {
    kind: 'confidence',
    render: (node) => (
      <ConfidenceNotice
        confidence={node.confidence}
        hasEvidence={node.hasEvidence}
        low={node.low}
        verification={node.verification}
      />
    ),
  },
  'memory-recall': {
    kind: 'memory-recall',
    render: (node, ctx) => (
      <MemoryRecallNotice count={node.count} memories={node.memories} threadId={ctx.threadId} />
    ),
  },
  'source-scope': {
    kind: 'source-scope',
    render: (node) => (
      <SourceScopeNotice scope={node.scope} excludedTools={node.excludedTools} knowledge={node.knowledge} />
    ),
  },
  'search-provenance': {
    kind: 'search-provenance',
    render: (node) => <SearchProvenanceNotice provenance={node.provenance} />,
  },
  'lifecycle-notices': {
    kind: 'lifecycle-notices',
    render: (node) => <LifecycleNotices notices={node.notices} />,
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
      <Show
        when={ctx.docked?.approvals}
        fallback={
          <ApprovalRequests
            approvals={node.approvals}
            onDecide={ctx.onApprovalDecision}
            onDecideEdited={ctx.onApprovalEditedDecision}
          />
        }
      >
        <DockedPointer />
      </Show>
    ),
  },
  'plan-approval': {
    kind: 'plan-approval',
    render: (node, ctx) => (
      <Show
        when={ctx.docked?.plan}
        fallback={
          <PlanApprovalControl
            planId={node.planId}
            summary={node.summary}
            steps={node.steps}
            grantedRung={node.grantedRung}
            rejected={node.rejected}
            pending={node.pending}
            error={node.error}
            onApprove={ctx.onApprovePlan}
            onReject={ctx.onRejectPlan}
          />
        }
      >
        <DockedPointer />
      </Show>
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
 * One run of a turn's node entries through the registry. The keyed form keeps a
 * row's DOM while the list re-derives; `VITE_CHAT_RENDER_LEGACY=true` keeps the old
 * unkeyed one. `SettledTurnContent` calls it once for the fold body and once for the rest.
 */
const renderNodeEntries = (entries: () => ConversationNodeEntry[]) =>
  chatRenderLegacy() ? (
    <For each={entries()}>
      {(entry) => CHAT_NODE_REGISTRY.render(entry.node, entry.context)}
    </For>
  ) : (
    <For each={entries()} keyed={(entry) => nodeKey(entry.node)}>
      {(entry) => CHAT_NODE_REGISTRY.renderLive(entry)}
    </For>
  )

/**
 * What the transcript keeps of a control the composer dock holds: where to look,
 * not a second copy of the control. Gone, and the full card back, once it is decided.
 */
function DockedPointer() {
  const i18n = useI18n()
  return (
    <p class="verevon-chat-docked-pointer">
      {i18n.tr('Venter på svar, se over skrivefeltet', 'Waiting for an answer, see above the composer')}
    </p>
  )
}

/**
 * The answer region: thinking indicator, error, or content.
 *
 * One component for all three because they share the streaming-class wrapper and
 * were previously chosen by a nested `Show`/fallback pair — a structure that made
 * rendering two of them at once impossible. Keeping that guarantee is why
 * `AnswerState` is a discriminated union rather than three separate nodes.
 */
export function AnswerRegion(props: {
  answer: AnswerState
  onRegenerate: () => void
  citations?: readonly Citation[]
  /**
   * The thread is temporary (ZDR): its run events are suppressed, so the
   * waiting state offers no "Varsle meg når ferdig" for it. Absent = ordinary.
   */
  temporaryChat?: boolean
}) {
  return (
    <Switch>
      <Match when={props.answer.state === 'pending' ? props.answer : null}>
        {(pending) => (
          <ThinkingDots
            since={pending().since}
            activity={pending().activity}
            runId={props.temporaryChat ? undefined : pending().runId}
          />
        )}
      </Match>
      <Match when={props.answer.state === 'failed' ? props.answer : null}>
        {(failed) => (
          <ErrorNotice
            message={failed().message}
            unsaved={failed().unsaved}
            action={failed().action}
            retryAt={failed().retryAt}
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

/**
 * A provider's own retry-after wait is only worth a line when a person would
 * have noticed it. Shorter waits are part of an ordinary answer.
 */
const PROVIDER_WAIT_NOTE_MS = 2_000

export function AssistantMessage(props: {
  copied: boolean
  message: ChatTurn
  /**
   * Message pinning. Absent when the surface does not offer it (a foreign-origin
   * or read-only thread), which is what hides the control entirely rather than
   * showing a dead one.
   */
  pinned?: boolean
  /** The thread already holds the maximum number of pins. */
  pinDisabled?: boolean
  onTogglePin?: () => void
  onBranch: () => void
  onCopy: () => void
  onFeedback: (rating: 'positive' | 'negative', note?: string) => Promise<boolean>
  onRegenerate: () => void
  onRerunAsNewTurn?: () => void
  /** See `MessageBlock`'s doc — same "Fortsett" affordance, forwarded through. */
  onContinue?: () => void
  /** See `MessageBlock`'s doc — same "Planlegg dette" affordance, forwarded through. */
  onScheduleThis?: () => void
  onApprovalDecision: (approvalId: string, decision: ApprovalDecision) => void
  /** Sends an approval with the person's edits (EP-10-S6); see `ApprovalRequests`. */
  onApprovalEditedDecision?: (approvalId: string, edit: ApprovalEdit) => Promise<void>
  onApprovePlan: (rung: AutonomyRung, justification: string, steps?: PlanStepEdit[]) => void
  onRejectPlan?: (planId: string, reason: string) => void
  planApproval?: PlanApprovalStatus
  onSelectFollowUp?: (text: string) => void
  /** See `MessageBlock`'s doc: a widget's quoted passage goes in front of the composer's draft (EP-4 W4-IO-2). */
  onQuoteSelection?: (text: string) => void
  onViewSteps: () => void
  onViewAttachments?: (attachmentId: string) => void
  version?: VersionBadge | null
  onSelectVersion?: (target: number) => void
  /**
   * The server's version switcher for this turn (WS17 S9). Set only on a versioned thread, where a switch
   * moves the thread's branch to the sibling the badge names. It replaces the client-only badge.
   */
  serverVersion?: ServerVersionBadge | null
  onSwitchServerVersion?: (messageId: string) => void
  /** The rewind this turn offers, and the turns it would hide, when a rewind is offered (WS17 S9). */
  rewindPlan?: RewindPlan | null
  /** Rewinds the thread to this turn, once the user has confirmed it. Nothing is deleted. */
  onRewind?: () => void
  /** The turns an undo of the rewind that landed on this turn would bring back. Set only under that turn. */
  rewindUndo?: HiddenTurn[] | null
  /** Undoes that rewind: the leaf returns to the turn it left. */
  onUndoRewind?: () => void
  /** See `MessageBlock`'s doc — marks the article `data-latest`. */
  isLatest?: boolean
  /** Forwarded into `ConversationNodeContext` — see its own doc for why. */
  threadId?: string | null
  /** See `MessageBlock`'s doc: disables "Les høyt". */
  temporaryChat?: boolean
  /** See `MessageBlock`'s doc: a Space thread's export is not offered. */
  spaceScoped?: boolean
  /** See `MessageBlock`'s doc. */
  docked?: DockedNodes
  /** See `MessageBlock`'s doc. */
  settledFold?: SettledFold
  /** See `MessageBlock`'s doc. */
  settledFoldChoices?: SettledFoldChoices
  /** See `MessageBlock`'s doc. */
  connectorSuggestion?: ConnectorSuggestionWiring
}) {
  const i18n = useI18n()
  const [reaction, setReaction] = createSignal<'up' | 'down' | null>(null)
  const [confirmingRewind, setConfirmingRewind] = createSignal(false)
  // Where the answer was processed, from the gateway's own stamp on `usage`.
  // Undefined (nothing rendered) unless the server asserted a residency or a
  // retention claim; never worked out from the model name.
  const processingNote = createMemo(() => answerProcessingNote(i18n, props.message.servedBy))
  // The longest single provider call's retry-after wait. Calls can run in
  // parallel, so summing them would overstate the time the person sat through.
  const providerWaitNote = createMemo(() => {
    const waitMs = Math.max(0, ...(props.message.timing?.phases ?? []).map((phase) => phase.provider?.retryWaitMs ?? 0))
    if (waitMs <= PROVIDER_WAIT_NOTE_MS) return undefined
    const seconds = Math.round(waitMs / 1000)
    return i18n.tr(`ventet ${seconds} s på leverandøren`, `waited ${seconds} s for the provider`)
  })

  // "Les høyt" reads the reply through governed text-to-speech (Azure Speech
  // via Verevon's speech route) in the UI language's voice. The browser's own
  // voices are never used, not even as a fallback: its online voices send the
  // reply text to the browser vendor.
  const player = speechPlayer()
  const playingId = createPlayingSpeechId(player)
  const readingAloud = createMemo(() => playingId() === props.message.id)
  const [readAloudFailed, setReadAloudFailed] = createSignal(false)
  // A press on "Les høyt" in Temporary chat: say why it is off, visibly.
  const [readAloudBlockedShown, setReadAloudBlockedShown] = createSignal(false)
  const readAloudUnavailable = () =>
    Boolean(props.temporaryChat) || props.message.streaming || !props.message.content.trim()
  // "Husk dette" (memory.remember): a user action, never the model's. `paused`
  // is its own outcome so a refused save is never shown as a failure of the
  // network, nor as a save.
  const [remembered, setRemembered] = createSignal<'idle' | 'saving' | 'saved' | 'paused' | 'failed'>('idle')
  const canRemember = () =>
    !props.temporaryChat && Boolean(props.threadId) && !props.message.streaming && Boolean(props.message.content.trim())
  const rememberThis = async () => {
    const threadId = props.threadId
    if (!threadId || remembered() === 'saving') return
    setRemembered('saving')
    try {
      await rememberMemory(threadId, props.message.content)
      setRemembered('saved')
    } catch (err) {
      setRemembered(err instanceof ApiError && err.code === 'memory_not_enabled' ? 'paused' : 'failed')
    }
  }
  // "Lagre som ferdighet" (EP-11 S1): an admin may save a private skill from an answer, a member only while
  // the member switch is on. A temporary chat and a conversation not yet saved never offer it. The member
  // answer is asked only for a signed-in member on an answer that could show the action, so an anonymous
  // or temporary render never calls the gateway.
  const [savingSkill, setSavingSkill] = createSignal(false)
  const skillActionShape = () =>
    !props.temporaryChat && Boolean(props.threadId) && !props.message.streaming && Boolean(props.message.content.trim())
  const askMemberSkillAuthoring = () =>
    skillActionShape() && getSession().status === 'authenticated' && !hasWorkspaceAdminAccess(getSession())
  const [memberSkills] = createResource(
    () => (askMemberSkillAuthoring() ? true : undefined),
    () => memberSkillAuthoring(),
  )
  const canSaveAsSkill = () =>
    skillActionShape() && (hasWorkspaceAdminAccess(getSession()) || memberSkills() === true)

  const toggleReadAloud = () => {
    if (player.playingId() === props.message.id) {
      player.stop()
      return
    }
    if (props.temporaryChat) {
      setReadAloudFailed(false)
      setReadAloudBlockedShown(true)
      return
    }
    setReadAloudBlockedShown(false)
    if (readAloudUnavailable()) return
    setReadAloudFailed(false)
    // Inside the tap, before the synthesis call: iOS only plays audio that
    // starts later on an element that already played in a gesture.
    player.unlock()
    void player
      .speak({ id: props.message.id, text: props.message.content, language: i18n.locale() })
      .then((outcome) => {
        // Fail closed with a notice; the reply itself stays on screen.
        if (outcome === 'failed') setReadAloudFailed(true)
      })
  }

  /**
   * Light the thumb immediately, then keep it only if the rating persisted.
   * Clicking the already-active thumb clears it, matching the previous toggle.
   */
  const rate = async (next: 'up' | 'down', wire: 'positive' | 'negative') => {
    const previous = reaction()
    setReaction(previous === next ? null : next)
    if (!(await props.onFeedback(wire))) setReaction(previous)
  }

  // Tiered feedback (thumbs -> optional category): a thumbs-down still records
  // immediately, same as before -- the category row only *enriches* that
  // rating with a `note` on a second call. It never gates or re-toggles the
  // thumb, so a category pick can't be confused with un-rating the message.
  const feedbackReasons = () => feedbackReasonLabels(i18n)
  const [showReasons, setShowReasons] = createSignal(false)
  const [pickedReason, setPickedReason] = createSignal<string | null>(null)

  const onThumbsDown = () => {
    const wasDown = reaction() === 'down'
    void rate('down', 'negative')
    setShowReasons(!wasDown)
    if (wasDown) setPickedReason(null)
  }

  const pickReason = async (reason: string) => {
    setPickedReason(reason)
    setShowReasons(false)
    await props.onFeedback('negative', reason)
  }

  const waiting = createMemo(() => props.message.status === 'waiting')
  const errored = createMemo(() => props.message.status === 'error')
  const effectful = createMemo(() => isEffectfulChatTurn(props.message.effectClass))
  // "Fortsett" (continue-after-stop, chat-parity §0.1) applies only to a
  // STOPPED turn that actually has something to continue from. A genuinely
  // empty stopped turn (the user stopped it before any text arrived) has
  // nothing to splice a continuation onto — "Generer på nytt" below already
  // covers that case correctly, so this stays false rather than offering a
  // control with nothing behind it.
  const canContinue = createMemo(
    () => props.message.status === 'stopped' && props.message.content.trim().length > 0,
  )

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
      onApprovalEditedDecision: props.onApprovalEditedDecision,
      onApprovePlan: props.onApprovePlan,
      onRejectPlan: props.onRejectPlan,
      onSelectFollowUp: props.onSelectFollowUp,
      onRegenerate: props.onRegenerate,
      citations: props.message.citations,
      threadId: props.threadId ?? null,
      temporaryChat: props.temporaryChat,
      docked: props.docked,
    }
    return nodes().map((node) => ({ node, context }))
  })

  // "Eksporter til Word/Excel/PowerPoint" / "Lagre som PDF" — captured here
  // (the rendered answer, not the `<article>`) is exactly what `print.ts`
  // clones for the PDF path; see the doc comment where it's set below.
  let contentRef: HTMLDivElement | undefined

  const [exportError, setExportError] = createSignal<string | null>(null)

  /**
   * What an export saves. Verevon's long work lands in a document artifact
   * while the reply itself stays a short note ("Utkastet er klart for
   * gjennomgang i Resultat"), so a short reply that produced a document
   * exports that document, under its own title: the note alone would make a
   * one-line Word file of the only part the reader did not need. The newest
   * document wins, the one the Resultat panel opens on. A reply long enough
   * to stand on its own is exported as written.
   */
  const exportDocument = createMemo(() => {
    // Nothing is exported from an answer still streaming in (`showExportButton`
    // is false for it and the action row is hidden), and measuring it parses the
    // whole message: once per token, on top of the Markdown's own tail parse.
    if (props.message.streaming) return null
    if (answerExportSize(props.message.content) >= EXPORT_BUTTON_MIN_CHARS) return null
    const documents = (props.message.artifacts ?? []).filter(
      (artifact) => artifactRenderKind(artifact) === 'document' && artifact.content.trim(),
    )
    return documents.at(-1) ?? null
  })

  const runExport = (format: ExportFormat) => {
    setExportError(null)
    const document = exportDocument()
    const input = {
      content: document?.content ?? props.message.content,
      // A document artifact names itself. A reply has no document title of
      // its own, so exportChatAnswer names the file after its leading
      // heading, or a markdown-free preview of its first text.
      title: document?.title,
      createdAt: props.message.createdAt,
      sources: (props.message.citations ?? []).map((citation) => ({
        title: citation.title || hostname(citation.url),
        url: citation.url,
      })),
      locale: i18n.locale(),
    }
    try {
      if (format === 'pdf' && document) {
        // Nothing on screen shows this document for sure (the Resultat panel
        // may be closed or open on another artifact), so it is rendered
        // off-screen with the same ChatMarkdown the panel uses. print.ts
        // copies the element synchronously, so it can be disposed right after.
        const host = window.document.createElement('div')
        host.className = 'verevon-chat-artifact-document'
        const dispose = render(() => <ChatMarkdown content={document.content} />, host)
        try {
          exportChatAnswer('pdf', { ...input, element: host })
        } finally {
          dispose()
        }
        return
      }
      exportChatAnswer(format, { ...input, element: format === 'pdf' ? (contentRef ?? null) : undefined })
    } catch {
      setExportError(i18n.tr('Eksport mislyktes. Prøv igjen.', 'Export failed. Please try again.'))
    }
  }

  /**
   * The four export choices, shared by the overflow "..." menu (always
   * offered) and the visible export button that joins it once the answer is
   * long enough to be worth a document of its own (`showExportButton` below).
   * Rebuilt on every read rather than memoized: `i18n.tr` needs to react to a
   * locale switch, exactly like every other inline menu-items array in this
   * component (see the "Fortsett i ny chat" / "Les høyt" list just below).
   */
  const exportMenuItems = (): MessageMenuItem[] => [
    { label: i18n.tr('Eksporter til Word', 'Export to Word'), icon: <FileText size={16} />, onClick: () => runExport('docx') },
    { label: i18n.tr('Eksporter til Excel', 'Export to Excel'), icon: <FileSpreadsheet size={16} />, onClick: () => runExport('xlsx') },
    { label: i18n.tr('Eksporter til PowerPoint', 'Export to PowerPoint'), icon: <Presentation size={16} />, onClick: () => runExport('pptx') },
    { label: i18n.tr('Lagre som PDF', 'Save as PDF'), icon: <Download size={16} />, onClick: () => runExport('pdf') },
  ]

  // Microsoft Copilot's own threshold for showing an export affordance at
  // all, rather than leaving it buried in the overflow menu for every reply
  // no matter how short. A short reply that produced a document qualifies
  // too: that document is what gets exported (`exportDocument`). A streaming
  // or still-empty answer never qualifies — there is nothing finished yet.
  const showExportButton = createMemo(
    () => !props.message.streaming
      && (answerExportSize(props.message.content) >= EXPORT_BUTTON_MIN_CHARS || exportDocument() !== null),
  )

  // The files this answer's run saved to its workspace, for the changed-files card (F-t3-076). Read from the tool
  // calls the transcript already holds, so nothing is stored or sent.
  const answerChangedFiles = createMemo(() => changedFiles(props.message.toolCalls ?? []))

  // The web-search query whose image strip sits under this answer (F-t3-044, W4-IO-1). Off unless VITE_CHAT_IMAGE_STRIP is
  // "true". Never for a temporary chat (zero retention) or a Space thread (room content). Not while the answer streams or
  // waits, and a search that failed or is still running gives no query (`webSearchQueryOf`).
  const imageStripQuery = createMemo(() => {
    if (!chatImageStripEnabled() || props.temporaryChat || props.spaceScoped) return undefined
    if (props.message.streaming || waiting() || errored()) return undefined
    return webSearchQueryOf(props.message.toolCalls ?? [])
  })

  return (
    // `tabindex=-1`: focusable for the arrow-key walk in ChatPage, but never
    // in the Tab sequence, which still runs answer -> actions -> next message
    // through DOM order (audit item 26).
    <article
      class="verevon-chat-message verevon-chat-message--assistant"
      data-latest={props.isLatest ? '' : undefined}
      data-turn-id={readerPolishOn() ? props.message.id : undefined}
      tabindex={-1}
    >
      <div class="verevon-chat-message__avatar">
        <span class="verevon-chat-message__logo" aria-hidden="true" />
      </div>
      <div class="verevon-chat-message__body">
        <div class="verevon-chat-message__heading">
          <span>Verevon</span>
          <time>{formatRelative(props.message.createdAt, i18n.locale())}</time>
        </div>
        {/*
          Rendered from the derived node list through the keyed registry. Adding
          a node kind means adding a renderer to CHAT_NODE_REGISTRY and a case to
          `deriveConversationNodes` — not editing this component. Rows are keyed
          by `nodeKey`, not by the node object: every derivation builds fresh
          node objects, so keying on identity rebuilt every node on every token
          during streaming. A row now keeps its DOM while its entry changes, and
          `renderLive` is what lets the renderer see the new entry.

          Wrapped in its own div (no styling of its own — plain block, same
          layout as before) so "Lagre som PDF" has a stable DOM node to clone:
          just the rendered answer, not the "Verevon" heading row above or the
          action buttons below (print.ts's own CSS also hides the latter, but
          never rendering them into the clone at all is one less thing that
          could leak into a "clean" PDF).
        */}
        <div class="verevon-chat-message__content" ref={(el) => { contentRef = el }}>
          {/* The one surface where a ```widget fence may run (EP-4 S1): `InlineWidgetSurface`.
              A Space thread shown here is room content (D2), so it keeps the code block. */}
          <InlineWidgetSurface value={inlineWidgetsEnabled() && !props.spaceScoped ? 'live' : 'inert'}>
            <InlineWidgetAnswer onSelectFollowUp={props.onSelectFollowUp} onQuoteSelection={props.onQuoteSelection}>
              <SettledTurnContent
                fold={props.settledFold}
                turnId={props.message.id}
                choices={props.settledFoldChoices}
                entries={nodeEntries()}
                renderEntries={renderNodeEntries}
              />
            </InlineWidgetAnswer>
            {/* EP-4 S9: the apps of this answer's settled MCP calls, under the same chat-only surface. */}
            <Show when={mcpAppsEnabled() && (props.message.toolCalls?.length ?? 0) > 0}>
              <McpAppsForAnswer
                calls={props.message.toolCalls ?? []}
                streaming={props.message.streaming}
                temporaryChat={props.temporaryChat}
                onSelectFollowUp={props.onSelectFollowUp}
                runId={props.message.runId}
              />
            </Show>
            {/* F-t3-076: the files this run saved to its workspace. Not in a temporary chat or while the answer streams. */}
            <Show when={chatChangedFilesEnabled() && !props.temporaryChat && !props.message.streaming}>
              <ChangedFilesCard files={answerChangedFiles()} />
            </Show>
          </InlineWidgetSurface>
          <Show when={stepStripEnabled && props.message.streaming}>
            <StepStrip
              calls={props.message.toolCalls ?? []}
              planSteps={props.message.proposedPlan?.steps.length}
              notices={props.message.lifecycle}
            />
          </Show>
        </div>
        {/* W4-IO-1 (F-t3-044): the images from this answer's web search, each linked to its page. Mounted outside the
            answer's content, so the PDF clone of the answer never carries them. */}
        <Show when={imageStripQuery()}>
          {(query) => <ChatImageStrip turnId={props.message.id} query={query()} />}
        </Show>
        {/* EP-2 S5: a reloaded plan turn shows its persisted plan as a read-only card once a later turn exists. While the
            plan turn is the latest answer the live card in the composer trays owns the plan, so nothing is mounted here. */}
        <Show when={researchPlanTurnEnabled() && !props.isLatest && !props.message.streaming ? props.message.researchPlan : undefined}>
          {(plan) => <ResearchPlanCard readOnly plan={plan()} onStart={() => {}} onCancel={() => {}} />}
        </Show>
        {/* EP-10 S8: at most one connected-app suggestion per turn; the card hides itself in a Space or temporary chat. */}
        <Show when={props.connectorSuggestion ? props.message.connectorSuggestion : undefined}>
          {(suggestion) => (
            <ConnectorSuggestionCard
              suggestion={suggestion()}
              orgId={props.connectorSuggestion?.orgId ?? ''}
              spaceScoped={props.spaceScoped}
              temporary={props.temporaryChat}
              onUse={(connectionId) => props.connectorSuggestion?.onUse(suggestion(), connectionId)}
              connectTurn={props.connectorSuggestion?.connectTurn}
              pendingConnectIntent={props.connectorSuggestion?.pendingConnectIntent}
              resumeConnectIntent={props.connectorSuggestion?.resumeConnectIntent}
              dismissConnectIntent={props.connectorSuggestion?.dismissConnectIntent}
            />
          )}
        </Show>
        <Show when={!waiting() && !errored()}>
          <div class="verevon-chat-message-actions">
            <Show when={props.pinned}>
              {/* Not only a changed action label: a pin steers every later
                  turn, so it has to be visible while reading rather than
                  discoverable by hovering. */}
              <span class="verevon-chat-message-pinned">
                <Pin size={11} />
                {i18n.tr('Festet i konteksten', 'Pinned into the context')}
              </span>
            </Show>
            <Show when={props.onTogglePin}>
              <MessageAction
                label={props.pinned
                  ? i18n.tr('Løsne fra konteksten', 'Unpin from the context')
                  : props.pinDisabled
                    ? i18n.tr('Maks antall festede meldinger', 'Pin limit reached')
                    : i18n.tr('Fest i konteksten', 'Pin into the context')}
                onClick={() => props.onTogglePin?.()}
              >
                <Pin size={14} />
              </MessageAction>
            </Show>
            <MessageAction label={props.copied ? 'Copied' : 'Copy'} onClick={props.onCopy}>
              {props.copied ? <Check size={14} /> : <Copy size={14} />}
            </MessageAction>
            <Show when={threadForkEnabled() && !props.temporaryChat && Boolean(props.threadId) && props.message.role === 'assistant' && !props.message.streaming}>
              <ForkAction threadId={props.threadId ?? ''} messageId={props.message.id} />
            </Show>
            {/* The export is about the whole conversation, so it is offered once: on the latest answer, beside fork. */}
            <Show when={threadExportEnabled() && props.isLatest && Boolean(props.threadId) && !props.message.streaming}>
              <ExportConversationAction
                threadId={props.threadId ?? ''}
                temporaryChat={props.temporaryChat}
                spaceScoped={props.spaceScoped}
              />
            </Show>
            {/* A snapshot shares a redacted copy with named members, so it sits beside the export on the same answer (W4-SH-6). */}
            <Show when={shareSnapshotEnabled() && props.isLatest && Boolean(props.threadId) && !props.message.streaming}>
              <ShareSnapshotDialog
                threadId={props.threadId ?? ''}
                temporaryChat={props.temporaryChat}
                spaceScoped={props.spaceScoped}
              />
            </Show>
            {/* An import makes a new conversation from a file, so it is offered beside the export on the same answer (EP-12 S15). */}
            <Show when={threadImportEnabled() && props.isLatest && Boolean(props.threadId) && !props.message.streaming}>
              <ImportConversationAction temporaryChat={props.temporaryChat} spaceScoped={props.spaceScoped} />
            </Show>
            <Show when={canRemember()}>
              <MessageAction
                label={remembered() === 'saved' ? i18n.tr('Husket', 'Remembered') : i18n.tr('Husk dette', 'Remember this')}
                onClick={() => void rememberThis()}
              >
                {remembered() === 'saved' ? <Check size={14} /> : <Brain size={14} />}
              </MessageAction>
            </Show>
            <Show when={canSaveAsSkill()}>
              <MessageAction
                label={i18n.tr('Lagre som ferdighet', 'Save as skill')}
                onClick={() => setSavingSkill(true)}
              >
                <Sparkles size={14} />
              </MessageAction>
            </Show>
            {/*
              A dedicated, always-visible export button once the answer is
              long enough to be worth a document of its own (Copilot's own
              600-character threshold — see `showExportButton`). A shorter
              answer still gets all four choices, just tucked into the "..."
              overflow menu below rather than earning its own icon.
            */}
            <Show when={showExportButton()}>
              <MessageMenu
                trigger={{ icon: <Download size={14} />, label: i18n.tr('Eksporter svar', 'Export answer') }}
                items={exportMenuItems()}
              />
            </Show>
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
              onClick={onThumbsDown}
            >
              <ThumbsDown size={14} />
            </MessageAction>
            <Show when={props.serverVersion}>
              {(badge) => (
                <div class="verevon-chat-version-switcher" role="group" aria-label={i18n.tr('Svarversjon', 'Answer version')}>
                  <button
                    type="button"
                    class="verevon-chat-version-switcher__arrow"
                    disabled={badge().previousId === null}
                    aria-label={i18n.tr('Forrige versjon', 'Previous version')}
                    onClick={() => {
                      const previous = badge().previousId
                      if (previous) props.onSwitchServerVersion?.(previous)
                    }}
                  >
                    <ChevronLeft size={14} />
                  </button>
                  <span class="verevon-chat-version-switcher__label">
                    {badge().current}/{badge().total}
                  </span>
                  <button
                    type="button"
                    class="verevon-chat-version-switcher__arrow"
                    disabled={badge().nextId === null}
                    aria-label={i18n.tr('Neste versjon', 'Next version')}
                    onClick={() => {
                      const next = badge().nextId
                      if (next) props.onSwitchServerVersion?.(next)
                    }}
                  >
                    <ChevronRight size={14} />
                  </button>
                </div>
              )}
            </Show>
            <Show when={props.rewindPlan}>
              {(plan) => (
                <>
                  <MessageAction
                    label={i18n.tr('Gå tilbake hit', 'Rewind to here')}
                    onClick={() => setConfirmingRewind(true)}
                  >
                    <RotateCcw size={14} />
                  </MessageAction>
                  <Show when={confirmingRewind()}>
                    <div
                      class="verevon-chat-rewind-confirm"
                      role="dialog"
                      aria-modal="true"
                      aria-label={i18n.tr('Gå tilbake hit', 'Rewind to here')}
                      tabindex={-1}
                      ref={(element) => queueMicrotask(() => element.focus())}
                      onKeyDown={(event) => {
                        if (event.key === 'Escape') setConfirmingRewind(false)
                      }}
                    >
                      <p>
                        {i18n.tr(
                          `${plan().hiddenTurns} svar og meldinger etter dette punktet skjules. Ingenting slettes: de ligger fortsatt i en egen gren. Du kan angre rett etterpå.`,
                          `${plan().hiddenTurns} later ${plan().hiddenTurns === 1 ? 'turn' : 'turns'} will be hidden. Nothing is deleted: they stay on their own branch. You can undo this straight after.`,
                        )}
                      </p>
                      <HiddenTurnList turns={plan().hidden} />
                      <button type="button" onClick={() => setConfirmingRewind(false)}>
                        {i18n.tr('Avbryt', 'Cancel')}
                      </button>
                      <button
                        type="button"
                        onClick={() => {
                          setConfirmingRewind(false)
                          props.onRewind?.()
                        }}
                      >
                        {i18n.tr('Gå tilbake hit', 'Rewind to here')}
                      </button>
                    </div>
                  </Show>
                </>
              )}
            </Show>
            <Show when={props.version}>
              {(version) => (
                <div class="verevon-chat-version-switcher" role="group" aria-label={i18n.tr('Svarversjon', 'Answer version')}>
                  <button
                    type="button"
                    class="verevon-chat-version-switcher__arrow"
                    disabled={version().current <= 1}
                    aria-label={i18n.tr('Forrige versjon', 'Previous version')}
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
                    aria-label={i18n.tr('Neste versjon', 'Next version')}
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
                    <MessageAction label={i18n.tr('Kjør som ny tur', 'Run as a new turn')} onClick={() => props.onRerunAsNewTurn?.()}>
                      <RefreshCw size={14} />
                    </MessageAction>
                  </Show>
                </>
              )}
            >
              {/* "Fortsett" — offered first, ahead of Regenerate, because it is
                  the more specific and usually more useful action on a turn
                  that already has partial text: it keeps what was written and
                  asks the model to pick up from exactly there, rather than
                  discarding it for a fresh answer. Only ever rendered
                  alongside Regenerate, never instead of it — a person who
                  would rather start over still can. */}
              <Show when={canContinue() && props.onContinue}>
                <MessageAction
                  label={i18n.tr('Fortsett', 'Continue')}
                  onClick={() => props.onContinue?.()}
                >
                  <ArrowRight size={14} />
                </MessageAction>
              </Show>
              {/* The label names the outcome, not the verb. Regenerating keeps
                  the previous answer as a switchable version, but the switcher
                  only appears AFTER the first regenerate — so the one moment a
                  user could learn the feature exists is the moment before they
                  use it, and "Regenerate" alone never told them. */}
              <MessageAction
                label={
                  props.version
                    ? `Generer på nytt (versjon ${props.version.current}/${props.version.total})`
                    : 'Generer på nytt — beholder dette svaret som versjon 1'
                }
                onClick={props.onRegenerate}
              >
                <RefreshCw size={14} />
              </MessageAction>
            </Show>
            <MessageMenu
              items={[
                { label: 'Fortsett i ny chat', icon: <MessageSquarePlus size={16} />, onClick: props.onBranch },
                ...(props.onScheduleThis
                  ? [{
                      label: i18n.tr('Planlegg dette', 'Schedule this'),
                      icon: <CalendarClock size={16} />,
                      onClick: () => props.onScheduleThis?.(),
                    } satisfies MessageMenuItem]
                  : []),
                readingAloud()
                  ? { label: i18n.tr('Stopp opplesing', 'Stop reading aloud'), icon: <Square size={16} />, onClick: toggleReadAloud }
                  : {
                      label: i18n.tr('Les høyt', 'Read aloud'),
                      icon: <Volume2 size={16} />,
                      onClick: toggleReadAloud,
                      // Temporary chat: focusable, described and explained on a press.
                      // A reply still streaming or with no text is only briefly off.
                      unavailableReason: props.temporaryChat ? readAloudBlockedMessage(i18n.tr) : undefined,
                      disabled: !props.temporaryChat && readAloudUnavailable(),
                    },
                // Always offered here, whether or not the dedicated export
                // button above is also showing — a short answer still has
                // four legitimate export choices, just no icon of its own.
                ...exportMenuItems(),
              ]}
            />
            <MessageMetricsBadge message={props.message} />
            <ReasoningPopover message={props.message} temporaryChat={props.temporaryChat} />
          </div>
          <Show when={processingNote()}>
            {(note) => (
              <p class="verevon-chat-processing-line" role="note" title={note().title}>
                {note().text}
              </p>
            )}
          </Show>
          <Show when={providerWaitNote()}>
            {(text) => (
              <p class="verevon-chat-processing-line" role="note">
                {text()}
              </p>
            )}
          </Show>
          <Show when={readAloudFailed()}>
            <p class="verevon-chat-feedback-reasons__note" role="alert">
              {readAloudUnavailableMessage(i18n.tr)}
            </p>
          </Show>
          <Show when={remembered() === 'paused'}>
            <p class="verevon-chat-feedback-reasons__note" role="status">
              {i18n.tr(
                'Minnet er pauset, av eller slått av av organisasjonen, så dette ble ikke lagret.',
                'Memory is paused, off or turned off by your organization, so this was not saved.',
              )}
            </p>
          </Show>
          <Show when={remembered() === 'failed'}>
            <p class="verevon-chat-feedback-reasons__note" role="alert">
              {i18n.tr('Kunne ikke lagre minnet.', 'Could not save the memory.')}
            </p>
          </Show>
          <Show when={exportError()}>
            <p class="verevon-chat-feedback-reasons__note" role="alert">
              {exportError()}
            </p>
          </Show>
          <Show when={props.rewindUndo && props.rewindUndo.length > 0 ? props.rewindUndo : undefined}>
            {(restored) => (
              <div class="verevon-chat-rewind-undo" role="status">
                <p>
                  {i18n.tr(
                    `Du er tilbake her. Angre gjenoppretter ${restored().length} ${restored().length === 1 ? 'melding' : 'meldinger'} etter dette punktet.`,
                    `You are back at this point. Undo restores ${restored().length} later ${restored().length === 1 ? 'turn' : 'turns'}.`,
                  )}
                </p>
                <HiddenTurnList turns={restored()} />
                <button type="button" onClick={() => props.onUndoRewind?.()}>
                  {i18n.tr('Angre', 'Undo')}
                </button>
              </div>
            )}
          </Show>
          <Show when={props.temporaryChat && readAloudBlockedShown()}>
            <p class="verevon-chat-feedback-reasons__note" role="status">
              {readAloudBlockedMessage(i18n.tr)}
            </p>
          </Show>
          <Show when={showReasons()}>
            <div class="verevon-chat-feedback-reasons" role="group" aria-label={i18n.tr('Hva var galt?', 'What was wrong?')}>
              <span>{i18n.tr('Hva var galt?', 'What was wrong?')}</span>
              <For each={feedbackReasons()}>
                {(reason) => (
                  <button
                    type="button"
                    class="verevon-chat-feedback-reasons__chip"
                    onClick={() => void pickReason(reason)}
                  >
                    {reason}
                  </button>
                )}
              </For>
            </div>
          </Show>
          <Show when={pickedReason()}>
            {(reason) => (
              <p class="verevon-chat-feedback-reasons__note">
                {i18n.tr('Takk — merket som', 'Thanks — flagged as')} “{reason()}”.
              </p>
            )}
          </Show>
        </Show>
        <Show when={savingSkill()}>
          <SaveAsSkillDialog
            threadId={props.threadId ?? ''}
            messageId={props.message.id}
            content={props.message.content}
            temporary={props.temporaryChat}
            onClose={() => setSavingSkill(false)}
          />
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
  const i18n = useI18n()
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
    <article class="verevon-chat-message verevon-chat-message--user" data-turn-id={readerPolishOn() ? props.message.id : undefined} tabindex={-1}>
      <div class="verevon-chat-user-meta">
        <span>Meg</span>
        <time>{formatRelative(props.message.createdAt, i18n.locale())}</time>
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
              <button type="button" disabled={!draft().trim()} onClick={submitEdit}>{i18n.tr('Send på nytt', 'Send again')}</button>
            </div>
          </div>
        )}
      >
        <>
          <div class="verevon-chat-bubble">
            <UserMessageContent content={props.message.content} />
            <ToolChips tools={props.message.tools} />
            <AttachmentChips attachments={props.message.attachments} tone="user" onOpen={props.onViewAttachments} />
            <ContextRefChips refs={props.message.contextRefs ?? []} />
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

/**
 * Keep pasted product links easy to scan without changing what the model
 * received. Long retail URLs are rendered as source rows while the original
 * message remains the value submitted to the conversation.
 */
function UserMessageContent(props: { content: string }) {
  const parts = createMemo(() => splitUserMessageContent(props.content))

  return (
    <div class="verevon-chat-bubble__text">
      <For each={parts()}>
        {(part) => (
          <Switch>
            <Match when={part.kind === 'copy' ? part : null}>
              {(copy) => <span class="verevon-chat-bubble__copy">{copy().value}</span>}
            </Match>
            <Match when={part.kind === 'link' ? part : null}>
              {(link) => (
                <a
                  class="verevon-chat-bubble__source"
                  href={link().href}
                  target="_blank"
                  rel="noreferrer"
                  title={link().href}
                >
                  <Globe2 size={14} strokeWidth={1.8} aria-hidden="true" />
                  <span class="verevon-chat-bubble__source-copy">
                    <span class="verevon-chat-bubble__source-title">{link().label}</span>
                    <span class="verevon-chat-bubble__source-domain">{link().domain}</span>
                  </span>
                  <ExternalLink size={13} strokeWidth={1.8} aria-hidden="true" />
                </a>
              )}
            </Match>
          </Switch>
        )}
      </For>
    </div>
  )
}

type UserMessagePart =
  | { kind: 'copy'; value: string }
  | { kind: 'link'; href: string; label: string; domain: string }

const USER_MESSAGE_URL = /https?:\/\/[^\s<>()]+/gi

function splitUserMessageContent(content: string): UserMessagePart[] {
  const parts: UserMessagePart[] = []
  let cursor = 0

  for (const match of content.matchAll(USER_MESSAGE_URL)) {
    const start = match.index ?? cursor
    const raw = match[0]
    const href = trimUrlPunctuation(raw)
    if (start > cursor) parts.push({ kind: 'copy', value: content.slice(cursor, start) })
    if (href) {
      const descriptor = describeUserUrl(href)
      parts.push({ kind: 'link', href, ...descriptor })
    } else {
      parts.push({ kind: 'copy', value: raw })
    }
    cursor = start + raw.length
  }

  if (cursor < content.length || parts.length === 0) {
    parts.push({ kind: 'copy', value: content.slice(cursor) })
  }

  return parts
}

function trimUrlPunctuation(value: string): string {
  return value.replace(/[.,;:!?]+$/u, '').replace(/[)\]}]+$/u, '')
}

function describeUserUrl(href: string): { domain: string; label: string } {
  try {
    const url = new URL(href)
    const domain = url.hostname.replace(/^www\./iu, '')
    const segments = url.pathname.split('/').filter(Boolean)
    const lastSegment = segments.at(-1)
    const decoded = lastSegment ? decodeURIComponent(lastSegment) : ''
    const label = decoded
      .replace(/[-_]+/gu, ' ')
      .replace(/\s+/gu, ' ')
      .trim()

    return { domain, label: label || domain }
  } catch {
    return { domain: 'Nettlenke', label: href }
  }
}

export function ChatMarkdown(props: { content: string; citations?: readonly Citation[] }) {
  const legacy = chatRenderLegacy()
  // A streamed answer only grows, so each parse resumes from the last one: the
  // sealed blocks come back as the same objects and only the tail is read again.
  // Plain closure state, not a signal: it is the memo's own previous value.
  let parsed: ParsedMarkdown | undefined
  const blocks = createMemo(() => {
    if (legacy) return parseMarkdownBlocks(props.content)
    parsed = parseMarkdownIncremental(parsed, props.content)
    return parsed.blocks
  })
  return (
    <div class="verevon-chat-markdown">
      {legacy ? (
        <For each={blocks()}>
          {(block) => <MarkdownBlockView block={block} citations={props.citations} />}
        </For>
      ) : (
        // Rows by position: a sealed block is the same object, so its row is not
        // touched; only the trailing rows receive a new block.
        <For each={blocks()} keyed={false}>
          {(block) => <MarkdownBlockView block={block()} citations={props.citations} />}
        </For>
      )}
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
      <Match when={props.block.kind === 'math'}>
        <MarkdownMathBlock block={props.block as Extract<MarkdownBlock, { kind: 'math' }>} />
      </Match>
      <Match when={props.block.kind === 'table'}>
        <MarkdownTable block={props.block as Extract<MarkdownBlock, { kind: 'table' }>} citations={props.citations} />
      </Match>
      <Match when={props.block.kind === 'list'}>
        <MarkdownList block={props.block as Extract<MarkdownBlock, { kind: 'list' }>} citations={props.citations} />
      </Match>
      <Match when={props.block.kind === 'quote'}>
        <MarkdownQuote block={props.block as Extract<MarkdownBlock, { kind: 'quote' }>} citations={props.citations} />
      </Match>
      <Match when={props.block.kind === 'hr'}>
        <hr />
      </Match>
      <Match when={props.block.kind === 'paragraph'}>
        <p>{parseInline((props.block as Extract<MarkdownBlock, { kind: 'paragraph' }>).text, props.citations)}</p>
      </Match>
      <Match when={props.block.kind === 'details'}>
        <MarkdownDetails block={props.block as Extract<MarkdownBlock, { kind: 'details' }>} citations={props.citations} />
      </Match>
    </Switch>
  )
}

/** Model-authored `<details>`: a real collapsible whose body is parsed
 * markdown (never injected HTML), instead of escaped tags around the text. */
function MarkdownDetails(props: { block: Extract<MarkdownBlock, { kind: 'details' }>; citations?: readonly Citation[] }) {
  return (
    <details class="verevon-chat-details">
      <summary>{parseInline(props.block.summary, props.citations)}</summary>
      <div class="verevon-chat-details__body">
        <For each={props.block.blocks}>
          {(block) => <MarkdownBlockView block={block} citations={props.citations} />}
        </For>
      </div>
    </details>
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

/**
 * A ```mermaid``` fenced block is a diagram once (and only once) its fence has
 * actually closed — `props.block.closed` — never mid-stream: an in-progress
 * diagram source is frequently invalid Mermaid syntax by construction (a cut-
 * off node definition, an unbalanced `[...]` label), and `mermaid.render`
 * rejecting that half-written source is not an error to recover from, it's
 * the expected shape of every token before the last one. Until the fence
 * closes this renders exactly like any other language's code block, same as
 * the rest of this file's streaming-resilience approach.
 *
 * A closed ```chart``` fence is the same story: the native chart when its JSON
 * reads, the plain code block plus a one-line note when it does not (see
 * `ChartFenceBlock`), and a plain code block while it is still streaming.
 *
 * A closed ```widget fence is a live sandboxed widget only where the surface
 * says so (`InlineWidgetSurface`, a chat assistant answer) and the switch is on;
 * everywhere else, and while streaming, it is a code block like any other.
 */
export function MarkdownCodeBlock(props: { block: Extract<MarkdownBlock, { kind: 'code' }> }) {
  const widgetsLive = useInlineWidgetsLive()
  return (
    <Show
      when={props.block.closed && props.block.lang.trim().toLowerCase() === 'chart'}
      fallback={
        <Show
          when={props.block.closed && props.block.lang.trim().toLowerCase() === 'mermaid'}
          fallback={
            <Show
              when={widgetsLive && isWidgetFence(props.block)}
              fallback={<TranscriptCodeBlock block={props.block} />}
            >
              <InlineWidget source={props.block.text} fallback={<TranscriptCodeBlock block={props.block} />} />
            </Show>
          }
        >
          <MermaidDiagram code={props.block.text} />
        </Show>
      }
    >
      <ChartFenceBlock text={props.block.text} fallback={<TranscriptCodeBlock block={props.block} />} />
    </Show>
  )
}

/** How long a copy confirmation ("Kopiert"/"Kunne ikke kopiere") stays up. */
const COPY_CONFIRMATION_MS = 1800

/**
 * A copy confirmation keyed by WHEN it happened rather than a live component
 * timer, so it survives `TranscriptCodeBlock` remounting mid-window — see
 * that component's own doc for why a remount happens constantly during
 * streaming. Keyed on the block's own text, same as `mermaidRenderCache`
 * above, so two blocks with identical content are treated as the same copy
 * (exactly like two identical mermaid diagrams sharing one cached render).
 */
const copyOutcomeCache = new Map<string, { result: 'copied' | 'failed'; at: number }>()

/**
 * A fenced code block that is not a closed ```mermaid``` diagram (any other
 * language, or a mermaid fence still streaming in — see `MarkdownCodeBlock`
 * above). Colored with the same dependency-free scanner the Artifacts panel
 * uses (`highlightCode` from chat-code-highlight.ts), reusing its
 * `.verevon-code-*` token classes rather than inventing a second palette —
 * but only once the fence has actually closed and its source sits within
 * `MAX_HIGHLIGHT_CHARS`. Exactly the mermaid branch's streaming-resilience
 * reasoning applies here too: a fence still streaming in is frequently not
 * yet complete, valid source for its language (a cut-off string literal, an
 * unbalanced bracket), so re-tokenizing it on every incoming token would be
 * wasted work that would also visibly re-color the block mid-stream. Until
 * the fence closes — or for a block too large to scan cheaply — this renders
 * the same plain `<pre><code>` it always has.
 */
function TranscriptCodeBlock(props: { block: Extract<MarkdownBlock, { kind: 'code' }> }) {
  const i18n = useI18n()
  // Under `VITE_CHAT_RENDER_LEGACY` (see `ChatMarkdown`) every incoming token
  // reparses the whole message into brand-new block objects, so `<For>` remounts
  // this component the moment ANYTHING else in a still-streaming message
  // changes, not only when this particular fence grows. A component-local timer
  // would then have its confirmation wiped back to "idle" well before 1800ms
  // whenever the reply keeps generating after this block has already closed.
  // `copyOutcomeCache` above carries the outcome and its timestamp across
  // that remount so the confirmation reads for the full window regardless. The
  // incremental path keeps a closed block's row, but a row can still be rebuilt
  // when the answer is replaced, so the cache stays.
  const cachedOutcome = () => {
    const cached = copyOutcomeCache.get(props.block.text)
    if (!cached) return null
    const remaining = COPY_CONFIRMATION_MS - (Date.now() - cached.at)
    return remaining > 0 ? { result: cached.result, remaining } : null
  }
  const initialOutcome = cachedOutcome()
  const [copyState, setCopyState] = createSignal<'copied' | 'failed' | 'idle'>(initialOutcome?.result ?? 'idle')
  let copyTimer: number | undefined
  const scheduleRevert = (delayMs: number) => {
    window.clearTimeout(copyTimer)
    copyTimer = window.setTimeout(() => {
      copyOutcomeCache.delete(props.block.text)
      setCopyState('idle')
    }, delayMs)
  }
  if (initialOutcome) scheduleRevert(initialOutcome.remaining)
  onCleanup(() => window.clearTimeout(copyTimer))

  const highlighted = createMemo(() => {
    if (!props.block.closed || props.block.text.length > MAX_HIGHLIGHT_CHARS) return null
    return highlightCode(props.block.text, props.block.lang)
  })

  const copyCode = async () => {
    let result: 'copied' | 'failed'
    try {
      await navigator.clipboard.writeText(props.block.text)
      result = 'copied'
    } catch {
      // Clipboard access can be denied (no permission, insecure context). Say so
      // rather than flashing a success state for something that did not happen.
      result = 'failed'
    }
    setCopyState(result)
    copyOutcomeCache.set(props.block.text, { result, at: Date.now() })
    scheduleRevert(COPY_CONFIRMATION_MS)
  }

  return (
    <div class="verevon-chat-codeblock">
      <div class="verevon-chat-codeblock__header">
        <Show when={props.block.lang}>
          <span class="verevon-chat-codeblock__label">{props.block.lang}</span>
        </Show>
        <button
          type="button"
          class={{
            'verevon-chat-codeblock__copy': true,
            'verevon-chat-codeblock__copy--failed': copyState() === 'failed',
          }}
          onClick={() => void copyCode()}
        >
          <Switch fallback={<Copy size={14} />}>
            <Match when={copyState() === 'copied'}><Check size={14} /></Match>
            <Match when={copyState() === 'failed'}><AlertCircle size={14} /></Match>
          </Switch>
          <Switch fallback={i18n.tr('Kopier kode', 'Copy code')}>
            <Match when={copyState() === 'copied'}>{i18n.tr('Kopiert', 'Copied')}</Match>
            <Match when={copyState() === 'failed'}>{i18n.tr('Kunne ikke kopiere', 'Could not copy')}</Match>
          </Switch>
        </button>
      </div>
      <Show
        when={highlighted()}
        fallback={<pre><code>{props.block.text}</code></pre>}
      >
        {(lines) => (
          <pre>
            <code>
              <For each={lines()}>
                {(line, index) => (
                  <>
                    {index() > 0 ? '\n' : ''}
                    <For each={line.tokens}>
                      {(token) => <span class={`verevon-code-${token.type}`}>{token.text}</span>}
                    </For>
                  </>
                )}
              </For>
            </code>
          </pre>
        )}
      </Show>
    </div>
  )
}

type MermaidRenderState =
  | { status: 'loading' }
  | { status: 'ok'; svg: string }
  | { status: 'error' }

// One shared module instance: mermaid.initialize() is documented as a
// single global call, so the dynamic import + init is done once no matter
// how many diagrams a conversation renders, and never before a mermaid
// block actually needs it (kept out of the main bundle otherwise).
let mermaidModulePromise: Promise<typeof import('mermaid')> | null = null

function loadMermaid(): Promise<typeof import('mermaid')> {
  if (!mermaidModulePromise) {
    mermaidModulePromise = import('mermaid').then((module) => {
      module.default.initialize({
        startOnLoad: false,
        securityLevel: 'strict',
        theme: 'neutral',
        fontFamily: 'inherit',
      })
      return module
    })
  }
  return mermaidModulePromise
}

let mermaidRenderSequence = 0

// Keyed on the diagram source itself (not the block object, which is a fresh
// reference every reparse — see `ChatMarkdown`) so a diagram that already
// finished rendering earlier in the stream comes back instantly from cache
// on every later re-render instead of re-running `mermaid.render` — and
// re-flashing its loading state — purely because later tokens elsewhere in
// the message produced a new block array.
const mermaidRenderCache = new Map<string, MermaidRenderState>()

async function renderMermaidDiagram(code: string): Promise<string> {
  const mermaid = await loadMermaid()
  mermaidRenderSequence += 1
  const { svg } = await mermaid.default.render(`verevon-mermaid-${mermaidRenderSequence}`, code)
  return svg
}

function MermaidDiagram(props: { code: string }) {
  const [state, setState] = createSignal<MermaidRenderState>(mermaidRenderCache.get(props.code) ?? { status: 'loading' })

  // Solid 2's `createEffect` splits into a tracked `compute` (here just
  // reading `props.code`) and an imperative `effect` phase that runs the
  // actual async render and returns its own cleanup — the single-callback
  // Solid 1 form throws `MISSING_EFFECT_FN` and halts the whole reactive
  // root, not just this component.
  createEffect(
    () => props.code,
    (code) => {
      const cached = mermaidRenderCache.get(code)
      if (cached) {
        setState(cached)
        return
      }
      let cancelled = false
      renderMermaidDiagram(code)
        .then((svg) => {
          const next: MermaidRenderState = { status: 'ok', svg }
          mermaidRenderCache.set(code, next)
          if (!cancelled) setState(next)
        })
        .catch(() => {
          // Invalid-but-complete diagram source (or the mermaid module failed
          // to load): fall back to the raw fenced text rather than leaving a
          // stuck spinner or throwing out of the render tree.
          const next: MermaidRenderState = { status: 'error' }
          mermaidRenderCache.set(code, next)
          if (!cancelled) setState(next)
        })
      return () => { cancelled = true }
    },
  )

  return (
    <Switch>
      <Match when={state().status === 'ok'}>
        <div
          class="verevon-chat-mermaid"
          // eslint-disable-next-line solid/no-innerhtml -- mermaid's own SVG output, rendered with securityLevel 'strict', which sanitises the diagram source.
          innerHTML={(state() as Extract<MermaidRenderState, { status: 'ok' }>).svg}
        />
      </Match>
      <Match when={state().status === 'error'}>
        <div class="verevon-chat-codeblock">
          <div class="verevon-chat-codeblock__label">mermaid</div>
          <pre><code>{props.code}</code></pre>
        </div>
      </Match>
      <Match when={state().status === 'loading'}>
        <div class="verevon-chat-codeblock verevon-chat-mermaid-loading">
          <div class="verevon-chat-codeblock__label">mermaid</div>
          <div class="verevon-chat-mermaid__placeholder" role="status" aria-live="polite">Tegner diagram…</div>
        </div>
      </Match>
    </Switch>
  )
}

/**
 * Block ("display") math, `$$...$$`. Only rendered through KaTeX once the
 * block's closing `$$` has actually arrived (`props.block.closed`) — the
 * same rule `MarkdownCodeBlock` applies to a streaming mermaid fence — and
 * only when KaTeX accepted the expression; both an unclosed, still-streaming
 * block and a malformed one fall back to the raw `$$...$$` source text
 * rather than showing nothing or throwing out of the render tree.
 */
function MarkdownMathBlock(props: { block: Extract<MarkdownBlock, { kind: 'math' }> }) {
  const html = () => (props.block.closed ? renderBlockMath(props.block.text) : null)
  return (
    <Show
      when={html()}
      fallback={
        <pre class="verevon-chat-math-raw">
          <code>{`$$\n${props.block.text}${props.block.closed ? '\n$$' : ''}`}</code>
        </pre>
      }
    >
      {(svg) => (
        <div
          class="verevon-chat-math verevon-chat-math--block"
          // eslint-disable-next-line solid/no-innerhtml -- KaTeX's own HTML output, rendered with its default trust: false, so the LaTeX source cannot embed markup, links or images.
          innerHTML={svg()}
        />
      )}
    </Show>
  )
}

/**
 * A table, with its hover toolbar (copy, CSV, "Vis som diagram") and, when the
 * reader switched it on, the chart under it.
 */
export function MarkdownTable(props: { block: Extract<MarkdownBlock, { kind: 'table' }>; citations?: readonly Citation[] }) {
  const chartState = createTableChartState(() => props.block.header, () => props.block.rows)
  return (
    <>
      <MarkdownTableGrid block={props.block} citations={props.citations} chartState={chartState} />
      <TableChart state={chartState} />
    </>
  )
}

function MarkdownTableGrid(props: {
  block: Extract<MarkdownBlock, { kind: 'table' }>
  citations?: readonly Citation[]
  chartState: ReturnType<typeof createTableChartState>
}) {
  const alignStyle = (column: number): JSX.CSSProperties | undefined => {
    const align = props.block.align[column]
    return align ? { 'text-align': align } : undefined
  }
  return (
    <div class="verevon-chat-table-wrap">
      <ChatTableTools header={props.block.header} rows={props.block.rows} state={props.chartState} />
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
  return (
    <>
      {renderMarkdownListLevel(
        props.block.items,
        0,
        props.block.items.length,
        props.block.ordered,
        props.citations,
        // Only the outermost <ol> honours the parsed start number — it is
        // the leading marker of the WHOLE message's list (see F-02), not of
        // whatever nested sublist happens to recurse through this function.
        props.block.startNumber,
      )}
    </>
  )
}

/**
 * Renders one nesting level of a flat, depth-annotated item list as a real
 * <ul>/<ol>, recursing for runs of deeper items so nested bullets indent the
 * way GFM renders them. Marker family per level follows the first item of
 * that level, so numbered children under bullets (and vice versa) work.
 * `startNumber` (when given and not 1) sets `<ol start>` so a leading number
 * like "391." is preserved instead of every ordered list silently
 * renumbering from 1 (F-02).
 */
function renderMarkdownListLevel(
  items: MarkdownListItem[],
  start: number,
  end: number,
  ordered: boolean,
  citations?: readonly Citation[],
  startNumber?: number,
): JSX.Element {
  const levelDepth = items[start]?.depth ?? 0
  const nodes: JSX.Element[] = []
  let index = start
  while (index < end) {
    const item = items[index]
    if (!item) break
    let childEnd = index + 1
    while (childEnd < end && (items[childEnd]?.depth ?? 0) > levelDepth) childEnd += 1
    const isTask = item.checked !== undefined
    nodes.push(
      // Task-list items suppress the marker glyph themselves — a checkbox
      // stands in its place, so a bullet drawn behind it would be a second,
      // meaningless marker.
      <li style={isTask ? { 'list-style': 'none' } : undefined}>
        {isTask ? <input type="checkbox" disabled checked={item.checked} /> : null}
        {parseInline(item.text, citations)}
        {childEnd > index + 1
          ? renderMarkdownListLevel(items, index + 1, childEnd, items[index + 1]?.ordered ?? false, citations)
          : null}
      </li>,
    )
    index = childEnd
  }
  return ordered
    ? <ol start={startNumber && startNumber !== 1 ? startNumber : undefined}>{nodes}</ol>
    : <ul>{nodes}</ul>
}

function MarkdownQuote(props: { block: Extract<MarkdownBlock, { kind: 'quote' }>; citations?: readonly Citation[] }) {
  return renderMarkdownQuoteLevel(props.block.lines, 0, props.block.lines.length, props.citations)
}

/**
 * Renders one nesting level of a flat, depth-annotated blockquote as a real
 * <blockquote>, recursing for runs of deeper lines so nested quotes (">"
 * then ">>", or "> >") indent the way GFM renders them — mirrors
 * `renderMarkdownListLevel`'s recursion over `MarkdownListItem.depth`.
 */
function renderMarkdownQuoteLevel(lines: MarkdownQuoteLine[], start: number, end: number, citations?: readonly Citation[]): JSX.Element {
  const levelDepth = lines[start]?.depth ?? 1
  const nodes: JSX.Element[] = []
  let paragraph: string[] = []
  const flushParagraph = () => {
    if (paragraph.length === 0) return
    // Quoted content can contain whole GFM blocks, including source tables.
    // Parsing it as one inline paragraph flattened every row after reload.
    nodes.push(...parseMarkdownBlocks(paragraph.join('\n')).map((block) => <MarkdownBlockView block={block} citations={citations} />))
    paragraph = []
  }
  let index = start
  while (index < end) {
    const line = lines[index]
    if (!line) break
    if (line.depth > levelDepth) {
      flushParagraph()
      let childEnd = index
      while (childEnd < end && (lines[childEnd]?.depth ?? 0) > levelDepth) childEnd += 1
      nodes.push(renderMarkdownQuoteLevel(lines, index, childEnd, citations))
      index = childEnd
      continue
    }
    paragraph.push(line.text)
    index += 1
  }
  flushParagraph()
  return <blockquote>{nodes}</blockquote>
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

/**
 * The Details popover's Routed line (W5-073-b): the mode, complexity and reason the gateway stated, each as its label and
 * joined by a middle dot. The gateway states closed tokens; a token this build does not know shows nothing, and a decision
 * with no known token shows no line at all. The labels are looked up by exact key, never by prototype.
 */
function routedLabel(decision: ChatRoutingDecision | undefined, tr: (noText: string, enText: string) => string): string {
  if (!decision) return ''
  const modeLabels = new Map<string, string>([
    ['budget', tr('Budsjett', 'Budget')],
    ['balance', tr('Balanse', 'Balance')],
    ['genius', tr('Genius', 'Genius')],
  ])
  const complexityLabels = new Map<string, string>([
    ['simple', tr('Enkel', 'Simple')],
    ['moderate', tr('Moderat', 'Moderate')],
    ['complex', tr('Kompleks', 'Complex')],
  ])
  const reasonLabels = new Map<string, string>([
    ['mode_cell', tr('Valgt modus', 'Chosen mode')],
    ['budget_tier_shift', tr('Budsjett: én klasse billigere', 'Budget: one tier cheaper')],
    ['budget_cheap_fallback', tr('Budsjett brukt opp: billig modell', 'Budget used up: cheap model')],
    ['posture_unknown', tr('Budsjettstatus ukjent', 'Budget status unknown')],
  ])
  return [
    modeLabels.get(decision.mode ?? ''),
    complexityLabels.get(decision.complexity ?? ''),
    reasonLabels.get(decision.reason ?? ''),
  ]
    .filter((label): label is string => label !== undefined)
    .join(' · ')
}

/**
 * `VITE_REASONING_TRANSLATE=true` offers Translate on a turn's reasoning in the Details popover (F-t3-098, W5-098-c).
 * Unset, or any value but `true` (the default), no reasoning is offered for translation. Exact string, as the other chat switches.
 */
export const reasoningTranslateEnabled = () => import.meta.env.VITE_REASONING_TRANSLATE === 'true'

/**
 * Function words common in Norwegian Bokmål and rare in English. The words with æ, ø or å are left out on purpose: a
 * string literal holding those letters must sit inside `i18n.tr`, and these are not copy.
 */
const NORWEGIAN_FUNCTION_WORDS: ReadonlySet<string> = new Set([
  'og', 'er', 'det', 'som', 'ikke', 'jeg', 'vi', 'har', 'kan', 'skal', 'men', 'ved', 'eller', 'dette', 'disse',
  'hvis', 'meg', 'deg', 'seg', 'til', 'fra', 'med', 'av', 'om', 'en', 'et', 'den', 'de', 'var',
  'hva', 'ble', 'blir', 'hvordan',
])

/** Function words common in English and rare in Norwegian Bokmål. */
const ENGLISH_FUNCTION_WORDS: ReadonlySet<string> = new Set([
  'the', 'and', 'is', 'are', 'was', 'were', 'be', 'of', 'that', 'this', 'these', 'with', 'it', 'not', 'we', 'you',
  'they', 'their', 'there', 'will', 'should', 'can', 'have', 'has', 'do', 'does', 'what', 'which', 'when', 'how',
  'from', 'or', 'an', 'a', 'if', 'so', 'on', 'in',
])

/**
 * The language a reasoning text is most likely in, `no` or `en`, from its function words; `undefined` when none of them
 * tells the two apart. A cheap guess that only decides whether a translation is offered, so a text it cannot tell is not
 * offered one.
 */
export function likelyReasoningLanguage(text: string): 'no' | 'en' | undefined {
  let norwegian = 0
  let english = 0
  for (const word of text.toLowerCase().match(/\p{L}+/gu) ?? []) {
    if (NORWEGIAN_FUNCTION_WORDS.has(word)) norwegian += 1
    else if (ENGLISH_FUNCTION_WORDS.has(word)) english += 1
  }
  if (norwegian === english) return undefined
  return norwegian > english ? 'no' : 'en'
}

/** A translation the popover holds: the reasoning it answers, the locale it reads in, and the text. */
type ReasoningTranslation = { source: string; target: ReasoningTranslateLocale; text: string }

export function ReasoningPopover(props: { message: ChatTurn; temporaryChat?: boolean }) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [tab, setTab] = createSignal('general')
  let ref!: HTMLDivElement
  // F-t3-098 (W5-098-c): the reader's translation of the reasoning. It lives in these signals only, never in the turn, a
  // history cache, the composer or a request context, and it is dropped with the popover.
  const [translation, setTranslation] = createSignal<ReasoningTranslation | null>(null)
  const [showOriginal, setShowOriginal] = createSignal(false)
  const [translating, setTranslating] = createSignal(false)
  const [translateFailure, setTranslateFailure] = createSignal<'failed' | 'unavailable' | null>(null)
  // Offered on a settled turn that keeps its content, when the text fits the route and reads in the other language from the reader's.
  const translatable = createMemo(() => {
    if (!reasoningTranslateEnabled() || props.temporaryChat || props.message.streaming) return false
    const text = props.message.reasoning ?? ''
    if (text.trim() === '' || reasoningTranslateChars(text) > REASONING_TRANSLATE_MAX_CHARS) return false
    const language = likelyReasoningLanguage(text)
    return language !== undefined && language !== i18n.locale()
  })
  // The held translation, while it still answers the reasoning on screen in the reader's current locale.
  const heldTranslation = createMemo(() => {
    const held = translation()
    if (held === null || held.source !== props.message.reasoning) return null
    return held.target === reasoningTranslateLocaleOf(i18n.locale()) ? held : null
  })
  const showingTranslation = () => heldTranslation() !== null && !showOriginal()
  /** Asks the route for the reasoning's translation. A result for text the turn no longer shows is dropped. */
  const translateReasoning = async () => {
    const source = props.message.reasoning ?? ''
    if (translating() || source.trim() === '') return
    const target = reasoningTranslateLocaleOf(i18n.locale())
    setTranslating(true)
    setTranslateFailure(null)
    try {
      const text = await translateText(source, target)
      if (text === null) {
        setTranslateFailure('unavailable')
      } else if (source === props.message.reasoning) {
        setTranslation({ source, target, text })
        setShowOriginal(false)
      }
    } catch {
      setTranslateFailure('failed')
    } finally {
      setTranslating(false)
    }
  }
  const model = createMemo(() => props.message.modelUsed ?? props.message.model)
  const routed = createMemo(() => routedLabel(props.message.routingDecision, (noText, enText) => i18n.tr(noText, enText)))
  const hasMetrics = createMemo(() => Boolean(
    model()
    || routed()
    || props.message.inputTokens != null
    || props.message.outputTokens != null
    || props.message.latencyMs != null
    || props.message.timing != null
    || props.message.confidence != null
    || props.message.costUsd != null
    || props.message.costBasis === 'subscription'
    || props.message.reasoning
    || props.message.grounding
    || (props.message.citations?.length ?? 0) > 0
    || (props.message.toolCalls?.length ?? 0) > 0,
  ))
  const tabs = createMemo(() => [
    { id: 'general', label: i18n.tr('Oversikt', 'Overview') },
    ...(props.message.reasoning ? [{ id: 'insight', label: i18n.tr('Innsikt', 'Insight') }] : []),
    ...((props.message.toolCalls?.length ?? 0) > 0 ? [{ id: 'tools', label: i18n.tr('Verktøy', 'Tools') }] : []),
    ...(props.message.grounding || (props.message.citations?.length ?? 0) > 0 ? [{ id: 'sources', label: i18n.tr('Kilder', 'Sources') }] : []),
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
        {/* The trigger used to read "Claude Sonnet · 5 tokens". UX spec finding
            7: raw model labels and per-turn tokens belong in details or Trace,
            not in the reading flow (audit item 20). The panel below already
            lists Modell, Input, Output, tid, Sikkerhet and Kostnad. */}
        <button type="button" aria-expanded={open() ? 'true' : 'false'} onClick={() => setOpen((value) => !value)}>
          <Sparkles size={12} />
          <span>{i18n.tr('Detaljer', 'Details')}</span>
        </button>
        <Show when={open()}>
          <div class="verevon-chat-reasoning-popover__panel">
            <div class="verevon-chat-reasoning-popover__head">
              <strong>Reasoning</strong>
              <button type="button" aria-label={i18n.tr('Lukk', 'Close')} onClick={() => setOpen(false)}>
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
                <p
                  class="verevon-chat-reasoning-popover__copy"
                  lang={showingTranslation() ? reasoningTranslateLocaleOf(i18n.locale()) : undefined}
                >
                  {showingTranslation() ? heldTranslation()?.text : props.message.reasoning}
                </p>
                <Show when={translatable() || heldTranslation() !== null || translateFailure() !== null}>
                  <div class="verevon-chat-reasoning-translate">
                    <Show when={heldTranslation() !== null}>
                      <button type="button" onClick={() => setShowOriginal((value) => !value)}>
                        {showOriginal() ? i18n.tr('Vis oversettelse', 'Show translation') : i18n.tr('Vis original', 'Show original')}
                      </button>
                      <Show when={showingTranslation()}>
                        <span class="verevon-chat-reasoning-translate__note">
                          {i18n.tr('Maskinoversettelse. Lagres ikke.', 'Machine translation. Not saved.')}
                        </span>
                      </Show>
                    </Show>
                    <Show when={heldTranslation() === null && translatable() && translateFailure() !== 'unavailable'}>
                      <button type="button" disabled={translating()} onClick={() => void translateReasoning()}>
                        {translating() ? i18n.tr('Oversetter…', 'Translating…') : i18n.tr('Oversett', 'Translate')}
                      </button>
                    </Show>
                    <Show when={translateFailure() === 'unavailable'}>
                      <span class="verevon-chat-reasoning-translate__note">
                        {i18n.tr('Oversettelse er ikke tilgjengelig akkurat nå.', 'Translation is not available right now.')}
                      </span>
                    </Show>
                    <Show when={translateFailure() === 'failed'}>
                      <span class="verevon-chat-reasoning-translate__note" role="alert">
                        {i18n.tr('Kunne ikke oversette. Prøv igjen.', 'Could not translate. Try again.')}
                      </span>
                    </Show>
                  </div>
                </Show>
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
                    {(citation) => isSafeLinkHref(citation.url)
                      ? <a href={citation.url} target="_blank" rel="noopener noreferrer">{citation.title || citation.url}</a>
                      : <span class="verevon-chat-link-blocked" title={i18n.tr('Lenke blokkert: ukjent adresse', 'Link blocked: unsupported address')}>{citation.title || citation.url}</span>}
                  </For>
                </div>
              </Match>
              <Match when={true}>
                <dl>
                  <Show when={model()}><MetricRow label="Modell" value={prettyModel(model() ?? '')} /></Show>
                  {/* The route beside the model it chose (W5-073-b): absent unless the gateway stated a known token. */}
                  <Show when={routed()}><MetricRow label={i18n.tr('Ruting', 'Routed')} value={routed()} /></Show>
                  <Show when={props.message.inputTokens != null}><MetricRow label="Input" value={`${props.message.inputTokens} tokens`} /></Show>
                  <Show when={props.message.outputTokens != null}><MetricRow label="Output" value={`${props.message.outputTokens} tokens`} /></Show>
                  <Show when={props.message.latencyMs != null}><MetricRow label="Total tid" value={formatLatency(props.message.latencyMs ?? 0)} /></Show>
                  {/* Where the server time went, from the gateway's own
                      `turn_timing`: provider calls vs. everything else. Shown
                      only when the gateway reported it, never estimated. */}
                  <Show when={props.message.timing}>
                    {(timing) => (
                      <>
                        <MetricRow label={i18n.tr('Modelltid', 'Model time')} value={formatLatency(timing().providerMs)} />
                        <MetricRow label={i18n.tr('Systemtid', 'System time')} value={formatLatency(timing().orchestrationMs)} />
                      </>
                    )}
                  </Show>
                  <Show when={props.message.confidence != null}><MetricRow label={i18n.tr('Sikkerhet', 'Confidence')} value={`${Math.round((props.message.confidence ?? 0) * 100)}%`} /></Show>
                  {/* A subscription-carried turn has no list-price cost; say so
                      rather than print the wire's placeholder zero. */}
                  <Show
                    when={props.message.costBasis === 'subscription'}
                    fallback={<Show when={props.message.costUsd != null}><MetricRow label="Kostnad" value={`$${(props.message.costUsd ?? 0).toFixed(4)}`} /></Show>}
                  >
                    <MetricRow label="Kostnad" value={i18n.tr('Inkludert i abonnementet', 'Included in the subscription')} />
                  </Show>
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
 * segment instead of ever showing "$0" or "$NaN" for it — which is also how a
 * subscription-carried turn reads here, since its placeholder cost is dropped
 * at ingestion (the details popover says it is included in the subscription).
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
  const i18n = useI18n()
  const total = () => props.calls.length
  const failed = () => props.calls.filter((call) => Boolean(call.error) || call.status === 'error').length
  const running = () => props.calls.some((call) => !call.status || call.status === 'running')
  const label = () => {
    if (running()) return i18n.tr(`Bruker verktøy … (${total()})`, `Using tools … (${total()})`)
    // Norwegian has one form for both counts, which is why this used to read
    // `'steg' : 'steg'`; English needs the plural.
    const unit = i18n.tr('steg', total() === 1 ? 'step' : 'steps')
    return `${total()} ${unit}`
  }

  return (
    <button type="button" class="verevon-chat-steps-pill" onClick={props.onViewSteps}>
      <Wrench size={13} />
      <span>{label()}</span>
      <Show when={failed() > 0}>
        <em class="verevon-chat-steps-pill__failed">{i18n.tr(`${failed()} feilet`, `${failed()} failed`)}</em>
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
  const i18n = useI18n()
  return (
    <div class="verevon-chat-followups" role="group" aria-label={i18n.tr('Forslag til oppfølgingsspørsmål', 'Suggested follow-up questions')}>
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

/**
 * Whether the card offers a standing scope for this approval (build plan WS13 S6). The server's
 * `scope_options` is the offer: it lists a standing scope only where the cause allows one and the
 * deployment has switched standing scopes on, so the card never offers what the decide route refuses.
 */
function offersStandingScope(approval: Approval, scope: Exclude<ApprovalScope, 'once'>): boolean {
  return approval.cause?.scopeOptions.includes(scope) ?? false
}

/** What a member may choose for a site the agent asks to open (WS21-S6). */
export type BrowserSiteConsentChoice = 'allow_this_run' | 'remember_read' | 'remember_act' | 'deny'

/**
 * The choices a browser-site consent card offers for one host. The card never offers more than the
 * admin's ceiling for that host, and it never appears under zero data retention, where the agent
 * browser is refused before any consent is asked.
 */
export function browserSiteConsentChoices(
  maxLevel: 'read' | 'act',
  zdr: boolean,
): BrowserSiteConsentChoice[] {
  if (zdr) return []
  return maxLevel === 'act'
    ? ['allow_this_run', 'remember_read', 'remember_act', 'deny']
    : ['allow_this_run', 'remember_read', 'deny']
}

export function browserSiteConsentLabel(
  choice: BrowserSiteConsentChoice,
  i18n: { tr: (noText: string, enText: string) => string },
): string {
  switch (choice) {
    case 'allow_this_run':
      return i18n.tr('Tillat for denne kjøringen', 'Allow for this run')
    case 'remember_read':
      return i18n.tr('Husk: bare lesing', 'Remember: reading only')
    case 'remember_act':
      return i18n.tr('Husk: lesing og handling', 'Remember: reading and acting')
    case 'deny':
      return i18n.tr('Ikke tillat', 'Do not allow')
  }
}

/**
 * WS21-S6: asks the member whether the agent may open one site. The host and the ceiling come from
 * the approval's cause, the record the server keeps, never from text on the page.
 */
export function BrowserSiteConsentCard(props: {
  host: string
  maxLevel: 'read' | 'act'
  zdr: boolean
  onDecide: (choice: BrowserSiteConsentChoice) => void
}) {
  const i18n = useI18n()
  const choices = () => browserSiteConsentChoices(props.maxLevel, props.zdr)
  return (
    <Show when={choices().length > 0}>
      <div
        class="verevon-chat-approval__consent"
        role="group"
        aria-label={i18n.tr('Nettside for agenten', 'Site for the agent')}
      >
        <p class="verevon-chat-approval__detail">
          {i18n.tr(
            `Agenten vil åpne ${props.host}. Skal den få lov?`,
            `The agent wants to open ${props.host}. Allow it?`,
          )}
        </p>
        <div class="verevon-chat-approval__actions">
          <For each={choices()}>
            {(choice) => (
              <button
                type="button"
                class={
                  choice === 'deny'
                    ? 'verevon-chat-approval__reject'
                    : 'verevon-chat-approval__approve'
                }
                onClick={() => props.onDecide(choice)}
              >
                {browserSiteConsentLabel(choice, i18n)}
              </button>
            )}
          </For>
        </div>
      </div>
    </Show>
  )
}

/**
 * EP-1 S2: the agent asks the person to clear a wall on the page (a sign-in, a human check, a
 * consent choice, a firewall). The words and the host come from the approval's cause, the record
 * the server keeps, never from text on the page. Take over flips the browser to the person through
 * the same control call the live panel uses, compare-and-set on the epoch the projection shows; it is
 * offered only when the turn's browser is bound and the owner may take it (`onTakeOver` present).
 * Skip this site declines the request: the run closes the page and looks elsewhere.
 */
export function BrowserHumanAssistCard(props: {
  approval: Approval
  control?: ChatRunBrowserControl
  onTakeOver?: (sessionId: string, expectedEpoch: number | null) => void
  onSkip: () => void
}) {
  const i18n = useI18n()
  const control = () => (props.onTakeOver ? props.control : undefined)
  return (
    <div class="verevon-chat-approval" role="group" aria-label={approvalHeadline(props.approval, i18n)}>
      <div class="verevon-chat-approval__head">
        <span class="verevon-chat-approval__badge">{i18n.tr('Hjelp', 'Help')}</span>
        <span class="verevon-chat-approval__kind">{approvalHeadline(props.approval, i18n)}</span>
      </div>
      <Show when={approvalReason(props.approval, i18n)}>
        {(reason) => <p class="verevon-chat-approval__detail">{reason()}</p>}
      </Show>
      <Show when={!control()}>
        <p class="verevon-chat-approval__detail">{browserAssistNoTakeOverLine(i18n)}</p>
      </Show>
      <div class="verevon-chat-approval__actions">
        <Show when={control()}>
          {(bound) => (
            <button
              type="button"
              class="verevon-chat-approval__approve"
              onClick={() => props.onTakeOver?.(bound().sessionId, bound().controlEpoch)}
            >
              {browserAssistActionLabel('take_over', i18n)}
            </button>
          )}
        </Show>
        <button type="button" class="verevon-chat-approval__reject" onClick={() => props.onSkip()}>
          {browserAssistActionLabel('skip', i18n)}
        </button>
      </div>
    </div>
  )
}

export function ApprovalRequests(props: {
  approvals: Approval[]
  onDecide: (approvalId: string, decision: ApprovalDecision, scope?: ApprovalScope) => void
  /** The turn's browser, as the person may address it (EP-1 S1); a help card offers Take over with it. */
  browserControl?: ChatRunBrowserControl
  /** Takes the turn's browser for a help request (EP-1 S2). Absent while takeover is not offered. */
  onBrowserTakeOver?: (sessionId: string, expectedEpoch: number | null) => void
  /**
   * Sends an approval with the person's edits to a drafted mail send (EP-10-S6), for this one call.
   * It rejects with the typed `ApiError`, so a 409 `approval_changed` keeps the card and the
   * person's text. Absent: the card offers no edit mode.
   */
  onDecideEdited?: (approvalId: string, edit: ApprovalEdit) => Promise<void>
}) {
  const i18n = useI18n()
  // The person's pending edits by approval id (EP-10-S6). Present only while the form differs from
  // the draft; an edited approval is ONCE only (decision D1), so a standing scope hides meanwhile.
  const [drafts, setDrafts] = createSignal<Record<string, MailEditDraft>>({})
  // Bumped on a 409 approval_changed: the disclosure refetches the draft and keeps the person's text.
  const [conflicts, setConflicts] = createSignal<Record<string, number>>({})
  const [editErrors, setEditErrors] = createSignal<Record<string, string>>({})
  const [editBusy, setEditBusy] = createSignal<string | null>(null)

  const noteDraft = (approvalId: string, draft: MailEditDraft | null) => {
    setDrafts((current) => {
      const next = { ...current }
      if (draft) next[approvalId] = draft
      else delete next[approvalId]
      return next
    })
  }

  const noteEditError = (approvalId: string, message: string | null) => {
    setEditErrors((current) => {
      const next = { ...current }
      if (message) next[approvalId] = message
      else delete next[approvalId]
      return next
    })
  }

  const sendEdited = async (approvalId: string, edit: ApprovalEdit) => {
    const send = props.onDecideEdited
    if (!send || editBusy() !== null) return
    setEditBusy(approvalId)
    noteEditError(approvalId, null)
    try {
      await send(approvalId, edit)
      noteDraft(approvalId, null)
    } catch (error) {
      const refusal = approvalEditRefusalOf(error)
      if (refusal === 'approval_changed') {
        setConflicts((current) => ({ ...current, [approvalId]: (current[approvalId] ?? 0) + 1 }))
      }
      noteEditError(
        approvalId,
        refusal
          ? mailEditProblemText(refusal, i18n.tr)
          : translateApiError(error, i18n.tr, {
              no: 'Godkjenningen med endringene gikk ikke gjennom.',
              en: 'The approval with your edits did not go through.',
            }),
      )
    } finally {
      setEditBusy(null)
    }
  }

  // An approval whose cause asks for a fresh password confirmation before it is granted (build
  // plan WS13 S5). Only the grant asks; a denial never does.
  const [confirming, setConfirming] = createSignal<string | null>(null)
  // The standing scope a confirmed grant carries. A standing scope is only ever chosen by its own
  // control, so the one decision stays what a plain approval means.
  const [confirmingScope, setConfirmingScope] = createSignal<ApprovalScope | undefined>(undefined)
  const [password, setPassword] = createSignal('')
  const [confirmError, setConfirmError] = createSignal<string | null>(null)
  const [confirmBusy, setConfirmBusy] = createSignal(false)

  const cancelConfirmation = () => {
    setConfirming(null)
    setConfirmingScope(undefined)
    setPassword('')
    setConfirmError(null)
  }

  const decide = (approvalId: string, decision: ApprovalDecision, scope?: ApprovalScope) => {
    if (scope) {
      props.onDecide(approvalId, decision, scope)
    } else {
      props.onDecide(approvalId, decision)
    }
  }

  const approve = (approval: Approval, scope?: ApprovalScope) => {
    const draft = scope === undefined ? drafts()[approval.id] : undefined
    if (draft?.problem) return
    if (approval.cause?.stepUpRequired) {
      setConfirmError(null)
      setConfirmingScope(scope)
      setConfirming(approval.id)
      return
    }
    if (draft) {
      void sendEdited(approval.id, draft.edit)
      return
    }
    decide(approval.id, 'approve', scope)
  }

  const confirmationFailure = (error: unknown): string => {
    const code = error instanceof ApiError ? error.code : null
    if (code === 'INVALID_PASSWORD') {
      return i18n.tr('Passordet stemmer ikke.', 'That password is not correct.')
    }
    if (code === 'RATE_LIMITED') {
      return i18n.tr('For mange forsøk. Prøv igjen senere.', 'Too many attempts. Try again later.')
    }
    if (code === 'STEP_UP_METHOD_UNAVAILABLE') {
      return i18n.tr('Kontoen har ikke noe passord å bekrefte med.', 'This account has no password to confirm with.')
    }
    return i18n.tr('Bekreftelsen lyktes ikke. Prøv igjen.', 'The confirmation failed. Try again.')
  }

  const confirmAndApprove = async (approval: Approval) => {
    const entered = password()
    if (entered.length === 0 || confirmBusy()) return
    setConfirmBusy(true)
    setConfirmError(null)
    try {
      await confirmStepUp(entered)
    } catch (error) {
      setConfirmError(confirmationFailure(error))
      return
    } finally {
      setConfirmBusy(false)
      setPassword('')
    }
    const scope = confirmingScope()
    cancelConfirmation()
    // The step-up runs first for an edited grant exactly as for a plain one (WS13-S5).
    const draft = scope === undefined ? drafts()[approval.id] : undefined
    if (draft && !draft.problem) {
      void sendEdited(approval.id, draft.edit)
      return
    }
    decide(approval.id, 'approve', scope)
  }

  // E3.2-b swap: the shared keyboard-first card (W3-B1-06), inline in the
  // transcript (D9), behind VITE_BROWSER_APPROVAL_CARD. Only an approval the
  // card can express takes it (no password step-up, no standing scope); the
  // copy is the cause wording this deck already shows.
  const cardModel = (approval: Approval): ApprovalCardModel | null => {
    if (!browserApprovalCardEnabled() || !runApprovalFitsCard(approval)) return null
    const base = runApprovalToCardModel(approval)
    if (!base) return null
    return {
      ...base,
      reason: approvalReason(approval, i18n) ?? '',
      actionType: approvalHeadline(approval, i18n),
    }
  }

  return (
    <div class="verevon-chat-approvals" role="group" aria-label={i18n.tr('Godkjenninger', 'Approvals')}>
      <For each={props.approvals}>
        {(approval) => (
          <Show
            when={!browserAssistKindOf(approval.cause)}
            fallback={
              <BrowserHumanAssistCard
                approval={approval}
                control={props.browserControl}
                onTakeOver={props.onBrowserTakeOver}
                onSkip={() => props.onDecide(approval.id, 'reject')}
              />
            }
          >
          <Show
            when={cardModel(approval)}
            fallback={
          <div class="verevon-chat-approval">
            <div class="verevon-chat-approval__head">
              <span class="verevon-chat-approval__badge">{i18n.tr('Godkjenning', 'Approval')}</span>
              <span class="verevon-chat-approval__kind">{approvalHeadline(approval, i18n)}</span>
            </div>
            <Show when={approvalReason(approval, i18n)}>
              {(reason) => <p class="verevon-chat-approval__detail">{reason()}</p>}
            </Show>
            <Show when={approvalWritePreviewEnabled() && isMailSendApproval(approval)}>
              <WritePreviewDisclosure
                approvalId={approval.id}
                editable={props.onDecideEdited !== undefined}
                onDraftChange={(draft) => noteDraft(approval.id, draft)}
                conflict={conflicts()[approval.id] ?? 0}
              />
            </Show>
            <Show when={editErrors()[approval.id]}>
              {(message) => (
                <p class="verevon-chat-approval__error" role="alert">
                  {message()}
                </p>
              )}
            </Show>
            <Show
              when={confirming() === approval.id}
              fallback={
                <div class="verevon-chat-approval__actions">
                  <button
                    type="button"
                    class="verevon-chat-approval__approve"
                    disabled={Boolean(drafts()[approval.id]?.problem) || editBusy() === approval.id}
                    onClick={() => approve(approval)}
                  >
                    {drafts()[approval.id]
                      ? i18n.tr('Send med mine endringer', 'Send with my edits')
                      : i18n.tr('Godkjenn', 'Approve')}
                  </button>
                  <Show when={!drafts()[approval.id] && offersStandingScope(approval, 'thread')}>
                    <button
                      type="button"
                      class="verevon-chat-approval__approve"
                      onClick={() => approve(approval, 'thread')}
                    >
                      {i18n.tr('Godkjenn for denne tråden', 'Approve for this thread')}
                    </button>
                  </Show>
                  <Show when={!drafts()[approval.id] && offersStandingScope(approval, 'run')}>
                    <button
                      type="button"
                      class="verevon-chat-approval__approve"
                      onClick={() => approve(approval, 'run')}
                    >
                      {i18n.tr('Godkjenn for denne kjøringen', 'Approve for this run')}
                    </button>
                  </Show>
                  <button
                    type="button"
                    class="verevon-chat-approval__reject"
                    onClick={() => props.onDecide(approval.id, 'reject')}
                  >
                    {i18n.tr('Avvis', 'Reject')}
                  </button>
                </div>
              }
            >
              <form
                class="verevon-chat-approval__confirm"
                onSubmit={(event) => {
                  event.preventDefault()
                  void confirmAndApprove(approval)
                }}
              >
                <label class="verevon-chat-approval__confirm-label">
                  {i18n.tr('Bekreft passordet ditt for å godkjenne', 'Confirm your password to approve')}
                  <input
                    type="password"
                    autocomplete="current-password"
                    required
                    disabled={confirmBusy()}
                    value={password()}
                    onInput={(event) => setPassword(event.currentTarget.value)}
                  />
                </label>
                <Show when={confirmError()}>
                  {(message) => (
                    <p class="verevon-chat-approval__error" role="alert">
                      {message()}
                    </p>
                  )}
                </Show>
                <div class="verevon-chat-approval__actions">
                  <button
                    type="submit"
                    class="verevon-chat-approval__approve"
                    disabled={confirmBusy() || password().length === 0}
                  >
                    {confirmBusy()
                      ? i18n.tr('Bekrefter…', 'Confirming…')
                      : drafts()[approval.id] && confirmingScope() === undefined
                        ? i18n.tr('Bekreft og send med mine endringer', 'Confirm and send with my edits')
                        : i18n.tr('Bekreft og godkjenn', 'Confirm and approve')}
                  </button>
                  <button
                    type="button"
                    class="verevon-chat-approval__reject"
                    onClick={cancelConfirmation}
                  >
                    {i18n.tr('Avbryt', 'Cancel')}
                  </button>
                </div>
              </form>
            </Show>
          </div>
            }
          >
            {(card) => (
              <BrowserApprovalDecisionCard
                approval={card()}
                allowRedirect={APPROVAL_REDIRECT_ENABLED}
                decide={(request) => {
                  // The parent owns the decide call and the transcript refresh.
                  decide(request.approvalId, request.decision)
                  return Promise.resolve()
                }}
              />
            )}
          </Show>
          </Show>
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
      <Match when={props.intent === 'image'}>
        <ImageIcon size={14} />
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
export function ToolCallCard(props: { call: ChatToolCall; onInspect?: () => void }) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const presentation = createMemo(() => toolPresentation(props.call))
  // A refused image failed whatever status the gateway gave it: the typed refusal is its outcome.
  const failed = () =>
    Boolean(props.call.error) || props.call.status === 'error' || presentation().imageRefusal !== undefined
  const running = () => !props.call.status || props.call.status === 'running'
  const statusLabel = () =>
    failed() ? i18n.tr('feilet', 'failed') : running() ? i18n.tr('kjører ...', 'running ...') : i18n.tr('fullført', 'completed')
  const args = () => formatToolArgs(props.call.args)
  // With the step strip on, the row reads as a sentence that already carries the count and any truncation,
  // so those marks are dropped below. Off, the raw name and both marks stay as they were.
  const label = () => (stepStripEnabled ? narrate(props.call, i18n) : props.call.name)
  // The image line under the header: who made the picture, or the typed refusal it came back with. Only an image
  // intent has one, and that intent exists only while its switch is on. With the step strip on, the row already says
  // a refusal in words, so the refusal is not said twice.
  const imageLine = () => {
    const current = presentation()
    if (current.intent !== 'image') return undefined
    if (current.imageRefusal !== undefined) {
      return stepStripEnabled ? undefined : { text: imageRefusalText(current.imageRefusal, i18n), refused: true }
    }
    if (current.imageModel === undefined) return undefined
    const made = current.imageProvider ? `${current.imageModel} (${current.imageProvider})` : current.imageModel
    const text = i18n.tr(`Laget med ${made}`, `Made with ${made}`)
    return { text: current.imageSize ? `${text} · ${current.imageSize}` : text, refused: false }
  }
  // The computer panel's viewer for this call's intent, when the panel is on. Off, or for an intent the registry
  // does not claim, the body stays the raw output it always was.
  const computerViewer = () => (chatComputerPanelEnabled() ? pickComputerViewer(presentation().intent) : undefined)

  return (
    <div class="verevon-chat-tool-call" data-intent={presentation().intent}>
      <button
        type="button"
        aria-expanded={open() ? 'true' : 'false'}
        onClick={() => setOpen((value) => !value)}
      >
        <ToolIntentIcon intent={presentation().intent} />
        <span>{label()}</span>
        {/* A search that returned nothing is a real outcome, so 0 must render —
            hence an explicit null check rather than a truthiness test. */}
        <Show when={presentation().count != null && !running() && !stepStripEnabled}>
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
        <Show when={presentation().sandbox}>
          {(sandbox) => (
            <em class="verevon-chat-tool-call__sandbox" data-sandbox={sandbox()}>
              {sandbox() === 'isolated_no_network'
                ? i18n.tr('Isolert, uten nett', 'Isolated, no network')
                : i18n.tr('ukjent', 'unknown')}
            </em>
          )}
        </Show>
        <Show when={presentation().truncated && !stepStripEnabled}>
          {/* Not the same as "no results": there IS more and the model did not
              see it, which matters for any answer built on this. */}
          <em
            class="verevon-chat-tool-call__truncated"
            title={i18n.tr('Resultatet ble avkortet', 'The result was truncated')}
          >
            avkortet
          </em>
        </Show>
        <em class={{ 'is-error': failed(), 'is-running': running() }}>
          {statusLabel()}
        </em>
        <ChevronRight size={14} class={{ 'verevon-chat-rotate': open() }} />
      </button>
      <Show when={imageLine()}>
        {(line) => (
          <span class={line().refused ? 'verevon-chat-tool-call__made-with is-refused' : 'verevon-chat-tool-call__made-with'}>
            {line().text}
          </span>
        )}
      </Show>
      <Show when={props.onInspect}>
        <button type="button" class="verevon-chat-tool-call__inspect" onClick={() => props.onInspect?.()}>
          {i18n.tr('Åpne i Trace', 'Open in Trace')}
        </button>
      </Show>
      <Show when={open() && (args() || props.call.output || props.call.error)}>
        <div>
          <Show when={args()}>
            <pre>{args()}</pre>
          </Show>
          <Show
            when={presentation().intent === 'diff' && props.call.output}
            fallback={
              <Show
                when={computerViewer()}
                fallback={
                  <Show when={props.call.output}>
                    <pre>{props.call.output}</pre>
                  </Show>
                }
              >
                <ComputerViewer call={props.call} intent={presentation().intent} />
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
 * `VITE_CHAT_QUEUE_RETRACT=true` lets the person take back a mid-run message while the gateway
 * still holds it. Off, the strip is the read-only tray it has always been. Exact string, as for
 * `VITE_CHAT_RENDER_LEGACY`.
 */
const chatQueueRetract = () => import.meta.env.VITE_CHAT_QUEUE_RETRACT === 'true'

/**
 * Only a message the gateway accepted and has not delivered can be taken back, and the gateway
 * answers a take-back by the client message id that send was made under.
 */
const canTakeBack = (entry: QueuedInput) =>
  entry.state === 'pending' && entry.accepted === true && Boolean(entry.clientMessageId)

/**
 * Edit and Remove for one queued message. Each control appears only with its handler: the
 * parent decides what a take-back does, and nothing here claims one the gateway has not answered.
 */
function QueuedInputControls(props: {
  entry: QueuedInput
  onEdit?: (entry: QueuedInput, text: string) => void
  onRetract?: (entry: QueuedInput) => void
}) {
  const i18n = useI18n()
  const [editing, setEditing] = createSignal(false)
  const [draft, setDraft] = createSignal('')

  const startEditing = () => {
    setDraft(props.entry.content)
    setEditing(true)
  }

  const submitEdit = () => {
    const next = draft().trim()
    if (!next || !props.onEdit) return
    setEditing(false)
    props.onEdit(props.entry, next)
  }

  return (
    <Show
      when={!editing()}
      fallback={(
        <div class="verevon-chat-edit-box">
          <textarea
            aria-label={i18n.tr('Rediger melding', 'Edit message')}
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
              if (event.key === 'Escape') setEditing(false)
            }}
          />
          <div>
            <button type="button" onClick={() => setEditing(false)}>{i18n.tr('Avbryt', 'Cancel')}</button>
            <button type="button" disabled={!draft().trim()} onClick={submitEdit}>{i18n.tr('Send på nytt', 'Send again')}</button>
          </div>
        </div>
      )}
    >
      <div class="verevon-chat-message-actions">
        <Show when={props.onEdit}>
          <MessageAction label={i18n.tr('Rediger', 'Edit')} onClick={startEditing}>
            <Pencil size={14} />
          </MessageAction>
        </Show>
        <Show when={props.onRetract}>
          <MessageAction label={i18n.tr('Fjern', 'Remove')} onClick={() => props.onRetract?.(props.entry)}>
            <X size={14} />
          </MessageAction>
        </Show>
      </div>
    </Show>
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
export function QueuedInputStrip(props: {
  entries: QueuedInput[]
  onEdit?: (entry: QueuedInput, text: string) => void
  onRetract?: (entry: QueuedInput) => void
}) {
  const i18n = useI18n()
  const label = (entry: QueuedInput) => {
    if (entry.state === 'delivered') return 'levert til agenten'
    if (entry.state === 'refused') return 'ikke levert'
    // The run ended without taking it, so it went out again as an ordinary turn (same id: the
    // transcript holds it once).
    if (entry.state === 'resent') return i18n.tr('sendt på nytt som ny melding', 'resent as a new message')
    return 'venter'
  }
  return (
    <Show when={props.entries.length > 0}>
      <ul class="verevon-chat-queued" aria-label={i18n.tr('Meldinger sendt underveis', 'Messages sent mid-run')}>
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
              <Show when={chatQueueRetract() && canTakeBack(entry) && (props.onEdit || props.onRetract)}>
                <QueuedInputControls entry={entry} onEdit={props.onEdit} onRetract={props.onRetract} />
              </Show>
            </li>
          )}
        </For>
      </ul>
    </Show>
  )
}

/**
 * One row of the editable step list, mid-edit (plan-gate §2.5).
 *
 * `localKey` (never the server `stepId`, which is empty for a brand-new row
 * and would collide across several new rows) is this row's STABLE identity
 * for `<For>` and for the uncontrolled title input below — reordering swaps
 * row OBJECTS in the array (same references, new positions), so `<For>`'s
 * reference-based reconciliation moves existing DOM/input nodes instead of
 * recreating them. `title` is deliberately NOT reactive state: it seeds an
 * uncontrolled `<input>` once, and the input's live DOM value is read back at
 * submit time via `inputRefs`. Reactive per-keystroke state here would force
 * `<input value={...}>` to reassert its DOM value on every render this
 * component makes for an unrelated reason (reordering a DIFFERENT row,
 * ticking `pending`, …), which is exactly what causes an uncontrolled input
 * elsewhere in this codebase to lose the caret/selection mid-type.
 */
type EditablePlanStepRow = {
  localKey: string
  /** Empty means this row is NOT yet an existing step (a new addition). */
  stepId: string
  title: string
  operation: string
  removed: boolean
}

let planStepRowKeySeq = 0
function nextPlanStepRowKey(): string {
  planStepRowKeySeq += 1
  return `plan-step-row-${planStepRowKeySeq}`
}

/**
 * Unsent edits to a proposed plan, kept across a REMOUNT of the control.
 *
 * The control does not own its lifetime. Its parent (`AssistantMessage`)
 * derives the turn's nodes as a fresh list whenever anything they read
 * changes, and `<For>` keys that list by object reference — so every node,
 * this one included, is torn down and rebuilt. Clicking Approve flips
 * `pending`, and a refused approval sets `error`: both rebuild the control,
 * which used to throw away the person's reordering, rewritten steps and
 * typed justification exactly when they most needed them (fix the problem
 * the server named, press Approve again). Keyed by plan id, so a different
 * (revised) plan never inherits them; bounded, so a long session cannot
 * accumulate them.
 */
type PlanDraft = { rung: AutonomyRung; reason: string; rows: EditablePlanStepRow[] }
const MAX_PLAN_DRAFTS = 20
const planDrafts = new Map<string, PlanDraft>()

function rememberPlanDraft(planId: string, draft: PlanDraft): void {
  planDrafts.delete(planId)
  planDrafts.set(planId, draft)
  while (planDrafts.size > MAX_PLAN_DRAFTS) {
    const oldest = planDrafts.keys().next().value
    if (oldest === undefined) break
    planDrafts.delete(oldest)
  }
}

/** Test seam: drafts are module state that outlives a single mount by design. */
export function __resetPlanDraftsForTests(): void {
  planDrafts.clear()
}

/**
 * The plan-approval control: the only thing that grants a planning run the
 * authority to execute — and, when the model proposed a structured plan
 * (plan-gate §2.1), the only place a person can reorder, rewrite, add,
 * remove, or reject its steps before that grant is made.
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
 * Before the structured plan existed, the model's plan was only prose the
 * browser could not edit — approving meant trusting it verbatim, or nothing.
 */
export function PlanApprovalControl(props: {
  planId?: string
  summary?: string
  steps?: ChatPlanStep[]
  grantedRung?: AutonomyRung
  /** The proposed plan was rejected — nothing was granted or executed. */
  rejected?: boolean
  pending: boolean
  error?: string
  onApprove: (rung: AutonomyRung, justification: string, steps?: PlanStepEdit[]) => void
  onReject?: (planId: string, reason: string) => void
}) {
  const i18n = useI18n()
  // Read once, on purpose: this is the identity the control's draft is kept
  // under, and a control never changes which plan it is showing.
  const draftKey = untrack(() => props.planId)
  const draft = draftKey ? planDrafts.get(draftKey) : undefined
  // A decided plan has nothing left to edit; do not let its draft outlive it.
  if (draftKey && untrack(() => Boolean(props.grantedRung || props.rejected))) {
    planDrafts.delete(draftKey)
  }
  const [rung, setRung] = createSignal<AutonomyRung>(draft?.rung ?? 'workspace_write')
  const [reason, setReason] = createSignal(draft?.reason ?? '')
  const tooShort = () => reason().trim().length < MIN_PLAN_JUSTIFICATION_CHARS
  const hasStructuredPlan = () => Boolean(props.planId)

  // Seeded ONCE (from the remembered draft if this control is a remount, else
  // from props.steps), never resynced on every render — same defensive
  // pattern as `reason` above: a mid-edit re-stream (a second plan.propose
  // call superseding this one before the user submits) must not silently
  // clobber edits already in progress.
  const [rows, setRows] = createSignal<EditablePlanStepRow[]>(
    draft?.rows ??
      untrack(() => props.steps ?? []).map((step) => ({
        localKey: nextPlanStepRowKey(),
        stepId: step.id,
        title: step.title,
        operation: step.operation ?? '',
        removed: false,
      })),
  )

  // Uncontrolled: `<input value={row.title}>` sets the DOM value once at
  // creation; the live, currently-typed value is read back from here at
  // submit time (`wireEdits`). Never used to drive a render.
  const inputRefs = new Map<string, HTMLInputElement>()
  const currentTitle = (row: EditablePlanStepRow) => inputRefs.get(row.localKey)?.value ?? row.title

  // The one thing typing DOES drive: whether the plan still has a real step
  // and whether every kept step has a title. A tick, not the text — the
  // inputs stay uncontrolled (see `EditablePlanStepRow`), this only tells the
  // two derived reads below to look at them again.
  const [titleTick, setTitleTick] = createSignal(0)
  // A row the person added and never filled in is not a step.
  const isBlankNewRow = (row: EditablePlanStepRow) =>
    row.stepId === '' && currentTitle(row).trim() === ''
  const remainingCount = () => {
    titleTick()
    return rows().filter((row) => !row.removed && !isBlankNewRow(row)).length
  }
  // A step the person is keeping but has emptied: the server refuses a step
  // with no title, so say so here instead of after a round trip.
  const hasBlankKeptStep = () => {
    titleTick()
    return rows().some(
      (row) => !row.removed && row.stepId !== '' && currentTitle(row).trim() === '',
    )
  }

  // Remember the edits if this control is torn down for a reason other than
  // the decision itself (see `PlanDraft`).
  onCleanup(() => {
    if (!draftKey) return
    rememberPlanDraft(draftKey, {
      rung: rung(),
      reason: reason(),
      rows: rows().map((row) => ({ ...row, title: currentTitle(row) })),
    })
  })

  const removeRow = (localKey: string) => {
    setRows((current) =>
      current.map((row) => (row.localKey === localKey ? { ...row, removed: true } : row)),
    )
  }
  const swap = (a: number, b: number) => {
    setRows((current) => {
      if (a < 0 || b < 0 || a >= current.length || b >= current.length) return current
      const rowA = current[a]
      const rowB = current[b]
      if (!rowA || !rowB) return current
      const next = current.slice()
      next[a] = rowB
      next[b] = rowA
      return next
    })
  }
  // `rows()` keeps removed rows in place (see the `<For>` doc above), so a
  // raw `index - 1`/`index + 1` neighbor can land on a HIDDEN removed row:
  // swapping with it changes the underlying array but moves nothing the
  // user can see, so the up/down button looks broken the first time a
  // removed row sits between two visible ones. This walks past removed
  // rows to find the next VISIBLE neighbor in `direction`, so "move
  // up/down" always reorders among what is actually on screen.
  const visibleNeighborIndex = (from: number, direction: 1 | -1): number | null => {
    const current = rows()
    let i = from + direction
    while (i >= 0 && i < current.length) {
      if (!current[i]?.removed) return i
      i += direction
    }
    return null
  }
  const addRow = () => {
    setRows((current) => [
      ...current,
      { localKey: nextPlanStepRowKey(), stepId: '', title: '', operation: '', removed: false },
    ])
  }
  const wireEdits = (): PlanStepEdit[] =>
    rows()
      // A never-filled new row is dropped rather than sent: the server refuses
      // a step with no title, and it never belonged to the proposal.
      .filter((row) => !isBlankNewRow(row))
      .map((row) => ({
        stepId: row.stepId || undefined,
        title: currentTitle(row).trim(),
        operation: row.operation || undefined,
        removed: row.removed,
      }))

  // Inside an English sentence the rung name has to read as a phrase, so it is
  // lower-cased there; the Norwegian names are lower-case already.
  const grantedSentence = () => {
    const label = rungLabel(props.grantedRung ?? 'read_only', i18n.tr)
    return i18n.tr(`Godkjent: agenten kan nå ${label}.`, `Approved: the agent can now ${label.toLowerCase()}.`)
  }

  const approveDisabled = () =>
    tooShort() ||
    props.pending ||
    (hasStructuredPlan() && (remainingCount() === 0 || hasBlankKeptStep()))

  return (
    <Show
      when={!props.grantedRung && !props.rejected}
      fallback={
        <p class="verevon-chat-plan-granted">
          <Show
            when={props.rejected}
            fallback={<>{grantedSentence()}</>}
          >
            {i18n.tr(
              'Planen ble avvist – ingenting ble utført.',
              'The plan was rejected – nothing was executed.',
            )}
          </Show>
        </p>
      }
    >
      <section class="verevon-chat-plan" aria-label={i18n.tr('Godkjenn planen', 'Approve the plan')}>
        <p class="verevon-chat-plan__lead">
          {i18n.tr(
            'Dette var en plan – ingenting er utført. Velg hvor mye agenten får gjøre, og skriv hvorfor.',
            'This was a plan – nothing has been executed. Choose how much the agent may do, and say why.',
          )}
        </p>
        <Show when={hasStructuredPlan()}>
          <Show when={props.summary}>
            {(summary) => <p class="verevon-chat-plan__summary">{summary()}</p>}
          </Show>
          <ol class="verevon-chat-plan__steps">
            {/* Iterates the FULL row array (stable object identity per row,
                including across reorders — see `EditablePlanStepRow`'s doc),
                hiding a removed row with `<Show>` INSIDE the callback rather
                than filtering the array `<For>` sees. Filtering there would
                hand `<For>` a freshly-mapped array on every removal, which
                is a new set of item references and would recreate every
                remaining row's DOM (and drop whatever the user was mid-typing
                into an unrelated row). */}
            <For each={rows()}>
              {(row, index) => (
                <Show when={!row.removed}>
                  <li class="verevon-chat-plan__step">
                    <input
                      type="text"
                      class="verevon-chat-plan__step-input"
                      ref={(el) => inputRefs.set(row.localKey, el)}
                      value={row.title}
                      aria-label={i18n.tr(`Steg`, `Step`)}
                      onInput={() => setTitleTick((tick) => tick + 1)}
                    />
                    <div class="verevon-chat-plan__step-actions">
                      <button
                        type="button"
                        disabled={visibleNeighborIndex(index(), -1) === null}
                        aria-label={i18n.tr('Flytt opp', 'Move up')}
                        onClick={() => {
                          const target = visibleNeighborIndex(index(), -1)
                          if (target !== null) swap(index(), target)
                        }}
                      >
                        <ChevronUp size={14} />
                      </button>
                      <button
                        type="button"
                        disabled={visibleNeighborIndex(index(), 1) === null}
                        aria-label={i18n.tr('Flytt ned', 'Move down')}
                        onClick={() => {
                          const target = visibleNeighborIndex(index(), 1)
                          if (target !== null) swap(index(), target)
                        }}
                      >
                        <ChevronDown size={14} />
                      </button>
                      <button
                        type="button"
                        class="verevon-chat-plan__step-remove"
                        aria-label={i18n.tr('Fjern', 'Remove')}
                        onClick={() => removeRow(row.localKey)}
                      >
                        <X size={14} />
                      </button>
                    </div>
                  </li>
                </Show>
              )}
            </For>
          </ol>
          <button type="button" class="verevon-chat-plan__add-step" onClick={addRow}>
            {i18n.tr('Legg til steg', 'Add step')}
          </button>
        </Show>
        <div class="verevon-chat-plan__rungs" role="radiogroup" aria-label={i18n.tr('Fullmakt', 'Authority')}>
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
                {rungLabel(option, i18n.tr)}
              </button>
            )}
          </For>
        </div>
        <textarea
          class="verevon-chat-plan__reason"
          rows={2}
          placeholder={i18n.tr(
            'Hvorfor trenger agenten denne fullmakten?',
            'Why does the agent need this authority?',
          )}
          value={reason()}
          onInput={(event) => setReason(event.currentTarget.value)}
        />
        <div class="verevon-chat-plan__actions">
          {/* The minimum is stated, not enforced silently: a disabled button
              with no reason given is indistinguishable from a broken one. */}
          <Show when={tooShort()}>
            <span class="verevon-chat-plan__hint">
              {i18n.tr(
                `Minst ${MIN_PLAN_JUSTIFICATION_CHARS} tegn – en begrunnelse noen kan vurdere.`,
                `At least ${MIN_PLAN_JUSTIFICATION_CHARS} characters – a reason someone can assess.`,
              )}
            </span>
          </Show>
          <Show when={hasStructuredPlan() && remainingCount() === 0 && !tooShort()}>
            <span class="verevon-chat-plan__hint">
              {i18n.tr('Planen må ha minst ett steg.', 'The plan needs at least one step.')}
            </span>
          </Show>
          <Show when={hasStructuredPlan() && remainingCount() > 0 && hasBlankKeptStep()}>
            <span class="verevon-chat-plan__hint">
              {i18n.tr('Hvert steg trenger en tittel.', 'Every step needs a title.')}
            </span>
          </Show>
          <Show when={props.error}>
            {(message) => (
              <span class="verevon-chat-plan__error" role="alert">
                {message()}
              </span>
            )}
          </Show>
          <Show when={hasStructuredPlan() && props.onReject}>
            <button
              type="button"
              class="verevon-chat-plan__reject"
              disabled={props.pending}
              onClick={() =>
                props.onReject?.(
                  props.planId ?? '',
                  reason().trim() || i18n.tr('Avvist av bruker', 'Rejected by the user'),
                )
              }
            >
              {i18n.tr('Avvis planen', 'Reject the plan')}
            </button>
          </Show>
          <button
            type="button"
            class="verevon-chat-plan__approve"
            disabled={approveDisabled()}
            onClick={() => {
              // Called with exactly two arguments for the plain rung-grant
              // path (no structured plan) — matching every caller/test that
              // predates plan-gate, rather than passing an explicit
              // `undefined` third argument.
              if (hasStructuredPlan()) {
                props.onApprove(rung(), reason().trim(), wireEdits())
              } else {
                props.onApprove(rung(), reason().trim())
              }
            }}
          >
            {props.pending ? i18n.tr('Godkjenner…', 'Approving…') : i18n.tr('Godkjenn', 'Approve')}
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

// Exported for the composer's "Kjør plan" pill, which names the same rung in the
// same words. That pill takes no translation yet, so it stays Norwegian until
// ChatPage passes rungLabel instead.
export const RUNG_LABEL: Record<AutonomyRung, string> = {
  read_only: 'bare undersøke',
  workspace_write: 'skrive i arbeidsområdet',
  danger_full_access: 'utføre alt, også utgående handlinger',
}

const RUNG_LABEL_EN: Record<AutonomyRung, string> = {
  read_only: 'Read only',
  workspace_write: 'Write in the workspace',
  danger_full_access: 'Do anything, including outbound actions',
}

/** A rung's name in the reader's language, for the grant control and the granted sentence. */
export function rungLabel(rung: AutonomyRung, tr: (noText: string, enText: string) => string): string {
  return tr(RUNG_LABEL[rung], RUNG_LABEL_EN[rung])
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

/**
 * Icons for the chips on a USER turn, keyed to the composer's own tool union.
 * Unrelated to inbound server tool calls, which arrive as free-form names and
 * render with `Wrench` — and unrelated to the `reasoning` node, which is fed by
 * `turn.reasoning` and keeps its own Brain icon.
 */
export function toolChipIcon(tool: ComposerToolId): IconComponent {
  if (tool === 'search') return Globe2
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

/**
 * The context chips a user turn was sent with (EP-3 S5), read back from `metadata.context_refs`. Labels
 * only: a chip is a reference the gateway resolved, never content, so nothing here opens or fetches it.
 */
function ContextRefChips(props: { refs: readonly ChatContextRef[] }) {
  const i18n = useI18n()
  return (
    <Show when={props.refs.length > 0}>
      <div class="verevon-chat-context-refs" role="list" aria-label={i18n.tr('Kontekst', 'Context')}>
        <For each={props.refs}>
          {(ref) => (
            <span role="listitem" class="verevon-chat-context-ref" data-kind={ref.kind}>
              {ref.label || ref.id}
            </span>
          )}
        </For>
      </div>
    </Show>
  )
}

/** Connector-attach picker provenance labels — mirrors the composer's own
 * CONNECTOR_SOURCE_LABELS (DashboardComposer.tsx), kept as a small local
 * copy here rather than a cross-file import: this is a display-only label
 * map, not shared logic, and ChatMessages.tsx is a hot file several other
 * Wave-2 items also touch. */
const ATTACHMENT_SOURCE_LABELS: Record<string, { no: string; en: string }> = {
  onedrive: { no: 'OneDrive', en: 'OneDrive' },
  gdrive: { no: 'Google Disk', en: 'Google Drive' },
  gmail: { no: 'Gmail', en: 'Gmail' },
  outlook: { no: 'Outlook', en: 'Outlook' },
  gcal: { no: 'Google Kalender', en: 'Google Calendar' },
  'outlook-cal': { no: 'Outlook-kalender', en: 'Outlook Calendar' },
}

function attachmentSourceLabel(attachment: ChatTurnAttachment, i18n: ReturnType<typeof useI18n>): string | undefined {
  const copy = attachment.source ? ATTACHMENT_SOURCE_LABELS[attachment.source] : undefined
  return copy ? i18n.tr(copy.no, copy.en) : undefined
}

function attachmentSourceTitle(attachment: ChatTurnAttachment, i18n: ReturnType<typeof useI18n>): string | undefined {
  const label = attachmentSourceLabel(attachment, i18n)
  if (!label) return undefined
  const fetchedAt = attachment.fetchedAt ? new Date(attachment.fetchedAt) : null
  const time = fetchedAt && !Number.isNaN(fetchedAt.getTime())
    ? fetchedAt.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
    : undefined
  return time
    ? i18n.tr(`Fra ${label} · hentet ${time}`, `From ${label} · fetched ${time}`)
    : i18n.tr(`Fra ${label}`, `From ${label}`)
}

export function AttachmentItem(props: {
  attachment: ChatTurnAttachment
  tone: 'assistant' | 'user'
  onOpen?: (attachmentId: string) => void
}) {
  const i18n = useI18n()
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
  const attachmentTitle = () => {
    const parts = [
      interaction() ? i18n.tr('Åpne i arbeidsflate', 'Open in the workspace') : undefined,
      attachmentSourceTitle(props.attachment, i18n),
    ].filter((part): part is string => Boolean(part))
    return parts.length > 0 ? parts.join(' · ') : undefined
  }
  const sourceBadge = () => attachmentSourceLabel(props.attachment, i18n)

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
          title={attachmentTitle()}
        >
          {props.attachment.name}
          <Show when={sourceBadge()}>
            <span class="verevon-chat-attachment-source-badge">{sourceBadge()}</span>
          </Show>
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
          title={attachmentTitle()}
        >
          <img src={url()} alt={props.attachment.name} onError={() => setFailed(true)} />
          <Show when={sourceBadge()}>
            <span class="verevon-chat-attachment-source-badge">{sourceBadge()}</span>
          </Show>
        </span>
      )}
    </Show>
  )
}

export function GeneratedImagePreviews(props: { previews: GeneratedImagePreview[] }) {
  const i18n = useI18n()
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
                  aria-label={i18n.tr(`Åpne ${preview.title}`, `Open ${preview.title}`)}
                  title={i18n.tr('Åpne bildet', 'Open image')}
                >
                  <ExternalLink size={14} />
                </a>
                <a
                  href={preview.src}
                  download={preview.downloadName}
                  aria-label={i18n.tr(`Last ned ${preview.title}`, `Download ${preview.title}`)}
                  title={i18n.tr('Last ned bildet', 'Download image')}
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
              aria-label={i18n.tr(`Åpne ${preview.title}`, `Open ${preview.title}`)}
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
/**
 * WS19-S3: the gateway's structural check of a generated file, as a chip. The
 * check reads the file's container only, so neither the label nor its title may
 * say the file opens in Office. A `skipped` check, or none, shows nothing.
 */
function GeneratedFileCheckChip(props: { check?: GeneratedFile['check'] }) {
  const i18n = useI18n()
  const chip = () => {
    switch (props.check?.level) {
      case 'passed':
        return {
          level: 'passed',
          label: i18n.tr('Kontrollert', 'Checked'),
          title: i18n.tr(
            'Filstrukturen er kontrollert. Det betyr ikke at Office åpner filen.',
            'The file structure was checked. This does not mean Office opens the file.',
          ),
        }
      case 'warnings':
        return {
          level: 'warnings',
          label: i18n.tr('Advarsler', 'Warnings'),
          title: i18n.tr(
            'Kontrollen av filstrukturen fant advarsler. Det er ikke prøvd om Office åpner filen.',
            'The file structure check found warnings. Whether Office opens the file was not tried.',
          ),
        }
      case 'failed':
        return {
          level: 'failed',
          label: i18n.tr('Feil funnet', 'Errors found'),
          title: i18n.tr(
            'Kontrollen av filstrukturen fant feil. Det er ikke prøvd om Office åpner filen.',
            'The file structure check found errors. Whether Office opens the file was not tried.',
          ),
        }
      default:
        return undefined
    }
  }
  return (
    <Show when={chip()}>
      {(c) => (
        <span class={`verevon-chat-generated-file__check verevon-chat-generated-file__check--${c().level}`} title={c().title}>
          {c().label}
        </span>
      )}
    </Show>
  )
}

export function GeneratedFiles(props: { files: GeneratedFile[] }) {
  const i18n = useI18n()
  return (
    <div class="verevon-chat-generated-files">
      <For each={props.files}>
        {(file) => {
          const bytes = () => (file.size > 0 ? file.size : dataUriByteSize(file.url))
          const remote = () => /^https?:\/\//i.test(file.url)
          return (
            <span class="verevon-chat-generated-file">
              <a href={file.url} download={file.name} aria-label={i18n.tr(`Last ned ${file.name}`, `Download ${file.name}`)}>
                <Paperclip size={12} />
                {file.name}
                <em>{friendlyMimeLabel(file.mime)}</em>
                <Show when={bytes() > 0}><span>{formatBytes(bytes())}</span></Show>
                <Download size={12} />
              </a>
              <GeneratedFileCheckChip check={file.check} />
              <Show when={remote()}>
                <a
                  class="verevon-chat-generated-file__open"
                  href={file.url}
                  target="_blank"
                  rel="noopener noreferrer"
                  aria-label={i18n.tr(`Åpne ${file.name}`, `Open ${file.name}`)}
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
 * The answer's confidence, inline on the message bubble — unlike the
 * `ReasoningPopover`'s "Sikkerhet" metric, this does not require the user to
 * open anything to see it.
 *
 * Below `LOW_CONFIDENCE_ANSWER_THRESHOLD` (see chat-types.ts for the threshold
 * rationale) it is a caveat, worded to point only at a verification path that
 * exists. At or above it, the same number is still shown, quietly: while only
 * the caveat rendered, the score appeared on one answer and vanished on the
 * next, which reads as a missing signal rather than a good one, and left the
 * user unable to tell a confident answer from an unscored one without opening
 * Detaljer.
 *
 * Independent of `message.grounding`: an answer can be low-confidence with no
 * grounding object at all (an ungrounded guess), which is exactly the case the
 * caveat exists to catch.
 */
export function ConfidenceNotice(props: {
  confidence: number
  hasEvidence: boolean
  low: boolean
  verification?: Extract<ConversationNode, { kind: 'confidence' }>['verification']
}) {
  const i18n = useI18n()
  const percent = () => Math.round(props.confidence * 100)
  // What the backend actually did, when it did anything. Absent is NOT "found
  // nothing": the pass only runs on low-scoring, unevidenced, short answers, so
  // saying "we checked" for a turn that was never checked would be a claim the
  // system cannot back.
  const checked = () => {
    const verification = props.verification
    if (!verification) return undefined
    const sources = verification.kbCitations + verification.webCitations
    if (verification.verdict === 'supports') {
      return i18n.tr(
        `bekreftet mot ${sources} ${sources === 1 ? 'kilde' : 'kilder'}`,
        `confirmed against ${sources} ${sources === 1 ? 'source' : 'sources'}`,
      )
    }
    if (verification.verdict === 'contradicts') {
      return i18n.tr('kildene sier noe annet', 'the sources say otherwise')
    }
    // verdict === 'unrelated': the verifier ran, but that alone doesn't mean
    // it checked anything. Zero citations means nothing was ever attempted —
    // a question like "17 * 23?" was never going to be in the KB — and
    // saying "checked the documents — found no backing" there claims an
    // active-but-failed search that never happened (F-04). Only make that
    // claim when there were sources to actually check against.
    if (sources === 0) return undefined
    return verification.webAllowed
      ? i18n.tr(
          'sjekket dokumentene og nettet — fant ingen dekning',
          'checked the documents and the web — found no backing',
        )
      : i18n.tr(
          'sjekket dokumentene — fant ingen dekning',
          'checked the documents — found no backing',
        )
  }
  // Point only at a verification path that exists. "Check the sources" on a
  // turn with no sources sends the reader after nothing, which is how a
  // caveat trains people to ignore caveats (audit item 23). When the system
  // has already checked, saying what it found beats telling the reader to go
  // and repeat the work.
  const advice = () => checked()
    ?? (props.hasEvidence
      ? i18n.tr('sjekk kildene før du stoler på dette', 'check the sources before relying on this')
      : i18n.tr('ingen kilder ble brukt, så bekreft det selv', 'no sources were used, so verify it yourself'))
  return (
    <Show
      when={props.low}
      fallback={
        <p class="verevon-chat-confidence-note" role="note">
          {i18n.tr(`${percent()}% sikkerhet`, `${percent()}% confidence`)}
          <Show when={checked()}>
            {(note) => <span> — {note()}</span>}
          </Show>
        </p>
      }
    >
      <p class="verevon-chat-low-confidence-notice" role="note">
        <AlertCircle size={12} />
        {i18n.tr(
          `Usikkert svar (${percent()}% sikkerhet) — ${advice()}.`,
          `Uncertain answer (${percent()}% confidence) — ${advice()}.`,
        )}
      </p>
    </Show>
  )
}

/**
 * Deterministic "memory was retrieved" indicator (Model Plane's
 * `memory_recall` event). Shown because a user cannot otherwise tell that
 * remembered context was in play for this turn — and a wrong remembered fact
 * is only correctable if you know it was in play. Recall is a fixed-cost
 * lookup that runs every turn regardless of relevance, not a signal that the
 * model actually drew on what came back (F-05) — the copy says "retrieved",
 * never "used", so it never claims more than the event proves.
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
 * "Memory was retrieved", and — when the backend said which — what came back.
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
  /**
   * The active thread, so "Rediger" can satisfy the write path's
   * thread-ownership check. Null disables editing (but never deleting,
   * which needs no thread) rather than sending an empty id.
   */
  threadId?: string | null
}) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  // A local, mutable copy: `props.memories` is the historical record of what
  // THIS turn recalled when it ran, and stays that way. Rediger/Glem act on
  // the real backend and must be reflected here immediately — "the user sees
  // the memory is gone/corrected without needing to reopen the panel" — which
  // a copy the component owns gives for free.
  const [items, setItems] = createSignal<RecalledMemory[]>(props.memories)
  const [editingId, setEditingId] = createSignal<string | null>(null)
  const [draft, setDraft] = createSignal('')
  const [loadingDraftId, setLoadingDraftId] = createSignal<string | null>(null)
  const [confirmForgetId, setConfirmForgetId] = createSignal<string | null>(null)
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)
  // A partial outcome that is not an error: the correction was saved, but the
  // original was not removed. Said out loud, because the list would otherwise
  // imply a replacement that did not happen.
  const [actionNotice, setActionNotice] = createSignal<string | null>(null)

  const listed = () => props.memories.length > 0
  // "Hentet"/"Retrieved", not "Brukte"/"Used": recall is a fixed-cost lookup
  // that runs on every turn regardless of relevance, so the count is what the
  // backend fetched — it is not proof the model drew on any of it (F-05).
  const summary = () =>
    i18n.tr(
      `Hentet ${props.count} ${props.count === 1 ? 'minne' : 'minner'} fra tidligere samtaler.`,
      `Retrieved ${props.count} ${props.count === 1 ? 'memory' : 'memories'} from earlier conversations.`,
    )

  /**
   * Only a "USER"-topic entry is one `ListMemory`/`DeleteMemory` (and so this
   * correct/forget pair, which reads and writes through the same session-core
   * scope) can ever act on — both RPCs filter to `scope = 'user'` server-side,
   * and topic "USER" is exactly what maps to that scope (see `dreaming.rs`'s
   * `index_agent_memory`). Gating on the label here means the button never
   * promises an action the backend would silently no-op on an org/workspace/
   * policy entry.
   */
  const isPersonal = (memory: RecalledMemory) => memory.label === 'USER'

  // A turn restored from storage says which memories it recalled but not what
  // they said (`recalledMemoryWithoutText`), so a memory forgotten or
  // corrected since cannot come back from the cache. The reader's own
  // memories are read once, the first time the list is opened, and show their
  // current text; a row not found there shows its label only.
  let previewsRequested = false
  const loadMissingPreviews = async () => {
    if (previewsRequested) return
    if (!items().some((memory) => isPersonal(memory) && !memory.preview.trim())) return
    previewsRequested = true
    try {
      const result = await listMemories()
      const current = new Map(result.memories.map((entry) => [entry.memoryId, entry.content]))
      setItems((list) =>
        list.map((memory) => {
          const content = memory.preview.trim() ? '' : current.get(memory.memoryId)?.trim()
          return content ? { ...memory, preview: truncateText(content, 160) } : memory
        }),
      )
    } catch {
      // Label only: the row still says what was recalled, just not its text.
    }
  }

  const toggleOpen = () => {
    const opening = !open()
    setOpen(opening)
    if (opening) void loadMissingPreviews()
  }

  const startEdit = async (memory: RecalledMemory) => {
    if (busyId()) return
    setActionError(null)
    setActionNotice(null)
    setConfirmForgetId(null)
    setEditingId(memory.memoryId)
    setDraft(memory.preview)
    // The recall event only carries a bounded (160-char) preview. Fetch the
    // full stored content so correcting a long memory can never silently
    // truncate it down to whatever happened to be shown here.
    setLoadingDraftId(memory.memoryId)
    try {
      const result = await listMemories()
      const full = result.memories.find((entry) => entry.memoryId === memory.memoryId)
      // Still editing the same row when this lands, not a stale response for
      // one the reader already moved past.
      if (!full) throw new Error(i18n.tr('Minnet finnes ikke lenger. Åpne en ny samtale for å oppdatere listen.', 'This memory no longer exists. Open a new conversation to refresh the list.'))
      if (editingId() === memory.memoryId) setDraft(full.content)
    } catch {
      // A recall preview is truncated. Never offer to save it as if it were
      // the full current entry when the authoritative read failed.
      setEditingId(null)
      setDraft('')
      setActionError(i18n.tr('Kunne ikke hente hele minnet. Prøv å redigere på nytt.', 'Could not load the full memory. Try editing again.'))
    } finally {
      setLoadingDraftId(null)
    }
  }

  const cancelEdit = () => {
    setEditingId(null)
    setDraft('')
  }

  const saveEdit = async (memory: RecalledMemory) => {
    const content = draft().trim()
    const threadId = props.threadId
    if (!content || !threadId || busyId()) return
    setBusyId(memory.memoryId)
    setActionError(null)
    setActionNotice(null)
    try {
      const result = await correctMemory(memory.memoryId, threadId, content)
      if (result.original === 'removed') {
        // The correction lands under a NEW id (no update-in-place RPC exists —
        // see correctMemory's own doc), so the id is swapped in along with the
        // text rather than just editing the preview in place.
        setItems((current) =>
          current.map((entry) =>
            entry.memoryId === memory.memoryId
              ? { ...entry, memoryId: result.memoryId || entry.memoryId, preview: content }
              : entry,
          ),
        )
      } else {
        // The original still exists, so the row stays exactly as it was: it
        // is what this turn recalled, and it can be recalled again. Only the
        // notice changes, saying what did and did not happen.
        setActionNotice(result.original === 'not_found'
          ? i18n.tr(
              'Rettelsen er lagret som et nytt minne. Det opprinnelige minnet ble ikke fjernet, fordi det ikke er ditt eget (for eksempel et organisasjonsminne) eller ikke finnes lenger, så det kan fortsatt bli hentet.',
              'The correction was saved as a new memory. The original was not removed, because it is not yours (an organisation memory, for example) or no longer exists, so it may still be recalled.',
            )
          : i18n.tr(
              'Rettelsen er lagret som et nytt minne, men det opprinnelige minnet kunne ikke fjernes og kan fortsatt bli hentet. Prøv «Glem» på det.',
              'The correction was saved as a new memory, but the original could not be removed and may still be recalled. Try "Forget" on it.',
            ))
      }
      setEditingId(null)
      setDraft('')
    } catch (err) {
      setActionError(
        translateApiError(err, i18n.tr, {
          no: 'Kunne ikke rette minnet.',
          en: 'Could not correct the memory.',
        }),
      )
    } finally {
      setBusyId(null)
    }
  }

  // Two-click confirm (arm, then confirm), same lightweight pattern the
  // Settings memory list already uses — deletion is real and immediate, but
  // does not need a full modal to say so.
  const forget = async (memory: RecalledMemory) => {
    if (busyId()) return
    if (confirmForgetId() !== memory.memoryId) {
      setConfirmForgetId(memory.memoryId)
      return
    }
    setBusyId(memory.memoryId)
    setActionError(null)
    setActionNotice(null)
    try {
      const result = await deleteMemory(memory.memoryId)
      // `deleted: false` is a real answer (a Zero Data Retention no-op, for
      // one): the memory is still there, so the row must stay.
      if (!result.deleted) {
        setActionError(i18n.tr('Minnet ble ikke glemt. Prøv igjen senere.', 'The memory was not forgotten. Try again later.'))
        return
      }
      setItems((current) => current.filter((entry) => entry.memoryId !== memory.memoryId))
      setConfirmForgetId(null)
    } catch (err) {
      // 404 is what session-core answers for a memory that is not the
      // caller's to remove as well as for a missing one.
      setActionError(err instanceof ApiError && err.status === 404
        ? i18n.tr(
            'Dette minnet kan ikke glemmes herfra: det er ikke ditt eget, eller det finnes ikke lenger.',
            'This memory cannot be forgotten from here: it is not yours, or it no longer exists.',
          )
        : translateApiError(err, i18n.tr, {
            no: 'Kunne ikke glemme minnet.',
            en: 'Could not forget the memory.',
          }),
      )
    } finally {
      setBusyId(null)
    }
  }

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
          onClick={toggleOpen}
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
            <For each={items()}>
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
                  <Show
                    when={editingId() === memory.memoryId}
                    fallback={
                      <Show when={memory.preview.trim()}>
                        <p class="verevon-chat-memory-recall__preview">
                          {memory.preview}
                        </p>
                      </Show>
                    }
                  >
                    <div class="verevon-chat-memory-recall__edit">
                      <textarea
                        class="verevon-chat-memory-recall__edit-textarea"
                        value={draft()}
                        disabled={loadingDraftId() === memory.memoryId || busyId() === memory.memoryId}
                        onInput={(event) => setDraft(event.currentTarget.value)}
                      />
                      <div class="verevon-chat-memory-recall__actions">
                        <button
                          type="button"
                          class="verevon-chat-memory-recall__action"
                          disabled={loadingDraftId() === memory.memoryId || busyId() === memory.memoryId || !draft().trim() || !props.threadId}
                          onClick={() => void saveEdit(memory)}
                        >
                          <Show
                            when={busyId() === memory.memoryId}
                            fallback={<Check size={11} aria-hidden="true" />}
                          >
                            <Loader2 size={11} aria-hidden="true" />
                          </Show>
                          {i18n.tr('Lagre', 'Save')}
                        </button>
                        <button
                          type="button"
                          class="verevon-chat-memory-recall__action"
                          disabled={busyId() === memory.memoryId}
                          onClick={cancelEdit}
                        >
                          <X size={11} aria-hidden="true" />
                          {i18n.tr('Avbryt', 'Cancel')}
                        </button>
                      </div>
                    </div>
                  </Show>
                  <Show when={isPersonal(memory) && editingId() !== memory.memoryId}>
                    <div class="verevon-chat-memory-recall__actions">
                      <button
                        type="button"
                        class="verevon-chat-memory-recall__action"
                        disabled={Boolean(busyId()) || !props.threadId}
                        title={
                          props.threadId
                            ? undefined
                            : i18n.tr(
                                'Ingen aktiv samtale å rette fra',
                                'No active conversation to correct from',
                              )
                        }
                        onClick={() => void startEdit(memory)}
                      >
                        <Pencil size={11} aria-hidden="true" />
                        {i18n.tr('Rediger', 'Correct')}
                      </button>
                      <button
                        type="button"
                        class={{
                          'verevon-chat-memory-recall__action': true,
                          'verevon-chat-memory-recall__action--danger':
                            confirmForgetId() === memory.memoryId,
                        }}
                        disabled={busyId() === memory.memoryId}
                        onClick={() => void forget(memory)}
                      >
                        <Show
                          when={busyId() === memory.memoryId}
                          fallback={<Trash2 size={11} aria-hidden="true" />}
                        >
                          <Loader2 size={11} aria-hidden="true" />
                        </Show>
                        {confirmForgetId() === memory.memoryId
                          ? i18n.tr('Bekreft glemsel?', 'Confirm forgetting?')
                          : i18n.tr('Glem', 'Forget')}
                      </button>
                    </div>
                  </Show>
                </li>
              )}
            </For>
          </ul>
          <Show when={actionNotice()}>
            {(message) => (
              <p class="verevon-chat-memory-recall__notice" role="status">
                {message()}
              </p>
            )}
          </Show>
          <Show when={actionError()}>
            {(message) => (
              <p class="verevon-chat-memory-recall__error" role="alert">
                {message()}
              </p>
            )}
          </Show>
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

/**
 * What the gateway did to this turn while it ran (WS15): a retry, a compaction, a context change, a reduced answer.
 * One plain note per notice, in the order they happened. Nothing here asks for an action, and nothing is an alarm.
 */
export function LifecycleNotices(props: { notices: LifecycleNotice[] }) {
  const i18n = useI18n()
  return (
    <div class="verevon-chat-lifecycle-notices">
      <For each={props.notices}>
        {(notice) => (
          <p class="verevon-chat-lifecycle-notice" role="note">
            <Info size={12} />
            {lifecycleNoticeSentence(notice, i18n)}
          </p>
        )}
      </For>
    </div>
  )
}

/**
 * The sentence a lifecycle notice reads as, shown and announced alike. It is built from the notice's counts and the
 * gateway's closed tokens only. A token this build does not know reads as the generic sentence, and a summary that
 * failed is never worded as one that worked.
 */
export function lifecycleNoticeSentence(notice: LifecycleNotice, i18n: Translator): string {
  switch (notice.kind) {
    case 'retry':
      return retrySentence(notice, i18n)
    case 'compaction':
      return compactionSentence(notice, i18n)
    case 'context':
      return contextSentence(notice, i18n)
    case 'retrieval':
      return retrievalSentence(notice, i18n)
  }
}

/** For a token this build does not know: the context changed, which is the one thing certain about it. */
const adjustedSentence = (i18n: Translator) => i18n.tr('Konteksten ble justert.', 'Context was adjusted.')

function retrySentence(notice: Extract<LifecycleNotice, { kind: 'retry' }>, i18n: Translator): string {
  const tool = notice.tool ? humanizeToolName(notice.tool) : i18n.tr('verktøyet', 'the tool')
  const attempt = i18n.tr(`(forsøk ${notice.attempt} av ${notice.maxAttempts})`, `(attempt ${notice.attempt} of ${notice.maxAttempts})`)
  switch (notice.reason) {
    case 'timeout':
      return i18n.tr(`Prøver ${tool} på nytt etter tidsavbrudd ${attempt}.`, `Retrying ${tool} after a timeout ${attempt}.`)
    case 'connection':
      return i18n.tr(`Prøver ${tool} på nytt etter en tilkoblingsfeil ${attempt}.`, `Retrying ${tool} after a connection error ${attempt}.`)
    case 'upstream_unavailable':
      return i18n.tr(
        `Prøver ${tool} på nytt etter at tjenesten var utilgjengelig ${attempt}.`,
        `Retrying ${tool} after the service was unavailable ${attempt}.`,
      )
    case 'rate_limited':
      return i18n.tr(`Prøver ${tool} på nytt etter for mange forespørsler ${attempt}.`, `Retrying ${tool} after too many requests ${attempt}.`)
    case 'other':
      return i18n.tr(`Prøver ${tool} på nytt etter en midlertidig feil ${attempt}.`, `Retrying ${tool} after a temporary error ${attempt}.`)
    default:
      return i18n.tr(`Prøver ${tool} på nytt ${attempt}.`, `Retrying ${tool} ${attempt}.`)
  }
}

function compactionSentence(notice: Extract<LifecycleNotice, { kind: 'compaction' }>, i18n: Translator): string {
  if (notice.scope === 'tool_results' && notice.outcome === 'cleared') {
    return i18n.tr(
      'Gamle verktøyresultater ble fjernet for å få plass i konteksten.',
      'Old tool results were removed to fit the context.',
    )
  }
  if (notice.scope === 'history') {
    switch (notice.outcome) {
      case 'summarized':
        return i18n.tr('Eldre meldinger ble oppsummert for å få plass i konteksten.', 'Older messages were summarized to fit the context.')
      case 'reused':
        return i18n.tr('En tidligere oppsummering av eldre meldinger ble brukt igjen.', 'An earlier summary of older messages was reused.')
      case 'truncated':
        return i18n.tr(
          'Oppsummeringen av eldre meldinger mislyktes, så de ble kuttet for å få plass.',
          'Summarizing older messages failed, so they were cut to fit the context.',
        )
    }
  }
  return adjustedSentence(i18n)
}

function contextSentence(notice: Extract<LifecycleNotice, { kind: 'context' }>, i18n: Translator): string {
  switch (notice.change) {
    case 'history_shed': {
      const dropped = notice.messagesDropped ?? 0
      if (dropped === 1) return i18n.tr('1 eldre melding ble utelatt fra konteksten.', '1 older message was left out of the context.')
      if (dropped > 1) {
        return i18n.tr(`${dropped} eldre meldinger ble utelatt fra konteksten.`, `${dropped} older messages were left out of the context.`)
      }
      return i18n.tr('Eldre meldinger ble utelatt fra konteksten.', 'Older messages were left out of the context.')
    }
    case 'skills_truncated':
      return i18n.tr('Instruksjonene for en ferdighet ble kortet ned.', 'The instructions for a skill were shortened.')
    case 'run_context_skipped':
      return i18n.tr('En del av kjørekonteksten ble hoppet over.', 'Part of the run context was skipped.')
    default:
      return adjustedSentence(i18n)
  }
}

/** A reduced answer is told as one, whatever the reason: the generic wording covers a key this build does not word. */
function retrievalSentence(notice: Extract<LifecycleNotice, { kind: 'retrieval' }>, i18n: Translator): string {
  if (notice.wordingKey === 'retrieval.zdr_dense_unavailable') {
    return i18n.tr(
      'Svaret bygger bare på tekstsøk i kildene, fordi semantisk søk ikke er tilgjengelig under Zero Data Retention.',
      'The answer uses text search in the sources only, because semantic search is not available under Zero Data Retention.',
    )
  }
  return i18n.tr('Svaret ble laget ut fra et begrenset søk i kildene.', 'The answer was produced from a reduced search of the sources.')
}

/**
 * Per-turn retrieval-source filter receipt (chat-wave2 scope spec §2.2's
 * "shown in the receipt" requirement) — a small disclosure chip next to the
 * other per-turn notices (verification, memory recall), never a silent
 * change of behavior. `excludedTools` is deliberately not rendered inline
 * here: it is the assembled-context inspector's job (`ChatPanels.tsx`), the
 * existing surface for that kind of transparency, to keep this chip short.
 *
 * `knowledge` is a Space's knowledge receipt (WS18-S3): what the Space's
 * knowledge did for this answer. It gets a chip of its own, so a turn with no
 * source filter shows only that one.
 */
export function SourceScopeNotice(props: {
  scope: 'knowledge_only' | 'web_only' | 'auto'
  excludedTools: string[]
  knowledge?: ChatKnowledgeReceipt
}) {
  const i18n = useI18n()
  const knowledgeText = (knowledge: ChatKnowledgeReceipt) => {
    if (knowledge.mode === 'not_entitled') {
      return i18n.tr('Prosjektkunnskap ikke tilgjengelig for dette svaret', 'Project knowledge not available for this answer')
    }
    if (knowledge.mode === 'not_searched') {
      return i18n.tr('Prosjektkunnskap ikke brukt i dette svaret', 'Project knowledge not used for this answer')
    }
    if (knowledge.sources === undefined) return i18n.tr('Prosjektkunnskap: søkt', 'Project knowledge: searched')
    return knowledge.sources === 1
      ? i18n.tr('Prosjektkunnskap: 1 kilde', 'Project knowledge: 1 source')
      : i18n.tr(`Prosjektkunnskap: ${knowledge.sources} kilder`, `Project knowledge: ${knowledge.sources} sources`)
  }
  return (
    <>
      <Show when={props.scope !== 'auto'}>
        <p class="verevon-chat-source-scope-notice" role="note">
          <Info size={12} />
          {props.scope === 'knowledge_only'
            ? i18n.tr('Kilder: kun organisasjonskunnskap', 'Sources: organization knowledge only')
            : i18n.tr('Kilder: kun nett', 'Sources: web only')}
        </p>
      </Show>
      <Show when={props.knowledge}>
        {(knowledge) => (
          <p class="verevon-chat-source-scope-notice" role="note">
            <Info size={12} />
            {knowledgeText(knowledge())}
          </p>
        )}
      </Show>
    </>
  )
}

/**
 * "Searched via X" (EP-9 W1): which search providers served this turn's web searches, and the route that allowed
 * them. The wording is keyed by the closed provider id, so a provider the server does not name shows as "other
 * provider" and never as its raw id. Names and a reason only: no query and no URL reach this line.
 */
export function SearchProvenanceNotice(props: { provenance: ChatSearchProvenanceEvent }) {
  const i18n = useI18n()
  const providerName = (id: ChatSearchProviderId) => {
    if (id === 'tantivy_local') return i18n.tr('Verevon-indeks', 'Verevon index')
    if (id === 'verevon') return i18n.tr('Verevon-søk', 'Verevon search')
    if (id === 'brave') return 'Brave Search'
    if (id === 'serper') return i18n.tr('Serper (Google-resultater)', 'Serper (Google results)')
    if (id === 'searxng') return 'SearXNG'
    if (id === 'stract') return 'Stract'
    return i18n.tr('annen leverandør', 'other provider')
  }
  const routeText = (route: ChatSearchRoute) => {
    if (route === 'zdr_free_only') {
      return i18n.tr(
        'Nulldatalagring er på, så bare gratis søketjenester ble brukt.',
        'Zero data retention is on, so only free search providers were used.',
      )
    }
    if (route === 'paid_allowed') {
      return i18n.tr('Betalte søketjenester er tillatt for dette svaret.', 'Paid search providers are allowed for this answer.')
    }
    return i18n.tr('Bare gratis søketjenester er tillatt for dette svaret.', 'Only free search providers are allowed for this answer.')
  }
  return (
    <p class="verevon-chat-source-scope-notice" role="note">
      <Info size={12} />
      {i18n.tr('Søkt via ', 'Searched via ')}
      {props.provenance.providers.map((provider) => providerName(provider.id)).join(', ')}
      {'. '}
      {routeText(props.provenance.route)}
    </p>
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

/** The longest delay `setTimeout` honours; a longer one would fire at once. */
const MAX_TIMER_DELAY_MS = 2 ** 31 - 1

/** Milliseconds from `now` to the next local midnight, where "i morgen" becomes "i dag". */
function msUntilNextLocalMidnight(now: number): number {
  const midnight = new Date(now)
  midnight.setHours(24, 0, 0, 0)
  return Math.max(1, midnight.getTime() - now)
}

/**
 * Where a ChatGPT subscription is reconnected, and where a zero-retention
 * refusal sends the reader to review that connection: its panel on the reader's
 * own Profile page. Not Settings → Integrations, which is admin-only and sends a
 * member to the dashboard. The connection is the reader's own, and only they
 * can renew or confirm it, so the link must work for every member.
 */
const CHATGPT_CONNECTION_HREF = '/account#chatgpt-subscription'

export function ErrorNotice(props: {
  message: string
  unsaved?: string
  /** What the reader can do about the failure. Absent means retry. */
  action?: StreamFailureAction
  /** With `action: 'wait'`: the instant (ISO) the retry control is enabled again. */
  retryAt?: string
  onRetry: () => void
}) {
  const i18n = useI18n()
  // The wall clock, read at mount and again only when a wait ends or the
  // local day turns over during it. The notice is rebuilt on every derivation
  // (see `ThinkingDots`), so the wait runs from the instant it ends at, never
  // from a countdown started at mount.
  const [now, setNow] = createSignal(Date.now())
  const action = (): StreamFailureAction => props.action ?? 'retry'
  /** Milliseconds before a `wait` may be retried; 0 once it may. */
  const remainingMs = () => {
    if (action() !== 'wait') return 0
    const at = Date.parse(props.retryAt ?? '')
    return Number.isFinite(at) ? Math.max(0, at - now()) : 0
  }
  const waiting = () => remainingMs() > 0
  // Enable the control when the wait ends. This only re-reads the clock: the
  // reader still presses it, and nothing is ever sent by itself. A delay past
  // `setTimeout`'s limit is split, so the control is never enabled early. The
  // clock is also re-read at local midnight, so a reset worded "i morgen" reads
  // "i dag" once that day has come.
  createEffect(
    () => ({ remaining: remainingMs(), at: now() }),
    ({ remaining, at }) => {
      if (remaining <= 0) return
      const delay = Math.min(remaining, msUntilNextLocalMidnight(at), MAX_TIMER_DELAY_MS)
      const timer = setTimeout(() => setNow(Date.now()), delay)
      return () => clearTimeout(timer)
    },
  )
  /** The message, with its reset phrase re-read against the clock above. */
  const message = () => (action() === 'wait' && props.retryAt
    ? refreshResetTimeWording(props.message, props.retryAt, i18n.locale(), now())
    : props.message)
  const retryLabel = () => {
    if (!waiting()) return i18n.tr('Prøv igjen', 'Try again')
    const time = formatResetTime(props.retryAt ?? '', i18n.locale(), now())
    return i18n.tr(`Kan prøves igjen ${time}`, `Available again ${time}`)
  }
  return (
    <div class="verevon-chat-error-notice">
      <p><AlertCircle size={16} /> {message()}</p>
      {/* Text that streamed before a failure the server saved nothing for.
          Kept reachable (the reader watched it arrive), but closed by
          default, labelled as not saved, and plain text rather than an
          answer bubble, so it can never be mistaken for the saved reply. */}
      <Show when={props.unsaved}>
        {(text) => (
          <details class="verevon-chat-error-notice__unsaved">
            <summary>{i18n.tr('Vis teksten som ikke ble lagret', 'Show the text that was not saved')}</summary>
            <p>{text()}</p>
          </details>
        )}
      </Show>
      {/* `none`: sending the same turn again cannot help, so nothing is offered. */}
      <Show when={action() !== 'none'}>
        <div class="verevon-chat-error-notice__actions">
          <Show when={action() === 'reconnect'}>
            <a class="verevon-chat-error-notice__reconnect" href={CHATGPT_CONNECTION_HREF} link>
              <Plug size={14} />
              {i18n.tr('Koble til ChatGPT på nytt', 'Reconnect ChatGPT')}
            </a>
          </Show>
          {/* A zero-retention refusal: the reader reviews the connection, then sends again. */}
          <Show when={action() === 'open_integrations'}>
            <a class="verevon-chat-error-notice__connection" href={CHATGPT_CONNECTION_HREF} link>
              <Plug size={14} />
              {i18n.tr('Åpne ChatGPT-tilkoblingen', 'Open the ChatGPT connection')}
            </a>
          </Show>
          <button
            type="button"
            disabled={waiting()}
            onClick={() => {
              if (!waiting()) props.onRetry()
            }}
          >
            <RefreshCw size={14} />
            {retryLabel()}
          </button>
        </div>
      </Show>
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

export type MessageMenuItem = {
  label: string
  icon: JSX.Element
  onClick: () => void
  /** Shown but not actionable; `title` says why. */
  disabled?: boolean
  title?: string
  /**
   * Off for a reason worth knowing. The item stays focusable (aria-disabled)
   * with the reason as its description, and `onClick` still runs so it can
   * show the reason to someone who cannot see a tooltip.
   */
  unavailableReason?: string
}

export function MessageMenu(props: {
  /**
   * 'auto' picks start or end when the menu opens, whichever keeps the panel
   * inside the nearest `[data-menu-boundary]` ancestor. The Resultat panel
   * clips its content (`overflow: hidden`), and a fixed side cut the items
   * off there at one panel width or another.
   */
  align?: 'start' | 'end' | 'auto'
  /** Opens above the trigger by default (message action rows sit low); 'down' for a header toolbar. */
  direction?: 'up' | 'down'
  items: MessageMenuItem[]
  /**
   * A custom trigger button in place of the default "..." (More actions)
   * ellipsis — the visible export button uses this to look like any other
   * `MessageAction`-styled icon instead of a second overflow menu. Sharing
   * this component (rather than a parallel one) means the open/close logic,
   * outside-click dismissal and Escape handling below never has to be kept
   * in sync between two implementations.
   */
  trigger?: {
    icon: JSX.Element
    label: string
    /** Visible text after the icon, for a toolbar of text buttons (the Resultat panel). */
    text?: string
    class?: string
  }
}) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [autoAlign, setAutoAlign] = createSignal<'start' | 'end'>('start')
  let ref!: HTMLDivElement

  // Measured at open, from two boxes in the same coordinate space, so the
  // desktop shell's CSS zoom scales both alike. 216 = the 208px panel plus
  // its 4px offset and a little air.
  const toggle = () => {
    const opening = !open()
    if (opening && props.align === 'auto') {
      const boundary = (ref.closest('[data-menu-boundary]') ?? document.documentElement).getBoundingClientRect()
      const trigger = ref.getBoundingClientRect()
      setAutoAlign(boundary.right - trigger.left >= 216 ? 'start' : 'end')
    }
    setOpen(opening)
    // A downward menu low in a scrolling panel (the Resultat draft under a
    // table) would open below the fold; bring it into view once it has
    // rendered. jsdom lacks scrollIntoView, hence the optional calls.
    if (opening && props.direction === 'down') {
      window.requestAnimationFrame?.(() => ref.querySelector('.verevon-chat-menu__panel')?.scrollIntoView?.({ block: 'nearest' }))
    }
  }
  const alignEnd = () => (props.align === 'auto' ? autoAlign() === 'end' : props.align === 'end')

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
      <button type="button" class={props.trigger?.class} aria-label={props.trigger?.label ?? i18n.tr('Flere handlinger', 'More actions')} aria-expanded={open() ? 'true' : 'false'} onClick={toggle}>
        {props.trigger?.icon ?? <MoreHorizontal size={14} />}
        {props.trigger?.text}
      </button>
      <Show when={open()}>
        <div class={{
          'verevon-chat-menu__panel': true,
          'verevon-chat-menu__panel--end': alignEnd(),
          'verevon-chat-menu__panel--down': props.direction === 'down',
        }}>
          <For each={props.items}>
            {(item) => {
              const reasonId = createUniqueId()
              return (
                <>
                  <button
                    type="button"
                    disabled={item.disabled}
                    aria-disabled={item.unavailableReason ? 'true' : undefined}
                    aria-describedby={item.unavailableReason ? reasonId : undefined}
                    title={item.title ?? item.unavailableReason}
                    onClick={() => {
                      item.onClick()
                      setOpen(false)
                    }}
                  >
                    {item.icon}
                    {item.label}
                  </button>
                  {/* Outside the button, so it describes the item without renaming it. */}
                  <Show when={item.unavailableReason}>
                    {(reason) => <span id={reasonId} class="sr-only">{reason()}</span>}
                  </Show>
                </>
              )
            }}
          </For>
        </div>
      </Show>
    </div>
  )
}

export function DateDivider(props: { value: string }) {
  const i18n = useI18n()
  return (
    <div class="verevon-chat-divider">
      <span />
      <time>{formatDayLabel(props.value, i18n.locale())}</time>
    </div>
  )
}

/**
 * How long a wait may stay wordless before it needs to account for itself.
 *
 * Ten seconds: past the point where a normal turn has already produced a first
 * token, so an ordinary answer never shows a counter at all, and well short of
 * the minute-plus that made the surface read as hung.
 */
const THINKING_ELAPSED_THRESHOLD_MS = 10_000

/**
 * The waiting state.
 *
 * Some routes take 60–95 seconds before the first token, and a static "Tenker"
 * held that long reads as a hung page rather than as work in progress (F-07).
 * After the threshold this starts naming how long it has actually been waiting
 * — and nothing else. No percentage, no estimate, no invented phase: the wait
 * has no known duration, so any bar or ETA here would be a number the product
 * made up.
 *
 * `activity` adds the other half of the question: not how long, but at what.
 * It is `deriveStreamActivity` over the turn's own tool calls, so it appears
 * only when the stream has actually said something — a turn that reports no
 * activity (the subscription route in F-07 reports none for its whole wait)
 * shows elapsed time alone, which is then the only honest thing to show.
 */
export function ThinkingDots(props: {
  /** When this turn's wait began — the turn's `createdAt`. See `startedAt`. */
  since?: string
  /** The last thing the stream reported. Absent = nothing reported yet. */
  activity?: StreamActivity
  /**
   * The turn's durable run. With one, a wait past the threshold also offers
   * "Varsle meg når ferdig". Absent = no control, exactly as before.
   */
  runId?: string
}) {
  const i18n = useI18n()
  const mountedAt = Date.now()
  /**
   * The turn's own start, not this component's.
   *
   * The transcript renders nodes through a `<For>`, which keys on item identity,
   * and every derivation builds fresh node objects — so this component is torn
   * down and rebuilt on each one. Anchored to mount, the clock restarted on
   * every streamed tool call, and on the deep-research routes it exists for (a
   * call every few hundred milliseconds) it never reached ten seconds, so the
   * counter was invisible in exactly the case F-07 is about.
   *
   * A timestamp in the future is clock skew rather than a wait that has not
   * begun, so it is clamped: a counter ticking up to zero would be worse than
   * one that starts late.
   */
  const startedAt = () => {
    const parsed = Date.parse(props.since ?? '')
    return Number.isFinite(parsed) ? Math.min(parsed, mountedAt) : mountedAt
  }
  // Wall-clock difference rather than an accumulating tick count: a background
  // tab throttles the interval, and a counter that undercounts a two-minute
  // wait would be exactly the reassuring fiction this is meant to remove. The
  // signal holds `now` rather than the difference so a rebuilt indicator shows
  // the real elapsed time on its first frame instead of blinking back to zero.
  const [now, setNow] = createSignal(mountedAt)
  const elapsedMs = () => now() - startedAt()
  const ticker = setInterval(() => setNow(Date.now()), 1000)
  onCleanup(() => clearInterval(ticker))
  return (
    <span class="verevon-chat-thinking">
      <span class="verevon-thinking-dots" aria-hidden="true">
        <span />
        <span />
        <span />
      </span>
      Tenker
      {/* Ungated by the elapsed threshold: knowing a tool is out is useful from
          the first second, and unlike the counter it cannot read as a stall. */}
      <Show when={props.activity}>
        {(activity) => (
          <small class="verevon-chat-thinking__activity">{streamActivityLabel(activity(), i18n)}</small>
        )}
      </Show>
      <Show when={elapsedMs() >= THINKING_ELAPSED_THRESHOLD_MS}>
        <small class="verevon-chat-thinking__elapsed">{formatElapsedWait(elapsedMs())}</small>
      </Show>
      {/* Same threshold as the counter: an ordinary answer never shows it. */}
      <Show when={elapsedMs() >= THINKING_ELAPSED_THRESHOLD_MS ? props.runId : undefined}>
        {(runId) => <RunNotifyButton runId={runId()} />}
      </Show>
    </span>
  )
}

export function TaskStep(props: { isLast: boolean; step: AgentTaskStep }) {
  const i18n = useI18n()
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
            <strong>{workStepLabel(props.step.title, i18n.locale())}</strong>
            <time>{formatTime(props.step.createdAt, i18n.locale())}</time>
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
                        isSafeLinkHref(href())
                          ? <a href={href()} target="_blank" rel="noopener noreferrer">
                            <strong>{item.label}</strong>
                            <em>{item.value}</em>
                          </a>
                          : <span class="verevon-chat-link-blocked" title={i18n.tr('Lenke blokkert: ukjent adresse', 'Link blocked: unsupported address')}>
                            <strong>{item.label}</strong>
                            <em>{item.value}</em>
                          </span>
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

/**
 * The opening surface states scope and offers a first move.
 *
 * Deliberately NOT a seeded assistant turn: an opening message rendered as
 * something Verevon "said" would attribute content the model never generated,
 * and would sit in the transcript as if it were part of the conversation. The
 * same job — say what I can see, suggest where to start — is done here without
 * putting words in the model's mouth.
 */
export function EmptyChatState(props: {
  children: JSX.Element
  onSelectPrompt: (prompt: string) => void
  /**
   * Open an earlier thread. Distinct from `onSelectPrompt` on purpose:
   * resuming a conversation is navigation, not a message. Typing a
   * sentence ABOUT an earlier thread into a new one sends the model a
   * reference it cannot resolve.
   */
  onResumeThread: (threadId: string) => void
  /** Names the actual grounding scope when the session knows it. */
  orgId?: string
  orgName?: string
  /** Greets the person by name when the session knows it. First name only. */
  userId?: string
  userName?: string
}) {
  const i18n = useI18n()
  const [ideasOpen, setIdeasOpen] = createSignal(false)
  const [inboxOpen, setInboxOpen] = createSignal(true)
  const [otherOpen, setOtherOpen] = createSignal(false)
  const [suggestions, setSuggestions] = createSignal<PromptSuggestion[]>([])
  const [suggestionStatus, setSuggestionStatus] = createSignal<'loading' | 'ready' | 'unavailable'>('loading')
  const inboxSuggestions = createMemo(() => suggestions().filter((item) => item.source === 'inbox'))
  const otherSuggestions = createMemo(() => suggestions().filter((item) => item.source !== 'inbox'))
  const inboxPanelId = createUniqueId()
  const otherPanelId = createUniqueId()
  createEffect(
    () => ({ status: suggestionStatus(), inboxCount: inboxSuggestions().length, otherCount: otherSuggestions().length }),
    ({ status, inboxCount, otherCount }) => {
      if (status === 'ready' && inboxCount === 0 && otherCount > 0) setOtherOpen(true)
    },
  )
  createEffect(() => ({ orgId: props.orgId, userId: props.userId }), ({ orgId, userId }) => {
    setSuggestions([])
    if (!orgId || !userId) {
      setSuggestionStatus('ready')
      return
    }

    setSuggestionStatus('loading')
    let controller: AbortController | null = null
    const refresh = () => {
      controller?.abort()
      controller = new AbortController()
      const { signal } = controller
      let notifications: Notification[] = []
      let conversations: LiveTicket[] = []
      let finished = 0
      let failed = 0
      const publish = () => {
        if (signal.aborted) return
        const next = buildPromptSuggestions({ notifications, conversations, orgId, userId })
        // Keep the previous suggestions until both feeds confirm they are gone.
        if (next.length || finished === 2) {
          setSuggestions(next)
          setSuggestionStatus(next.length || !failed ? 'ready' : 'unavailable')
        }
      }
      void listNotifications(signal)
        .then((items) => { notifications = Array.isArray(items) ? items : [] }, () => { failed += 1 })
        .finally(() => { finished += 1; publish() })
      void listConversations(orgId, { limit: 30, state: 'open' }, signal)
        .then((result) => { conversations = result.tickets }, () => { failed += 1 })
        .finally(() => { finished += 1; publish() })
    }
    refresh()
    const timer = window.setInterval(() => { if (!document.hidden) refresh() }, 60_000)
    const onVisible = () => { if (!document.hidden) refresh() }
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      controller?.abort()
      window.clearInterval(timer)
      document.removeEventListener('visibilitychange', onVisible)
    }
  })
  // Solid 2 evaluates component bodies untracked. Capture the prop through a
  // memo before handing it to <Show>, instead of reading the reactive prop
  // directly in the component body (STRICT_READ_UNTRACKED).
  const orgName = createMemo(() => props.orgName?.trim() || null)
  const scopeDescription = createMemo(() => {
    const name = orgName()
    const shortName = name?.replace(/\s+(AS|ASA)$/i, '') ?? null
    const possessiveName = shortName ? `${shortName}${/s$/i.test(shortName) ? '' : 's'}` : null
    return name
      ? i18n.tr(
          `Spør om noe dere jobber med. Verevon finner svar i ${possessiveName} kunnskap og systemer, viser kildene og foreslår neste steg. Du godkjenner før noe sendes videre. Prøv et eksempel, eller skriv ditt eget spørsmål.`,
          `Ask about something you're working on. Verevon finds answers in ${name}'s knowledge and systems, shows its sources, and suggests next steps. You approve before anything is sent. Try an example, or write your own question.`,
        )
      : i18n.tr(
          'Spør om noe dere jobber med. Verevon finner svar i organisasjonens kunnskap og systemer, viser kildene og foreslår neste steg. Du godkjenner før noe sendes videre. Prøv et eksempel, eller skriv ditt eget spørsmål.',
          "Ask about something you're working on. Verevon finds answers in your organization's knowledge and systems, shows its sources, and suggests next steps. You approve before anything is sent. Try an example, or write your own question.",
        )
  })

  const promptForSuggestion = (suggestion: PromptSuggestion) => suggestion.source === 'alert'
    ? i18n.tr(
        `Gå gjennom varselet «${suggestion.title}». Finn hva som har skjedd i tilgjengelige kilder, forklar hva det betyr, og foreslå neste steg. Ikke utfør eller send noe før jeg godkjenner.`,
        `Review the alert “${suggestion.title}”. Find what happened in available sources, explain what it means, and suggest next steps. Do not act or send anything before I approve.`,
      )
    : i18n.tr(
        `Gå gjennom innbokssamtalen «${suggestion.title}» (ID: ${suggestion.id.slice(6)}). Oppsummer hva som trenger oppfølging, finn relevant kunnskap og foreslå et svar jeg kan godkjenne.`,
        `Review inbox conversation “${suggestion.title}” (ID: ${suggestion.id.slice(6)}). Summarize what needs follow-up, find relevant knowledge, and suggest a reply I can approve.`,
      )

  const displayTitleForSuggestion = (suggestion: PromptSuggestion) => suggestion.source === 'inbox'
    ? suggestion.title.split(/\s+\|\s+/)[0]
    : suggestion.title

  const suggestionRow = (suggestion: PromptSuggestion) => (
    <button
      type="button"
      class="verevon-chat-empty__activity-item"
      onClick={() => props.onSelectPrompt(promptForSuggestion(suggestion))}
    >
      <strong>{suggestion.source === 'alert'
        ? i18n.tr(`Undersøk ${displayTitleForSuggestion(suggestion)}`, `Investigate ${displayTitleForSuggestion(suggestion)}`)
        : i18n.tr(`Følg opp ${displayTitleForSuggestion(suggestion)}`, `Follow up ${displayTitleForSuggestion(suggestion)}`)}</strong>
      <small>{suggestion.detail}</small>
    </button>
  )

  /**
   * A greeting instead of a question. "Hva vil du få gjort?" asked the user to
   * supply everything, on a surface that already knows who they are and what
   * it can reach — plan item 17, and the one part of designpixil's
   * "two sentences and a suggestion" the empty state was missing.
   *
   * Time of day is read once per render of an empty thread, not tracked: the
   * greeting must not change under someone mid-sentence at 11:59.
   */
  const greeting = () => {
    const firstName = props.userName?.trim().split(/\s+/)[0] ?? ''
    const hour = new Date().getHours()
    const partOfDay = hour < 10
      ? i18n.tr('God morgen', 'Good morning')
      : hour < 17
        ? i18n.tr('God dag', 'Good afternoon')
        : i18n.tr('God kveld', 'Good evening')
    return firstName ? `${partOfDay}, ${firstName}` : partOfDay
  }

  /**
   * The most recent thread, when there is one worth resuming. Read once on
   * mount rather than reactively: this is an empty-thread surface, so the
   * history behind it cannot change while it is on screen, and a signal here
   * would re-render the greeting for no reason.
   */
  const resumeTarget = () => {
    const recent = readChatThreadHistory()
      .filter((item) => item.title.trim() && item.threadId.trim())
      .sort((a, b) => (b.updatedAt ?? '').localeCompare(a.updatedAt ?? ''))[0]
    if (!recent) return null
    // A title long enough to wrap would break the one-line affordance, and a
    // truncated one reads as broken. Skip it rather than mangle it.
    return recent.title.length <= 60 ? recent : null
  }

  const starterPrompts = () => [
    {
      description: i18n.tr('Trekk ut beslutninger, risiko og neste steg.', 'Extract decisions, risks, and next steps.'),
      label: i18n.tr('Oppsummer et dokument', 'Summarize a document'),
      prompt: i18n.tr('Oppsummer dokumentet jeg legger ved. Fremhev beslutninger, risiko og neste steg.', 'Summarize the document I attach. Highlight decisions, risks, and next steps.'),
    },
    {
      description: i18n.tr('Finn et pålitelig svar med sporbare kilder.', 'Find a reliable answer with traceable sources.'),
      label: i18n.tr('Undersøk med kilder', 'Research with sources'),
      prompt: i18n.tr('Undersøk dette spørsmålet grundig og vis hvilke kilder som støtter konklusjonen.', 'Research this question thoroughly and show which sources support the conclusion.'),
    },
    {
      description: i18n.tr('Gå fra idé til et nyttig førsteutkast.', 'Turn an idea into a useful first draft.'),
      label: i18n.tr('Lag et førsteutkast', 'Create a first draft'),
      prompt: i18n.tr('Lag et tydelig førsteutkast som jeg kan gjennomgå og forbedre.', 'Create a clear first draft that I can review and improve.'),
    },
    {
      description: i18n.tr('Prioriter nye hendelser og åpne saker.', 'Prioritize recent events and open cases.'),
      label: i18n.tr('Lag en dagsbrief', 'Prepare a daily brief'),
      prompt: i18n.tr('Lag en kort dagsbrief fra tilgjengelige varsler og åpne saker. Skill mellom bekreftede fakta, mulige risikoer og anbefalte neste steg. Si tydelig fra hvis en kilde ikke er tilgjengelig.', 'Prepare a short daily brief from available alerts and open cases. Separate confirmed facts, possible risks, and recommended next steps. Say clearly if a source is unavailable.'),
    },
    {
      description: i18n.tr('Samle fremdrift, avvik og beslutninger.', 'Bring progress, exceptions, and decisions together.'),
      label: i18n.tr('Lag en statusrapport', 'Create a status report'),
      prompt: i18n.tr('Lag en statusrapport for arbeidet jeg beskriver. Bruk bare tilgjengelige kilder, vis hvor opplysningene kommer fra, og marker det som må avklares før rapporten kan deles.', 'Create a status report for the work I describe. Use only available sources, show where the information comes from, and mark what needs clarification before the report can be shared.'),
    },
    {
      description: i18n.tr('Finn avvik før de blir liggende.', 'Find exceptions before they linger.'),
      label: i18n.tr('Se etter avvik', 'Review exceptions'),
      prompt: i18n.tr('Gå gjennom dokumentene og tallene jeg legger ved for avvik, manglende informasjon og mulige feil. Ranger funnene etter konsekvens, vis kilder og foreslå hva som bør kontrolleres videre. Ikke endre noe uten godkjenning.', 'Review the documents and figures I attach for exceptions, missing information, and possible errors. Rank findings by impact, show sources, and suggest what to check next. Do not change anything without approval.'),
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
          <h1>{greeting()}</h1>
          <Show when={suggestionStatus() === 'ready' && suggestions().length === 0}>
            <p>{scopeDescription()}</p>
          </Show>
          <Show when={suggestionStatus() === 'ready' && suggestions().length > 0}>
            <p>{i18n.tr('Dette kan trenge oppfølging', 'This may need follow-up')}</p>
          </Show>
          <Show when={suggestionStatus() === 'unavailable'}>
            <p>{i18n.tr('Jeg får ikke hentet hendelser akkurat nå. Du kan fortsatt stille et spørsmål.', "I can't load recent activity right now. You can still ask a question.")}</p>
          </Show>
          <Show when={suggestions().length > 0}>
            <div class="verevon-chat-empty__activity" role="group" aria-label={i18n.tr('Forslag basert på ny aktivitet', 'Suggestions from recent activity')}>
              <section class="verevon-chat-empty__activity-source">
                <button type="button" class="verevon-chat-empty__source-toggle verevon-chat-empty__source-toggle--inbox" aria-expanded={inboxOpen() ? 'true' : 'false'} aria-controls={inboxPanelId} onClick={() => setInboxOpen((open) => !open)}>
                  <span>{i18n.tr('Innboks', 'Inbox')}</span><ChevronDown size={14} aria-hidden="true" />
                </button>
                <Show when={inboxOpen()}>
                  <div class="verevon-chat-empty__source-items" id={inboxPanelId}>
                    <Show when={inboxSuggestions().length} fallback={<p class="verevon-chat-empty__source-empty">{i18n.tr('Ingen innbokssaker akkurat nå.', 'No inbox cases right now.')}</p>}>
                      <For each={inboxSuggestions()}>{suggestionRow}</For>
                    </Show>
                  </div>
                </Show>
              </section>
              <section class="verevon-chat-empty__activity-source">
                <button type="button" class="verevon-chat-empty__source-toggle" aria-expanded={otherOpen() ? 'true' : 'false'} aria-controls={otherPanelId} onClick={() => setOtherOpen((open) => !open)}>
                  <span>{i18n.tr('Annet', 'Other')}</span><ChevronDown size={14} aria-hidden="true" />
                </button>
                <Show when={otherOpen()}>
                  <div class="verevon-chat-empty__source-items" id={otherPanelId}>
                    <Show when={otherSuggestions().length} fallback={<p class="verevon-chat-empty__source-empty">{i18n.tr('Ingen andre forslag akkurat nå.', 'No other suggestions right now.')}</p>}>
                      <For each={otherSuggestions()}>{suggestionRow}</For>
                    </Show>
                  </div>
                </Show>
              </section>
            </div>
          </Show>
          {/* The suggested first move. Plan item 17 / designpixil's "two
              sentences and a suggestion": the scope line above says what
              Verevon can reach, this says what to do with it right now.
              Sourced from real state — the most recent thread when there is
              one — because a suggestion the product invented is just another
              generic prompt, and there are three of those below already.

              It OPENS that thread. It used to type a prompt naming the
              thread's title into the new conversation instead, so the model
              was asked to continue a conversation it had never been given —
              and answered, correctly, that it did not have it. The
              affordance promised continuity the wire never carried.
              `selectChatThread` fires the same event the sidebar's thread
              rows do, so this reuses the one working resume path rather than
              adding a second. */}
          <Show when={suggestionStatus() === 'ready' && suggestions().length === 0 ? resumeTarget() : null}>
            {(target) => (
              <button
                type="button"
                class="verevon-chat-empty__resume"
                onClick={() => props.onResumeThread(target().threadId)}
              >
                <ArrowRight size={14} aria-hidden="true" />
                <span>
                  {i18n.tr('Fortsett der du slapp', 'Pick up where you left off')}
                  <small>{target().title}</small>
                </span>
              </button>
            )}
          </Show>
        </div>
        <div class="verevon-chat-empty__composer">{props.children}</div>
        <Show when={suggestionStatus() === 'ready'}>
          <div class="verevon-chat-empty__prompts" id="chat-appearance-prompt-ideas">
            <button type="button" class="verevon-chat-empty__examples-toggle" aria-expanded={ideasOpen() ? 'true' : 'false'} aria-controls="chat-appearance-prompt-list" onClick={() => setIdeasOpen((open) => !open)}>
              {ideasOpen() ? i18n.tr('Skjul eksempler', 'Hide examples') : i18n.tr('Vis eksempler', 'Show examples')}
              <ChevronRight size={14} aria-hidden="true" />
            </button>
            <Show when={ideasOpen()}>
              <div class="verevon-chat-empty__prompt-list" id="chat-appearance-prompt-list">
                <For each={starterPrompts()}>
                  {({ label, description, prompt }) => (
                    <button
                      type="button"
                      class="verevon-chat-starter"
                      aria-label={label}
                      onClick={() => props.onSelectPrompt(prompt)}
                    >
                      <span class="verevon-chat-starter__copy"><strong>{label}</strong><small>{description}</small></span>
                    </button>
                  )}
                </For>
              </div>
            </Show>
          </div>
        </Show>
        <p class="verevon-chat-empty__trust">{i18n.tr('Organisasjonens kunnskap er standard. Nettbruk er alltid synlig og valgfri.', 'Organisation knowledge is the default. Web access is always visible and optional.')}</p>
      </div>
    </div>
  )
}
