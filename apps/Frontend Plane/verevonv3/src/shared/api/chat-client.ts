import { ApiError, apiErrorFromResponse, requestJson, signalSessionExpiry } from './http'
import { gatewayBaseUrl } from './config'
import {
  normalizePlan,
  planStepEditToWire,
  type Plan,
  type PlanStepEdit,
} from './orchestration-client'

import { readSseStream, type SseEvent } from './sse'
import { createDeltaCoalescer, type DeltaCoalescer } from './delta-coalescer'
import type { ChatEventPayloads } from './chat-event-payloads.generated'
import { isSpaceDenial } from './space-denials'
import { isZdrCode } from './zdr-refusals'
import { normalizeConnectorScope } from '@/shared/actions/chat-connector-scope'
import { normalizeContextRefs } from '@/shared/actions/chat-context-refs'
import {
  isSelectablePrivacyTier,
  normalizeAnswerServedBy,
  normalizePrivacyTier,
  privacyTierWireValue,
  type AnswerServedBy,
  type PrivacyTier,
} from './privacy-tier'
import { createSelectedAgentToolSpecs } from '@/shared/actions/agent-tools'
import { parseChatHistoryAction, threadExportInput } from '@/shared/actions/chat-history'
import { isSupportChatThread } from '@/shared/chat/support-chat-thread'
import { isLocale, localeIntl, localeStorageKey, type Locale } from '@/shared/i18n/locales'
import { isAgUiEventName, toVerevonUiEvent, type VerevonUiEvent } from '@/shared/chat/verevon-ui-events'
import {
  applyServerRetention,
  forgetThreadLocally,
  reconcileLocalThreads,
} from '@/features/chat/lib/chat-retention'
import { runParkedFrameOf, type RunParkedFrame } from '@/features/chat/lib/run-park'
import type { ThreadGoalStatus } from './thread-goal-client'

// ── Wide research (EP-2 S8) ─────────────────────────────────────────────────

/** The count presets of the "Bred forskning" entry; 16 is the platform's hard maximum. */
export const WIDE_RESEARCH_PRESETS = [5, 10, 16] as const

/** The most items the entry sends; model-gateway reads no more than this many either. */
export const WIDE_RESEARCH_MAX_ITEMS = 64

/** The longest item the entry sends; model-gateway cuts at the same length. */
export const WIDE_RESEARCH_MAX_ITEM_CHARS = 500

/** What the person asked a wide-research run to cover: a pasted list, or a count preset. */
export type WideResearchStart = {
  /** The person's ask: a preset count, or the number of listed items when omitted. */
  itemCap?: number
  /** One research question per entry, in the order given. */
  items?: string[]
  /** A one-item retry (EP-2 S9): the child run whose item runs again; execution-core reads the item back. */
  retryOfChildRunId?: string
  /** The fan-out batch the retry joins. */
  batchId?: string
}

/**
 * The items of a pasted list: one per line, trimmed, blank lines skipped, each cut to
 * {@link WIDE_RESEARCH_MAX_ITEM_CHARS} characters and at most {@link WIDE_RESEARCH_MAX_ITEMS} kept.
 */
export function parseWideResearchItems(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .slice(0, WIDE_RESEARCH_MAX_ITEMS)
    .map((line) => line.slice(0, WIDE_RESEARCH_MAX_ITEM_CHARS))
}

/** One answer the person gave to a deep-research plan question (EP-2 S5). */
export type ResearchPlanAnswer = { question: string; answer: string }

/** The plan a deep-research run turn sends back (EP-2 S5): the edited sub-queries and the answers. */
export type ResearchPlanEdit = { subQueries: string[]; answers: ResearchPlanAnswer[] }

/** The editable plan a deep-research plan turn ends with (`research_plan`, EP-2 S5). */
export type ChatResearchPlanEvent = {
  subQueries: string[]
  questions: string[]
  /** The most sub-queries a run turn keeps; the card stops adding at it. */
  maxSubQueries: number
  requestId?: string
}

/** The cap the gateway ships by default, for a frame that names none. */
export const RESEARCH_PLAN_DEFAULT_MAX_SUB_QUERIES = 9

function researchPhaseWire(
  phase: 'plan' | 'run',
  plan: ResearchPlanEdit | undefined,
): { research_phase: 'plan' | 'run'; research_plan?: { sub_queries: string[]; answers: ResearchPlanAnswer[] } } {
  if (phase === 'plan') return { research_phase: 'plan' }
  return {
    research_phase: 'run',
    research_plan: {
      sub_queries: (plan?.subQueries ?? []).map((query) => query.trim()).filter((query) => query.length > 0),
      answers: (plan?.answers ?? [])
        .map(({ question, answer }) => ({ question: question.trim(), answer: answer.trim() }))
        .filter((entry) => entry.answer.length > 0),
    },
  }
}

function stringList(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string' && item.trim().length > 0)
    : []
}

function wideResearchWire(
  start: WideResearchStart,
): { item_cap?: number; items?: string[] } | { retry_of_child_run_id: string; batch_id?: string } {
  // A retry names the failed child run only: the item itself never leaves the browser again.
  if (start.retryOfChildRunId) {
    return {
      retry_of_child_run_id: start.retryOfChildRunId,
      ...(start.batchId ? { batch_id: start.batchId } : {}),
    }
  }
  const items = (start.items ?? []).slice(0, WIDE_RESEARCH_MAX_ITEMS)
  const itemCap =
    typeof start.itemCap === 'number' && Number.isInteger(start.itemCap) && start.itemCap > 0
      ? start.itemCap
      : undefined
  return {
    ...(itemCap !== undefined ? { item_cap: itemCap } : {}),
    ...(items.length > 0 ? { items } : {}),
  }
}

/** Model-gateway's estimate of a wide-research run, before it starts. A range, never a promise. */
export type WideResearchEstimate = {
  itemsRequested: number
  itemsAfterCap: number
  concurrency: number
  roundsPerItem: number
  minutesLow: number
  minutesHigh: number
  /** Null on a subscription route or for a model the price catalogue does not name. */
  usdLow: number | null
  usdHigh: number | null
  costBasis: 'metered' | 'subscription'
  cap: number
  capSource: 'org_policy' | 'platform_default'
}

function wideResearchUsd(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function wideResearchCount(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? Math.floor(value) : 0
}

/** Reads the estimate answer; a field of the wrong type reads as zero or null, never as a guess. */
export function normalizeWideResearchEstimate(raw: unknown): WideResearchEstimate {
  const value = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>
  return {
    itemsRequested: wideResearchCount(value.items_requested),
    itemsAfterCap: wideResearchCount(value.items_after_cap),
    concurrency: wideResearchCount(value.concurrency),
    roundsPerItem: wideResearchCount(value.rounds_per_item),
    minutesLow: wideResearchCount(value.minutes_low),
    minutesHigh: wideResearchCount(value.minutes_high),
    usdLow: wideResearchUsd(value.usd_low),
    usdHigh: wideResearchUsd(value.usd_high),
    costBasis: value.cost_basis === 'subscription' ? 'subscription' : 'metered',
    cap: wideResearchCount(value.cap),
    capSource: value.cap_source === 'org_policy' ? 'org_policy' : 'platform_default',
  }
}

/**
 * `POST /api/v1/agents/wide-research/estimate`: what a wide-research run of `itemCount` items
 * would look like. Nothing is stored or started. The gateway answers 404 while the entry is off and
 * a typed `agentic` refusal for a zero-retention caller.
 */
export async function estimateWideResearch(input: {
  itemCount: number
  model?: string
  provider?: string
  subscriptionConnectionId?: string
}): Promise<WideResearchEstimate> {
  const raw = await requestJson<unknown>('/api/v1/agents/wide-research/estimate', {
    method: 'POST',
    body: JSON.stringify({
      item_count: Math.max(0, Math.floor(input.itemCount)),
      ...(input.model ? { model: input.model } : {}),
      ...(input.provider ? { provider: input.provider } : {}),
      ...(input.subscriptionConnectionId
        ? { subscription_connection_id: input.subscriptionConnectionId }
        : {}),
    }),
  })
  return normalizeWideResearchEstimate(raw)
}

// ── Wire contract ───────────────────────────────────────────────────────────
// `truncated` is set when a workbook was cut on a row boundary to fit one turn;
// the cut is also marked inside `content`, where the model reads it.
export async function extractChatDocument(filename: string, content_base64: string, content_type: string): Promise<{ content: string; truncated?: boolean }> {
  return requestJson('/api/v1/chat/documents/extract', {
    method: 'POST', body: JSON.stringify({ filename, content_base64, content_type }),
  })
}

// The gateway forwards the chat body verbatim to model-gateway `/v1/invoke/*`,
// whose `InvokeRequest` is snake_case with `tools: ToolSpec[]`. This client owns
// the logical→wire translation so callers stay ergonomic (camelCase + intent).

export type ChatToolSpec = {
  name: string
  description?: string
  /** JSON Schema for the tool's parameters, as a JSON string. */
  parametersJson?: string
}

/** A specialized action activated via the composer "/" menu. */
export type ChatAction = {
  id: string
  name: string
  kind: 'skill' | 'capability' | 'connector' | 'tool'
}

export type ChatAttachment = {
  data_base64?: string
  kind?: string
  mime_type?: string
  url?: string
}

/** The four agent modes a person can choose for a turn (WS25-S6). The ids match `api/contracts/agent-modes.json`. */
export type AgentModeId = 'lockdown' | 'read_only' | 'ask_first' | 'auto'

export type ChatInvokeRequest = {
  sourceScope?: import('@/shared/actions/chat-source-scope').ChatSourceScope
  /**
   * Per-turn retrieval-source filter (chat-wave2 scope spec), orthogonal to
   * `sourceScope`. Omitted or `'auto'` reproduces today's behavior
   * byte-identically; forced to `'auto'`/omitted on the wire whenever
   * `sourceScope === 'conversation'`, which already excludes both families.
   */
  retrievalScope?: import('@/shared/actions/chat-source-scope').RetrievalScope
  /**
   * The connected accounts this turn may read (WS23 S6), picked in the composer's
   * connected-apps chips. Sent as `connector_scope` only when non-empty and the turn
   * can take it: `buildChatWireBody` withholds it on the turns the composer blocks.
   */
  connectorScope?: string[]
  /**
   * The typed context references attached with the composer's chips (EP-3 S5). Sent as
   * `context_refs` only when non-empty, and never on a zero-retention or temporary turn or a
   * subscription route: the gateway refuses those, so this client never sends what it would refuse.
   */
  contextRefs?: import('@/shared/actions/chat-context-refs').ChatContextRef[]
  /**
   * The tabs the gateway resolved for the turn's `@tab` mentions (E3.3-c), from
   * `resolveBrowserTabMentions`. Context references only: no page content and no grant. Sent as
   * `browser_tab_refs` only when non-empty, and never on a zero-retention, conversation-only,
   * Support-read-only or subscription turn.
   */
  browserTabRefs?: import('@/shared/api/browser-client').BrowserTabContextRef[]
  /** The thread goal a Continue turn works toward (EP-7 S1). Sent as `goal_id` only when set, never on a zero-retention turn. */
  goalId?: string
  /**
   * The goal pump sent this Continue turn after its countdown, not the person (EP-7 S4). Sent as
   * `goal_auto` only beside a `goal_id`; it grants nothing and the gateway refuses it while
   * auto-continue is off.
   */
  goalAuto?: boolean
  content: string
  model?: string
  /** Explicit provider route for a user-owned model subscription. */
  provider?: string
  /** Opaque Integration Core connection id; never a ChatGPT token. */
  subscriptionConnectionId?: string
  threadId?: string
  /** The requested personal Space for a new durable chat thread. The BFF
   * exchanges this selection for a Control-signed, effect-bound decision. */
  spaceRef?: string
  /**
   * The viewer's interface language (WS16-S6), a plain framing hint for the model on a Space turn.
   * Request-scoped: the gateway maps `no` to `nb-NO` and `en` to `en-US` (anything else is dropped,
   * and the field is removed while its flag is off) and model-gateway never stores it. It decides
   * nothing about access.
   */
  locale?: 'no' | 'en'
  /**
   * The Control `subject_id` of a Space agent addressed by `@` mention this
   * turn (`docs/space-defenition.md`, "Invocation rule"). The gateway is the
   * one that decides whether this agent may actually answer — it re-resolves
   * Control membership and the Application binding before injecting a
   * persona; a mention naming an unbound or inactive agent is rejected, never
   * silently ignored or silently granted. Meaningless without `spaceRef`.
   */
  mentionedAgentRef?: string
  /**
   * WS08: wake the one agent in `mentionedAgentRef` from a stored human post. `messageId` names the
   * post (the trigger; its text must be this request's `content`) and `clientWakeId` is this wake's
   * own retry identity. Needs `threadId`. The gateway signs and Session Core verifies the wake; the
   * browser only names the post.
   */
  wake?: { messageId: string; clientWakeId: string }
  sessionKey?: string
  features?: string[]
  generateImage?: boolean
  browseWeb?: boolean
  /** Deep research mode: gateway runs plan -> concurrent searches -> page reads -> a cited report. */
  deepResearch?: boolean
  /**
   * Which turn of a two-turn deep research this is (EP-2 S5): `plan` ends with an editable plan and
   * searches nothing, `run` runs `researchPlan`. Sent only with `deepResearch` on a turn that keeps
   * data; model-gateway ignores both while DEEP_RESEARCH_PLAN_TURN is off and re-clamps the plan.
   */
  researchPhase?: 'plan' | 'run'
  /** The edited plan a `run` turn carries. */
  researchPlan?: ResearchPlanEdit
  /**
   * Wide research from the composer's "Bred forskning" entry (EP-2 S8): the items to research, or
   * only a count when the model should split the question itself. Sent as `wide_research` only on
   * an agentic turn that keeps data (never zero-retention); model-gateway ignores it while
   * WIDE_RESEARCH_ENTRY_ENABLED is off and clamps the count to the org's item cap.
   */
  wideResearch?: WideResearchStart
  /**
   * The brand deck template picked in `DeckTemplatePicker` (EP-5 S7): a skill id from
   * `listDeckTemplates`, never a name. Sent as `deck_template` only when set and the turn keeps data
   * (never zero-retention, a subscription route, support or conversation-only); model-gateway ignores
   * it while SKILL_DECK_TEMPLATES_ENABLED is off and checks it against the org's templates otherwise.
   */
  deckTemplate?: string
  /**
   * Durable ids of earlier messages the user pinned into context.
   *
   * Ids only — never the pinned text. model-gateway resolves each one
   * against the durable thread and re-expresses the match as leading
   * `system` context, where neither history shedder reaches it. Sending
   * content instead would let this browser assert that the user said
   * something earlier; sending an id can only ever SELECT a message that
   * already exists.
   */
  pinnedMessageIds?: string[]
  /**
   * Extended-thinking effort: `quick` | `standard` | `deep`.
   *
   * The gateway derives the token budget from this profile — the client picks an
   * effort, never a raw budget. `standard` (and unset) requests no thinking, so
   * the ordinary turn is unchanged; only `deep`/`quick` cost the extra latency
   * and tokens. Reasoning arrives on `reasoning_delta`, which requires the
   * `reasoning` feature (already in `DEFAULT_FEATURES`).
   *
   * A canonical {@link EffortLevel} is accepted too: the gateway takes one
   * `effort` string and reads either vocabulary. Only a level the selected
   * model's catalogue entry lists (`modelEffortLevels`) is ever sent.
   */
  effort?: EffortProfile | EffortLevel
  /**
   * Response-style dial, orthogonal to `effort`: how much prose the answer
   * spends, not how hard the model thinks.
   *
   * The gateway maps this to a short system directive (`crate::verbosity`);
   * `balanced` is deliberately not sent, because an absent value already means
   * "the model's natural verbosity" there. The composer has offered this
   * setting all along but never transmitted it — it was read once, to draw its
   * own checkmark.
   */
  verbosity?: 'concise' | 'balanced' | 'detailed'
  attachments?: ChatAttachment[]
  /** Explicit tool definitions (advanced); usually derived from actions/browseWeb. */
  tools?: ChatToolSpec[]
  /** Skills/capabilities/connectors activated in the composer. */
  actions?: ChatAction[]
  profile?: string
  zdr?: boolean
  /**
   * Plan mode — when true, opt into the Model Plane's agentic run path so the
   * agent plans + executes tools under human-approval gates (risky tools pause
   * for Approve/Reject in chat). Adds the `agentic` feature family.
   */
  planMode?: boolean
  /**
   * The agent mode this turn runs under, when the person chose one (WS25-S6). Sent as `agent_mode` only
   * when set, so an unchanged turn is byte-identical; the server refuses an id it does not know and can
   * only tighten the posture the platform already sets.
   */
  agentMode?: AgentModeId
  /**
   * This turn runs the plan the person just approved (WS14 S2a "Run the
   * approved plan"). Only the chat controller sets it, and only after it has
   * checked that a grant for this thread is armed and usable. It carries NO rung:
   * model-gateway runs the turn under the grant it holds for the thread and
   * refuses it typed (`plan_grant_unavailable`, `plan_execute_disabled`) when it
   * cannot. Dropped here, with no `agentic` feature, when the turn has no
   * thread, is a plan turn, is conversation-only, Support-derived, zero-retention
   * or on the subscription route, so it can never ride on a turn that cannot
   * execute. Regenerate, edit-resubmit, continue-generation and retry never set it.
   */
  executeGranted?: boolean
  /**
   * The plan this execute turn runs (WS14 S2, approve then execute): sent only with
   * `executeGranted`, and only for the turn a plan approval answered `execute_now` for. The
   * gateway binds the thread's grant to the run only when that grant names this plan, so a
   * stale or replaced approval refuses the turn (`plan_grant_unavailable`) instead of running it.
   */
  approvedPlanId?: string
  /**
   * The id this send is named with (the receipt). Sent as `idempotency_key`, which the gateway
   * already reads as the retry identity of a send; with send receipts on it also stores the
   * user's message under it, so a repeat of the send is one transcript row. Never sent for a
   * zero-retention turn.
   */
  clientMessageId?: string
  /**
   * This turn is a REGENERATE of the previous answer, not a new question.
   *
   * Client-declared because the server cannot infer it: a regenerate arrives as
   * an ordinary turn carrying the same text. model-gateway feeds it to the
   * implicit-dissatisfaction classifier, where it is deliberately WEAK evidence
   * — a regenerate often just means "give me another style" — so asserting it
   * can only nudge one skill's score, never condemn it.
   */
  regenerated?: boolean
  /** This turn is an EDITED resubmit of the previous question. Same reasoning. */
  editResubmit?: boolean
  /**
   * The answer this turn regenerates, by its server message id (WS17 S9). Sent only on a versioned
   * thread, where the regenerate moves the branch instead of appending the question again.
   */
  regenerateOfMessageId?: string
  /** The user turn this turn edits, by its server message id (WS17 S9). Sent only on a versioned thread. */
  editOfMessageId?: string
  /**
   * Minimum Venice-style privacy tier this turn may be served by
   * (`min_privacy_tier` on the wire). Enforcement is server-side: ineligible
   * providers are skipped and an empty remainder fails closed — never a silent
   * downgrade. OMITTED from the wire unless explicitly set: unspecified means
   * today's behavior byte-identically.
   */
  minPrivacyTier?: PrivacyTier
}

// ── SSE events (mapped from model-gateway's real event names) ────────────────

export type ChatConnectedEvent = {
  ok?: boolean
  requestId?: string
  threadId?: string
  model?: string
  /** Durable run id. Work surfaces only use it when the turn is plan/research. */
  runId?: string
  /**
   * The server keeps a message tree for conversations (WS17 S9), so a regenerate or an edit may name its
   * turn. Absent while the server's switch is off.
   */
  versionsEnabled?: boolean
}
export type ChatMessageEvent = { content: string; requestId?: string }
export type ChatDoneEvent = {
  requestId?: string
  modelUsed?: string
  outputTokens?: number
  /**
   * Why generation stopped, when the backend reported it. `'stream_incomplete'`
   * means the provider connection broke before any proper termination signal —
   * the answer may be truncated. See model_plane.v1.InferChunk.stop_reason.
   */
  stopReason?: string
}
/**
 * The run was stopped before finishing. Distinct from `onDone` on purpose: a
 * server-side stop used to be routed to `onDone`, which rendered a halted
 * answer as a normally-completed one.
 */
export type ChatStoppedEvent = { requestId?: string; reason?: string }
/**
 * Long-term memory was injected into this turn's prompt (Model Plane's
 * `memory_recall` event, `memory` feature family). Emitted at most once per
 * turn and only when memory was genuinely recalled, so `count` is never 0.
 */
/**
 * How a recalled memory came to exist.
 *
 * THREE states, not a boolean. `unrecorded` covers rows written before
 * provenance was tracked, and the backend contract is explicit that those must
 * never be presented as `stated` — saying "you told me this" about a row that
 * does not record it manufactures consent. Any value this build does not
 * recognise is read as `unrecorded` for the same reason.
 */
export type MemoryOrigin = 'stated' | 'inferred' | 'unrecorded'

/** Whether the memory came from another conversation or is org-level context. */
export type MemoryRole = 'recall' | 'inject'

export type RecalledMemory = {
  /** Stable id, so the reader can act on this exact row. */
  memoryId: string
  role: MemoryRole
  origin: MemoryOrigin
  label: string
  preview: string
}

export type ChatMemoryRecallEvent = {
  count: number
  latencyMs?: number
  /** What was recalled. Empty when the backend reported only a count. */
  memories: RecalledMemory[]
}

/** What the sources said about a low-scoring answer. */
export type VerificationVerdict = 'supports' | 'contradicts' | 'unrelated'

/**
 * The backend went looking for backing after scoring this answer low, and this
 * is what it found. Emitted only for turns that actually ran the check, so its
 * absence means "not checked" — which is why the verdict is worth showing at
 * all: a score that moved silently cannot be told apart from one that never
 * had anything behind it.
 */
export type ChatVerificationEvent = {
  verdict: VerificationVerdict
  kbCitations: number
  webCitations: number
  /** Whether the check was allowed to escalate past the knowledge base. */
  webAllowed: boolean
}

/**
 * Mid-run user messages the agent has just been handed. `ids` (additive, send receipts on)
 * holds the `client_message_id` of each message in `messages`, in order, with an empty string
 * for one sent without an id; absent when the gateway names none of them.
 */
export type ChatQueuedInputEvent = { messages: string[]; ids?: string[] }
/**
 * An answer produced in a reduced form (`retrieval_notice`). `code` is the stable reason and `wordingKey` the
 * key the client's wording is filed under: the gateway sends no sentence, so the text is the client's to write.
 */
export type ChatRetrievalNoticeEvent = { code: string; wordingKey: string; requestId?: string }
/**
 * A tool call is being retried after a transient failure (`retry_scheduled`, `steps` family). Counts and
 * closed tokens only: the error text never reaches the client. An unknown `reason` is passed through as-is.
 */
export type ChatRetryScheduledEvent = {
  scope: string
  tool?: string
  attempt: number
  maxAttempts: number
  waitMs?: number
  reason: string
  requestId?: string
}
/**
 * The history or tool results were compacted to fit the model (`compaction`, control). `outcome` says what
 * happened, so a summary that failed arrives as `truncated`, never as `summarized`.
 */
export type ChatCompactionEvent = {
  scope: string
  outcome: string
  messagesCompacted?: number
  messagesKept?: number
  pinnedKept?: number
  requestId?: string
}
/**
 * The context the model was given changed (`context_change`, control): history was shed, skill guidance was
 * cut short, or a run-context part was skipped. Counts and the gateway's own tokens only.
 */
export type ChatContextChangeEvent = {
  change: string
  messagesDropped?: number
  skillsTruncated?: number
  skillsDropped?: number
  layer?: string
  state?: string
  requestId?: string
}
/**
 * The workspace state of a tool call's sandboxed files (`sandbox_state`, `steps` family, WS22-S6): where
 * they were kept and whether they were. Closed tokens and an optional cause only; the gateway sends no path,
 * file name or content. An unknown `state` or `cause` is passed through as-is, as the lifecycle notices are.
 */
export type ChatSandboxStateEvent = {
  scope: string
  state: string
  cause?: string
  requestId?: string
}
/** The statuses a `goal_progress` frame may state: a goal's, or `cleared`. */
export type ChatGoalProgressStatus = ThreadGoalStatus | 'cleared'
const GOAL_PROGRESS_STATUSES: readonly ChatGoalProgressStatus[] = [
  'active',
  'paused',
  'met',
  'budget_reached',
  'turns_reached',
  'blocked',
  'cleared',
]
/**
 * A settled goal turn (`goal_progress`, EP-7 S2, ungated): the goal's spend against its caps and its
 * status after the turn. Ids, counts and closed tokens only; a budget the goal does not set is absent.
 */
export type ChatGoalProgressEvent = {
  goalId: string
  turnNo: number
  tokensUsed: number
  tokenBudget?: number
  costUsedUsdMicros: number
  costBudgetUsdMicros?: number
  turnsUsed: number
  maxTurns: number
  status: ChatGoalProgressStatus
  statusReason?: string
  requestId?: string
}
export type ChatErrorEvent = {
  code: string
  message: string
  retryable?: boolean
  /** Correlation preserved when an error belongs to a durable run. */
  runId?: string
  requestId?: string
  /**
   * How long, in whole milliseconds from when the frame was sent, until the
   * refusing service accepts a call again (a throttle, or a usage limit whose
   * reset is known). Content-free account metadata; absent when unknown.
   */
  retryAfterMs?: number
  /**
   * When the connected ChatGPT plan's usage limit resets, as the server's
   * RFC 3339 instant. Kept only when it parses, so a renderer never has to
   * re-validate it.
   */
  resetsAt?: string
  /**
   * For a zero-retention refusal that names a feature (`zdr_feature_not_supported`):
   * the feature's stable key, such as `web_search`. The SPA words it in the
   * reader's language (`zdr-refusals.ts`). Absent for every other error.
   */
  feature?: string
  /**
   * For a typed Space denial (`space-denials.ts`): the refusal's `details`
   * (`reason`, `resolvable_by` and the allowlisted keys), as a refused request
   * carried them. Absent for every other error.
   */
  details?: Readonly<Record<string, unknown>>
}
export type ChatArtifactEvent = {
  id?: string
  kind?: string
  title?: string
  content?: string
  version?: number
  /** What a research report was built from (EP-2-S4); only on a deep-research report, and only when the server sends it. */
  provenance?: ResearchProvenance
  /** Who made this revision (EP-3 S3); only while the server's revision history is on. */
  editor?: ArtifactEditorKind
}

/** Who made a stored artifact revision (EP-3 S3): the model, the person, or a restore of an older one. */
export type ArtifactEditorKind = 'model' | 'user' | 'restore'

function isArtifactEditorKind(value: unknown): value is ArtifactEditorKind {
  return value === 'model' || value === 'user' || value === 'restore'
}

/**
 * Where a knowledge citation's passage sits in Data Plane (EP-2-S4), in the wire shape, so a live frame and a
 * stored turn read the same. Ids only: the text is read again through Data Plane's own ACL'd chunk read.
 */
export type CitationLocator = { document_id: string; chunk_index: number }

/** The provenance stamp of a deep-research report (EP-2-S4), in the wire shape the gateway writes and stores. */
export type ResearchProvenance = {
  question: string
  sub_queries: string[]
  sources: { number: number; url: string; title: string; pass?: number }[]
  model: string
  coverage: { found: number; read: number; unread: number; deadline_hit: boolean }
  generated_at: string
  pipeline: string
  /** EP-2-S6: how many research passes ran (2 when the gap-driven second pass ran). */
  passes?: number
  /** EP-2-S6: the digest of the claim ledger below. */
  ledger_digest?: string
  /** EP-2-S6: the ledger's claims counted by status. */
  claims_by_status?: Record<string, number>
  /** EP-2-S6: the closed claim ledger, kept as the gateway sent it. */
  research_ledger?: unknown
}

/** A locator only when both halves have the right type; anything else is dropped, never guessed. */
export function parseCitationLocator(value: unknown): CitationLocator | undefined {
  const record = objectValue(value)
  const documentId = str(record?.document_id)
  const chunkIndex = record?.chunk_index
  if (!documentId || typeof chunkIndex !== 'number' || !Number.isInteger(chunkIndex) || chunkIndex < 0) return undefined
  return { document_id: documentId, chunk_index: chunkIndex }
}

/** A provenance stamp only when its shape is complete; a partial one is dropped rather than shown half-filled. */
export function parseResearchProvenance(value: unknown): ResearchProvenance | undefined {
  const record = objectValue(value)
  const coverage = objectValue(record?.coverage)
  if (!record || !coverage) return undefined
  const question = str(record.question)
  const model = str(record.model)
  const generatedAt = str(record.generated_at)
  const pipeline = str(record.pipeline)
  const counts = [coverage.found, coverage.read, coverage.unread]
  if (question === undefined || model === undefined || !generatedAt || !pipeline) return undefined
  if (!counts.every((count) => typeof count === 'number') || typeof coverage.deadline_hit !== 'boolean') return undefined
  const subQueries = Array.isArray(record.sub_queries)
    ? record.sub_queries.filter((entry): entry is string => typeof entry === 'string')
    : []
  const sources = (Array.isArray(record.sources) ? record.sources : []).flatMap((entry) => {
    const source = objectValue(entry)
    const number = source?.number
    const url = str(source?.url)
    if (typeof number !== 'number' || url === undefined) return []
    return [{ number, url, title: str(source?.title) ?? '', ...(typeof source?.pass === 'number' ? { pass: source.pass } : {}) }]
  })
  return {
    question,
    sub_queries: subQueries,
    sources,
    model,
    coverage: {
      found: coverage.found as number,
      read: coverage.read as number,
      unread: coverage.unread as number,
      deadline_hit: coverage.deadline_hit as boolean,
    },
    generated_at: generatedAt,
    pipeline,
    ...(typeof record.passes === 'number' ? { passes: record.passes } : {}),
    ...(str(record.ledger_digest) ? { ledger_digest: str(record.ledger_digest) } : {}),
    ...(objectValue(record.claims_by_status)
      ? { claims_by_status: objectValue(record.claims_by_status) as Record<string, number> }
      : {}),
    ...(record.research_ledger !== undefined ? { research_ledger: record.research_ledger } : {}),
  }
}
export type ChatReasoningEvent = { delta: string }
export type ChatToolCallEvent = { id?: string; name?: string; args?: unknown }
export type ChatToolResultEvent = {
  id?: string
  output?: string
  error?: string
  status?: string
}
export type ChatCitationEvent = {
  id?: string
  title?: string
  url?: string
  snippet?: string
  claimId?: string
  sourceGroupId?: string
  sourceGroupLabel?: string
  start?: number
  end?: number
  /** The server's `[n]` for this source (see `Citation.number`); a chip binds by it, not by position. */
  number?: number
  /** See `Citation.origin` (chat-wave2 scope spec §2.2). */
  origin?: string
  /** The passage this source contributed (EP-2-S4), when the server sends one. */
  quote?: string
  /** Where a knowledge source's passage sits in Data Plane (EP-2-S4), when the server sends one. */
  locator?: CitationLocator
}
export type ChatGroundingEvent = { value: unknown }
export type ChatStepEvent = {
  id?: string
  title?: string
  detail?: string
  status?: string
  /**
   * The SSE `id:` of the frame that carried this step, when it had one. A step
   * the server sent without an id of its own is keyed on it, so the same frame
   * replayed by a resume updates the step it created instead of adding a twin.
   */
  frameId?: string
}
export type ChatAttachmentEvent = {
  id?: string
  name?: string
  mime?: string
  type?: string
  url?: string
  size?: number
  /** The gateway's structural check of an Office or PDF file, as sent; the normalizer validates it. */
  check?: unknown
  preview?: unknown
}
/**
 * How a turn's cost was reached. `subscription` means a user-owned model
 * subscription carried the provider call: the gateway computes no list-price
 * cost for it and keeps `cost_usd` at 0.0 only for wire compatibility, so that
 * zero is "included in the subscription", never "free". Absent on older
 * gateways, where the reported cost stays as it always was.
 */
export type ChatCostBasis = 'metered' | 'subscription'

export type ChatUsageEvent = {
  inputTokens?: number
  outputTokens?: number
  costUsd?: number
  costBasis?: ChatCostBasis
  latencyMs?: number
  confidence?: number
  cacheReadTokens?: number
  cacheWriteTokens?: number
  /** Model calls the total covers: tool-decision rounds, artifact checks, overhead and the answer. Absent on an older gateway. */
  calls?: number
  /** Of `calls`, the metered ones whose price was unknown. A total with any has a null `costUsd`. */
  unpricedCalls?: number
  /** The cost split by what was billed, present exactly when the total was priced. */
  costByCategory?: ChatCostByCategory
  /**
   * Where the answer was processed, exactly as the gateway stamped it
   * (`provider_used`, `residency`, `privacy_tier`). Absent on an older gateway
   * and on a turn no provider served: the UI then renders nothing, and never
   * fills the gap from a model name.
   */
  servedBy?: AnswerServedBy
}

export type ChatCostByCategory = {
  inputUsd: number
  cacheReadUsd: number
  cacheWriteUsd: number
  outputUsd: number
}

/** The provider call a progress frame reports, as its closed phase token. */
export type ChatUsagePhase = 'answer' | 'tool_round' | 'agent_round' | 'overhead'

/**
 * The turn's running total after one of its provider calls (`usage_progress`, `usage` family).
 *
 * The gateway streams one per call while the turn runs. The terminal `usage` frame stays the
 * authority: its totals replace every progress total of the turn. Counts and closed tokens only.
 */
export type ChatUsageProgressEvent = {
  /** Calls the total covers so far, the call this frame reports included. */
  calls?: number
  phase?: ChatUsagePhase
  inputTokens?: number
  outputTokens?: number
  cacheReadTokens?: number
  cacheWriteTokens?: number
  /**
   * The metered cost so far. Absent while a call is unpriced or the turn is a subscription's, and
   * `unpricedCalls` or `costBasis` then says which.
   */
  costUsd?: number
  /** Absent when the turn mixes metered and subscription calls. */
  costBasis?: ChatCostBasis
  /** Of `calls`, the metered ones whose price was unknown. Absent at zero. */
  unpricedCalls?: number
  /** The metered spend cap, when the turn has one (WS27-S6). */
  ceilingUsd?: number
  /** The metered token cap, when the turn has one (WS27-S6). */
  ceilingTokens?: number
  /** The share of the ceiling used, when a capped figure is known. Above 1 once the cap is passed. */
  fraction?: number
  /** The ceiling warning this frame is the first to carry. */
  warning?: 'run_ceiling_80' | 'run_ceiling_reached'
  /** How the budget moved this call's route (WS27-S7), stated only on a call it moved. */
  routing?: 'tier_shifted' | 'cheap_fallback'
}

function costBasisValue(value: unknown): ChatCostBasis | undefined {
  return value === 'metered' || value === 'subscription' ? value : undefined
}

/** The breakdown of a usage total: present only when all four parts are numbers. */
function costByCategoryValue(value: unknown): ChatCostByCategory | undefined {
  if (!value || typeof value !== 'object') return undefined
  const parts = value as Record<string, unknown>
  const inputUsd = num(parts.input_usd)
  const cacheReadUsd = num(parts.cache_read_usd)
  const cacheWriteUsd = num(parts.cache_write_usd)
  const outputUsd = num(parts.output_usd)
  if (inputUsd === undefined || cacheReadUsd === undefined || cacheWriteUsd === undefined || outputUsd === undefined) {
    return undefined
  }
  return { inputUsd, cacheReadUsd, cacheWriteUsd, outputUsd }
}

function usagePhaseValue(value: unknown): ChatUsagePhase | undefined {
  return value === 'answer' || value === 'tool_round' || value === 'agent_round' || value === 'overhead'
    ? value
    : undefined
}

function runCeilingWarningValue(value: unknown): 'run_ceiling_80' | 'run_ceiling_reached' | undefined {
  return value === 'run_ceiling_80' || value === 'run_ceiling_reached' ? value : undefined
}

function routingValue(value: unknown): 'tier_shifted' | 'cheap_fallback' | undefined {
  return value === 'tier_shifted' || value === 'cheap_fallback' ? value : undefined
}

/** A `usage_progress` payload, read by its closed keys and nothing else. */
function usageProgressFrame(payload: Record<string, unknown>): ChatUsageProgressEvent {
  return {
    calls: num(payload.calls),
    phase: usagePhaseValue(payload.phase),
    inputTokens: num(payload.input_tokens),
    outputTokens: num(payload.output_tokens),
    cacheReadTokens: num(payload.cache_read_tokens),
    cacheWriteTokens: num(payload.cache_write_tokens),
    costUsd: num(payload.cost_usd),
    costBasis: costBasisValue(payload.cost_basis),
    unpricedCalls: num(payload.unpriced_calls),
    ceilingUsd: num(payload.ceiling_usd),
    ceilingTokens: num(payload.ceiling_tokens),
    fraction: num(payload.fraction),
    warning: runCeilingWarningValue(payload.warning),
    routing: routingValue(payload.routing),
  }
}

/** The `servedBy` key of a usage event, present only when the server stamped something usable. */
function servedByField(payload: Record<string, unknown>): { servedBy?: AnswerServedBy } {
  const servedBy = normalizeAnswerServedBy(payload)
  return servedBy ? { servedBy } : {}
}

export type TurnTimingOutcome = 'completed' | 'failed' | 'cancelled' | 'timeout'

/** Provider-side timing for one provider call, as reported by Inference Core. */
export type TurnTimingProvider = {
  handlerMs: number
  providerMs: number
  firstTokenMs?: number
  attempts: number
  failedAttemptsMs?: number
  retryWaitMs?: number
  responseCacheHit: boolean
  upstreamMs?: Record<string, number>
}

/** One bounded span of a turn, in milliseconds from the gateway's turn origin. */
export type TurnTimingPhase = {
  phase: string
  startMs: number
  endMs: number
  round?: number
  pass?: number
  batch?: number
  attempt?: number
  outcome?: string
  tools?: string[]
  provider?: TurnTimingProvider
}

/**
 * Where a turn's server time went, from the gateway's `turn_timing` event.
 *
 * Emitted once per turn just before `done` (and before `error`/`stopped` on a
 * failed, timed-out or cancelled turn), in the same opt-in family as `usage`.
 * Every value is milliseconds on the gateway's own monotonic clock, measured
 * from the moment the request id was minted, so the numbers are comparable
 * with each other but never with a browser timestamp. It carries ids, names and
 * durations only — no prompt, message, tool argument or tool result.
 *
 * Only the three totals are required. A restored transcript keeps just those,
 * which is why the breakdown fields are optional here.
 */
export type ChatTurnTiming = {
  requestId?: string
  outcome?: TurnTimingOutcome
  /** Admission to the end of the turn, on the gateway. */
  serverMs: number
  /** Union of provider-call intervals; parallel review batches count once. */
  providerMs: number
  /** Server time spent outside provider calls. */
  orchestrationMs: number
  marks?: {
    connectedMs?: number
    sourceReadyMs?: number
    firstTokenMs?: number
    firstDraftMs?: number
    firstValidatedMs?: number
    durableMs?: number
  }
  counts?: {
    modelRounds: number
    providerCalls: number
    reviewPasses: number
    reviewBatches: number
    repairs: number
    reviewerEdits: number
  }
  tokensReported?: boolean
  phases?: TurnTimingPhase[]
  phasesDropped?: number
}

/** Mirrors the gateway's span bound; anything beyond it is counted, not kept. */
const MAX_TURN_TIMING_PHASES = 64

function millis(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : undefined
}

function countValue(value: unknown): number {
  return millis(value) ?? 0
}

function turnTimingProvider(value: unknown): TurnTimingProvider | undefined {
  const raw = objectValue(value)
  if (!raw) return undefined
  const handlerMs = millis(raw.handler_ms)
  const providerMs = millis(raw.provider_ms)
  if (handlerMs === undefined || providerMs === undefined) return undefined
  const upstream = objectValue(raw.upstream_ms)
  const upstreamMs = upstream
    ? Object.fromEntries(Object.entries(upstream).flatMap(([key, entry]) => {
        const ms = millis(entry)
        return ms === undefined ? [] : [[key, ms] as const]
      }))
    : undefined
  return {
    handlerMs,
    providerMs,
    ...(millis(raw.first_token_ms) !== undefined ? { firstTokenMs: millis(raw.first_token_ms) } : {}),
    attempts: countValue(raw.attempts),
    ...(millis(raw.failed_attempts_ms) !== undefined ? { failedAttemptsMs: millis(raw.failed_attempts_ms) } : {}),
    ...(millis(raw.retry_wait_ms) !== undefined ? { retryWaitMs: millis(raw.retry_wait_ms) } : {}),
    responseCacheHit: raw.response_cache_hit === true,
    ...(upstreamMs && Object.keys(upstreamMs).length > 0 ? { upstreamMs } : {}),
  }
}

function turnTimingPhase(value: unknown): TurnTimingPhase | null {
  const raw = objectValue(value)
  const phase = str(raw?.phase)?.trim()
  const startMs = millis(raw?.start_ms)
  const endMs = millis(raw?.end_ms)
  if (!raw || !phase || startMs === undefined || endMs === undefined) return null
  const optionalCount = (key: string) => {
    const count = millis(raw[key])
    return count === undefined ? {} : { [key]: count }
  }
  const tools = Array.isArray(raw.tools)
    ? raw.tools.filter((tool): tool is string => typeof tool === 'string' && tool.trim().length > 0)
    : []
  const provider = turnTimingProvider(raw.provider)
  return {
    phase,
    startMs,
    endMs: Math.max(startMs, endMs),
    ...optionalCount('round'),
    ...optionalCount('pass'),
    ...optionalCount('batch'),
    ...optionalCount('attempt'),
    ...(str(raw.outcome) ? { outcome: str(raw.outcome) } : {}),
    ...(tools.length > 0 ? { tools } : {}),
    ...(provider ? { provider } : {}),
  }
}

/**
 * Read a `turn_timing` payload leniently.
 *
 * Unknown fields are ignored and malformed phases dropped, so a newer gateway
 * cannot break an older client. Only a payload without the server and provider
 * totals is rejected (`null`): the metrics rows it exists for would otherwise
 * show a number the server never measured.
 */
export function parseTurnTiming(payload: Record<string, unknown>): ChatTurnTiming | null {
  const serverMs = millis(payload.server_ms)
  const providerMs = millis(payload.provider_ms)
  if (serverMs === undefined || providerMs === undefined) return null
  const outcome = str(payload.outcome)
  const marksRaw = objectValue(payload.marks) ?? {}
  const marks: NonNullable<ChatTurnTiming['marks']> = {}
  for (const [wire, key] of [
    ['connected_ms', 'connectedMs'],
    ['source_ready_ms', 'sourceReadyMs'],
    ['first_token_ms', 'firstTokenMs'],
    ['first_draft_ms', 'firstDraftMs'],
    ['first_validated_ms', 'firstValidatedMs'],
    ['durable_ms', 'durableMs'],
  ] as const) {
    const ms = millis(marksRaw[wire])
    if (ms !== undefined) marks[key] = ms
  }
  const countsRaw = objectValue(payload.counts) ?? {}
  const rawPhases = Array.isArray(payload.phases) ? payload.phases : []
  const phases = rawPhases
    .map(turnTimingPhase)
    .filter((phase): phase is TurnTimingPhase => phase !== null)
  return {
    requestId: str(payload.request_id),
    outcome: outcome === 'completed' || outcome === 'failed' || outcome === 'cancelled' || outcome === 'timeout'
      ? outcome
      : undefined,
    serverMs,
    providerMs,
    orchestrationMs: millis(payload.orchestration_ms) ?? Math.max(0, serverMs - providerMs),
    marks,
    counts: {
      modelRounds: countValue(countsRaw.model_rounds),
      providerCalls: countValue(countsRaw.provider_calls),
      reviewPasses: countValue(countsRaw.review_passes),
      reviewBatches: countValue(countsRaw.review_batches),
      repairs: countValue(countsRaw.repairs),
      reviewerEdits: countValue(countsRaw.reviewer_edits),
    },
    tokensReported: typeof payload.tokens_reported === 'boolean' ? payload.tokens_reported : undefined,
    phases: phases.slice(0, MAX_TURN_TIMING_PHASES),
    phasesDropped: countValue(payload.phases_dropped) + Math.max(0, phases.length - MAX_TURN_TIMING_PHASES),
  }
}

/**
 * The gateway's record of what it checked before publishing a turn (word
 * range, revision scope, source review). Kept as the server sent it: the
 * receipt is content-derived evidence, and reshaping it here would make the
 * browser's copy disagree with the durable one on the assistant message.
 */
export type ChatResultReceiptEvent = {
  receipt: Record<string, unknown>
  requestId?: string
}
/**
 * What a Space's knowledge did for one turn (`knowledge` on `source_scope_applied`, WS18-S3): a mode and,
 * when a search ran this turn, its source count. A mode and a count, never content.
 */
export type ChatKnowledgeReceipt = {
  mode: 'retrieval' | 'not_entitled' | 'not_searched'
  sources?: number
}
/**
 * Per-turn retrieval-source filter transparency frame (chat-wave2 scope
 * spec §2.2/§2.3's "shown in the receipt" requirement, `source_scope_applied`
 * family `citations`). Emitted at most once per turn, when the turn's
 * effective `RetrievalScope` was not `auto`, or when a Space scoped the turn
 * and so carries a `knowledge` receipt. An `auto` scope arrives only with that
 * receipt. Consumed by `ChatMessages.tsx`'s scope badge and `ChatPanels.tsx`'s
 * assembled-context inspector line.
 */
export type ChatSourceScopeAppliedEvent = {
  scope: 'knowledge_only' | 'web_only' | 'auto'
  excludedTools: string[]
  knowledge?: ChatKnowledgeReceipt
}
/**
 * Which search providers served a turn's web searches, and the route its entitlement allowed (`search_provenance`,
 * EP-9 W1). The ids are closed: a provider the SPA does not name arrives as `other`, and the line is worded by id in
 * the SPA's own two languages, so no provider name the server sends reaches the screen unread. Consumed by the
 * "Searched via" line beside the source-scope notice (`ChatMessages.tsx`).
 */
export type ChatSearchProviderId = 'tantivy_local' | 'brave' | 'serper' | 'searxng' | 'stract' | 'verevon' | 'other'
export type ChatSearchRoute = 'free_chain' | 'paid_allowed' | 'zdr_free_only'
export type ChatSearchProvenanceEvent = {
  providers: { id: ChatSearchProviderId; hits: number }[]
  route: ChatSearchRoute
}
/**
 * AI-generated thread title, emitted once by model-gateway after a thread's
 * FIRST exchange completes (before the terminal `done`). Already sanitized
 * server-side: single line, no quotes/emoji, ≤64 chars.
 */
export type ChatTitleEvent = { title: string; requestId?: string }
/**
 * AI-generated follow-up question suggestions, emitted by model-gateway after
 * (almost) every non-ZDR exchange completes (before the terminal `done`).
 * Already sanitized server-side: 0-3 short questions, no quotes/emoji/
 * numbering. Never emitted for a ZDR turn.
 */
export type ChatFollowUpsEvent = { suggestions: string[]; requestId?: string }

/** One step of a model-proposed plan, as carried on the `plan_proposed` SSE event. */
export type ChatPlanStep = { id: string; title: string; operation?: string }

/**
 * The model's structured plan-mode plan (plan-gate §2.1), emitted once a
 * plan-mode turn finishes when its `plan.propose` tool call created one. See
 * `sse_events.rs`'s `ChatEvent::PlanProposed` for the server side.
 */
export type ChatPlanProposedEvent = {
  planId: string
  summary: string
  steps: ChatPlanStep[]
  requestId?: string
}

/** One choice of a question's single- or multi-choice item (WS14 S4). */
export type ChatQuestionOption = { id: string; label: string; description?: string }

/**
 * One thing a run's `ask_user` call asks its owner (WS14 S4). `options` is empty for a free-text item;
 * `allowOther` says a single- or multi-choice item also takes the person's own words.
 */
export type ChatQuestionItem = {
  id: string
  kind: 'single' | 'multi' | 'text'
  prompt: string
  options: ChatQuestionOption[]
  allowOther: boolean
}

/**
 * The question a run put to its owner, sent once that run completed with the question still waiting
 * (WS14 S4). The card records its answer through the question's answer route, and the answer is then the
 * person's next turn.
 */
export type ChatUserQuestionEvent = {
  questionId: string
  delivery: string
  /** RFC 3339. Past it the question reads as expired and cannot be answered. */
  expiresAt?: string
  items: ChatQuestionItem[]
  requestId?: string
  /**
   * Who asked, when an MCP server did (EP-4 S6). Only a server's question carries it, and the card then names
   * that server; a model question leaves both this and `originLabel` unset.
   */
  origin?: 'mcp_elicitation'
  /** The asking server's display name, 1 to 80 characters. Set exactly with `origin`. */
  originLabel?: string
}

/** The owner's answer to a question, one entry per item, or a skip (WS14 S4). */
export type ChatQuestionAnswer = {
  answers: { itemId: string; optionIds: string[]; text: string }[]
  skip?: boolean
}

/**
 * Read the recalled-memory list off the wire.
 *
 * Unknown `origin`/`role` values degrade rather than being dropped — the
 * backend's vocabulary may already be wider than this build's, and a row the
 * reader cannot see is worse than one labelled conservatively. `origin`
 * specifically degrades to `unrecorded`, never to `stated`: reading an
 * unrecognised value as "you told me this" is the one mistake that misleads.
 */
function normalizeRecalledMemories(raw: unknown): RecalledMemory[] {
  if (!Array.isArray(raw)) return []
  return raw.flatMap((entry): RecalledMemory[] => {
    if (!entry || typeof entry !== 'object') return []
    const record = entry as Record<string, unknown>
    const memoryId =
      typeof record.memory_id === 'string' ? record.memory_id : ''
    const preview = typeof record.preview === 'string' ? record.preview : ''
    // Without an id the row cannot be acted on, and without a preview there is
    // nothing to read; either way it earns no line.
    if (!memoryId || !preview.trim()) return []
    const origin = record.origin
    const role = record.role
    return [
      {
        memoryId,
        role: role === 'inject' ? 'inject' : 'recall',
        origin:
          origin === 'stated' || origin === 'inferred' ? origin : 'unrecorded',
        label:
          typeof record.label === 'string' && record.label
            ? record.label
            : 'Minne',
        preview,
      },
    ]
  })
}

export type ChatStreamHandlers = {
  /** Stable Verevon UI projection; focused callbacks remain below for compatibility. */
  onUiEvent?: (event: VerevonUiEvent) => void
  onConnected?: (event: ChatConnectedEvent) => void
  onMessage?: (event: ChatMessageEvent) => void
  onDone?: (event: ChatDoneEvent) => void
  onError?: (event: ChatErrorEvent) => void
  onArtifact?: (event: ChatArtifactEvent) => void
  onReasoning?: (event: ChatReasoningEvent) => void
  onCitation?: (event: ChatCitationEvent) => void
  onGrounding?: (event: ChatGroundingEvent) => void
  onStep?: (event: ChatStepEvent) => void
  onToolCall?: (event: ChatToolCallEvent) => void
  onToolResult?: (event: ChatToolResultEvent) => void
  onAttachment?: (event: ChatAttachmentEvent) => void
  onUsage?: (event: ChatUsageEvent) => void
  /** The running total after one provider call, while the turn runs (`usage_progress`). See `ChatUsageProgressEvent`. */
  onUsageProgress?: (event: ChatUsageProgressEvent) => void
  onTitle?: (event: ChatTitleEvent) => void
  onFollowUps?: (event: ChatFollowUpsEvent) => void
  /** See `ChatPlanProposedEvent`. */
  onPlanProposed?: (event: ChatPlanProposedEvent) => void
  /** See `ChatUserQuestionEvent`. Ungated: the card is the only way to answer the question. */
  onUserQuestion?: (event: ChatUserQuestionEvent) => void
  /** The connected-app suggestion card (EP-10 S8). Ungated control event. */
  onConnectorSuggestion?: (event: { suggestionId?: string; provider?: string; reason?: string; requestId?: string }) => void
  /** See `ChatResearchPlanEvent`. Ungated: the plan card is the plan turn's whole answer. */
  onResearchPlan?: (event: ChatResearchPlanEvent) => void
  onStopped?: (event: ChatStoppedEvent) => void
  onMemoryRecall?: (event: ChatMemoryRecallEvent) => void
  onVerification?: (event: ChatVerificationEvent) => void
  /** Where the turn's server time went (`turn_timing`, `usage` family). */
  onTurnTiming?: (event: ChatTurnTiming) => void
  /** What the gateway checked before it published the turn (`result_receipt`). */
  onResultReceipt?: (event: ChatResultReceiptEvent) => void
  /** See `ChatSourceScopeAppliedEvent`. */
  onSourceScopeApplied?: (event: ChatSourceScopeAppliedEvent) => void
  /** See `ChatSearchProvenanceEvent`. */
  onSearchProvenance?: (event: ChatSearchProvenanceEvent) => void
  /**
   * A message the user sent mid-run has reached the agent. Emitted at delivery
   * rather than at enqueue: the POST already confirmed acceptance, and what the
   * client cannot otherwise know is when the agent actually saw it.
   */
  onQueuedInput?: (event: ChatQueuedInputEvent) => void
  /**
   * The answer was produced in a reduced form, so the user must be told: a ZDR answer that used text search
   * only. Ungated: sent whatever features the client opted into. See `ChatRetrievalNoticeEvent`.
   */
  onRetrievalNotice?: (event: ChatRetrievalNoticeEvent) => void
  /** A tool call is being retried (`retry_scheduled`, `steps` family). See `ChatRetryScheduledEvent`. */
  onRetryScheduled?: (event: ChatRetryScheduledEvent) => void
  /** The history or tool results were compacted (`compaction`). Ungated. See `ChatCompactionEvent`. */
  onCompaction?: (event: ChatCompactionEvent) => void
  /** The context the model was given changed (`context_change`). Ungated. See `ChatContextChangeEvent`. */
  onContextChange?: (event: ChatContextChangeEvent) => void
  /** The workspace state of a tool call's files (`sandbox_state`, `steps` family). See `ChatSandboxStateEvent`. */
  onSandboxState?: (event: ChatSandboxStateEvent) => void
  /**
   * The run was PARKED (WS03 S8-A): it stopped on a typed, recoverable failure and stays open, so
   * it can be resumed later. Display only: read with `runParkedFrameOf`, and a frame whose reason
   * this build does not know goes to `onUnknownEvent` instead. Nothing here resumes the run.
   */
  onRunParked?: (event: RunParkedFrame) => void
  /**
   * A goal turn was settled (EP-7 S2): the goal bar's counts and status after the turn. Read with
   * `goalProgressEventOf`; a frame without a goal id or with a status this build does not know goes
   * to `onUnknownEvent` instead.
   */
  onGoalProgress?: (event: ChatGoalProgressEvent) => void
  /**
   * Any SSE event this client has no case for. The gateway relays upstream
   * events verbatim with no allowlist, so a new backend event reaches the
   * browser and — before this hook — died here silently. Log, never drop.
   */
  onUnknownEvent?: (event: {
    name: string
    payload: Record<string, unknown>
  }) => void
  /**
   * The SSE `id:` of every frame that carries one, in arrival order.
   *
   * Resume needs this. The server buffers each frame under a monotonic seq and
   * replays those with `seq > Last-Event-ID`, so without tracking the highest id
   * we saw, a reconnect can only replay the stream **from the beginning** — which
   * is what it did: the controller dropped the cached partial answer on the
   * first replayed delta precisely because it knew everything was coming again.
   */
  onFrameId?: (id: string) => void
  /**
   * Asked before every frame is dispatched, with the frame's SSE `id:` (or
   * `undefined` for a live-only frame). Returning `false` drops the frame
   * entirely: no `onFrameId`, no `onUiEvent`, no focused callback.
   *
   * A resume uses it to refuse a frame it already applied. The server filters
   * by `Last-Event-ID`, but a replay that overlaps its live tail, or a
   * replica that ignores the cursor, would otherwise land the same tool call,
   * citation or step in the Work feed twice.
   */
  acceptFrame?: (id: string | undefined) => boolean
}

export type ChatMessage = {
  sourceScope?: import('@/shared/actions/chat-source-scope').ChatSourceScope
  /** See `ChatInvokeRequest.retrievalScope`. Read back on history playback
   * so a reopened thread's past turns can show their own receipt badge. */
  retrievalScope?: import('@/shared/actions/chat-source-scope').RetrievalScope
  /** The context chips a user turn was sent with (EP-3 S5), from `metadata.context_refs`: kind, id and label only. */
  contextRefs?: import('@/shared/actions/chat-context-refs').ChatContextRef[]
  id: string
  role: 'user' | 'assistant'
  content: string
  model?: string
  createdAt: string
  /**
   * The answer's persisted confidence score, flattened onto the message from
   * session-core's turn metadata. Absent for a user message, and for assistant
   * turns recorded before the score was persisted — which must stay
   * distinguishable from a score of 0.
   */
  confidence?: number
  /** What the backend's verification pass found, when it ran on this turn. */
  verification?: ChatVerificationEvent
  /**
   * Where this turn sits among the turns that share its parent, as the server reports it on a versioned
   * thread (WS17 S9). Absent when the server keeps no version tree for the thread.
   */
  version?: ChatMessageVersion
}

/** A turn's place in the server's message tree (WS17 S9): its parent, its place among the turns sharing it, and those turns. */
export type ChatMessageVersion = {
  /** The turn this one continues. Empty for the first turn of a thread. */
  parentMessageId: string
  /** 1-based place among the turns that share the parent. */
  siblingOrdinal: number
  /** How many turns share the parent, this one included. */
  siblingCount: number
  /** Whether the turn is on the active path, the conversation the thread shows. */
  onActivePath: boolean
  /** The ids of the turns that share the parent, in order: the versions a switch can move to. */
  siblings: string[]
}

/**
 * The attention state of a thread, as Session Core derives it (build plan WS16 S1). A closed set:
 * a value outside it is dropped rather than guessed at.
 */
const CHAT_ATTENTION_STATES = [
  'needs_approval',
  'needs_input',
  'plan_ready',
  'working',
  'completed',
  'failed',
  'archived',
  'idle',
] as const

export type ChatAttentionState = (typeof CHAT_ATTENTION_STATES)[number]

export type ChatThreadSession = {
  preview: string
  threadId: string
  title: string
  updatedAt: string
  /**
   * Whether the user pinned this thread. Server-owned so a pin follows the user
   * across devices, and defaults to false for an index written before pins
   * existed.
   */
  pinned: boolean
  /**
   * Persona spec — the per-thread override of the user's global persona
   * switch (Settings → Persona). Server-owned, same "absent == true" default
   * as `pinned` false defaults to unpinned: a thread saved before this field
   * existed has always had its persona on. Disabled (not hidden) in the UI
   * for a temporary/ZDR chat, which cannot durably persist this toggle.
   */
  personaEnabled: boolean
  /** Newest run projection from Session Core, when the thread has one. */
  latestRunId?: string
  latestRunStatus?: string
  latestRunUpdatedAt?: string
  /** Non-secret Space routing reference for a scoped thread. The BFF still
   * resolves fresh Control authority before an append. */
  spaceRef?: string
  /** Which surface created this thread (session-core's `ThreadSummary.origin`
   * — "chat" | "space" | "agent_run" | "support" | "system" | "mail_task").
   * Empty/absent for a thread that predates this field, which is exactly
   * "chat" in practice (see history.rs's backfill note). Read today only to
   * show the "Fra e-post / From email" badge on a mail-task thread. */
  origin?: string
  /** Set when the owner archived the thread. Absent for an active thread. */
  archivedAt?: string
  /** The thread this one was forked from (build plan WS17 S6). Absent for a thread that was not forked. */
  forkedFromThreadId?: string
  /** Whether the owner starred the thread. Server-owned, like `pinned`. */
  starred?: boolean
  /**
   * Whether the thread holds a reply the owner has not read yet. The one unread
   * definition (build plan DEC-11): the server compares the newest assistant reply
   * with the owner's last read, and the client never derives its own.
   */
  unread?: boolean
  /**
   * The derived attention state (WS16 S1). Absent while the server's attention switch is off, and
   * for a state this build does not name.
   */
  attentionState?: ChatAttentionState
  /** When the attention state began (ISO 8601). Absent when the state has no start. */
  attentionSince?: string
  /** Whether the thread holds an unread reply. Present only with an attention state. */
  attentionUnseen?: boolean
}

export type ChatThreadTranscriptSnapshot = {
  taskSteps?: unknown[]
  threadId: string
  turns: unknown[]
  updatedAt: string
}

/** Safe envelope metadata recovered from Session Core's durable event log. */
export type ChatThreadEvent = {
  eventId: string
  eventType: string
  schemaVersion: number
  at: string
  producer?: string
  correlationId?: string
  causationId?: string
  resourceRef?: string
}

export type ChatThreadEventsPage = {
  threadId: string
  events: ChatThreadEvent[]
  /** The server filled its bounded replay request; use the last event id to continue. */
  truncated: boolean
}

export type SaveChatThreadSnapshotRequest = {
  preview?: string
  taskSteps?: unknown[]
  title?: string
  turns?: unknown[]
  updatedAt?: string
  /**
   * OMIT to leave the pin untouched. The SPA saves a snapshot on every turn to
   * refresh title/preview, so sending `false` by default would silently unpin
   * the thread on the user's next message.
   */
  pinned?: boolean
  /**
   * OMIT to leave the per-thread persona toggle untouched — same
   * "omission = leave unchanged" contract as `pinned` above.
   */
  personaEnabled?: boolean
}

export type ModelModality = 'chat' | 'image' | 'video' | 'audio' | 'embedding' | 'other'

// ── Verevon intent modes ──────────────────────────────────────────────────────
// Three first-class pseudo-models. They are NOT catalog entries: the backend
// (inference-core) resolves each id SERVER-SIDE via its intent layer (complexity
// + budget → a concrete model, with model-router as the cheap fallback). The
// frontend only offers them and sends the chosen id as `model`. They are always
// pinned at the top of the picker, independent of what `/v1/models` returns.

export type VerevonModeBadge = 'cheap' | 'premium'

export type VerevonMode = {
  id: string
  /** Display label, e.g. "Verevon Balance". */
  label: string
  /** Short one-liner shown under the label in the picker. */
  description: string
  /** Which cost badge to show next to the row. */
  badge: VerevonModeBadge
}

/** Verevon Balance is the default selection (backend treats `''` as balance too). */
export const VEREVON_BALANCE_MODE_ID = 'verevon-balance'

/** The pinned "Verevon" group, authoritative + always shown, most-default first. */
export const VEREVON_MODES: readonly VerevonMode[] = [
  {
    id: 'verevon-budget',
    label: 'Verevon Budget',
    description: 'Cheapest, fast',
    badge: 'cheap',
  },
  {
    id: VEREVON_BALANCE_MODE_ID,
    label: 'Verevon Balance',
    description: 'Smart balance of cost & quality',
    badge: 'cheap',
  },
  {
    id: 'verevon-genius',
    label: 'Verevon Genius',
    description: 'Most capable, premium',
    badge: 'premium',
  },
]

const VEREVON_MODE_IDS: ReadonlySet<string> = new Set(VEREVON_MODES.map((mode) => mode.id))

/** True when the id is one of the pinned Verevon intent modes. */
export function isVerevonModeId(id: string): boolean {
  return VEREVON_MODE_IDS.has(id)
}

/** Look up a Verevon mode by id (for label/description/badge rendering). */
export function verevonModeById(id: string): VerevonMode | undefined {
  return VEREVON_MODES.find((mode) => mode.id === id)
}

/** The composer's named effort profiles; `standard` asks for no reasoning. */
export type EffortProfile = 'quick' | 'standard' | 'deep'

/**
 * The canonical reasoning ladder, weakest first. Mirrors the names the gateway
 * accepts for `effort` and lists in `effort_levels` on `/v1/models`.
 */
export const EFFORT_LEVELS = ['minimal', 'low', 'medium', 'high', 'xhigh', 'max'] as const
export type EffortLevel = (typeof EFFORT_LEVELS)[number]

/** Narrows an arbitrary wire value to a canonical level; anything else is no level. */
export function isEffortLevel(value: unknown): value is EffortLevel {
  return typeof value === 'string' && (EFFORT_LEVELS as readonly string[]).includes(value)
}

/**
 * A chat model's list price per one million tokens, as the catalogue states it
 * (`price` on `/v1/models`). It is shown only when every part is present.
 */
export type ModelPrice = {
  inputPerMillion: number
  outputPerMillion: number
  currency: string
}

/** The vendor lifecycle stage a catalogue record names (`lifecycle` on `/v1/models`). */
export type ModelLifecycle = 'ga' | 'preview' | 'deprecated'

/**
 * The data-use statements a catalogue record makes for a model (`data_use` on `/v1/models`). A field is
 * set only when a source states it: `undefined` is "not stated", never "no".
 */
export type ModelDataUse = {
  /** Whether the provider states it trains on the inputs it receives. */
  trainsOnInputs?: boolean
  /** Days the provider states it keeps the inputs; `0` when it states none are kept. */
  retentionDays?: number
  /** `true` only when the deployment carries a verified zero-retention attestation in force. */
  zdrAttested?: boolean
}

export type ModelInfo = {
  id: string
  name: string
  capabilities?: string[]
  /** Upstream provider, e.g. 'openai', 'anthropic', 'deepseek', 'microsoft'. */
  provider?: string
  /** Primary modality of the model. Used to filter chat-only choices. */
  modality?: ModelModality
  /** Whether the model is a cheap / cost-efficient option (cost-aware default). */
  cheap?: boolean
  /** Relative cost tier hint, when the backend reports it. */
  costTier?: 'low' | 'medium' | 'high'
  /**
   * Venice-style programmatic privacy tier for this model's provider
   * (`privacy_tier` on `/v1/models`). `undefined` when absent/unknown — the
   * UI then renders no claim at all rather than guessing one.
   */
  privacyTier?: PrivacyTier
  /** Data-residency region the backend reports alongside the tier (`residency`). */
  residency?: string
  /**
   * Reasoning levels this model accepts (`effort_levels` on `/v1/models`).
   * `undefined` when the server lists none: the model has no effort control,
   * or the gateway's ladder is off.
   */
  effortLevels?: EffortLevel[]
  /**
   * Context window in tokens (`context_window_tokens` on `/v1/models`). `undefined` when the
   * catalogue has no sourced record for the model, so no window is claimed.
   */
  contextWindowTokens?: number
  /** Largest synchronous output in tokens this model is served at (`max_output_tokens`). */
  maxOutputTokens?: number
  /** Vendor lifecycle stage (`lifecycle`). `undefined` when the catalogue names none. */
  lifecycle?: ModelLifecycle
  /**
   * The catalogue's list price for a chat model (`price` on `/v1/models`). Present only when
   * the gateway's list-price switch is on and the catalogue names a price.
   */
  price?: ModelPrice
  /** What the provider states about the request data sent to this model (`data_use`). Absent when none is stated. */
  dataUse?: ModelDataUse
}

// Opt-in rich SSE families the model-gateway understands (chat-parity §2).
// `memory` opts this client in to the `memory_recall` visibility event. The
// memory INJECTION itself is unconditional server-side (prompt quality is not
// a client choice) — this only asks to be TOLD when it happened, so the UI can
// show "recalled N memories" instead of silently benefiting from it.
const DEFAULT_FEATURES = [
  'usage',
  'citations',
  'reasoning',
  'steps',
  'artifacts',
  'memory',
]
function str(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined
}

/**
 * The items of a `user_question` frame, read defensively: an item this build cannot read is dropped, and a
 * choice item with fewer than two readable options is dropped with it (it is not a choice).
 */
function normalizeQuestionItems(value: unknown): ChatQuestionItem[] {
  if (!Array.isArray(value)) return []
  return value
    .map((item): ChatQuestionItem | null => {
      const raw = objectValue(item)
      if (!raw) return null
      const id = str(raw.id)
      const prompt = str(raw.prompt)
      const kind = raw.kind === 'single' || raw.kind === 'multi' || raw.kind === 'text' ? raw.kind : undefined
      if (!id || !prompt || !kind) return null
      const options = (Array.isArray(raw.options) ? raw.options : [])
        .map((option): ChatQuestionOption | null => {
          const entry = objectValue(option)
          const optionId = str(entry?.id)
          const label = str(entry?.label)
          if (!optionId || !label) return null
          const description = str(entry?.description)
          return description ? { id: optionId, label, description } : { id: optionId, label }
        })
        .filter((option): option is ChatQuestionOption => option !== null)
      if (kind !== 'text' && options.length < 2) return null
      return { id, kind, prompt, options, allowOther: raw.allow_other === true }
    })
    .filter((item): item is ChatQuestionItem => item !== null)
}

/**
 * Who asked a `user_question` frame (EP-4 S6). The card names a server only for a known server origin with a
 * label of 1 to 80 characters; any other frame reads as the model's question, as it did before.
 */
function questionOrigin(payload: Record<string, unknown>): Pick<ChatUserQuestionEvent, 'origin' | 'originLabel'> {
  const label = str(payload.origin_label)?.trim()
  if (payload.origin !== 'mcp_elicitation' || !label || [...label].length > 80) return {}
  return { origin: 'mcp_elicitation', originLabel: label }
}

function strOrJson(value: unknown): string | undefined {
  if (typeof value === 'string') return value
  if (value == null) return undefined
  try {
    return JSON.stringify(value)
  } catch {
    return undefined
  }
}

function num(value: unknown): number | undefined {
  return typeof value === 'number' ? value : undefined
}

function objectValue(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : undefined
}

/** A count from a frame: a finite number, never below zero; anything else is 0. */
function frameCount(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) ? Math.max(0, Math.trunc(value)) : 0
}

/**
 * A `goal_progress` frame (EP-7 S2) as the SPA reads it, or `undefined` when it names no goal or a
 * status this build does not know. A budget that is absent, `null` or not positive is "none".
 */
export function goalProgressEventOf(payload: Record<string, unknown>): ChatGoalProgressEvent | undefined {
  const goalId = str(payload.goal_id)
  const status = GOAL_PROGRESS_STATUSES.find((candidate) => candidate === payload.status)
  if (!goalId || !status) return undefined
  const tokenBudget = frameCount(payload.token_budget)
  const costBudgetUsdMicros = frameCount(payload.cost_budget_usd_micros)
  const statusReason = str(payload.status_reason)
  const requestId = str(payload.request_id)
  return {
    goalId,
    turnNo: frameCount(payload.turn_no),
    tokensUsed: frameCount(payload.tokens_used),
    ...(tokenBudget > 0 ? { tokenBudget } : {}),
    costUsedUsdMicros: frameCount(payload.cost_used_usd_micros),
    ...(costBudgetUsdMicros > 0 ? { costBudgetUsdMicros } : {}),
    turnsUsed: frameCount(payload.turns_used),
    maxTurns: frameCount(payload.max_turns),
    status,
    ...(statusReason ? { statusReason } : {}),
    ...(requestId ? { requestId } : {}),
  }
}

/**
 * A `source_scope_applied` frame as the SPA reads it. An `auto` scope arrives only with a knowledge receipt
 * (WS18-S3), so an `auto` frame without one is not a filter and is dropped. A receipt whose mode the SPA does
 * not name is dropped on its own, and the frame's scope still applies.
 */
function sourceScopeAppliedEvent(payload: Record<string, unknown>): ChatSourceScopeAppliedEvent | undefined {
  const scope = str(payload.scope)
  const excludedTools = Array.isArray(payload.excluded_tools)
    ? payload.excluded_tools.filter((name): name is string => typeof name === 'string')
    : []
  const knowledge = knowledgeReceiptValue(payload.knowledge)
  if (scope === 'knowledge_only' || scope === 'web_only') {
    return knowledge ? { scope, excludedTools, knowledge } : { scope, excludedTools }
  }
  return scope === 'auto' && knowledge ? { scope, excludedTools, knowledge } : undefined
}

const SEARCH_PROVIDER_IDS: readonly ChatSearchProviderId[] = [
  'tantivy_local',
  'brave',
  'serper',
  'searxng',
  'stract',
  'verevon',
  'other',
]
const SEARCH_ROUTES: readonly ChatSearchRoute[] = ['free_chain', 'paid_allowed', 'zdr_free_only']

/**
 * A `search_provenance` frame as the SPA reads it. A provider id the SPA does not name is read as `other`, so a
 * provider Quarry adds later cannot reach the line under its raw name. A count that is not a whole number, a route
 * the SPA does not name, or a frame that names no provider drops the frame.
 */
function searchProvenanceEvent(payload: Record<string, unknown>): ChatSearchProvenanceEvent | undefined {
  const route = SEARCH_ROUTES.find((candidate) => candidate === payload.route)
  if (!route || !Array.isArray(payload.providers)) return undefined
  const providers = payload.providers.flatMap((entry) => {
    const provider = objectValue(entry)
    const hits = num(provider?.hits)
    if (!provider || hits === undefined || !Number.isInteger(hits) || hits < 0) return []
    const id = SEARCH_PROVIDER_IDS.find((candidate) => candidate === provider.id) ?? 'other'
    return [{ id, hits }]
  })
  return providers.length > 0 ? { providers, route } : undefined
}

function knowledgeReceiptValue(value: unknown): ChatKnowledgeReceipt | undefined {
  const receipt = objectValue(value)
  const mode = str(receipt?.mode)
  if (mode !== 'retrieval' && mode !== 'not_entitled' && mode !== 'not_searched') return undefined
  const sources = num(receipt?.sources)
  return sources === undefined ? { mode } : { mode, sources }
}

/**
 * The content-free retry timing on an `error` frame: when a refusing service
 * (the ChatGPT plan's usage limit, or a throttle) accepts calls again. Both
 * spellings are read, as elsewhere in this client. A delay that is not a
 * non-negative finite number, or a reset instant that does not parse, is
 * dropped rather than passed on for the UI to misread. Keys are present only
 * when known, so an error without them keeps its exact shape.
 */
function failureTiming(payload: Record<string, unknown>): Pick<ChatErrorEvent, 'retryAfterMs' | 'resetsAt'> {
  const delay = num(payload.retry_after_ms) ?? num(payload.retryAfterMs)
  const resetsAt = str(payload.resets_at) ?? str(payload.resetsAt)
  return {
    ...(delay !== undefined && Number.isFinite(delay) && delay >= 0 ? { retryAfterMs: delay } : {}),
    ...(resetsAt !== undefined && Number.isFinite(Date.parse(resetsAt)) ? { resetsAt } : {}),
  }
}

/**
 * The feature a zero-retention refusal names, as the server sent its key: on
 * the frame, or in its error envelope. Absent when neither names one, so an
 * error without it keeps its exact shape.
 */
function failureFeature(
  payload: Record<string, unknown>,
  envelope: Record<string, unknown> | undefined,
): Pick<ChatErrorEvent, 'feature'> {
  const feature = str(payload.feature) ?? str(envelope?.feature)
  return feature ? { feature } : {}
}

function emitCitation(value: unknown, handlers: ChatStreamHandlers): void {
  // The golden's keys are typed; the legacy spellings read below stay `unknown` until they are retired.
  const payload = objectValue(value) as (Partial<ChatEventPayloads['citation']> & Record<string, unknown>) | undefined
  if (!payload) return
  handlers.onCitation?.({
    id: str(payload.id),
    title: str(payload.title),
    url: str(payload.url) ?? str(payload.href),
    snippet: str(payload.snippet) ?? str(payload.description),
    claimId: str(payload.claim_id) ?? str(payload.claimId),
    sourceGroupId: str(payload.source_group_id) ?? str(payload.sourceGroupId),
    sourceGroupLabel: str(payload.source_group_label) ?? str(payload.sourceGroupLabel),
    start: num(payload.start) ?? num(payload.start_offset) ?? num(payload.startOffset),
    end: num(payload.end) ?? num(payload.end_offset) ?? num(payload.endOffset),
    number: num(payload.number),
    origin: str(payload.origin),
    // Absent keys stay absent, so a frame without a passage hands on exactly the event it did before.
    ...citationPassage(payload),
  })
}

/** The `quote` and `locator` of a citation frame or stored citation (EP-2-S4), each only when well formed. */
export function citationPassage(payload: Record<string, unknown>): Pick<ChatCitationEvent, 'quote' | 'locator'> {
  const quote = str(payload.quote)?.trim()
  const locator = parseCitationLocator(payload.locator)
  return { ...(quote ? { quote } : {}), ...(locator ? { locator } : {}) }
}

/** A report's provenance stamp, as an artifact frame's optional key. */
function artifactProvenance(payload: Record<string, unknown>): Pick<ChatArtifactEvent, 'provenance'> {
  const provenance = parseResearchProvenance(payload.provenance)
  return provenance ? { provenance } : {}
}

/** The frame's editor, when it names one of the three the server writes; anything else is dropped. */
function artifactEditor(payload: Record<string, unknown>): Pick<ChatArtifactEvent, 'editor'> {
  return isArtifactEditorKind(payload.editor) ? { editor: payload.editor } : {}
}

/**
 * A knowledge citation's chunk, read again through the BFF's Data Plane chunk read (EP-2-S4). Data Plane resolves
 * the viewer's own grants before it fetches any text, so a document this person may not read comes back empty and
 * reads as `denied`, the same as a 403. The read returns every chunk of the document, so the one the citation
 * names is picked here.
 */
export type CitedChunk =
  | { status: 'ok'; text: string }
  | { status: 'denied' }
  | { status: 'missing' }

export async function fetchCitedChunk(locator: CitationLocator): Promise<CitedChunk> {
  const payload = await requestJson<{ chunks?: unknown } | null>('/api/v1/knowledge/retrieve/chunks', {
    method: 'POST',
    body: JSON.stringify({ document_id: locator.document_id }),
  }).catch((error: unknown) => {
    if (error instanceof ApiError && (error.status === 403 || error.status === 404)) return null
    throw error
  })
  if (payload === null) return { status: 'denied' }
  const chunks = Array.isArray(payload?.chunks) ? payload.chunks : []
  if (chunks.length === 0) return { status: 'denied' }
  const chunk = chunks
    .map(objectValue)
    .find((entry) => entry?.chunk_index === locator.chunk_index && entry?.document_id === locator.document_id)
  const text = str(chunk?.text)
  return text ? { status: 'ok', text } : { status: 'missing' }
}

type WireToolSpec = { name: string; description: string; parameters_json: string }

function buildToolSpecs(request: ChatInvokeRequest): WireToolSpec[] {
  return createSelectedAgentToolSpecs({
    browseWeb: request.browseWeb,
    // Skills are guidance, not tools. model-gateway injects a picked skill's
    // body as system context from `skill_ids` (see buildChatWireBody). The old
    // path synthesized a no-op tool spec named after the skill: the model was
    // offered something to call, and nothing steered the answer.
    actions: request.actions?.filter((action) => action.kind !== 'skill'),
    explicitTools: request.tools,
  }).map((tool) => ({
    name: tool.name,
    description: tool.description,
    parameters_json: tool.parametersJson,
  }))
}

/** Mirrors model-gateway `MAX_REQUESTED_SKILLS`; the server caps again. */
const MAX_SKILL_IDS = 4

/**
 * The skills the member picked explicitly (`/` in a composer), as ids for the
 * wire. Trimmed, de-duplicated, capped. The server resolves each against the
 * org's catalogue and the ownership rule and drops what the caller may not use
 * — this list is a request, never a grant.
 */
function skillIdsField(actions: readonly ChatAction[] | undefined): { skill_ids?: string[] } {
  const ids = pickedSkillIds(actions)
  return ids.length > 0 ? { skill_ids: ids } : {}
}

function pickedSkillIds(actions: readonly ChatAction[] | undefined): string[] {
  const out: string[] = []
  for (const action of actions ?? []) {
    if (action.kind !== 'skill') continue
    const id = action.id.trim()
    if (id && !out.includes(id)) out.push(id)
    if (out.length >= MAX_SKILL_IDS) break
  }
  return out
}

/**
 * Keep Global Chat workspace-aware without turning every message into a
 * support query. The gateway re-checks the authenticated organization and
 * removes this control field before forwarding to Model Plane.
 */
export function shouldRequestSupportContext(content: string): boolean {
  const query = content.trim().toLocaleLowerCase()
  if (!query) return false
  return [
    /\bsla\b/,
    /\bsupport (ticket|conversation|issue)/,
    /\bticket(s)? at risk\b/,
    /\bunresolved (customer )?issues?\b/,
    /\b(recurring|shipping) problem\b/,
    /\b(follow[- ]?up|needs follow[- ]?up)\b/,
    /\b(relevant|knowledge) (support |knowledge )?(source|article|link|document)s?\b/,
    /\b(find|relevant).{0,80}\b(source|sources|article|articles|knowledge)\b/,
    /\b(støtte|saker|samtaler|oppfølging|kunnskapsbase)\b/,
  ].some((pattern) => pattern.test(query))
}

/**
 * Thrown by `buildChatWireBody` when a turn carries media its route cannot
 * deliver. The ChatGPT-subscription wire has no attachments, so the images used
 * to be dropped while the transcript said they were added; nothing is sent now,
 * and the code is typed so the chat can say so (`streamFailurePresentation`).
 * The composer refuses these files first (`attachment-policy.ts`); this is the
 * backstop for a turn that reaches the client by another path.
 */
export class AttachmentsNotSentError extends Error {
  readonly code = 'attachments_not_sent'
  constructor() {
    super('attachments_not_sent')
    this.name = 'AttachmentsNotSentError'
  }
}

/** The browser's IANA zone name, when the runtime reports one (EP-8-S1). */
export function clientTimeZone(): string | undefined {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || undefined
}

/**
 * The UTC offset in minutes, east positive (EP-8-S1). `getTimezoneOffset` reports minutes behind UTC, so
 * it is negated; `|| 0` folds negative zero, which UTC produces, into zero.
 */
export function clientUtcOffsetMinutes(now: Pick<Date, 'getTimezoneOffset'>): number {
  return -now.getTimezoneOffset() || 0
}

/**
 * The app language the person chose, for a turn that names none (EP-8-S1): the stored setting the app shows.
 * An unreadable store, or a value outside the app's two languages, gives none rather than a guess.
 */
function storedUiLocale(): Locale | undefined {
  try {
    const stored = window.localStorage.getItem(localeStorageKey)
    return isLocale(stored) ? stored : undefined
  } catch {
    return undefined
  }
}

export function buildChatWireBody(request: ChatInvokeRequest): Record<string, unknown> {
  const conversationOnly = request.sourceScope === 'conversation'
  // Per-turn retrieval-source filter (chat-wave2 scope spec): conversation
  // always wins — that mode already excludes both source families outright,
  // so a `retrievalScope` also passed alongside it is never sent. Narrowed
  // inline rather than trusted, same as `conversationOnly` above: the server
  // fails safe to `auto` for an unrecognized value regardless, but a caller
  // that bypassed the `RetrievalScope` type (e.g. a value round-tripped
  // through storage) must not put an arbitrary string on the wire.
  const retrievalScope = conversationOnly
    ? 'auto'
    : request.retrievalScope === 'knowledge_only' || request.retrievalScope === 'web_only'
      ? request.retrievalScope
      : 'auto'
  const threadId = request.threadId?.trim() || undefined
  const supportReadOnly = isSupportChatThread(threadId)
  const subscriptionBacked = request.provider === 'openai-codex-subscription'
  if (subscriptionBacked && (request.attachments ?? []).length > 0) throw new AttachmentsNotSentError()
  const tools = supportReadOnly || subscriptionBacked ? [] : buildToolSpecs(request)
  const features = new Set(request.features ?? DEFAULT_FEATURES)
  if (conversationOnly) features.add('conversation_only')
  else features.delete('conversation_only')
  // Request the tools family by default, even with zero client-declared specs.
  // Support-derived threads are the exception because their durable history
  // contains customer-authored transcript text and must remain read-only.
  // model-gateway merges its own builtins (tool_loop::builtin_tool_defs —
  // fetch_url, knowledge_search, get_weather, code_interpreter) into the tool
  // list whenever this feature is present, regardless of what the client
  // sent (sse.rs). Without it, a plain chat turn never gets knowledge_search
  // attached, so the model can't ground answers in the org's own ingested
  // knowledge base unless the user happens to also toggle Browse or an
  // action first. web_search stays gated behind its own explicit-Search-
  // toggle check server-side, so this does not grant unrestricted web access
  // — only the safe, always-useful builtins turn on.
  //
  // Subscription models propose tool calls through the scoped broker. Model
  // Gateway keeps execution, authorization and audit ownership; decision and
  // answer inference retain the selected subscription model and connection.
  if (supportReadOnly) features.delete('tools')
  else features.add('tools')
  // Plan mode → agentic run path (orchestration-backed, supports approval gates
  // + run pause/resume). Without it the gateway uses the direct tool loop, which
  // never pauses for human approval.
  //
  // This now holds on the ChatGPT-subscription route too. Whether that route may
  // run agentic is the server's call (the AGENTIC_SUBSCRIPTION_ENABLED switch,
  // advertised as the `agentic` model capability): the composer only offers Do
  // when the catalogue says so, and a request that still arrives while the
  // switch is off is refused with the typed `agentic_subscription_disabled`
  // code, which the chat presents in words (`streamFailurePresentation`). The
  // body stays as narrow as before on every other axis: no client tools,
  // browsing, image generation, deep research or skills, and an image is
  // refused (`AttachmentsNotSentError`), never dropped.
  //
  // An EXECUTE turn (WS14 S2a, the turn that runs an approved plan) is agentic
  // too, with plan_mode false: model-gateway runs it under the grant it holds
  // for the thread, or refuses it typed. It is only ever put on the wire when
  // every precondition the server enforces also holds here (an existing thread,
  // not Support-derived, not conversation-only, not zero-retention, not the
  // subscription route, not a plan turn), so the body this builds never asks for
  // something the gateway would refuse as malformed. The controller has already
  // decided whether the turn may execute; this is the last, narrow gate.
  const executeGranted =
    request.executeGranted === true &&
    !request.planMode &&
    !conversationOnly &&
    !supportReadOnly &&
    !subscriptionBacked &&
    !request.zdr &&
    Boolean(threadId)
  if ((request.planMode || executeGranted) && !conversationOnly && !supportReadOnly) features.add('agentic')
  else features.delete('agentic')
  // EP-4 S2: the gateway writes the widget notice into the prompt only for a client that can draw a
  // widget, so the family is sent only with the frontend switch on: the same literal `true` the widget
  // component reads (`inlineWidgetsEnabled`).
  if (import.meta.env.VITE_CHAT_INLINE_WIDGETS === 'true') features.add('inline_widgets')
  else features.delete('inline_widgets')

  const supportContextQuery = !conversationOnly && !supportReadOnly && shouldRequestSupportContext(request.content)
    ? request.content.trim().slice(0, 240)
    : undefined
  // Connected apps (WS23 S6): the field exists only when the person picked an account for this
  // turn. It is withheld on the turns the composer blocks (zero retention, Space, conversation-only,
  // subscription, a filtered source, support) and on an execute turn, which carries no picks of its
  // own, so the gateway never has to refuse a field this client should not have sent.
  const connectorScope = conversationOnly || supportReadOnly || subscriptionBacked || request.zdr
    || executeGranted || request.spaceRef?.trim() || retrievalScope !== 'auto'
    ? []
    : normalizeConnectorScope(request.connectorScope)
  // Context references (EP-3 S5): present only when the person attached a chip. A zero-retention or
  // temporary turn and a subscription route carry none; the server refuses them there regardless.
  const contextRefs = request.zdr || subscriptionBacked
    ? []
    : normalizeContextRefs(request.contextRefs)
  // Browser tab refs (E3.3-c): the tabs the gateway re-resolved for the turn's @tab mentions. Page context is
  // never attached on a zero-retention turn, and the other blocked turns carry no browser context either.
  const browserTabRefs = request.zdr || subscriptionBacked || supportReadOnly || conversationOnly
    ? []
    : (request.browserTabRefs ?? []).slice(0, 8).map((ref) => ({
        session_id: ref.sessionId,
        tab_id: ref.tabId,
        label: ref.label,
        label_source: ref.labelSource,
        target: ref.target,
        trust_class: ref.trustClass,
        grant: ref.grant,
      }))
  // Deck template (EP-5 S7): present only when the person picked one. A zero-retention turn never applies a
  // template, and the server refuses one there, so this client never sends it on the turns it would refuse.
  const deckTemplate = request.zdr || subscriptionBacked || supportReadOnly || conversationOnly
    ? null
    : deckTemplateId(request.deckTemplate)

  const uiLocale = request.locale ?? storedUiLocale()
  return {
    content: request.content,
    model: request.model,
    provider: request.provider,
    subscription_connection_id: request.subscriptionConnectionId,
    profile: request.profile ?? 'chat',
    thread_id: threadId,
    session_key: request.sessionKey?.trim() || threadId,
    space_ref: request.spaceRef?.trim() || undefined,
    // Narrowed inline like `retrievalScope` above: only the two known codes ever reach the wire.
    locale: request.locale === 'no' || request.locale === 'en' ? request.locale : undefined,
    // The browser's own clock and app language, on every turn (EP-8-S1). The gateway keeps each one only in its
    // closed form and reads it for the prompt alone; nothing stores it. `ui_locale` is the turn's language, else
    // the stored app setting, and absent when neither is set.
    client_timezone: clientTimeZone(),
    client_utc_offset_minutes: clientUtcOffsetMinutes(new Date()),
    ...(uiLocale ? { ui_locale: uiLocale } : {}),
    mentioned_agent_ref: request.mentionedAgentRef?.trim() || undefined,
    // Present only on a wake, so every other body is byte-identical to what it was.
    ...(request.wake ? { wake: { message_id: request.wake.messageId, client_wake_id: request.wake.clientWakeId } } : {}),
    // Explicit skill picks. The KEY is absent when none — not `undefined`, not
    // `[]` — so a turn that names no skill is byte-for-byte what it was before
    // this field existed.
    ...(supportReadOnly || subscriptionBacked ? {} : skillIdsField(request.actions)),
    browse_web: conversationOnly || supportReadOnly || subscriptionBacked ? false : request.browseWeb ?? false,
    generate_image: conversationOnly || supportReadOnly || subscriptionBacked ? false : request.generateImage ?? false,
    // Sent as a real field, not just as the `agentic` feature above: the
    // gateway needs it to mark the run itself (in-memory plan-mode store +
    // session-core's durable `run.mode`). Without this the toggle only widened
    // the feature set and nothing server-side could tell a planning run from an
    // executing one.
    //
    // On the subscription route the server admits plan mode only together with
    // the `agentic` feature (a bare plan_mode is refused), so there it follows
    // the feature exactly: a conversation-only turn sends neither.
    plan_mode: supportReadOnly
      ? false
      : subscriptionBacked
        ? (request.planMode ?? false) && !conversationOnly
        : request.planMode ?? false,
    // Present ONLY on an execute turn, so every other body is byte-identical to
    // what it was before this field existed. It carries no rung: the server holds
    // the grant, and a rung supplied here would never be read. The plan id rides
    // along only on the approve-then-execute turn (WS14 S2), which the gateway
    // checks against the thread's grant before the run starts.
    ...(executeGranted
      ? {
          execute_granted: true,
          ...(request.approvedPlanId?.trim()
            ? { approved_plan_id: request.approvedPlanId.trim() }
            : {}),
        }
      : {}),
    // Present only when the person chose a mode (WS25-S6), so every other body is byte-identical to what it
    // was before the field existed. The gateway resolves it onto the posture and refuses an id it does not know.
    ...(request.agentMode ? { agent_mode: request.agentMode } : {}),
    // Deep research is its OWN field, not just `browse_web`. "Dyp research"
    // used to set only browseWeb, so it was indistinguishable from a plain
    // Search turn — the same failure planMode had. The gateway keys the
    // multi-round research pipeline off this flag.
    deep_research: conversationOnly || supportReadOnly || subscriptionBacked ? false : (request.deepResearch ?? false),
    // Present only on a deep-research turn that keeps data (EP-2 S5), so every other body is
    // byte-identical to what it was before the plan turn existed.
    ...(request.researchPhase &&
    request.deepResearch &&
    !conversationOnly &&
    !supportReadOnly &&
    !subscriptionBacked &&
    !request.zdr
      ? researchPhaseWire(request.researchPhase, request.researchPlan)
      : {}),
    // Present only on an agentic turn that keeps data (EP-2 S8), so every other body is
    // byte-identical to what it was before wide research had an entry.
    ...(request.wideResearch && features.has('agentic') && !request.zdr
      ? { wide_research: wideResearchWire(request.wideResearch) }
      : {}),
    // Omitted entirely when unset or `standard`: the gateway treats an absent
    // effort as "no thinking", and sending `standard` explicitly would be a
    // no-op field on every ordinary turn.
    ...(request.effort && request.effort !== 'standard'
      ? { effort: request.effort }
      : {}),
    // Same shape as `effort` above, and for the same reason: the gateway reads
    // an absent verbosity as "no directive", so `balanced` is omitted rather
    // than sent as a no-op on every turn.
    ...(request.verbosity && request.verbosity !== 'balanced'
      ? { verbosity: request.verbosity }
      : {}),
    // Same omit-when-default convention as `effort`/`verbosity` above: the
    // gateway reads an absent value as `auto` (today's byte-identical
    // behavior), so this is only ever sent for a real `knowledge_only`/
    // `web_only` pick on a `workspace`-scope thread.
    //
    // Never on an execute turn: the gateway refuses an agentic run with a
    // filtered scope, and the execute turn carries no per-turn source filter.
    ...(retrievalScope !== 'auto' && !executeGranted ? { retrieval_scope: retrievalScope } : {}),
    // Omitted entirely when nothing was picked, like `retrieval_scope`: an ordinary turn is unchanged.
    ...(connectorScope.length > 0 ? { connector_scope: connectorScope } : {}),
    // Omitted entirely when no chip was attached, like `connector_scope`: an ordinary turn is unchanged.
    ...(contextRefs.length > 0 ? { context_refs: contextRefs } : {}),
    // Omitted entirely when the turn named no tab, like `context_refs`: an ordinary turn is unchanged.
    ...(browserTabRefs.length > 0 ? { browser_tab_refs: browserTabRefs } : {}),
    // Omitted entirely when no template was picked, like `context_refs`: an ordinary turn is unchanged.
    ...(deckTemplate ? { deck_template: deckTemplate } : {}),
    // The goal of a Continue turn (EP-7 S1): omitted on every other turn, so an ordinary body is unchanged.
    ...(request.goalId && !request.zdr ? { goal_id: request.goalId } : {}),
    // A pump-sent Continue turn (EP-7 S4) says so, and only beside its goal; every other body is unchanged.
    ...(request.goalId && request.goalAuto && !request.zdr ? { goal_auto: true } : {}),
    attachments: request.attachments ?? [],
    // Opt-in only, like `min_privacy_tier` below: omitted entirely when
    // nothing is pinned, so an ordinary turn's body is byte-identical to
    // what it was before pinning existed. Support threads have this
    // stripped at the gateway regardless (their history is customer text
    // this surface may only read).
    ...((request.pinnedMessageIds ?? []).length > 0
      ? { pinned_message_ids: request.pinnedMessageIds }
      : {}),
    ...(supportContextQuery ? { support_context_query: supportContextQuery } : {}),
    features: [...features],
    tools,
    zdr: request.zdr ?? false,
    // Snake_case on the wire; model-gateway also accepts the camelCase aliases
    // (http_routes.rs `alias = "regenerate"` / `alias = "editResubmit"`), but
    // matching the rest of this body keeps one convention.
    regenerated: request.regenerated ?? false,
    edited_resubmit: request.editResubmit ?? false,
    // Only on a versioned thread (WS17 S9), so every other body is byte-identical to before.
    ...(request.regenerateOfMessageId ? { regenerate_of_message_id: request.regenerateOfMessageId } : {}),
    ...(request.editOfMessageId ? { edit_of_message_id: request.editOfMessageId } : {}),
    // The send's receipt id, omitted unless set so every other body is byte-identical to what it
    // was before this field existed. Never on a zero-retention turn.
    ...(request.clientMessageId && !request.zdr ? { idempotency_key: request.clientMessageId } : {}),
    // Tier selection is opt-in ONLY: omitted unless the user explicitly picked
    // a tier, so a default send stays byte-identical to today's behavior
    // (UNSPECIFIED ⇒ no constraint server-side). Same pattern as
    // `support_context_query` above. `isSelectablePrivacyTier` (not mere
    // truthiness) keeps a literal `'unspecified'` OFF the wire too.
    ...(isSelectablePrivacyTier(request.minPrivacyTier)
      ? { min_privacy_tier: privacyTierWireValue(request.minPrivacyTier) }
      : {}),
  }
}

/**
 * Project the AG-UI standard event vocabulary onto the existing focused chat
 * callbacks. Verevon remains the internal authority: this is an ingress
 * adapter only, and unsupported AG-UI events still reach `onUiEvent`/the
 * unknown-event hook instead of being silently discarded.
 */
function dispatchAgUiEvent(
  name: string,
  payload: Record<string, unknown>,
  handlers: ChatStreamHandlers,
): boolean {
  const upper = name.toUpperCase()
  const metadata = objectValue(payload.metadata) ?? {}
  const field = (...keys: string[]): unknown => {
    for (const key of keys) {
      if (payload[key] !== undefined) return payload[key]
      if (metadata[key] !== undefined) return metadata[key]
    }
    return undefined
  }
  const runId = str(payload.runId) ?? str(payload.run_id)
  // AG-UI names the durable execution identity `runId`; Verevon's focused
  // chat callbacks historically called the same correlation slot
  // `requestId`. Use the run id as the compatibility fallback so an AG-UI
  // stream cannot lose its identity simply because the provider did not emit
  // a provider-specific request id.
  const requestId = str(field('requestId', 'request_id')) ?? runId
  switch (upper) {
    case 'RUN_STARTED':
      handlers.onConnected?.({
        ok: true,
        requestId,
        threadId: str(payload.threadId) ?? str(payload.thread_id),
        runId,
        model: str(field('model', 'modelUsed', 'model_used')),
      })
      return true
    case 'RUN_FINISHED':
      {
        const outcome = objectValue(field('outcome'))
        const outcomeType = str(outcome?.type)?.toLowerCase()
        const terminal = {
          requestId,
          modelUsed: str(field('modelUsed', 'model_used', 'model')),
          outputTokens: num(field('outputTokens', 'output_tokens')),
          stopReason: str(field('finishReason', 'finish_reason')),
        }
        // AG-UI interrupts are resumable pauses represented by RUN_FINISHED;
        // the canonical `onUiEvent` projection emitted above has already
        // carried the pause and approval details. Calling onDone here would
        // incorrectly settle the turn and hide its Work controls.
        if (outcomeType === 'interrupt') return true
        const status = str(outcome?.status)?.toLowerCase()
        if ((status === 'stopped' || status === 'cancelled' || status === 'canceled') && handlers.onStopped) {
          handlers.onStopped({ requestId, reason: status })
        } else {
          handlers.onDone?.(terminal)
        }
      }
      return true
    case 'RUN_ERROR':
      handlers.onError?.({
        code: str(payload.code) ?? 'run_error',
        message: str(payload.message) ?? 'Agent run failed',
        retryable: payload.retryable === true,
        runId,
        requestId,
        ...failureFeature(payload, undefined),
      })
      return true
    case 'TEXT_MESSAGE_CONTENT':
    case 'TEXT_MESSAGE_CHUNK': {
      const delta = str(payload.delta) ?? str(payload.content) ?? ''
      if (delta) handlers.onMessage?.({ content: delta, requestId })
      return true
    }
    case 'TEXT_MESSAGE_END':
      // `TEXT_MESSAGE_END` closes one assistant message, not the invocation.
      // RUN_FINISHED is the sole terminal callback; emitting `onDone` here
      // would settle the same run twice and discard the final run metadata.
      return true
    case 'TOOL_CALL_START':
      handlers.onToolCall?.({
        id: str(payload.toolCallId) ?? str(payload.tool_call_id),
        name: str(payload.toolCallName) ?? str(payload.tool_call_name),
      })
      return true
    case 'TOOL_CALL_RESULT':
      handlers.onToolResult?.({
        id: str(payload.toolCallId) ?? str(payload.tool_call_id),
        output: strOrJson(payload.content) ?? strOrJson(payload.output) ?? strOrJson(payload.result),
        status: 'completed',
      })
      return true
    case 'STEP_STARTED':
    case 'STEP_FINISHED':
      handlers.onStep?.({
        id: str(payload.stepName) ?? str(payload.step_name),
        title: str(payload.stepName) ?? str(payload.step_name),
        status: upper === 'STEP_STARTED' ? 'started' : 'finished',
      })
      return true
    case 'REASONING_MESSAGE_CONTENT':
    case 'REASONING_MESSAGE_CHUNK': {
      const delta = str(payload.delta) ?? str(payload.content) ?? ''
      if (delta) handlers.onReasoning?.({ delta })
      return true
    }
    case 'REASONING_START':
    case 'REASONING_MESSAGE_START':
    case 'REASONING_MESSAGE_END':
    case 'REASONING_END':
      // Lifecycle-only reasoning events are represented by the canonical UI
      // projection; no raw hidden reasoning is sent to focused renderers.
      return true
    case 'TOOL_CALL_ARGS':
    case 'TOOL_CALL_END':
    case 'TOOL_CALL_CHUNK':
    case 'STATE_SNAPSHOT':
    case 'STATE_DELTA':
    case 'MESSAGES_SNAPSHOT':
    case 'ACTIVITY_SNAPSHOT':
    case 'ACTIVITY_DELTA':
      // These are intentionally UI-projection-only for now. Their typed
      // payload remains available through `onUiEvent`, while the existing
      // message store is not mutated without a Verevon-specific reducer.
      return true
    case 'SUBAGENT_STARTED':
    case 'SUBAGENT_FINISHED':
    case 'SUBAGENT_ERROR':
      handlers.onStep?.({
        id: runId,
        title: 'Underagent',
        detail: str(payload.message) ?? str(payload.agentId) ?? str(payload.agent_id),
        status: upper === 'SUBAGENT_STARTED' ? 'started' : upper === 'SUBAGENT_ERROR' ? 'error' : 'finished',
      })
      return true
    case 'CUSTOM': {
      // The gateway carries Verevon-specific governance/evidence events in an
      // AG-UI CUSTOM envelope. Keep the envelope non-executable, but project
      // its allowlisted names into the same focused callbacks as native SSE so
      // approvals, citations, and artifacts remain functional for AG-UI
      // clients too.
      const customName = str(payload.name)
      const value = objectValue(payload.value) ?? {}
      switch (customName) {
        case 'artifact':
          handlers.onArtifact?.({
            id: str(value.id),
            kind: str(value.kind),
            title: str(value.title),
            content: str(value.content),
            version: num(value.version),
            ...artifactProvenance(value),
            ...artifactEditor(value),
          })
          break
        case 'attachment':
          handlers.onAttachment?.({
            id: str(value.id),
            name: str(value.name),
            mime: str(value.mime) ?? str(value.type),
            type: str(value.type),
            url: str(value.url),
            size: num(value.size),
            check: value.check,
            preview: value.preview,
          })
          break
        case 'reasoning_delta':
          handlers.onReasoning?.({ delta: str(value.delta) ?? '' })
          break
        case 'citation':
          handlers.onCitation?.({
            id: str(value.id),
            title: str(value.title),
            url: str(value.url) ?? str(value.href),
            snippet: str(value.snippet) ?? str(value.description),
            claimId: str(value.claim_id) ?? str(value.claimId),
            sourceGroupId: str(value.source_group_id) ?? str(value.sourceGroupId),
            sourceGroupLabel: str(value.source_group_label) ?? str(value.sourceGroupLabel),
            start: num(value.start) ?? num(value.start_offset) ?? num(value.startOffset),
            end: num(value.end) ?? num(value.end_offset) ?? num(value.endOffset),
            number: num(value.number),
            origin: str(value.origin),
          })
          break
        case 'source_scope_applied': {
          const applied = sourceScopeAppliedEvent(value)
          if (applied) handlers.onSourceScopeApplied?.(applied)
          break
        }
        case 'grounding':
          handlers.onGrounding?.({ value: value.grounding ?? value })
          break
        case 'step_update':
          handlers.onStep?.({
            id: str(value.id) ?? str(value.step_id),
            title: str(value.title) ?? str(value.name),
            detail: str(value.detail) ?? str(value.message),
            status: str(value.status),
          })
          break
        case 'run_parked': {
          const parked = runParkedFrameOf(value)
          if (parked) handlers.onRunParked?.(parked)
          else handlers.onUnknownEvent?.({ name: 'run_parked', payload: value })
          break
        }
        case 'tool_result':
          handlers.onToolResult?.({
            id: str(value.id) ?? str(value.tool_call_id),
            output: strOrJson(value.output) ?? strOrJson(value.result),
            error: str(value.error),
            status: str(value.status),
          })
          break
        case 'usage':
          handlers.onUsage?.({
            inputTokens: num(value.input_tokens),
            outputTokens: num(value.output_tokens),
            costUsd: num(value.cost_usd),
            costBasis: costBasisValue(value.cost_basis),
            latencyMs: num(value.latency_ms),
            confidence: num(value.confidence),
            cacheReadTokens: num(value.cache_read_tokens) ?? num(value.cacheReadTokens),
            cacheWriteTokens: num(value.cache_write_tokens) ?? num(value.cacheWriteTokens),
            calls: num(value.calls),
            unpricedCalls: num(value.unpriced_calls),
            costByCategory: costByCategoryValue(value.cost_by_category),
            ...servedByField(value),
          })
          break
        case 'usage_progress':
          handlers.onUsageProgress?.(usageProgressFrame(value))
          break
        case 'awaiting_approval':
        case 'run_paused_for_approval':
          handlers.onStep?.({
            id: str(value.run_id) ?? str(value.runId),
            title: 'Paused',
            detail: str(value.approval_id) ?? 'Awaiting approval',
            status: 'paused',
          })
          break
        case 'run_resumed_after_approval':
          handlers.onStep?.({
            id: str(value.run_id) ?? str(value.runId),
            title: 'Resumed',
            detail: str(value.approval_id) ?? 'Approval resolved',
            status: 'running',
          })
          break
        case 'approval_state_changed':
          handlers.onStep?.({
            id: str(value.approval_id) ?? str(value.approvalId),
            title: 'Approval',
            // The wire kind is an enum token (APPROVAL_KIND_*); this module has no
            // locale to word it in, so the step carries the state alone.
            status: str(value.to) ?? str(value.state) ?? str(value.status),
          })
          break
        case 'browser_action_approval_required':
          handlers.onStep?.({
            id: str(value.run_id) ?? str(value.runId),
            title: 'Approval required',
            detail: [str(value.action_type) ?? str(value.actionType), str(value.reason)]
              .filter(Boolean)
              .join(' · '),
            status: 'waiting_approval',
          })
          break
        case 'browser_action_decided':
          handlers.onStep?.({
            id: str(value.run_id) ?? str(value.runId),
            title: 'Approval decided',
            detail: str(value.decision),
            status: str(value.decision),
          })
          break
        case 'approval_continuation_verified':
          handlers.onStep?.({
            id: str(value.receipt_id) ?? str(value.receiptId),
            title: 'Verified',
            detail: str(value.verification_status) ?? str(value.verificationStatus),
            status: 'done',
          })
          break
      }
      return true
    }
    case 'RAW':
      return true
    default:
      return false
  }
}

function dispatchEvent(event: SseEvent, handlers: ChatStreamHandlers): void {
  if (!event.data) return
  let payload: Record<string, unknown>
  try {
    payload = JSON.parse(event.data) as Record<string, unknown>
  } catch {
    return
  }

  const eventName = event.event ?? str(payload.type) ?? 'unknown'

  // Emit the product-owned projection before compatibility callbacks. This
  // lets append-only surfaces subscribe once without learning provider event
  // names, while existing focused handlers continue to drive their features.
  handlers.onUiEvent?.(toVerevonUiEvent(eventName, payload))

  // AG-UI events arrive through the same SSE transport but use a distinct
  // uppercase type vocabulary. Adapt them before the native switch; keeping
  // this branch explicit prevents an AG-UI `RAW`/`CUSTOM` payload from being
  // interpreted as a native Verevon mutation by accident.
  if (isAgUiEventName(eventName)) {
    if (!dispatchAgUiEvent(eventName, payload, handlers)) {
      handlers.onUnknownEvent?.({ name: eventName, payload })
    }
    return
  }

  switch (event.event) {
    case 'connected':
      handlers.onConnected?.({
        ok: typeof payload.ok === 'boolean' ? payload.ok : undefined,
        requestId: str(payload.request_id),
        threadId: str(payload.thread_id),
        model: str(payload.model_used) ?? str(payload.model),
        runId: str(payload.run_id),
        versionsEnabled: payload.versions_enabled === true ? true : undefined,
      })
      break
    case 'chunk': {
      const delta = str(payload.delta) ?? str(payload.content) ?? ''
      if (delta) handlers.onMessage?.({ content: delta, requestId: str(payload.request_id) })
      break
    }
    case 'done':
      handlers.onDone?.({
        requestId: str(payload.request_id),
        modelUsed: str(payload.model_used) ?? str(payload.modelUsed),
        outputTokens: num(payload.output_tokens) ?? num(payload.outputTokens),
        stopReason: str(payload.stop_reason) ?? str(payload.stopReason),
      })
      break
    case 'stopped':
      // Routed to its OWN handler. Previously this called `onDone`, so a
      // server-side stop (cancelled upstream, budget exhausted) rendered as a
      // normally-completed answer and the user had no way to tell.
      // `onStopped` falls back to `onDone` only for callers that predate it,
      // preserving their existing behaviour rather than silently going quiet.
      if (handlers.onStopped) {
        handlers.onStopped({
          requestId: str(payload.request_id),
          reason: str(payload.reason),
        })
      } else {
        handlers.onDone?.({ requestId: str(payload.request_id) })
      }
      break
    case 'error': {
      // Model Gateway sends its codes flat. The BFF reports a broken upstream
      // byte stream in its JSON error envelope instead
      // (`{"error":{"code":"upstream_unavailable",...}}`, see the gateway's
      // `sse_transport_error_event`). Reading only the flat fields turned that
      // transport break into an anonymous terminal "Stream error", so the
      // controller never attempted the resume a lost connection deserves.
      const envelope = payload.error && typeof payload.error === 'object'
        ? payload.error as Record<string, unknown>
        : undefined
      handlers.onError?.({
        code: str(payload.code) ?? str(envelope?.code) ?? 'error',
        message: str(payload.message) ?? str(envelope?.message) ?? 'Stream error',
        retryable: typeof payload.retryable === 'boolean' ? payload.retryable : undefined,
        runId: str(payload.run_id) ?? str(payload.runId),
        requestId: str(payload.request_id) ?? str(payload.requestId),
        ...failureTiming(payload),
        ...failureFeature(payload, envelope),
      })
      break
    }
    case 'artifact':
      handlers.onArtifact?.({
        id: str(payload.id),
        kind: str(payload.kind),
        title: str(payload.title),
        content: str(payload.content),
        version: num(payload.version),
        ...artifactProvenance(payload),
        ...artifactEditor(payload),
      })
      break
    case 'attachment':
      handlers.onAttachment?.({
        id: str(payload.id),
        name: str(payload.name),
        mime: str(payload.mime),
        type: str(payload.type),
        url: str(payload.url),
        size: num(payload.size),
        check: payload.check,
        preview: payload.preview,
      })
      break
    case 'reasoning_delta':
      handlers.onReasoning?.({ delta: str(payload.delta) ?? '' })
      break
    case 'tool_call':
      handlers.onToolCall?.({
        id: str(payload.id) ?? str(payload.tool_call_id) ?? str(payload.call_id),
        name: str(payload.name) ?? str(payload.tool) ?? str(payload.function_name),
        args: payload.args ?? payload.arguments,
      })
      break
    case 'tool_result':
      handlers.onToolResult?.({
        id: str(payload.id) ?? str(payload.tool_call_id) ?? str(payload.call_id),
        output: strOrJson(payload.output) ?? strOrJson(payload.result) ?? strOrJson(payload.data),
        error: strOrJson(payload.error),
        status: str(payload.status),
      })
      break
    case 'citation':
      emitCitation(payload, handlers)
      break
    case 'source_scope_applied': {
      const applied = sourceScopeAppliedEvent(payload)
      if (applied) handlers.onSourceScopeApplied?.(applied)
      break
    }
    case 'search_provenance': {
      const provenance = searchProvenanceEvent(payload)
      if (provenance) handlers.onSearchProvenance?.(provenance)
      break
    }
    case 'citations':
    case 'search_results': {
      const citations = Array.isArray(payload.citations)
        ? payload.citations
        : Array.isArray(payload.results)
          ? payload.results
          : []
      for (const citation of citations) emitCitation(citation, handlers)
      break
    }
    case 'grounding':
      handlers.onGrounding?.({ value: payload.grounding ?? payload })
      break
    case 'step_update':
      handlers.onStep?.({
        id: str(payload.id) ?? str(payload.step_id),
        title: str(payload.title) ?? str(payload.name),
        detail: str(payload.detail) ?? str(payload.message),
        status: str(payload.status),
        ...(event.id ? { frameId: event.id } : {}),
      })
      break
    case 'run_parked': {
      // A parked run (WS03 S8-A): display only. A reason this build does not know is not read as
      // a park; it is reported like any other event this client has not learned.
      const parked = runParkedFrameOf(payload)
      if (parked) handlers.onRunParked?.(parked)
      else handlers.onUnknownEvent?.({ name: 'run_parked', payload })
      break
    }
    case 'goal_progress': {
      // A settled goal turn (EP-7 S2): the bar's counts. Unknown keys are ignored; a frame the
      // client cannot read is reported, never guessed at.
      const progress = goalProgressEventOf(payload)
      if (progress) handlers.onGoalProgress?.(progress)
      else handlers.onUnknownEvent?.({ name: 'goal_progress', payload })
      break
    }
    case 'awaiting_approval':
      // Agentic runs can pause at the run boundary before a derived
      // `step_update` is available. Keep the same compatibility callback so
      // the controller rehydrates the authoritative approval queue.
      handlers.onStep?.({
        id: str(payload.run_id) ?? str(payload.runId),
        title: 'Approval',
        detail: 'Awaiting approval',
        status: 'awaiting_approval',
        ...(event.id ? { frameId: event.id } : {}),
      })
      break
    case 'usage': {
      const usage = payload as Partial<ChatEventPayloads['usage']> & Record<string, unknown>
      handlers.onUsage?.({
        inputTokens: num(usage.input_tokens),
        outputTokens: num(usage.output_tokens),
        costUsd: num(usage.cost_usd),
        costBasis: costBasisValue(usage.cost_basis),
        latencyMs: num(usage.latency_ms),
        confidence: num(usage.confidence),
        cacheReadTokens: num(usage.cache_read_tokens) ?? num(usage.cacheReadTokens),
        cacheWriteTokens: num(usage.cache_write_tokens) ?? num(usage.cacheWriteTokens),
        calls: num(usage.calls),
        unpricedCalls: num(usage.unpriced_calls),
        costByCategory: costByCategoryValue(usage.cost_by_category),
        ...servedByField(payload),
      })
      break
    }
    case 'usage_progress':
      handlers.onUsageProgress?.(usageProgressFrame(payload))
      break
    case 'turn_timing': {
      // Optional by contract: an older gateway never sends it, and a payload
      // without its totals is dropped rather than shown as a zero.
      const timing = parseTurnTiming(payload)
      if (timing) handlers.onTurnTiming?.(timing)
      break
    }
    case 'result_receipt': {
      // Named explicitly: it used to fall through to `onUnknownEvent` and a
      // console warning on every checked turn.
      const receipt = objectValue(payload.receipt)
      if (receipt) handlers.onResultReceipt?.({ receipt, requestId: str(payload.request_id) })
      break
    }
    case 'title': {
      const title = str(payload.title)?.trim()
      if (title) handlers.onTitle?.({ title, requestId: str(payload.request_id) })
      break
    }
    case 'follow_ups': {
      const suggestions = Array.isArray(payload.suggestions)
        ? payload.suggestions.filter((item): item is string => typeof item === 'string' && item.trim().length > 0)
        : []
      if (suggestions.length > 0) {
        handlers.onFollowUps?.({ suggestions, requestId: str(payload.request_id) })
      }
      break
    }
    case 'plan_proposed': {
      const planId = str(payload.plan_id)
      const summary = str(payload.summary)
      const rawSteps = Array.isArray(payload.steps) ? payload.steps : []
      const steps = rawSteps
        .map((item): ChatPlanStep | null => {
          const step = objectValue(item)
          if (!step) return null
          const id = str(step.id)
          const title = str(step.title)
          if (!id || !title) return null
          return { id, title, operation: str(step.operation) }
        })
        .filter((step): step is ChatPlanStep => step !== null)
      // A plan with no readable id/summary is a malformed frame, not a real
      // proposal — dropping it degrades to today's rung-only
      // PlanApprovalControl, exactly the fallback plan-gate's rollout note
      // calls for, rather than rendering a broken edit surface.
      if (planId && summary) {
        handlers.onPlanProposed?.({ planId, summary, steps, requestId: str(payload.request_id) })
      }
      break
    }
    case 'user_question': {
      // A question with no id or no answerable item is a malformed frame. The prose the turn ends with
      // still carries the question, so dropping the card loses nothing the person needs to read.
      const questionId = str(payload.question_id)
      const items = normalizeQuestionItems(payload.items)
      if (questionId && items.length > 0) {
        handlers.onUserQuestion?.({
          questionId,
          delivery: str(payload.delivery) || 'next_turn',
          expiresAt: str(payload.expires_at) || undefined,
          items,
          requestId: str(payload.request_id),
          ...questionOrigin(payload),
        })
      }
      break
    }
    case 'connector_suggestion': {
      handlers.onConnectorSuggestion?.({
        suggestionId: str(payload.suggestion_id),
        provider: str(payload.provider),
        reason: str(payload.reason),
        requestId: str(payload.request_id),
      })
      break
    }
    case 'research_plan': {
      // The plan card of a deep-research plan turn (EP-2 S5). An empty plan still shows the card,
      // where the person can add a sub-query; a cap the frame does not state is the gateway default.
      const capValue = num(objectValue(payload.caps)?.max_sub_queries)
      handlers.onResearchPlan?.({
        subQueries: stringList(payload.sub_queries),
        questions: stringList(payload.questions),
        maxSubQueries:
          capValue !== undefined && Number.isInteger(capValue) && capValue > 0
            ? capValue
            : RESEARCH_PLAN_DEFAULT_MAX_SUB_QUERIES,
        requestId: str(payload.request_id),
      })
      break
    }
    case 'verification': {
      // Only the three verdicts the backend defines. An unrecognized one is a
      // contract drift, and rendering "checked" from a value we cannot read
      // would be worse than rendering nothing.
      const verdict = payload.verdict
      if (verdict === 'supports' || verdict === 'contradicts' || verdict === 'unrelated') {
        handlers.onVerification?.({
          verdict,
          kbCitations: num(payload.kb_citations) ?? 0,
          webCitations: num(payload.web_citations) ?? 0,
          webAllowed: payload.web_allowed === true,
        })
      }
      break
    }
    case 'memory_recall': {
      const count = num(payload.count)
      // The backend only emits this when memory was genuinely injected, so a
      // 0/absent count means a malformed payload — not "recalled nothing".
      // Dropping it is better than rendering "recalled 0 memories".
      if (count !== undefined && count > 0) {
        handlers.onMemoryRecall?.({
          count,
          latencyMs: num(payload.latency_ms),
          memories: normalizeRecalledMemories(payload.memories),
        })
      }
      break
    }
    case 'queued_input': {
      const messages: string[] = []
      const ids: string[] = []
      if (Array.isArray(payload.messages)) {
        // `ids` is parallel to `messages`: the pair is kept together, so dropping an unreadable
        // message drops its id and never shifts the rest.
        const wireIds: unknown[] = Array.isArray(payload.ids) ? payload.ids : []
        payload.messages.forEach((item: unknown, index: number) => {
          if (typeof item !== 'string' || item.trim().length === 0) return
          messages.push(item)
          const id = wireIds[index]
          ids.push(typeof id === 'string' ? id : '')
        })
      }
      // A delivery with no readable text is a malformed frame, not a delivery.
      // Reporting it would tell the user their message arrived when we cannot
      // show which one.
      if (messages.length > 0) {
        handlers.onQueuedInput?.(ids.some((id) => id.length > 0) ? { messages, ids } : { messages })
      }
      break
    }
    case 'retrieval_notice': {
      // Control: a reduced answer is told to the user whatever features were opted into. A notice without its
      // reason and wording key is a malformed frame, so it is not reported as a reduction.
      const code = str(payload.code)
      const wordingKey = str(payload.wording_key)
      if (code && wordingKey) {
        handlers.onRetrievalNotice?.({ code, wordingKey, requestId: str(payload.request_id) })
      }
      break
    }
    case 'retry_scheduled': {
      // A retry with no scope, reason or counts cannot say what was retried, so it is not reported.
      const scope = str(payload.scope)
      const reason = str(payload.reason)
      const attempt = num(payload.attempt)
      const maxAttempts = num(payload.max_attempts)
      if (scope && reason && attempt !== undefined && maxAttempts !== undefined) {
        handlers.onRetryScheduled?.({
          scope,
          tool: str(payload.tool),
          attempt,
          maxAttempts,
          waitMs: num(payload.wait_ms),
          reason,
          requestId: str(payload.request_id),
        })
      }
      break
    }
    case 'compaction': {
      const scope = str(payload.scope)
      const outcome = str(payload.outcome)
      if (scope && outcome) {
        handlers.onCompaction?.({
          scope,
          outcome,
          messagesCompacted: num(payload.messages_compacted),
          messagesKept: num(payload.messages_kept),
          pinnedKept: num(payload.pinned_kept),
          requestId: str(payload.request_id),
        })
      }
      break
    }
    case 'context_change': {
      const change = str(payload.change)
      if (change) {
        handlers.onContextChange?.({
          change,
          messagesDropped: num(payload.messages_dropped),
          skillsTruncated: num(payload.skills_truncated),
          skillsDropped: num(payload.skills_dropped),
          layer: str(payload.layer),
          state: str(payload.state),
          requestId: str(payload.request_id),
        })
      }
      break
    }
    case 'sandbox_state': {
      // A frame that names neither the workspace nor its state cannot say where the files went, so it is not reported.
      const scope = str(payload.scope)
      const state = str(payload.state)
      if (scope && state) {
        handlers.onSandboxState?.({ scope, state, cause: str(payload.cause), requestId: str(payload.request_id) })
      }
      break
    }
    default: {
      // STREAM_* envelopes are emitted by some upstreams and are noise here.
      // Everything else is a backend event this client has not learned yet:
      // surface it instead of dropping it, so adding an event server-side is
      // discoverable rather than invisible.
      //
      // `event.event` is optional on the wire type, and an unnamed frame is not
      // a discoverable new event — it is malformed. Reporting it as one would
      // put `undefined` in the log line this hook exists to make useful.
      const name = event.event
      if (name && !name.startsWith('STREAM_')) {
        handlers.onUnknownEvent?.({ name, payload })
      }
      break
    }
  }
}

/**
 * The `onError` a failed stream connection reports.
 *
 * Only a genuine transport failure is `connection_error`, which the chat
 * controller answers by reattaching to the same invocation. A 401 or 403 means
 * the browser session is no longer accepted: the run itself is unaffected, but
 * no reconnect can succeed until the user signs in again, so reporting it as a
 * lost connection told them the wrong thing and retried into the same refusal.
 *
 * A zero-retention refusal is the server's answer, not a broken connection, so
 * it keeps its code and feature (and a 403 for it is not a sign-in problem). It
 * is checked first, for that reason.
 */
function streamConnectionFailure(err: unknown, fallback: string): ChatErrorEvent {
  if (err instanceof ApiError && err.code !== null && isZdrCode(err.code)) {
    return {
      code: err.code,
      message: err.message,
      ...(err.feature ? { feature: err.feature } : {}),
    }
  }
  // A typed Space denial (a policy, entitlement, role or ZDR refusal) is the
  // server's answer about the room, not a sign-in problem: keep its code and
  // details so the room can say why. An untyped 403 still reads as lost access.
  if (err instanceof ApiError && (err.status === 403 || err.status === 503) && isSpaceDenial(err)) {
    return {
      code: err.code ?? 'forbidden',
      message: err.message,
      ...(err.details ? { details: err.details } : {}),
    }
  }
  if (err instanceof ApiError && err.status === 401) {
    return {
      code: 'unauthorized',
      message: 'Økten din er utløpt. Logg inn på nytt for å se svaret.',
    }
  }
  if (err instanceof ApiError && err.status === 403) {
    return {
      code: 'forbidden',
      message: 'Du har ikke lenger tilgang til denne samtalen. Logg inn på nytt og prøv igjen.',
    }
  }
  return { code: 'connection_error', message: err instanceof Error ? err.message : fallback }
}

/**
 * WS09 S4: with `VITE_CHAT_DELTA_FRAME_BATCH` set to exactly `true`, a stream's
 * text callbacks go through a frame batch. The dispatcher is handed an object
 * whose text callbacks feed the coalescer, and the coalescer emits to the real
 * ones. Off, there is no batch and the stream dispatches as it always did.
 */
type FrameBatch = { deltas: DeltaCoalescer; handlers: ChatStreamHandlers }

function frameBatchFor(handlers: ChatStreamHandlers, signal?: AbortSignal): FrameBatch | undefined {
  if (import.meta.env.VITE_CHAT_DELTA_FRAME_BATCH !== 'true') return undefined
  const deltas = createDeltaCoalescer((run) => {
    if (run.kind === 'message') handlers.onMessage?.({ content: run.text, requestId: run.requestId })
    else handlers.onReasoning?.({ delta: run.text })
  }, signal)
  // Inherits every other callback from `handlers`, which is then read at dispatch time as before.
  const textHandlers: ChatStreamHandlers = Object.create(handlers)
  textHandlers.onMessage = ({ content, requestId }) => deltas.push('message', requestId, content)
  textHandlers.onReasoning = ({ delta }) => deltas.push('reasoning', undefined, delta)
  return { deltas, handlers: textHandlers }
}

/** One SSE frame: the resume filter first, then the cursor, then the event. */
function dispatchFrame(event: SseEvent, handlers: ChatStreamHandlers, batch?: FrameBatch): void {
  if (handlers.acceptFrame && !handlers.acceptFrame(event.id || undefined)) return
  if (event.id) handlers.onFrameId?.(event.id)
  if (!batch) {
    dispatchEvent(event, handlers)
    return
  }
  if (event.event === 'chunk' || event.event === 'reasoning_delta') {
    dispatchEvent(event, batch.handlers)
    return
  }
  // Text still waiting for its frame reaches the handler before a frame that is not a delta.
  batch.deltas.flush()
  dispatchEvent(event, handlers)
}

export async function streamChat(
  request: ChatInvokeRequest,
  handlers: ChatStreamHandlers,
  signal?: AbortSignal,
  lastEventId?: string,
): Promise<void> {
  let connError: unknown

  let body: string
  try {
    body = JSON.stringify(buildChatWireBody(request))
  } catch (err) {
    // Refused locally before any request: the typed code reaches the chat the
    // way a server refusal does, instead of a rejected promise nobody awaits.
    if (!(err instanceof AttachmentsNotSentError)) throw err
    handlers.onError?.({ code: err.code, message: err.message })
    return
  }

  const batch = frameBatchFor(handlers, signal)
  await readSseStream(
    '/api/v1/chat/stream',
    {
      method: 'POST',
      body,
      signal,
      lastEventId,
    },
    (event) => dispatchFrame(event, handlers, batch),
    (err) => {
      connError = err
    },
  )
  // The stream ended, by close, error or abort: the text still waiting for its
  // frame goes out before the connection error is reported.
  batch?.deltas.close()

  if (connError) {
    handlers.onError?.(streamConnectionFailure(connError, 'Connection failed'))
  }
}

export async function resumeStream(
  requestId: string,
  handlers: ChatStreamHandlers,
  signal?: AbortSignal,
  lastEventId?: string,
): Promise<void> {
  let connError: unknown

  const batch = frameBatchFor(handlers, signal)
  await readSseStream(
    `/api/v1/chat/stream/resume/${encodeURIComponent(requestId)}`,
    { method: 'GET', signal, lastEventId },
    (event) => dispatchFrame(event, handlers, batch),
    (err) => {
      connError = err
    },
  )
  batch?.deltas.close()

  if (connError) {
    handlers.onError?.(streamConnectionFailure(connError, 'Resume failed'))
  }
}

export async function cancelInvocation(requestId: string): Promise<void> {
  await requestJson(`/api/v1/chat/invocations/${encodeURIComponent(requestId)}/cancel`, {
    method: 'POST',
  })
}

/**
 * How much a run is permitted to do. A strictly ordered ladder — each rung
 * includes everything below it.
 *
 * Mirrors the Model Plane's `AutonomyRung`. Sent as a keyword, not a number: a
 * number that means something else on the other side is a silent mis-grant.
 */
export type AutonomyRung = 'read_only' | 'workspace_write' | 'danger_full_access'

/**
 * Shortest justification the Model Plane accepts.
 *
 * Mirrored here so the UI can say so BEFORE the request. Not zero-plus-one on
 * purpose: 'ok' and 'ja' are non-empty and say nothing, and a person has to read
 * this to decide.
 */
export const MIN_PLAN_JUSTIFICATION_CHARS = 12

export type PlanApprovalResult = {
  grantedRung: AutonomyRung
  /** False when the run was not actually in plan mode — an idempotent re-approve. */
  wasInPlanMode: boolean
  /**
   * False when the grant reached the running agent but was not written to the
   * run. Surfaced rather than swallowed: the next turn would then not carry it,
   * and the agent would refuse work it was just authorized to do.
   */
  persisted: boolean
  /** The finalized, post-edit plan (plan-gate §2.3) — absent when this turn
   * never proposed a structured plan (the pure rung-grant path, unchanged). */
  plan?: Plan
  /**
   * When the grant the gateway just recorded lapses, in epoch milliseconds
   * (`grant_expires_at_unix` on the wire is seconds). Absent for an older
   * gateway, a null, or a value that is not a positive finite number. A pointer
   * for the UI only: the gateway's grant is the authority, never this value.
   */
  grantExpiresAtMs?: number
  /**
   * What the client does next (WS14 S2). `execute_now`: the gateway has recorded the grant for
   * this plan and will run one execute turn that names it, so the client sends that turn now.
   * `next_message` (the answer of every gateway without the switch, and of any approval the
   * gateway did not record for the plan): the grant waits for the person's own next message.
   */
  continuation: 'execute_now' | 'next_message'
}

/**
 * Approve a planning run: grant it the authority to execute, at a named rung,
 * with a stated reason. When `planId` is set (a structured plan was proposed,
 * plan-gate §2.1), the user's `editedSteps` are applied and the plan
 * transitions to APPROVED atomically before the rung is granted — an
 * autonomy grant with no corresponding approved plan is exactly the
 * "rubber stamp" this control exists to prevent.
 *
 * Both `grantedRung`/`justification` are required by the Model Plane — an
 * approval with no reason, or a reason granting nothing, is refused rather
 * than recorded as an empty justification.
 */
export async function approvePlan(
  runId: string,
  grantedRung: AutonomyRung,
  justification: string,
  planId?: string,
  editedSteps?: PlanStepEdit[],
): Promise<PlanApprovalResult> {
  const raw = await requestJson<Record<string, unknown>>(
    `/api/v1/agents/runs/${encodeURIComponent(runId)}/plan-approval`,
    {
      method: 'POST',
      body: JSON.stringify({
        decision: 'approve',
        granted_rung: grantedRung,
        justification,
        plan_id: planId ?? '',
        edited_steps: (editedSteps ?? []).map(planStepEditToWire),
      }),
    },
  )
  const plan = objectValue(raw.plan)
  return {
    grantedRung: (typeof raw.granted_rung === 'string'
      ? raw.granted_rung
      : grantedRung) as AutonomyRung,
    wasInPlanMode: raw.was_in_plan_mode === true,
    persisted: raw.persisted !== false,
    plan: plan ? (normalizePlan(plan) ?? undefined) : undefined,
    ...(typeof raw.grant_expires_at_unix === 'number' &&
    Number.isFinite(raw.grant_expires_at_unix) &&
    raw.grant_expires_at_unix > 0
      ? { grantExpiresAtMs: raw.grant_expires_at_unix * 1000 }
      : {}),
    continuation: raw.continuation === 'execute_now' ? 'execute_now' : 'next_message',
  }
}

/**
 * Reject a proposed plan (plan-gate §2.3): grants nothing, closes the plan out
 * as REJECTED. The composer's plan-mode toggle is turned off the same way an
 * approve does, so the user must ask again in a fresh message rather than
 * silently retrying the same turn (owner ruling, 2026-09-27: no
 * revise-after-reject in this wave).
 */
export async function rejectPlan(
  runId: string,
  planId: string,
  reason: string,
): Promise<{ plan?: Plan }> {
  const raw = await requestJson<Record<string, unknown>>(
    `/api/v1/agents/runs/${encodeURIComponent(runId)}/plan-approval`,
    {
      method: 'POST',
      body: JSON.stringify({
        decision: 'reject',
        justification: reason,
        plan_id: planId,
        edited_steps: [],
      }),
    },
  )
  const plan = objectValue(raw.plan)
  return { plan: plan ? (normalizePlan(plan) ?? undefined) : undefined }
}

/**
 * What happened to a message sent while a run was still streaming.
 *
 * Every arm is distinct because each needs a different response from the UI, and
 * collapsing them is what made the original behaviour invisible: the send path
 * simply returned, and the user's words were gone.
 */
export type QueuedInputResult =
  /**
   * Accepted for delivery to the running agent at its next tool-round boundary.
   * With send receipts on, `clientMessageId` and `messageId` name the stored message.
   */
  | {
      outcome: 'queued'
      pending: number
      persisted: boolean
      clientMessageId?: string
      messageId?: string
    }
  /**
   * The run finished before the message landed. Not an error — the caller should
   * send it as an ordinary turn instead, under the same id, so nothing is lost and
   * the transcript holds it once (`persisted`: it was already stored).
   */
  | { outcome: 'run_ended'; persisted?: boolean; clientMessageId?: string }
  /**
   * Refused with a reason worth showing: too many pending, too long, or the gateway
   * could not say (`unavailable`, retryable).
   */
  | {
      outcome: 'refused'
      reason: 'too_many' | 'too_long' | 'unavailable' | 'other'
      message: string
    }

/**
 * Send a message to a run that is already streaming.
 *
 * `spaceRef` and `threadId` are hints, not authority: the BFF strips every
 * authority field and mints a fresh, content-bound append decision from Control
 * for this exact message, and the Model Plane appends to the thread its own
 * stream registration recorded — never to a thread named here.
 */
export async function queueInvocationInput(
  requestId: string,
  content: string,
  options: { threadId?: string; spaceRef?: string; clientMessageId?: string } = {},
): Promise<QueuedInputResult> {
  try {
    const raw = await requestJson<Record<string, unknown>>(
      `/api/v1/chat/invocations/${encodeURIComponent(requestId)}/queue`,
      {
        method: 'POST',
        body: JSON.stringify({
          content,
          ...(options.threadId ? { thread_id: options.threadId } : {}),
          ...(options.spaceRef ? { space_ref: options.spaceRef } : {}),
          // The receipt: what the gateway stores the message under and names in the delivery.
          ...(options.clientMessageId ? { client_message_id: options.clientMessageId } : {}),
        }),
      },
    )
    // The BFF translates Model Gateway's "stream just ended" 404 into a
    // successful protocol response. It is an expected race and the caller
    // replays the text as a new turn; treating it as a failed HTTP request only
    // adds a misleading red entry to DevTools. Keep accepting a legacy 404 in
    // the catch below while older gateways roll out.
    if (raw.queued === false && raw.resend_as_new_turn === true) {
      return {
        outcome: 'run_ended',
        ...(typeof raw.persisted === 'boolean' ? { persisted: raw.persisted } : {}),
        ...(typeof raw.client_message_id === 'string' ? { clientMessageId: raw.client_message_id } : {}),
      }
    }
    return {
      outcome: 'queued',
      pending: typeof raw.pending === 'number' ? raw.pending : 1,
      // `false` means the run WILL read the message but the thread does not
      // record it. Surfaced rather than swallowed: the reply would otherwise
      // appear in history answering nothing.
      persisted: raw.persisted !== false,
      ...(typeof raw.client_message_id === 'string' ? { clientMessageId: raw.client_message_id } : {}),
      ...(typeof raw.message_id === 'string' ? { messageId: raw.message_id } : {}),
    }
  } catch (error) {
    if (!(error instanceof ApiError)) {
      return {
        outcome: 'refused',
        reason: 'other',
        message:
          error instanceof Error
            ? error.message
            : 'Kunne ikke levere meldingen.',
      }
    }
    // 404 is the ordinary race, not a failure: the stream ended between the
    // keystroke and the request.
    if (error.status === 404) return { outcome: 'run_ended' }
    if (error.status === 429) {
      return { outcome: 'refused', reason: 'too_many', message: error.message }
    }
    if (error.status === 413) {
      return { outcome: 'refused', reason: 'too_long', message: error.message }
    }
    // The gateway could not tell whether the message was queued (the run-control bus did not
    // answer, or the message could not be recorded): nothing was claimed, and sending again
    // under the same id stores it once.
    if (error.status === 503) {
      return { outcome: 'refused', reason: 'unavailable', message: error.message }
    }
    return { outcome: 'refused', reason: 'other', message: error.message }
  }
}

/** One segment of the assembled context window. */
export type ContextSegment = {
  kind: string
  content: string
  estimatedTokens: number
}

/**
 * What is actually in the model's context window for a thread.
 *
 * session-core has itemized this all along and the gateway already called it to
 * BUILD prompts — it was simply never exposed, so the one surface that could
 * answer "why did it answer from that?" was unreachable from the UI.
 */
export type ThreadContext = {
  threadId: string
  /** The assembler's own total — reported, not summed from the segments. */
  estimatedTokens: number
  /** The budget the segments were assembled against. */
  budgetTokens: number
  segments: ContextSegment[]
}

/** One document ingested from a chat attachment. */
export type ChatDocumentUpload = {
  documentId: string
  status: string
}

/** Where a chat document came from; the server refuses any other origin (EP-3 S8). */
export type ChatDocumentOrigin = 'chat-attachment' | 'chat-artifact'

/** The artifact revision a `chat-artifact` copy was made from, kept in the document's source trace. */
export type ChatDocumentSourceRef = {
  threadId: string
  artifactId: string
  version: number
}

/**
 * Ingest an attached TEXT document so the conversation can retrieve it.
 *
 * `POST /api/v1/chat/documents` existed with zero callers: the composer
 * accepted nine document types, showed a chip for each, and then discarded
 * every non-image file on send while the transcript still said "1 attachment
 * added". This is the route that closes that gap.
 *
 * Text only, and that is the route's own contract rather than a shortcut: the
 * handler takes JSON with a `content` STRING (it creates a Data Plane document
 * from text), so a PDF or DOCX cannot go through it. Those keep using the
 * per-file "Add to knowledge base" action, which posts real `File` bytes to
 * imports-core and lets the server extract them.
 *
 * Refused server-side under ZDR (412) — a temporary chat must not leave a
 * durable document behind. Callers should skip it rather than rely on that.
 *
 * `options.origin` `chat-artifact` with `sourceRef` saves one artifact
 * revision to knowledge (EP-3 S8, action `chat.save_artifact_to_knowledge`):
 * the copy is the text sent here, independent of the thread, and the server
 * honours it only while its promote switch is on. Without options the body is
 * exactly the attachment body it always was.
 */
export async function uploadChatDocument(
  title: string,
  content: string,
  options: { origin?: ChatDocumentOrigin; sourceRef?: ChatDocumentSourceRef } = {},
): Promise<ChatDocumentUpload> {
  const body: Record<string, unknown> = { title, content, source: 'chat-upload', type: 'text' }
  if (options.origin) body.origin = options.origin
  if (options.sourceRef) {
    body.source_ref = {
      thread_id: options.sourceRef.threadId,
      artifact_id: options.sourceRef.artifactId,
      version: options.sourceRef.version,
    }
  }
  const raw = await requestJson<Record<string, unknown>>(
    '/api/v1/chat/documents',
    {
      method: 'POST',
      body: JSON.stringify(body),
    },
  )
  return {
    documentId: typeof raw.document_id === 'string' ? raw.document_id : '',
    status: typeof raw.status === 'string' ? raw.status : '',
  }
}

/** Which saved documents the chat library lists: generated artifact copies, uploaded text, or both (F-t3-080). */
export type ChatLibraryKind = 'generated' | 'uploaded' | 'all'

/** One chat library row: the document's metadata. Its text is read by id, never carried on a page. */
export type ChatLibraryItem = {
  documentId: string
  title: string
  kind: 'generated' | 'uploaded'
  ownerId: string
  visibility: string
  contentBytes: number
  createdAt: string
  updatedAt: string
  /** The thread a generated copy was saved from, from its source trace. Absent for an upload. */
  threadId?: string
}

/** One page of the chat library and the total it pages over. */
export type ChatLibraryPage = {
  items: ChatLibraryItem[]
  total: number
}

function chatLibraryItem(value: unknown): ChatLibraryItem | null {
  const record = objectValue(value)
  if (!record || typeof record.document_id !== 'string' || !record.document_id) return null
  const metadata = objectValue(record.metadata)
  const threadValue = objectValue(metadata?.source_ref)?.thread_id
  const threadId = typeof threadValue === 'string' && threadValue ? threadValue : undefined
  return {
    documentId: record.document_id,
    title: typeof record.title === 'string' ? record.title : '',
    kind: metadata?.origin === 'chat-artifact' ? 'generated' : 'uploaded',
    ownerId: typeof record.owner_id === 'string' ? record.owner_id : '',
    visibility: typeof record.visibility === 'string' ? record.visibility : '',
    contentBytes: typeof record.content_bytes === 'number' ? record.content_bytes : 0,
    createdAt: typeof record.created_at === 'string' ? record.created_at : '',
    updatedAt: typeof record.updated_at === 'string' ? record.updated_at : '',
    threadId,
  }
}

/**
 * One page of the person's chat library (F-t3-080), newest first. Read only: nothing is stored.
 * The gateway answers 404 `library_disabled` while the artifact-promote switch is off, so a caller
 * reads a failure as "no library" and shows nothing.
 */
export async function listChatLibrary(
  options: { kind?: ChatLibraryKind; limit?: number; offset?: number } = {},
): Promise<ChatLibraryPage> {
  const params = new URLSearchParams({
    kind: options.kind ?? 'all',
    limit: String(options.limit ?? 20),
    offset: String(options.offset ?? 0),
  })
  const raw = await requestJson<unknown>(`/api/v1/chat/library?${params.toString()}`)
  const record = objectValue(raw)
  const documents = Array.isArray(record?.documents) ? record.documents : []
  return {
    items: documents.flatMap((value) => chatLibraryItem(value) ?? []),
    total: typeof record?.total === 'number' ? record.total : 0,
  }
}

/** The text of one library document, read by id through the knowledge document route. */
export async function readChatLibraryText(documentId: string): Promise<string> {
  const raw = await requestJson<unknown>(`/api/v1/knowledge/documents/${encodeURIComponent(documentId)}`)
  const content = objectValue(raw)?.content
  return typeof content === 'string' ? content : ''
}

export async function getThreadContext(
  threadId: string,
  runId?: string,
): Promise<ThreadContext> {
  const query = runId?.trim()
    ? `?run_id=${encodeURIComponent(runId.trim())}`
    : ''
  const raw = await requestJson<Record<string, unknown>>(
    `/api/v1/chat/threads/${encodeURIComponent(threadId)}/context${query}`,
  )
  const segments = Array.isArray(raw?.segments) ? raw.segments : []
  return {
    threadId: typeof raw?.thread_id === 'string' ? raw.thread_id : threadId,
    estimatedTokens:
      typeof raw?.estimated_tokens === 'number' ? raw.estimated_tokens : 0,
    budgetTokens:
      typeof raw?.budget_tokens === 'number' ? raw.budget_tokens : 0,
    segments: segments.flatMap((entry): ContextSegment[] => {
      if (!entry || typeof entry !== 'object') return []
      const segment = entry as Record<string, unknown>
      const kind = typeof segment.kind === 'string' ? segment.kind : ''
      // A segment with no kind cannot be labelled, and an unlabelled block of
      // prompt text in an inspector is worse than omitting it. Deliberate, and
      // covered by "drops a segment with no kind ..." in chat-client.test.ts:
      // the dropped payload can be raw prompt content the product has not
      // chosen to expose, so relabelling it "unknown" would leak rather than
      // clarify. The trade-off is that segments need not sum to
      // `estimatedTokens`; the panel reports the assembler's own total.
      if (!kind) return []
      return [
        {
          kind,
          content: typeof segment.content === 'string' ? segment.content : '',
          estimatedTokens:
            typeof segment.estimated_tokens === 'number'
              ? segment.estimated_tokens
              : 0,
        },
      ]
    }),
  }
}

/**
 * WS09 S9. With `VITE_CHAT_PAGED_TRANSCRIPT` set to exactly `true`, a thread is
 * read in pages: the newest page on open, older pages on request. Off, the whole
 * thread is read in one call, as it always was. Read at call time so a test can
 * stub it.
 */
export function pagedTranscriptEnabled(): boolean {
  return import.meta.env.VITE_CHAT_PAGED_TRANSCRIPT === 'true'
}

/** A page of a thread's messages, oldest first. */
export type ChatThreadMessagesPage = {
  messages: ChatMessage[]
  /** True when older messages exist before `messages[0]`. */
  hasMore: boolean
}

export type ChatThreadMessagesPageRequest = {
  /** How many of the newest messages to return; the server clamps it. */
  limit: number
  /** The id of the oldest message already held: the page is the messages before it. */
  beforeMessageId?: string
}

export function getThreadMessages(threadId: string): Promise<ChatMessage[]>
export function getThreadMessages(
  threadId: string,
  page: ChatThreadMessagesPageRequest,
): Promise<ChatThreadMessagesPage>
export async function getThreadMessages(
  threadId: string,
  page?: ChatThreadMessagesPageRequest,
): Promise<ChatMessage[] | ChatThreadMessagesPage> {
  const search = new URLSearchParams()
  if (page) {
    search.set('limit', String(page.limit))
    if (page.beforeMessageId) search.set('before', page.beforeMessageId)
  }
  const query = search.toString()
  const raw = await requestJson<unknown>(
    `/api/v1/chat/threads/${encodeURIComponent(threadId)}/messages${query ? `?${query}` : ''}`,
  )
  const messages = normalizeThreadMessages(raw)
  if (!page) return messages
  const record = objectValue(raw)
  const oldestId = objectValue(Array.isArray(record?.messages) ? record?.messages[0] : undefined)?.message_id
  // A page can only be continued from a message the server can name. A row
  // written before ids were returned has none, so the page it starts cannot
  // anchor the next one, and the reader is not offered a page it cannot ask for.
  const anchored = typeof oldestId === 'string' && oldestId !== ''
  return { messages, hasMore: record?.has_more === true && anchored }
}

/**
 * Apply the server's retention verdict carried on a threads response.
 *
 * Deliberately applied HERE rather than at each call site. `listChatThreads`
 * has three independent callers (the sidebar, the dashboard composer, the chat
 * controller); a policy any one of them can forget to apply is the same class
 * of defect as the client-side ZDR gate this replaces. This is the single point
 * every server answer passes through.
 */
function applyRetentionFrom(raw: unknown): void {
  const record = objectValue(raw)
  const data = objectValue(record?.data) ?? record
  const retention = objectValue(data?.retention)
  const zdr = retention?.zdr
  applyServerRetention(typeof zdr === 'boolean' ? zdr : undefined)
}

export type ChatThreadPage = {
  sessions: ChatThreadSession[]
  /** Keyset cursor for the next page; null on the last page. */
  nextCursor: string | null
}

export type ChatThreadArchiveFilter = 'active' | 'archived' | 'all'

/**
 * One page of the owner's threads, in the order Session Core sorts them. Does
 * not reconcile local copies: a single page is never the whole listing.
 */
export async function listChatThreadsPage(
  options: { cursor?: string; archived?: ChatThreadArchiveFilter; starredOnly?: boolean } = {},
): Promise<ChatThreadPage> {
  const params = new URLSearchParams()
  if (options.cursor) params.set('cursor', options.cursor)
  if (options.archived) params.set('archived', options.archived)
  if (options.starredOnly) params.set('starred_only', 'true')
  const query = params.toString()
  const raw = await requestJson<unknown>(`/api/v1/chat/threads${query ? `?${query}` : ''}`)
  applyRetentionFrom(raw)
  // requestJson hands back `data` only, so the BFF repeats the cursor there.
  return {
    sessions: normalizeChatThreadSessions(raw),
    nextCursor: str(objectValue(raw)?.nextCursor) ?? null,
  }
}

/**
 * WS09 S6. With `VITE_CHAT_PARALLEL_THREAD_LOAD` set to exactly `true`, a call
 * that arrives while a listing walk is running takes that walk's result instead
 * of paging the whole listing again. Off, every call walks, as it always did.
 * Read at call time so a test can stub it.
 */
export function parallelThreadLoadEnabled(): boolean {
  return import.meta.env.VITE_CHAT_PARALLEL_THREAD_LOAD === 'true'
}

let listChatThreadsInFlight: Promise<ChatThreadSession[]> | null = null

export async function listChatThreads(): Promise<ChatThreadSession[]> {
  if (!parallelThreadLoadEnabled()) return listAllChatThreadPages()
  // The sidebar, the dashboard and the retention check all list at mount. Each
  // caller gets its own array, so one that sorts its copy cannot reorder another's.
  if (!listChatThreadsInFlight) {
    listChatThreadsInFlight = listAllChatThreadPages().finally(() => {
      listChatThreadsInFlight = null
    })
  }
  return [...(await listChatThreadsInFlight)]
}

async function listAllChatThreadPages(): Promise<ChatThreadSession[]> {
  // Walk every page before reconciling: a thread on page two is still listed,
  // and reconciling against page one alone would erase its local copy. A cursor
  // the walk has already requested ends it, so a server that stops advancing
  // cannot loop the client; that walk is incomplete and reconciles nothing.
  const sessions: ChatThreadSession[] = []
  const requested = new Set<string>()
  let cursor: string | undefined
  let complete = false
  for (;;) {
    const page = await listChatThreadsPage(cursor ? { cursor } : {})
    sessions.push(...page.sessions)
    if (page.nextCursor === null) {
      complete = true
      break
    }
    if (requested.has(page.nextCursor)) break
    requested.add(page.nextCursor)
    cursor = page.nextCursor
  }
  // Reached only on a SUCCESSFUL listing — `requestJson` throws otherwise — so
  // an empty array here really means "the server has no threads for you", not
  // "the request failed". That distinction is what makes this safe to act on:
  // a thread the server no longer lists has been erased upstream, and its local
  // copy must go with it. This is the only way a GDPR erasure reaches the
  // device at all. Only a walk that reached the last page is the whole listing,
  // so only that walk may reconcile.
  if (complete) {
    reconcileLocalThreads(sessions.map((session) => session.threadId))
  }
  return sessions
}

/**
 * One thread's session by id, so a thread past the first page resolves like
 * any other. Null when the server no longer has it as a chat thread.
 */
export async function getChatThread(threadId: string): Promise<ChatThreadSession | null> {
  let raw: unknown
  try {
    raw = await requestJson<unknown>(`/api/v1/chat/threads/${encodeURIComponent(threadId)}`)
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) return null
    throw error
  }
  applyRetentionFrom(raw)
  return normalizeChatThreadSession(objectValue(raw)?.session)
}

/**
 * Hide a thread from the ordinary list, or bring it back. Both return the
 * thread's session as the server now holds it (`archivedAt` set or absent), and
 * null when the server no longer has it as a chat thread.
 */
export async function archiveChatThread(threadId: string): Promise<ChatThreadSession | null> {
  parseChatHistoryAction({ type: 'thread.archive', threadId })
  return postChatThreadArchiveState(threadId, 'archive')
}

export async function unarchiveChatThread(threadId: string): Promise<ChatThreadSession | null> {
  parseChatHistoryAction({ type: 'thread.unarchive', threadId })
  return postChatThreadArchiveState(threadId, 'unarchive')
}

async function postChatThreadArchiveState(
  threadId: string,
  verb: 'archive' | 'unarchive',
): Promise<ChatThreadSession | null> {
  let raw: unknown
  try {
    raw = await requestJson<unknown>(
      `/api/v1/chat/threads/${encodeURIComponent(threadId)}/${verb}`,
      { method: 'POST' },
    )
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) return null
    throw error
  }
  applyRetentionFrom(raw)
  return normalizeChatThreadSession(objectValue(raw)?.session)
}

/**
 * Start a new thread from a copy of this one, up to an assistant turn (build plan WS17 S6). Resolves
 * to the new thread's session. Rejects with the server's typed refusal: `history_fork_disabled` while
 * forking is off, `history_fork_refused` when the source cannot be copied.
 */
export async function forkChatThread(
  threadId: string,
  input: { fromMessageId: string; title?: string },
): Promise<ChatThreadSession | null> {
  parseChatHistoryAction({
    type: 'thread.fork',
    threadId,
    fromMessageId: input.fromMessageId,
    ...(input.title ? { title: input.title } : {}),
  })
  const raw = await requestJson<unknown>(
    `/api/v1/chat/threads/${encodeURIComponent(threadId)}/fork`,
    {
      method: 'POST',
      body: JSON.stringify({ fromMessageId: input.fromMessageId, title: input.title ?? '' }),
    },
  )
  applyRetentionFrom(raw)
  return normalizeChatThreadSession(objectValue(raw)?.session)
}

/** The file formats a conversation is exported in (EP-12 S11). The person picks one; there is no default. */
export type ThreadExportFormat = 'json' | 'markdown'

/** `redacted` leaves out tool payloads, connector data and attachment names (EP-12 D7). The person picks one. */
export type ThreadExportProfile = 'full' | 'redacted'

/** One export as the gateway wrote it: the file's text, its media type and the name it asks to be saved under. */
export type ChatThreadExport = {
  body: string
  mediaType: string
  fileName: string
}

/**
 * Export one of the owner's conversations as a file (EP-12 S12). The choices are checked against the action
 * contract before anything is sent. The file is read whole as text: `requestJson` parses JSON, and a Markdown
 * export is not JSON. Rejects with the server's typed refusal: `thread_export_disabled` while exporting is off,
 * `thread_not_exportable` (with `details.reason`) for a thread that may not leave in full, and the zero-retention
 * code for a zero-retention caller.
 */
export async function exportThread(
  threadId: string,
  format: ThreadExportFormat,
  profile: ThreadExportProfile,
): Promise<ChatThreadExport> {
  const request = threadExportInput.parse({ threadId, format, profile })
  const path = `/api/v1/chat/threads/${encodeURIComponent(request.threadId)}/export`
  const response = await fetch(
    `${gatewayBaseUrl()}${path}?format=${request.format}&profile=${request.profile}`,
    { credentials: 'include' },
  )
  if (!response.ok) {
    const failure = apiErrorFromResponse(await response.json().catch(() => null), response)
    signalSessionExpiry(failure, path)
    throw failure
  }
  const extension = request.format === 'json' ? 'json' : 'md'
  return {
    body: await response.text(),
    mediaType: response.headers.get('Content-Type') ?? 'application/octet-stream',
    fileName: downloadNameOf(response.headers.get('Content-Disposition')) ?? `verevon-thread.${extension}`,
  }
}

/** The save name the gateway gave the file (`filename="..."`), or null when the header names none. */
function downloadNameOf(disposition: string | null): string | null {
  return disposition?.match(/filename="([^"]+)"/)?.[1] ?? null
}

/** Where a branch move landed: the new active leaf, and the path up to it (WS17 S9). */
export type ChatBranchMove = {
  activeLeafMessageId: string
  activePath: string[]
}

/**
 * Make another version of a message the active one (WS17 S9). `messageId` is a sibling of the turn the user
 * saw, `index` its 0-based place among them, and `expectedLeafMessageId` the leaf the client last displayed.
 * A conflict (`active_leaf_stale`) means the thread moved on: reload it before moving again.
 */
export async function switchChatVersion(
  threadId: string,
  messageId: string,
  index: number,
  expectedLeafMessageId: string,
): Promise<ChatBranchMove> {
  parseChatHistoryAction({ type: 'message.switch_version', threadId, messageId, index })
  return postChatBranchMove(threadId, 'branch', { targetMessageId: messageId, expectedLeafMessageId })
}

/**
 * Rewind a thread to an earlier turn of its active path (WS17 S9). The user confirms it first. Nothing is
 * deleted: the turns after the rewind stay on their branch, reachable through the version switcher.
 */
export async function rewindChatThread(
  threadId: string,
  messageId: string,
  expectedLeafMessageId: string,
): Promise<ChatBranchMove> {
  parseChatHistoryAction({ type: 'thread.rewind', threadId, messageId, confirm: true })
  return postChatBranchMove(threadId, 'rewind', { targetMessageId: messageId, expectedLeafMessageId, confirm: true })
}

/**
 * Undo a rewind (WS17 S9): the leaf returns to `messageId`, the turn the rewind left, even when a later branch
 * continues from it. `expectedLeafMessageId` is the leaf the rewind landed on; a thread that moved on refuses the
 * undo with `active_leaf_stale`, and the client reloads it.
 */
export async function undoChatRewind(
  threadId: string,
  messageId: string,
  expectedLeafMessageId: string,
): Promise<ChatBranchMove> {
  parseChatHistoryAction({ type: 'thread.undo_rewind', threadId, messageId })
  return postChatBranchMove(threadId, 'undo-rewind', { targetMessageId: messageId, expectedLeafMessageId })
}

async function postChatBranchMove(
  threadId: string,
  verb: 'branch' | 'rewind' | 'undo-rewind',
  body: Record<string, unknown>,
): Promise<ChatBranchMove> {
  const raw = await requestJson<unknown>(
    `/api/v1/chat/threads/${encodeURIComponent(threadId)}/${verb}`,
    { method: 'POST', body: JSON.stringify(body) },
  )
  const record = objectValue(raw)
  const path = Array.isArray(record?.active_path) ? record.active_path : []
  return {
    activeLeafMessageId: str(record?.active_leaf_message_id) ?? '',
    activePath: path.filter((id): id is string => typeof id === 'string'),
  }
}

/**
 * A revision the person wrote in the Output panel (EP-3 S4), as the edit and restore routes return it.
 * `editorKind` is `user` for an edit and `restore` for a restore; `restoredFromVersion` names the copied version.
 */
export type SavedArtifactRevision = {
  id: string
  artifactId: string
  version: number
  kind: string
  title: string
  content: string
  editorKind: ArtifactEditorKind
  restoredFromVersion?: number
  createdAt: string
}

/**
 * Save the person's text as the artifact's newest revision (EP-3 S4). `expectedVersion` is the head the editor
 * opened; a moved head rejects with an {@link ApiError} coded `revision_mismatch` whose details name
 * `head_version` (read it with {@link artifactConflictHeadVersion}). Off, ZDR, waived, Space and running-run
 * refusals reject with their own typed codes.
 */
export async function saveArtifactRevision(input: {
  threadId: string
  artifactId: string
  content: string
  title?: string
  expectedVersion: number
}): Promise<SavedArtifactRevision> {
  const raw = await requestJson<unknown>(artifactRevisionPath(input.threadId, input.artifactId), {
    method: 'PUT',
    body: JSON.stringify({
      content: input.content,
      ...(input.title?.trim() ? { title: input.title.trim() } : {}),
      expected_version: input.expectedVersion,
    }),
  })
  return normalizeSavedArtifactRevision(raw)
}

/** Make an older revision the newest one (EP-3 S4), under the same compare-and-set as {@link saveArtifactRevision}. */
export async function restoreArtifactRevision(input: {
  threadId: string
  artifactId: string
  version: number
  expectedVersion: number
}): Promise<SavedArtifactRevision> {
  const raw = await requestJson<unknown>(`${artifactRevisionPath(input.threadId, input.artifactId)}/restore`, {
    method: 'POST',
    body: JSON.stringify({ version: input.version, expected_version: input.expectedVersion }),
  })
  return normalizeSavedArtifactRevision(raw)
}

/** The head another tab or turn moved the artifact to, when `error` is the edit routes' `revision_mismatch`. */
export function artifactConflictHeadVersion(error: unknown): number | null {
  if (!(error instanceof ApiError) || error.status !== 409 || error.code !== 'revision_mismatch') return null
  const head = error.details?.head_version
  return typeof head === 'number' && Number.isInteger(head) && head > 0 ? head : null
}

function artifactRevisionPath(threadId: string, artifactId: string): string {
  return `/api/v1/chat/threads/${encodeURIComponent(threadId)}/artifacts/${encodeURIComponent(artifactId)}`
}

function normalizeSavedArtifactRevision(raw: unknown): SavedArtifactRevision {
  const record = objectValue(objectValue(objectValue(raw)?.data)?.revision)
  const version = num(record?.version)
  if (!record || version == null || version < 1) {
    throw new ApiError('The saved revision could not be read.', 502, 'artifact_edit_failed')
  }
  const restored = num(record.restored_from_version)
  return {
    id: str(record.id) ?? '',
    artifactId: str(record.artifact_id) ?? '',
    version,
    kind: str(record.kind) ?? '',
    title: typeof record.title === 'string' ? record.title : '',
    content: typeof record.content === 'string' ? record.content : '',
    editorKind: isArtifactEditorKind(record.editor_kind) ? record.editor_kind : 'user',
    ...(restored != null && restored > 0 ? { restoredFromVersion: restored } : {}),
    createdAt: str(record.created_at) ?? '',
  }
}

/**
 * Star or unstar a thread, or mark it read or unread. Each call writes only the
 * flag it names, so the server leaves the title, preview and pin alone. Null when
 * the server no longer has the thread as one the caller owns.
 */
export async function setChatThreadStarred(threadId: string, starred: boolean): Promise<ChatThreadSession | null> {
  parseChatHistoryAction({ type: 'thread.star', threadId, starred })
  return postChatThreadFlags(threadId, { starred })
}

export async function markChatThreadUnread(threadId: string): Promise<ChatThreadSession | null> {
  parseChatHistoryAction({ type: 'thread.mark_unread', threadId })
  return postChatThreadFlags(threadId, { unread: true })
}

export async function markChatThreadRead(threadId: string): Promise<ChatThreadSession | null> {
  parseChatHistoryAction({ type: 'thread.mark_read', threadId })
  return postChatThreadFlags(threadId, { unread: false })
}

async function postChatThreadFlags(
  threadId: string,
  flags: { starred?: boolean; unread?: boolean },
): Promise<ChatThreadSession | null> {
  let raw: unknown
  try {
    raw = await requestJson<unknown>(
      `/api/v1/chat/threads/${encodeURIComponent(threadId)}/flags`,
      { method: 'PUT', body: JSON.stringify(flags) },
    )
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) return null
    throw error
  }
  applyRetentionFrom(raw)
  return normalizeChatThreadSession(objectValue(raw)?.session)
}

/** One conversation search hit. The snippet carries match markers, never markup. */
export type ChatSearchHit = {
  threadId: string
  messageId: string
  role: string
  /**
   * The matched text, with each matched word between U+E000 and U+E001. The
   * page splits on those markers into text nodes; it never parses this as HTML.
   */
  snippet: string
  createdAt: string
  threadTitle: string
  archived: boolean
}

export type ChatSearchResult =
  | { status: 'ok'; hits: ChatSearchHit[] }
  /**
   * Search is off on this deployment, or it ran past its time budget. Not an
   * error: the sidebar keeps its local filter and says so.
   */
  | { status: 'unavailable'; reason: 'disabled' | 'timeout' }

/**
 * Search the owner's chat threads, first page only. Rejects on any failure other
 * than the two typed unavailable answers, so the caller decides what a broken
 * request means for its view.
 */
export async function searchChatThreads(
  query: string,
  options: { limit?: number } = {},
): Promise<ChatSearchResult> {
  // The contract refuses an empty or oversized query before anything is sent.
  parseChatHistoryAction({ type: 'thread.search', query })
  const params = new URLSearchParams({ q: query.trim() })
  if (options.limit != null && Number.isFinite(options.limit) && options.limit > 0) {
    params.set('limit', String(Math.floor(options.limit)))
  }
  let raw: unknown
  try {
    raw = await requestJson<unknown>(`/api/v1/chat/threads/search?${params.toString()}`)
  } catch (error) {
    const reason = searchUnavailableReason(error)
    if (reason) return { status: 'unavailable', reason }
    throw error
  }
  return { status: 'ok', hits: normalizeChatSearchHits(raw) }
}

function searchUnavailableReason(error: unknown): 'disabled' | 'timeout' | null {
  if (!(error instanceof ApiError) || error.status !== 503) return null
  if (error.code === 'history_search_disabled') return 'disabled'
  if (error.code === 'history_search_timeout') return 'timeout'
  return null
}

function normalizeChatSearchHits(raw: unknown): ChatSearchHit[] {
  const source = objectValue(raw)?.hits
  if (!Array.isArray(source)) return []
  return source.flatMap((item): ChatSearchHit[] => {
    const record = objectValue(item)
    const threadId = str(record?.threadId)
    const messageId = str(record?.messageId)
    if (!threadId || !messageId) return []
    return [{
      threadId,
      messageId,
      role: str(record?.role) ?? '',
      snippet: str(record?.snippet) ?? '',
      createdAt: str(record?.createdAt) ?? '',
      threadTitle: str(record?.threadTitle) ?? '',
      archived: record?.archived === true,
    }]
  })
}

/** One stored revision of a thread artifact (EP-3 S3), newest first in a page. */
export type ArtifactRevision = {
  id: string
  artifactId: string
  version: number
  kind: string
  title: string
  content: string
  editorKind: ArtifactEditorKind
  /** For a restore, the version whose text it brought back. */
  restoredFromVersion?: number
  createdAt: string
}

export type ArtifactRevisionPage = {
  revisions: ArtifactRevision[]
  /** Older revisions exist below the last one in this page. */
  hasMore: boolean
}

/** What the composer and the Output panel may offer, from the gateway's own switches (EP-3). */
export type ComposerCapabilities = {
  artifactHistory: boolean
  artifactEditing: boolean
  contextRefs: boolean
  threadReferences: boolean
  selectionEdit: boolean
  /** Save an artifact revision to knowledge (EP-3 S8): the BFF's own switch, added to the gateway's answer. */
  artifactPromote: boolean
}

export const NO_COMPOSER_CAPABILITIES: ComposerCapabilities = {
  artifactHistory: false,
  artifactEditing: false,
  contextRefs: false,
  threadReferences: false,
  selectionEdit: false,
  artifactPromote: false,
}

/**
 * The gateway's composer capabilities. Any failure (an older gateway has no
 * such route) reads as nothing offered, so a feature never appears on a guess.
 */
export async function getComposerCapabilities(): Promise<ComposerCapabilities> {
  let raw: unknown
  try {
    raw = await requestJson<unknown>('/api/v1/chat/composer-capabilities')
  } catch {
    return NO_COMPOSER_CAPABILITIES
  }
  const record = objectValue(raw)
  return {
    artifactHistory: record?.artifact_history === true,
    artifactEditing: record?.artifact_editing === true,
    contextRefs: record?.context_refs === true,
    threadReferences: record?.thread_references === true,
    selectionEdit: record?.selection_edit === true,
    artifactPromote: record?.artifact_promote === true,
  }
}

/** A brand deck template as the picker shows it (EP-5 S7): display fields only, never a file. */
export type DeckTemplate = {
  skillId: string
  name: string
  description: string
  colors: { primary: string; secondary: string; text: string; background: string; accent: string[] }
  fonts: { heading: string; body: string }
}

/** The skill-id shape model-gateway accepts as a template id; anything else is never sent. */
const DECK_TEMPLATE_ID = /^[A-Za-z0-9_.:-]{1,128}$/
const HEX_COLOUR = /^#[0-9a-f]{6}$/i

function deckTemplateId(value: unknown): string | null {
  const id = typeof value === 'string' ? value.trim() : ''
  return DECK_TEMPLATE_ID.test(id) ? id : null
}

function deckTemplateColour(value: unknown): string | null {
  return typeof value === 'string' && HEX_COLOUR.test(value) ? value.toLowerCase() : null
}

function deckTemplateText(value: unknown, max: number): string {
  return typeof value === 'string' ? value.trim().slice(0, max) : ''
}

/** One listed template, or null when the entry is not one: the picker never shows a guessed colour or font. */
function normalizeDeckTemplate(raw: unknown): DeckTemplate | null {
  const record = objectValue(raw)
  const colors = objectValue(record?.colors)
  const fonts = objectValue(record?.fonts)
  const skillId = deckTemplateId(record?.skill_id)
  const name = deckTemplateText(record?.name, 80)
  const primary = deckTemplateColour(colors?.primary)
  const secondary = deckTemplateColour(colors?.secondary)
  const text = deckTemplateColour(colors?.text)
  const background = deckTemplateColour(colors?.background)
  const heading = deckTemplateText(fonts?.heading, 64)
  const body = deckTemplateText(fonts?.body, 64)
  if (!skillId || !name || !primary || !secondary || !text || !background || !heading || !body) return null
  const accents = colors?.accent
  const accent = Array.isArray(accents)
    ? accents.map(deckTemplateColour).filter((colour): colour is string => colour !== null).slice(0, 4)
    : []
  return {
    skillId,
    name,
    description: deckTemplateText(record?.description, 200),
    colors: { primary, secondary, text, background, accent },
    fonts: { heading, body },
  }
}

/**
 * The org's brand deck templates the person may pick (EP-5 S7), in the gateway's order. `null` when the
 * picker must not show: the route answers 404 while the gateway's switch is off, and a zero-retention
 * session is refused (403). Any other failure rejects with the typed `ApiError`.
 */
export async function listDeckTemplates(): Promise<DeckTemplate[] | null> {
  let raw: unknown
  try {
    raw = await requestJson<unknown>('/api/v1/deck-templates')
  } catch (error) {
    if (error instanceof ApiError && (error.status === 404 || error.status === 403)) return null
    throw error
  }
  const templates = objectValue(raw)?.templates
  return Array.isArray(templates)
    ? templates.map(normalizeDeckTemplate).filter((template): template is DeckTemplate => template !== null)
    : []
}

/** An MCP tool's app document (EP-4 S9), read on demand and never stored. */
export type McpAppResource = {
  uri: string
  html: string
  /** The app declared network origins; the frame's CSP still allows none (D12). */
  needsNetwork: boolean
}

/**
 * The `ui://` app `tool` on MCP server `serverId` declares, read by the gateway with the viewer's authority
 * (`chat.open_mcp_app`). `null` when there is no app to draw: the route answers 404 while the gateway's switch
 * is off and for a tool without an app or a server the viewer may not use, and a zero-retention session is
 * refused (403). Any other failure rejects with the typed `ApiError`. Not cached here: the caller holds the
 * document only as long as its frame.
 */
export async function getMcpAppResource(serverId: string, tool: string): Promise<McpAppResource | null> {
  const params = new URLSearchParams({ server_id: serverId, tool })
  let raw: unknown
  try {
    raw = await requestJson<unknown>(`/api/v1/mcp-apps/resource?${params.toString()}`)
  } catch (error) {
    if (error instanceof ApiError && (error.status === 404 || error.status === 403)) return null
    throw error
  }
  const record = objectValue(raw)
  const uri = str(record?.uri)
  const html = typeof record?.html === 'string' ? record.html : ''
  if (!uri?.startsWith('ui://') || !html.trim()) return null
  return { uri, html, needsNetwork: record?.needs_network === true }
}

/** One call an MCP App asks its host to make on the app's own server (EP-4 S10). */
export type McpAppToolCallRequest = {
  /** The run of the turn that drew the app; the gateway checks the viewer owns it. */
  runId: string
  /** The app's own server: the frame's, never one the app names. */
  serverId: string
  tool: string
  arguments: Record<string, unknown>
  /** The nonce the frame minted when it mounted; the gateway bounds calls per frame by it. */
  frameNonce: string
}

/** What the app is answered: the tool's output, or the typed code of the refusal. */
export type McpAppToolCallOutcome =
  | { ok: true; output: string; truncated: boolean }
  | { ok: false; code: string }

/**
 * Ask the gateway to run a read tool on the app's own server as the viewer (`chat.mcp_app_tool_call`). The
 * gateway decides everything: the run owner, the server's allowlist, the tool's registered class, the frame's
 * rate limit, and zero retention; a refusal comes back as its typed code (`app_call_requires_chat`,
 * `rate_limited`, `not_found` while the switch is off or for a run the viewer does not own, ...). Only a
 * failure that is not a typed answer (the network) rejects.
 */
export async function callMcpAppTool(request: McpAppToolCallRequest): Promise<McpAppToolCallOutcome> {
  let raw: unknown
  try {
    raw = await requestJson<unknown>('/api/v1/mcp-apps/tool-call', {
      method: 'POST',
      body: JSON.stringify({
        run_id: request.runId,
        server_id: request.serverId,
        tool: request.tool,
        arguments: request.arguments,
        frame_nonce: request.frameNonce,
      }),
    })
  } catch (error) {
    if (error instanceof ApiError) return { ok: false, code: error.code ?? 'tool_call_failed' }
    throw error
  }
  const record = objectValue(raw)
  const output = record?.output
  if (typeof output !== 'string') return { ok: false, code: 'tool_call_failed' }
  return { ok: true, output, truncated: record?.truncated === true }
}

/**
 * One page of an artifact's stored revisions, newest first, text included.
 * `beforeVersion` pages to older revisions. Rejects with the typed `ApiError`
 * (404 `artifact_history_unavailable` while the server's history is off).
 */
export async function listArtifactRevisions(
  threadId: string,
  artifactId: string,
  options: { beforeVersion?: number; limit?: number } = {},
): Promise<ArtifactRevisionPage> {
  const params = new URLSearchParams()
  if (options.beforeVersion != null && Number.isInteger(options.beforeVersion) && options.beforeVersion > 0) {
    params.set('before_version', String(options.beforeVersion))
  }
  if (options.limit != null && Number.isInteger(options.limit) && options.limit > 0) {
    params.set('limit', String(options.limit))
  }
  const query = params.toString()
  const raw = await requestJson<unknown>(
    `/api/v1/chat/threads/${encodeURIComponent(threadId)}/artifacts/${encodeURIComponent(artifactId)}/revisions${query ? `?${query}` : ''}`,
  )
  const record = objectValue(raw)
  return {
    revisions: normalizeArtifactRevisions(record?.revisions),
    hasMore: record?.has_more === true,
  }
}

function normalizeArtifactRevisions(source: unknown): ArtifactRevision[] {
  if (!Array.isArray(source)) return []
  return source.flatMap((item): ArtifactRevision[] => {
    const record = objectValue(item)
    const id = str(record?.id)
    const artifactId = str(record?.artifact_id)
    const version = num(record?.version)
    const content = str(record?.content)
    const editorKind = record?.editor_kind
    // A revision without its text, its number or a known editor cannot stand in the stepper.
    if (!id || !artifactId || version == null || version < 1 || content == null) return []
    if (!isArtifactEditorKind(editorKind)) return []
    const restored = num(record?.restored_from_version)
    return [{
      id,
      artifactId,
      version,
      kind: str(record?.kind) ?? '',
      title: str(record?.title) ?? '',
      content,
      editorKind,
      ...(restored != null && restored > 0 ? { restoredFromVersion: restored } : {}),
      createdAt: str(record?.created_at) ?? '',
    }]
  })
}

export async function saveChatThreadSnapshot(
  threadId: string,
  snapshot: SaveChatThreadSnapshotRequest,
): Promise<ChatThreadSession | null> {
  const raw = await requestJson<unknown>(
    `/api/v1/chat/threads/${encodeURIComponent(threadId)}`,
    {
      method: 'PUT',
      body: JSON.stringify(snapshot),
    },
  )
  const record = objectValue(raw)
  const data = objectValue(record?.data) ?? record
  // `retained: false` means the server understood the request and deliberately
  // kept nothing — a ZDR posture it derived server-side (org policy, the
  // thread's own ZDR marker, or this request's own flag). If the server kept
  // nothing, neither may the device.
  if (data?.retained === false) {
    forgetThreadLocally(threadId)
    return null
  }
  return normalizeChatThreadSession(record?.session)
}

export async function getChatThreadTranscript(threadId: string): Promise<ChatThreadTranscriptSnapshot | null> {
  const raw = await requestJson<unknown>(
    `/api/v1/chat/threads/${encodeURIComponent(threadId)}/transcript`,
  )
  const record = objectValue(raw)
  return normalizeChatThreadTranscript(record?.transcript)
}

/** Read the canonical, envelope-only event history for one durable Chat thread. */
export async function listChatThreadEvents(
  threadId: string,
  options: { afterEventId?: string; limit?: number } = {},
): Promise<ChatThreadEventsPage> {
  const search = new URLSearchParams()
  if (options.afterEventId?.trim()) search.set('after_event_id', options.afterEventId.trim())
  if (options.limit != null && Number.isFinite(options.limit) && options.limit > 0) {
    search.set('limit', String(Math.floor(options.limit)))
  }
  const query = search.toString()
  const raw = await requestJson<unknown>(
    `/api/v1/chat/threads/${encodeURIComponent(threadId)}/events${query ? `?${query}` : ''}`,
  )
  const record = objectValue(raw)
  const data = objectValue(record?.data) ?? record
  const source = Array.isArray(data?.events) ? data.events : []
  return {
    threadId: str(data?.threadId) ?? str(data?.thread_id) ?? threadId,
    events: source.map(normalizeChatThreadEvent).filter((event): event is ChatThreadEvent => event !== null),
    truncated: data?.truncated === true,
  }
}

export async function deleteChatThread(threadId: string): Promise<ChatThreadSession[]> {
  const raw = await requestJson<unknown>(
    `/api/v1/chat/threads/${encodeURIComponent(threadId)}`,
    { method: 'DELETE' },
  )
  applyRetentionFrom(raw)
  return normalizeChatThreadSessions(raw)
}

export async function clearChatThreads(): Promise<void> {
  await requestJson('/api/v1/chat/threads', { method: 'DELETE' })
}

function normalizeThreadMessages(raw: unknown): ChatMessage[] {
  const source = Array.isArray(raw)
    ? raw
    : raw && typeof raw === 'object' && Array.isArray((raw as { messages?: unknown }).messages)
      ? (raw as { messages: unknown[] }).messages
      : []

  // Fields are read by name, never spread: the message's `metadata` map is
  // forwarded whole to the thread owner and carries gateway bookkeeping that
  // must never render. `head_summary` (the budget compaction's summary of
  // earlier turns) and `native_compaction_summary` are model context, not
  // anything the user said or was told, so neither is read here.
  return source
    .filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === 'object')
    .map((item, index) => ({
      id: str(item.id) ?? str(item.message_id) ?? `msg-${index}`,
      role: item.role === 'assistant' ? 'assistant' : 'user',
      content: str(item.content) ?? '',
      model: str(item.model) ?? str(item.model_used),
      createdAt: str(item.created_at) ?? str(item.createdAt) ?? '',
      sourceScope: (item.source_scope ?? objectValue(item.metadata)?.source_scope) === 'conversation' ? 'conversation' : 'workspace',
      // See `ChatInvokeRequest.retrievalScope`. Undefined (not `'auto'`) for
      // a turn that used the default, or one recorded before this field
      // existed — omitted from the type is the same "nothing to show"
      // signal `sourceScope`'s absence-of-'conversation' already relies on.
      retrievalScope: (() => {
        const raw = item.retrieval_scope ?? objectValue(item.metadata)?.retrieval_scope
        return raw === 'knowledge_only' ? 'knowledge_only' : raw === 'web_only' ? 'web_only' : undefined
      })(),
      // The chips the turn was sent with (EP-3 S5), read by name and normalized like the wire: a turn
      // recorded before references existed has no key at all.
      ...contextRefsField(objectValue(item.metadata)?.context_refs),
      // Turn metadata the backend flattens onto the message. Present only for
      // assistant turns recorded since the score became durable, so a missing
      // field means "not recorded" — never a score of zero.
      confidence: num(item.confidence),
      verification: normalizeVerification(item.verification),
      ...messageVersionField(item.version),
    }))
}

/** The `contextRefs` key for a turn that stored chips, and no key at all for one that did not. */
function contextRefsField(value: unknown): { contextRefs?: import('@/shared/actions/chat-context-refs').ChatContextRef[] } {
  const refs = normalizeContextRefs(value)
  return refs.length > 0 ? { contextRefs: refs } : {}
}

/** A turn's place in a versioned thread, read by name. A malformed place is dropped, never guessed. */
function normalizeMessageVersion(value: unknown): ChatMessageVersion | undefined {
  const record = objectValue(value)
  if (!record) return undefined
  const siblingOrdinal = num(record.sibling_ordinal)
  const siblingCount = num(record.sibling_count)
  if (siblingOrdinal === undefined || siblingCount === undefined || typeof record.on_active_path !== 'boolean') {
    return undefined
  }
  return {
    parentMessageId: str(record.parent_message_id) ?? '',
    siblingOrdinal,
    siblingCount,
    onActivePath: record.on_active_path,
    siblings: Array.isArray(record.siblings)
      ? record.siblings.filter((id): id is string => typeof id === 'string')
      : [],
  }
}

/** The `version` key for a turn that has one, and no key at all for a turn that does not. */
function messageVersionField(value: unknown): { version?: ChatMessageVersion } {
  const version = normalizeMessageVersion(value)
  return version ? { version } : {}
}

/**
 * Read a persisted verification record. Anything that is not one of the three
 * verdicts the backend defines is dropped: rendering "checked" from a value we
 * cannot interpret would be a claim the data does not support.
 */
function normalizeVerification(raw: unknown): ChatVerificationEvent | undefined {
  if (!raw || typeof raw !== 'object') return undefined
  const record = raw as Record<string, unknown>
  const verdict = record.verdict
  if (verdict !== 'supports' && verdict !== 'contradicts' && verdict !== 'unrelated') {
    return undefined
  }
  return {
    verdict,
    kbCitations: num(record.kbCitations) ?? num(record.kb_citations) ?? 0,
    webCitations: num(record.webCitations) ?? num(record.web_citations) ?? 0,
    webAllowed: record.webAllowed === true || record.web_allowed === true,
  }
}

function normalizeChatThreadSessions(raw: unknown): ChatThreadSession[] {
  const source = Array.isArray(raw)
    ? raw
    : raw && typeof raw === 'object' && Array.isArray((raw as { sessions?: unknown }).sessions)
      ? (raw as { sessions: unknown[] }).sessions
      : []

  return source
    .map(normalizeChatThreadSession)
    .filter((item): item is ChatThreadSession => Boolean(item))
}

function normalizeChatThreadSession(raw: unknown): ChatThreadSession | null {
  const item = objectValue(raw)
  if (!item) return null
  const threadId = str(item.threadId) ?? str(item.thread_id)
  const title = str(item.title)
  if (!threadId || !title) return null
  const latestRunId = str(item.latestRunId) ?? str(item.latest_run_id)
  const latestRunStatus = str(item.latestRunStatus) ?? str(item.latest_run_status)
  const latestRunUpdatedAt = normalizeOptionalIsoTimestamp(
    str(item.latestRunUpdatedAt) ?? str(item.latest_run_updated_at),
  )
  const archivedAt = normalizeOptionalIsoTimestamp(
    str(item.archivedAt) ?? str(item.archived_at),
  )
  const attentionState = normalizeChatAttentionState(str(item.attentionState))
  const attentionSince = normalizeOptionalIsoTimestamp(str(item.attentionSince))
  return {
    threadId,
    title,
    preview: str(item.preview) ?? '',
    updatedAt: normalizeIsoTimestamp(str(item.updatedAt) ?? str(item.updated_at)),
    pinned: item.pinned === true,
    // Absent (a legacy thread the BFF never saw a personaEnabled key for, or
    // a snapshot response that omits it) reads as enabled, the mirror image
    // of `pinned` defaulting to false — see this field's own doc comment.
    personaEnabled: item.personaEnabled !== false,
    ...(latestRunId ? { latestRunId } : {}),
    ...(latestRunStatus ? { latestRunStatus } : {}),
    ...(latestRunUpdatedAt ? { latestRunUpdatedAt } : {}),
    spaceRef: str(item.spaceRef) ?? str(item.space_ref),
    origin: str(item.origin),
    ...(item.starred === true ? { starred: true } : {}),
    ...(str(item.forkedFromThreadId) ? { forkedFromThreadId: str(item.forkedFromThreadId) } : {}),
    ...(item.unread === true ? { unread: true } : {}),
    ...(attentionState ? { attentionState } : {}),
    ...(attentionState && attentionSince ? { attentionSince } : {}),
    ...(attentionState ? { attentionUnseen: item.attentionUnseen === true } : {}),
    ...(archivedAt ? { archivedAt } : {}),
  }
}

function normalizeChatThreadTranscript(raw: unknown): ChatThreadTranscriptSnapshot | null {
  const item = objectValue(raw)
  if (!item) return null
  const threadId = str(item.threadId) ?? str(item.thread_id)
  const turns = Array.isArray(item.turns) ? item.turns : null
  if (!threadId || !turns) return null
  const taskSteps = Array.isArray(item.taskSteps)
    ? item.taskSteps
    : Array.isArray(item.task_steps)
      ? item.task_steps
      : undefined
  return {
    threadId,
    turns,
    taskSteps,
    updatedAt: normalizeIsoTimestamp(str(item.updatedAt) ?? str(item.updated_at)),
  }
}

function normalizeChatThreadEvent(raw: unknown): ChatThreadEvent | null {
  const item = objectValue(raw)
  if (!item) return null
  const eventId = str(item.eventId) ?? str(item.event_id)
  const eventType = str(item.eventType) ?? str(item.event_type)
  if (!eventId || !eventType) return null
  const schemaValue = item.schemaVersion ?? item.schema_version
  const schemaVersion = typeof schemaValue === 'number' && Number.isFinite(schemaValue)
    ? schemaValue
    : typeof schemaValue === 'string' && Number.isFinite(Number(schemaValue))
      ? Number(schemaValue)
      : 0
  const atRaw = str(item.at) ?? str(item.ts)
  const at = atRaw ? normalizeOptionalIsoTimestamp(atRaw) : undefined
  // ReplayThread is an authoritative event stream. Do not fabricate an epoch
  // timestamp for malformed records; dropping the record keeps Trace honest
  // and prevents an invalid event from sorting to the beginning of history.
  if (!at) return null
  return {
    eventId,
    eventType,
    schemaVersion,
    at,
    producer: str(item.producer),
    correlationId: str(item.correlationId) ?? str(item.correlation_id),
    causationId: str(item.causationId) ?? str(item.causation_id),
    resourceRef: str(item.resourceRef) ?? str(item.resource_ref),
  }
}

function normalizeIsoTimestamp(value: string | undefined): string {
  if (!value) return new Date().toISOString()
  const parsed = Date.parse(value)
  return Number.isNaN(parsed) ? new Date().toISOString() : new Date(parsed).toISOString()
}

/** The attention state a wire value names, or `undefined` for a value outside the closed set. */
function normalizeChatAttentionState(value: string | undefined): ChatAttentionState | undefined {
  return CHAT_ATTENTION_STATES.find((state) => state === value)
}

function normalizeOptionalIsoTimestamp(value: string | undefined): string | undefined {
  if (!value) return undefined
  const parsed = Date.parse(value)
  return Number.isNaN(parsed) ? undefined : new Date(parsed).toISOString()
}

// ── Model catalog normalization ──────────────────────────────────────────────
// `/api/v1/models` historically returned `{ id, name, features }`. The backend is
// growing the payload to expose the full Azure Foundry catalog with provider +
// modality + cost metadata. We parse BOTH shapes defensively: every new field is
// optional and recovered from name/id heuristics when absent, so an old-shape
// response never breaks the picker.

const CHAT_MODALITIES: ReadonlySet<ModelModality> = new Set(['chat'])

/** Non-chat ids/names we must never offer as a chat model, even with no metadata. */
const NON_CHAT_NAME_PATTERNS: readonly RegExp[] = [
  /\bimage\b/i,
  /\bsora\b/i,
  /\bvideo\b/i,
  /\bvision-only\b/i,
  /\btranscrib/i,
  /\bwhisper\b/i,
  /\btts\b/i,
  /\bspeech\b/i,
  /\bembed/i,
  /text-embedding/i,
  /\bdall-?e\b/i,
  /\brerank/i,
]

function normalizeModality(value: string | undefined, id: string, name: string): ModelModality | undefined {
  const explicit = (value ?? '').toLowerCase()
  if (explicit) {
    if (explicit.includes('chat') || explicit.includes('text') || explicit.includes('reason')) return 'chat'
    if (explicit.includes('image') || explicit.includes('vision')) return 'image'
    if (explicit.includes('video')) return 'video'
    if (explicit.includes('audio') || explicit.includes('speech') || explicit.includes('transcri')) return 'audio'
    if (explicit.includes('embed')) return 'embedding'
    return 'other'
  }
  // No explicit modality → infer from id/name heuristics.
  const haystack = `${id} ${name}`
  if (NON_CHAT_NAME_PATTERNS.some((re) => re.test(haystack))) {
    if (/embed/i.test(haystack)) return 'embedding'
    if (/sora|video/i.test(haystack)) return 'video'
    if (/transcrib|whisper|tts|speech/i.test(haystack)) return 'audio'
    if (/image|dall-?e/i.test(haystack)) return 'image'
    return 'other'
  }
  return undefined
}

function normalizeCostTier(value: unknown): ModelInfo['costTier'] {
  const tier = str(value)?.toLowerCase()
  if (tier === 'low' || tier === 'cheap' || tier === 'economy') return 'low'
  if (tier === 'medium' || tier === 'standard') return 'medium'
  if (tier === 'high' || tier === 'premium' || tier === 'expensive') return 'high'
  return undefined
}

/** Unknown names are dropped, not guessed at: a level the client cannot name is never offered. */
function normalizeEffortLevels(value: unknown): ModelInfo['effortLevels'] {
  if (!Array.isArray(value)) return undefined
  const levels = value.filter(isEffortLevel)
  return levels.length > 0 ? levels : undefined
}

/**
 * A list price is kept whole or not at all: two finite, non-negative rates and a three-letter
 * currency. A partial or unexpected shape is dropped, never completed with a guess.
 */
function normalizeModelPrice(value: unknown): ModelPrice | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined
  const record = value as Record<string, unknown>
  const input = record.input_per_million
  const output = record.output_per_million
  const currency = str(record.currency)?.trim().toUpperCase()
  if (typeof input !== 'number' || typeof output !== 'number') return undefined
  if (!Number.isFinite(input) || !Number.isFinite(output) || input < 0 || output < 0) return undefined
  if (!currency || !/^[A-Z]{3}$/.test(currency)) return undefined
  return { inputPerMillion: input, outputPerMillion: output, currency }
}

/** The data-use record a catalogue entry carries. A statement the record does not make stays unset. */
function normalizeModelDataUse(value: unknown): ModelDataUse | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined
  const record = value as Record<string, unknown>
  const trainsOnInputs = readBool(record.trains_on_inputs)
  const days = record.retention_days
  const retentionDays =
    typeof days === 'number' && Number.isSafeInteger(days) && days >= 0 ? days : undefined
  const zdrAttested = readBool(record.zdr_attested)
  const dataUse: ModelDataUse = {
    ...(trainsOnInputs === undefined ? {} : { trainsOnInputs }),
    ...(retentionDays === undefined ? {} : { retentionDays }),
    ...(zdrAttested === undefined ? {} : { zdrAttested }),
  }
  return Object.keys(dataUse).length > 0 ? dataUse : undefined
}

/** A token count the record names as a positive whole number; anything else is unknown. */
function normalizeTokenCount(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isSafeInteger(value) && value > 0 ? value : undefined
}

const MODEL_LIFECYCLES: readonly ModelLifecycle[] = ['ga', 'preview', 'deprecated']

function normalizeLifecycle(value: unknown): ModelLifecycle | undefined {
  return MODEL_LIFECYCLES.find((stage) => stage === value)
}

function readBool(value: unknown): boolean | undefined {
  if (typeof value === 'boolean') return value
  if (typeof value === 'string') {
    const v = value.toLowerCase()
    if (v === 'true' || v === 'yes') return true
    if (v === 'false' || v === 'no') return false
  }
  return undefined
}

/**
 * What the server says a chat turn may attach (`attachment_policy` beside
 * `models` on `/api/v1/models`). The server enforces these numbers; the client
 * declares them instead of keeping a copy that can drift.
 */
export interface AttachmentPolicy {
  readonly imageMimes: readonly string[]
  readonly maxImages: number
  readonly maxImageBytes: number
  readonly maxFiles: number
  readonly maxFileBytes: number
  readonly maxTotalBytes: number
  readonly modes: {
    readonly vision: boolean
    readonly sandboxFiles: boolean
    /**
     * Images ride as parts of a chat turn on the server (`CHAT_VISION_AS_PARTS_ENABLED`). Optional in the type so
     * a literal written before this mode existed still type-checks; {@link parseAttachmentPolicy} always sets it.
     */
    readonly visionAsParts?: boolean
    /** A Do run carries the turn's attachments on the server (`CHAT_DO_ATTACHMENTS_ENABLED`). Optional, as above. */
    readonly doAttachments?: boolean
  }
}

function positiveInteger(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isInteger(value) && value > 0 ? value : undefined
}

/**
 * A switch the server declares beside `modes`. Absent means off: a server that predates the switch does not
 * declare it. Present but not a boolean is malformed, so the whole policy is dropped like any other bad field.
 */
function declaredSwitch(value: unknown): boolean | undefined {
  if (value === undefined) return false
  return typeof value === 'boolean' ? value : undefined
}

/**
 * The declared policy, or `undefined` when the response carries none or any
 * field of it is malformed. Never half-trusted: a policy the client cannot read
 * whole is the same as no policy, which keeps today's constants.
 */
export function parseAttachmentPolicy(value: unknown): AttachmentPolicy | undefined {
  if (!value || typeof value !== 'object') return undefined
  const raw = value as Record<string, unknown>
  const modes = raw.modes && typeof raw.modes === 'object' ? (raw.modes as Record<string, unknown>) : undefined
  const imageMimes = Array.isArray(raw.image_mimes)
    ? raw.image_mimes.filter((mime): mime is string => typeof mime === 'string')
    : undefined
  const maxImages = positiveInteger(raw.max_images)
  const maxImageBytes = positiveInteger(raw.max_image_bytes)
  const maxFiles = positiveInteger(raw.max_files)
  const maxFileBytes = positiveInteger(raw.max_file_bytes)
  const maxTotalBytes = positiveInteger(raw.max_total_bytes)
  const visionAsParts = declaredSwitch(modes?.vision_as_parts)
  const doAttachments = declaredSwitch(modes?.do_attachments)
  if (
    !imageMimes || imageMimes.length !== (raw.image_mimes as unknown[]).length
    || maxImages === undefined || maxImageBytes === undefined
    || maxFiles === undefined || maxFileBytes === undefined || maxTotalBytes === undefined
    || !modes || typeof modes.vision !== 'boolean' || typeof modes.sandbox_files !== 'boolean'
    || visionAsParts === undefined || doAttachments === undefined
  ) {
    return undefined
  }
  return {
    imageMimes,
    maxImages,
    maxImageBytes,
    maxFiles,
    maxFileBytes,
    maxTotalBytes,
    modes: { vision: modes.vision, sandboxFiles: modes.sandbox_files, visionAsParts, doAttachments },
  }
}

let declaredAttachmentPolicy: AttachmentPolicy | undefined

/**
 * The attachment policy the last catalogue load declared. `undefined` before the
 * catalogue answers, when the server declares none, or after a load that failed:
 * callers then use today's constants. Set by {@link listModels}.
 */
export function getAttachmentPolicy(): AttachmentPolicy | undefined {
  return declaredAttachmentPolicy
}

/**
 * What the organisation's model policy did to this catalogue (`entitlement` beside
 * `models` on `/api/v1/models`): counts, a revision and defaults, never a model
 * list. The server enforces it; the client only reconciles what it shows.
 */
export interface ModelEntitlementMeta {
  /** `none`, `org`, `role` or `member_not_found`. */
  readonly state: string
  /** False in shadow mode: the policy is counted but nothing is hidden or lowered. */
  readonly enforced: boolean
  readonly revision: number
  readonly restricted: boolean
  /** Chat models the policy removed from the catalogue. */
  readonly hiddenCount: number
  readonly defaultModel?: string
  readonly defaultEffort?: string
  /** The highest reasoning level the policy allows. */
  readonly maxEffort?: EffortLevel
}

/**
 * The declared meta, or `undefined` when the response carries none or a required
 * field of it is malformed. Never half-trusted: without it the catalogue is shown
 * as served and the server's own refusal stays the only gate.
 */
export function parseModelEntitlementMeta(value: unknown): ModelEntitlementMeta | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined
  const raw = value as Record<string, unknown>
  const state = str(raw.state)
  if (
    state === undefined
    || typeof raw.enforced !== 'boolean'
    || typeof raw.restricted !== 'boolean'
    || typeof raw.revision !== 'number' || !Number.isFinite(raw.revision)
    || typeof raw.hidden_count !== 'number' || !Number.isFinite(raw.hidden_count)
  ) {
    return undefined
  }
  const defaultModel = str(raw.default_model)
  const defaultEffort = str(raw.default_effort)
  return {
    state,
    enforced: raw.enforced,
    revision: raw.revision,
    restricted: raw.restricted,
    hiddenCount: Math.max(0, Math.trunc(raw.hidden_count)),
    ...(defaultModel ? { defaultModel } : {}),
    ...(defaultEffort ? { defaultEffort } : {}),
    ...(isEffortLevel(raw.max_effort) ? { maxEffort: raw.max_effort } : {}),
  }
}

let declaredModelEntitlement: ModelEntitlementMeta | undefined

/**
 * The entitlement meta the last catalogue load declared. `undefined` before the
 * catalogue answers or when no policy applies. Set by
 * {@link listModels}, like {@link getAttachmentPolicy}, so a caller that already
 * loads the catalogue reads it without a second request.
 */
export function getModelEntitlementMeta(): ModelEntitlementMeta | undefined {
  return declaredModelEntitlement
}

/** The catalogue and what the organisation's policy did to it. */
export interface ModelCatalogue {
  readonly models: ModelInfo[]
  readonly entitlement?: ModelEntitlementMeta
  /**
   * Whether side tasks ask the platform role table (`role_routing` beside `models`). The gateway leaves it out
   * while the switch is off, so `undefined` means off.
   */
  readonly roleRouting?: boolean
}

/**
 * The ceiling the server applies to a request's reasoning effort, or `undefined`
 * when it applies none (no policy, shadow mode, or no `max_effort`).
 */
export function enforcedMaxEffort(meta: ModelEntitlementMeta | undefined): EffortLevel | undefined {
  return meta?.enforced ? meta.maxEffort : undefined
}

/** Whether `level` is within the ceiling; no ceiling allows every level. */
export function effortLevelWithin(level: EffortLevel, ceiling: EffortLevel | undefined): boolean {
  return ceiling === undefined || EFFORT_LEVELS.indexOf(level) <= EFFORT_LEVELS.indexOf(ceiling)
}

/**
 * Whether a selection the policy no longer offers must be dropped. Only an
 * enforced, restrictive policy resets, and only against a catalogue that lists
 * chat models: an empty catalogue is an outage (`listModels()` swallows one into
 * `[]`), and a pinned Verevon mode is resolved server-side so it is never absent.
 */
export function selectionIsStale(
  selection: { readonly model: string; readonly provider?: string },
  catalogue: readonly ModelInfo[],
  meta: ModelEntitlementMeta | undefined,
): boolean {
  if (!meta?.enforced || !meta.restricted) return false
  if (!selection.model || isVerevonModeId(selection.model)) return false
  const chat = catalogue.filter(isChatModel)
  if (chat.length === 0) return false
  return !chat.some((model) => model.id === selection.model && model.provider === selection.provider)
}

export async function listModelCatalogue(): Promise<ModelCatalogue> {
  const raw = await requestJson<unknown>('/api/v1/models')
  const envelope = raw && typeof raw === 'object' && !Array.isArray(raw) ? (raw as Record<string, unknown>) : undefined
  declaredAttachmentPolicy = parseAttachmentPolicy(envelope?.attachment_policy)
  const entitlement = parseModelEntitlementMeta(envelope?.entitlement)
  declaredModelEntitlement = entitlement
  const source = Array.isArray(raw)
    ? raw
    : raw && typeof raw === 'object' && Array.isArray((raw as { models?: unknown }).models)
      ? (raw as { models: unknown[] }).models
      : []

  const models = source
    .filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === 'object')
    .map((item) => {
      const id = str(item.id) ?? ''
      const name = str(item.name) ?? str(item.display_name) ?? str(item.id) ?? 'model'
      const provider = str(item.provider) ?? str(item.vendor) ?? str(item.owned_by)
      const modality = normalizeModality(
        str(item.modality) ?? str(item.type) ?? str(item.kind) ?? str(item.category),
        id,
        name,
      )
      const costTier = normalizeCostTier(item.cost_tier ?? item.cost ?? item.tier)
      // Unknown/garbage tier values normalize to undefined → neutral UI, never
      // a fabricated residency claim.
      const privacyTier = normalizePrivacyTier(item.privacy_tier ?? item.privacyTier)
      return {
        id,
        name: provider === 'openai-codex-subscription'
          ? subscriptionModelDisplayName(id)
          : name,
        capabilities: Array.isArray(item.features)
          ? (item.features as unknown[]).filter((f): f is string => typeof f === 'string')
          : Array.isArray(item.capabilities)
            ? (item.capabilities as unknown[]).filter((f): f is string => typeof f === 'string')
            : undefined,
        provider,
        modality,
        costTier,
        privacyTier,
        residency: str(item.residency),
        cheap: readBool(item.cheap) ?? readBool(item.is_cheap),
        effortLevels: normalizeEffortLevels(item.effort_levels ?? item.effortLevels),
        contextWindowTokens: normalizeTokenCount(item.context_window_tokens),
        maxOutputTokens: normalizeTokenCount(item.max_output_tokens),
        lifecycle: normalizeLifecycle(item.lifecycle),
        price: normalizeModelPrice(item.price),
        dataUse: normalizeModelDataUse(item.data_use),
      }
    })
    .filter((model) => model.id.length > 0)
  return {
    models,
    ...(entitlement ? { entitlement } : {}),
    ...(envelope?.role_routing === true ? { roleRouting: true } : {}),
  }
}

export async function listModels(): Promise<ModelInfo[]> {
  return (await listModelCatalogue()).models
}

/** Product-facing label for a ChatGPT-plan model. `codex` is an implementation
 * detail of the broker, not the entitlement the user selected. */
export function subscriptionModelDisplayName(id: string): string {
  const words = id
    .split('-')
    .filter((word) => word && word.toLocaleLowerCase() !== 'codex')
    .map((word) => word.toLocaleLowerCase() === 'gpt'
      ? 'GPT'
      : word.charAt(0).toLocaleUpperCase() + word.slice(1))
  return `${words.join(' ') || 'GPT'} Subscription`
}

/**
 * The per-model capability flag inference-core emits for a deployment whose
 * zero-retention contract an operator has explicitly attested.
 *
 * It is NOT derived from the region or the provider brand. `feature_flags()` in
 * inference-core only adds it when `supports_zdr` is true, which for Azure
 * OpenAI means the operator supplied a valid evidence-bound attestation
 * (`AZURE_OPENAI_ZDR_*`, validated in `provider/zdr.rs`). The Anthropic
 * provider's own model list hardcodes its features and never lists it, so for
 * that provider the privacy tier below is the only place the same bit shows.
 */
export const ZERO_RETENTION_CAPABILITY = 'zdr'

/**
 * Privacy tiers inference-core only assigns to a provider whose zero-retention
 * attestation holds (`PrivacyTier::classify`: without `supports_zdr` the best a
 * provider gets is `eu_resident`). A second signal for the same bit as
 * {@link ZERO_RETENTION_CAPABILITY}.
 */
const ZERO_RETENTION_PRIVACY_TIERS: ReadonlySet<PrivacyTier> = new Set<PrivacyTier>([
  'zdr_contractual',
  'sovereign',
])

/**
 * Whether one catalogue entry is a CHAT model whose provider carries a recorded
 * zero-retention attestation. A model of another modality (an image or
 * embedding deployment) can carry the flag but cannot serve a ZDR chat turn, so
 * it never counts. Mirrors `model_attests_zdr_for_chat` in the gateway's
 * `orgs/settings.rs`, which decides whether the org ZDR switch may be turned on;
 * both read `apps/gateway/tests/fixtures/zdr-model-catalogue-cases.json`, so the
 * composer, the settings toggle and that gate cannot disagree.
 */
export function modelAttestsZeroRetention(
  model: Pick<ModelInfo, 'capabilities' | 'modality' | 'privacyTier'>,
): boolean {
  if (model.modality !== 'chat') return false
  if (model.capabilities?.includes(ZERO_RETENTION_CAPABILITY)) return true
  return model.privacyTier !== undefined && ZERO_RETENTION_PRIVACY_TIERS.has(model.privacyTier)
}

/**
 * Whether ANY catalogue model can serve a no-retention turn.
 *
 * This exists because the alternative is worse than no feature: inference-core
 * fails a ZDR request CLOSED, skipping every provider that is not attested, so
 * offering the temporary-chat toggle against an unattested fleet produces a
 * turn that always dies. Asking the catalogue lets the UI say so before the
 * user spends a message finding out. The org-level ZDR switch in the workspace
 * settings asks the same question.
 *
 * An EMPTY catalogue returns false. That is deliberate: `listModels()` swallows
 * a gateway outage into `[]`, and in that state we know nothing about provider
 * attestation — offering a mode that fails closed downstream is the wrong guess.
 */
export function hasZeroRetentionModel(models: readonly ModelInfo[]): boolean {
  return models.some(modelAttestsZeroRetention)
}

/**
 * The per-model capability flag inference-core emits on the ChatGPT-subscription
 * models only while the server's `AGENTIC_SUBSCRIPTION_ENABLED` switch is on:
 * the server's own statement that Do (an agentic run) can run on that route.
 * Nothing in the client derives it from the provider name.
 */
export const AGENTIC_CAPABILITY = 'agentic'

/**
 * Whether the catalogue says an agentic run can use this model. `false` for a
 * model that is not listed (yet), so a catalogue that has not answered never
 * switches Do on by guess.
 */
export function modelSupportsAgentic(model: Pick<ModelInfo, 'capabilities'> | undefined): boolean {
  return model?.capabilities?.includes(AGENTIC_CAPABILITY) ?? false
}

/**
 * The catalogue flag for connected-app reads (WS23 S6): a chat model lists it only while
 * the gateway's connector-read switch is on for the deployment, so the composer shows its
 * chip row only when the server says the reads can run. Nothing here derives it from a name.
 */
export const CONNECTOR_SCOPE_CAPABILITY = 'connector_scope'

/**
 * Whether some chat model in the catalogue advertises connected-app reads. Deployment-wide,
 * like the switch behind it: a Verevon mode is not a catalogue entry, so the check is not
 * per model. An empty or unanswered catalogue advertises nothing.
 */
export function catalogueAdvertisesConnectorScope(
  models: readonly Pick<ModelInfo, 'capabilities' | 'modality'>[],
): boolean {
  return models.some((model) => model.modality === 'chat' && (model.capabilities?.includes(CONNECTOR_SCOPE_CAPABILITY) ?? false))
}

/**
 * The per-model capability flag inference-core lists on a model that reads
 * images (`list_models` emits `vision` per model, not per provider).
 */
export const VISION_CAPABILITY = 'vision'

/**
 * Whether the picked model can read an attached image, for the turns on which
 * the model, not the vision-only diversion, is the reader (images as parts).
 *
 * An explicit catalogue model answers for itself. A Verevon mode is not a
 * catalogue entry (`model` is `undefined` for it), so it follows the server's
 * declared policy: some model that reads images exists. Anything else is
 * `false`: a catalogue that has not answered never switches images on by guess.
 */
export function modelSupportsVision(
  model: Pick<ModelInfo, 'capabilities'> | undefined,
  policy: Pick<AttachmentPolicy, 'modes'> | undefined,
  options: { verevonMode?: boolean } = {},
): boolean {
  if (model) return model.capabilities?.includes(VISION_CAPABILITY) ?? false
  return options.verevonMode === true && (policy?.modes.vision ?? false)
}

/**
 * The reasoning levels the catalogue lists for a model, weakest first. Empty
 * for a model that is not listed or lists none, so a catalogue that has not
 * answered never offers a level by guess.
 */
export function modelEffortLevels(model: Pick<ModelInfo, 'effortLevels'> | undefined): readonly EffortLevel[] {
  return model?.effortLevels ?? []
}

/**
 * The context window as a picker row states it, such as "200k kontekst" or "1M kontekst".
 * `undefined` when no window is known, so a row never claims one it has not been given.
 */
export function formatContextWindow(tokens: number | undefined): string | undefined {
  if (tokens === undefined) return undefined
  if (tokens >= 1_000_000 && tokens % 1_000_000 === 0) return `${tokens / 1_000_000}M kontekst`
  return `${Math.round(tokens / 1_000)}k kontekst`
}

/** Patterns that mark a model as expensive enough to warrant a deliberate pick. */
const EXPENSIVE_ID_PATTERNS: readonly RegExp[] = [/opus/i, /sonnet/i, /gpt-5/i, /\bo[13]\b/i, /\bo3\b/i]

/**
 * True when the model is chat-capable. Prefers explicit modality metadata; falls
 * back to name/id heuristics so the old `{ id, name }` shape still filters cleanly.
 */
export function isChatModel(model: ModelInfo): boolean {
  if (model.modality) return CHAT_MODALITIES.has(model.modality)
  const haystack = `${model.id} ${model.name}`
  return !NON_CHAT_NAME_PATTERNS.some((re) => re.test(haystack))
}

/** True for models a user should select deliberately (opus/sonnet/gpt-5.x …). */
export function isExpensiveModel(model: ModelInfo): boolean {
  if (model.costTier === 'high') return true
  if (model.cheap === true) return false
  return EXPENSIVE_ID_PATTERNS.some((re) => re.test(`${model.id} ${model.name}`))
}

/**
 * The cost-aware default model id. Verevon Balance is the first-class default:
 * the backend resolves `verevon-balance` server-side (complexity + budget →
 * concrete model, with model-router as the cheap fallback), so we don't need to
 * pick a concrete catalog model. `models` is accepted for API compatibility and
 * future heuristics, but the balance mode is authoritative.
 *
 * The composer/ChatPage fall back to `''` if anything is off — the backend treats
 * an empty model as balance too — so a bad value can never crash the picker.
 */
export function cheapDefaultModelId(models: readonly ModelInfo[]): string {
  // `models` is intentionally not consulted today: Verevon Balance is resolved
  // server-side. Kept in the signature for API compatibility + future heuristics.
  void models
  return VEREVON_BALANCE_MODE_ID
}

const PROVIDER_GROUP_ORDER: readonly { label: string; test: (m: ModelInfo) => boolean }[] = [
  { label: 'Subscription', test: (m) => m.provider === 'openai-codex-subscription' },
  { label: 'Claude', test: (m) => m.provider === 'anthropic' || /claude/i.test(`${m.id} ${m.name}`) },
  { label: 'OpenAI GPT', test: (m) => m.provider === 'openai' || /gpt|^o[0-9]|model-router/i.test(`${m.id} ${m.name}`) },
  { label: 'DeepSeek', test: (m) => m.provider === 'deepseek' || /deepseek/i.test(`${m.id} ${m.name}`) },
]

export type ModelGroup = { label: string; models: ModelInfo[]; folded?: boolean }

/** The fold that holds models the vendor has deprecated: listed last and collapsed. */
const DEPRECATED_GROUP_LABEL = 'Eldre modeller'

/**
 * Group chat models by family/provider for the picker. Unmatched providers fall
 * into an "Other models" bucket. Empty groups are dropped. Group order is stable.
 * A deprecated model leaves its provider group and sits in a folded last group.
 *
 * Verevon intent modes are filtered out here: the backend may advertise them
 * synthetically (`provider === 'verevon'` or a `verevon-*` mode id), but the pinned
 * Verevon group in the UI is authoritative, so we drop the duplicates from the
 * normal catalog groups.
 */
export function groupChatModels(models: readonly ModelInfo[]): ModelGroup[] {
  const chat = models
    .filter(isChatModel)
    .filter((model) => model.provider !== 'verevon' && !isVerevonModeId(model.id))
  const buckets = new Map<string, ModelInfo[]>()
  const otherLabel = 'Other models'
  const deprecated: ModelInfo[] = []

  for (const model of chat) {
    if (model.lifecycle === 'deprecated') {
      deprecated.push(model)
      continue
    }
    const group = PROVIDER_GROUP_ORDER.find((g) => g.test(model))
    const label = group?.label ?? otherLabel
    const existing = buckets.get(label)
    if (existing) existing.push(model)
    else buckets.set(label, [model])
  }

  const ordered: ModelGroup[] = []
  for (const { label } of PROVIDER_GROUP_ORDER) {
    const models = buckets.get(label)
    if (models && models.length > 0) ordered.push({ label, models })
  }
  const other = buckets.get(otherLabel)
  if (other && other.length > 0) ordered.push({ label: otherLabel, models: other })
  if (deprecated.length > 0) {
    ordered.push({ label: DEPRECATED_GROUP_LABEL, models: deprecated, folded: true })
  }
  return ordered
}

/**
 * Lower-cases and drops accents so `cafe` finds `café`. The Norwegian letters æ, ø and å are kept
 * as they are: they are letters of their own, not accented forms of other letters.
 */
const NORWEGIAN_LETTERS: ReadonlySet<string> = new Set(['æ', 'ø', 'å'])

function searchKey(value: string): string {
  return Array.from(value.normalize('NFC').toLowerCase(), (char) =>
    NORWEGIAN_LETTERS.has(char) ? char : char.normalize('NFD').replace(/\p{M}/gu, ''),
  ).join('')
}

function matchesModelQuery(query: string, ...fields: readonly string[]): boolean {
  const needle = searchKey(query.trim())
  if (needle === '') return true
  return fields.some((field) => searchKey(field).includes(needle))
}

/**
 * The models whose name, id or provider match the query, case- and accent-insensitively. An empty
 * query keeps every model in its order.
 */
export function filterModels(models: readonly ModelInfo[], query: string): ModelInfo[] {
  if (query.trim() === '') return [...models]
  return models.filter((model) => matchesModelQuery(query, model.name, model.id, model.provider ?? ''))
}

/** A list price as one line of picker copy, in the reader's language and currency format. */
export function formatModelListPrice(price: ModelPrice, locale: Locale): string {
  const money = new Intl.NumberFormat(localeIntl(locale), {
    style: 'currency',
    currency: price.currency,
    minimumFractionDigits: 2,
    maximumFractionDigits: 4,
  })
  const input = money.format(price.inputPerMillion)
  const output = money.format(price.outputPerMillion)
  return locale === 'no'
    ? `${input} inn / ${output} ut per million tokens`
    : `${input} in / ${output} out per million tokens`
}

export type ChatFeedbackRating = 'positive' | 'negative'

/**
 * Rate one chat turn.
 *
 * Wire contract: `{requestId, rating, note}` → gateway → model-gateway
 * `/v1/feedback`. `requestId` is the SSE turn id; model-gateway resolves it
 * server-side to the durable run id and to the skill ids it injected into that
 * turn. The client deliberately does NOT send skill ids — it does not reliably
 * know them, and a client-asserted skill id would let a rating be aimed at any
 * skill in the org.
 *
 * Rejects with {@link ApiError} on failure. Callers must surface that: a
 * swallowed rejection is what made the UI light the thumb up while nothing was
 * recorded.
 */
export async function submitFeedback(
  requestId: string,
  rating: ChatFeedbackRating,
  options: { note?: string; runId?: string } = {},
): Promise<void> {
  await requestJson('/api/v1/chat/feedback', {
    method: 'POST',
    body: JSON.stringify({
      requestId,
      rating,
      note: options.note,
      // Only agentic turns learn their durable run id (the `connected` event
      // carries it). Sending it lets the rating still land as a run-only rating
      // if the gateway no longer holds the turn record. It is not a trust
      // shortcut: model-gateway still verifies run ownership server-side.
      run_id: options.runId,
    }),
  })
}

/**
 * Rate a durable run's result by its run id: an agent run's answer or a scheduled
 * run's fire, where there is no chat turn to rate.
 *
 * Wire contract: `{run_id, rating, note}` → gateway → model-gateway `/v1/feedback`,
 * which checks that the caller owns the run before it stores anything. A
 * zero-retention caller is refused (412) and nothing is stored. As in
 * `submitFeedback`, no skill id is sent.
 *
 * Rejects with {@link ApiError} on failure, so a caller never shows a rating that was not stored.
 */
export async function submitRunFeedback(
  runId: string,
  rating: ChatFeedbackRating,
  note?: string,
): Promise<void> {
  await requestJson('/api/v1/chat/feedback', {
    method: 'POST',
    body: JSON.stringify({ run_id: runId, rating, note }),
  })
}

/**
 * User-facing reason a rating did not stick. Keeps the wording in one place so
 * the chat surface only has to decide *where* to show it. Norwegian, unless the
 * caller passes its reader's chooser.
 */
export function describeFeedbackFailure(
  error: unknown,
  tr: (noText: string, enText: string) => string = (noText) => noText,
): string {
  if (error instanceof ApiError) {
    if (error.code === 'turn_not_rateable') {
      return tr(
        'Denne meldingen kan ikke vurderes lenger. Vurderingen ble ikke lagret.',
        'This answer can no longer be rated. The rating was not saved.',
      )
    }
    if (error.status === 401 || error.status === 403) {
      return tr(
        'Du har ikke tilgang til å vurdere denne meldingen. Vurderingen ble ikke lagret.',
        'You do not have access to rate this answer. The rating was not saved.',
      )
    }
    if (error.status === 404) {
      return tr(
        'Vi fant ikke kjøringen som skulle vurderes. Vurderingen ble ikke lagret.',
        'The run to rate could not be found. The rating was not saved.',
      )
    }
    if (error.status === 412) {
      return tr(
        'Vurderinger lagres ikke når Zero Data Retention er aktivt.',
        'Ratings are not saved while zero data retention is on.',
      )
    }
  }
  return tr('Kunne ikke lagre vurderingen. Prøv igjen.', 'The rating could not be saved. Try again.')
}
