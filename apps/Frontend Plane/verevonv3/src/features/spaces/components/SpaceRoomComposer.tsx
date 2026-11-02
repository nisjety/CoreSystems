import { createEffect, createMemo, createSignal, For, onCleanup, Show } from 'solid-js'

import { ChatMarkdown, QueuedInputStrip } from '@/features/chat/components/ChatMessages'
import type { ChatArtifact, QueuedInput } from '@/features/chat/components/chat-types'
import { listAvailableSkills, MAX_PICKED_SKILLS, type Skill } from '@/shared/api/skills-client'
import { Paperclip, WandSparkles } from '@/shared/icons'
import { ATTACHMENT_ACCEPT, ATTACHMENT_MAX_IMAGES, isImageAttachment, type AttachmentPolicyContext, visionAsPartsEnabled } from '@/features/chat/lib/attachment-policy'
import { isLocalRetentionAllowed } from '@/features/chat/lib/chat-retention'
import { getAttachmentPolicy, modelSupportsVision, type ModelInfo } from '@/shared/api/chat-client'
import {
  formatChipSize,
  pastedImageFiles,
  pastedTextFile,
  pasteBecomesAttachment,
  prepareRoomTurn,
  revokeRoomAttachment,
  splitRoomMessage,
  toRoomAttachment,
  vetRoomAttachments,
  type RoomAttachment,
} from '../lib/room-attachments'
import {
  getInstallableSpaceAgents,
  SPACE_POST_MAX_CHARS_DEFAULT,
  type InstallableSpaceAgent,
  type SpaceAgent,
  type SpaceMentionsInput,
  type SpacePostReceipt,
  type SpaceAgentWork,
  type SpaceRosterMember,
  type SpaceThread,
} from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { translateApiError } from '@/shared/i18n/errors'
import { isSpaceObserved, liveThreadsIn, ownStream } from '../lib/space-live-work'
import { createSpaceMidRun, type SpaceMidRunResend } from '../lib/space-mid-run'
import { agentAvatarColor } from '../lib/agent-avatar'
import { AgentAvatar } from './AgentAvatar'
import { threadTitle } from '../lib/space-thread-presentation'
import {
  type SpaceComposerModeEntry,
  type SpaceComposerModeId,
  type SpaceComposerSlotContext,
  visibleSpaceComposerModes,
} from '../lib/space-composer-registry'
import { createSpaceSendPipeline } from '../lib/space-send-pipeline'
import {
  currentSearchOpener,
  matchPlatformCommands,
  openRoomTabByHash,
  openShortcutSheetViaHotkeyLayer,
  parseSlashCommandText,
  type PlatformCommand,
  type SlashCommandContext,
} from '../lib/space-slash-commands'
import { spaceRetrievalDeclineCopy } from '../lib/space-retrieval-decline'
import { createSpacePostSender } from '../lib/space-post-sender'
import { createSpaceSendJournal, type SpaceSendJournal } from '../lib/space-send-journal'
import { qualifyCandidates, REFUSAL_REASON_CODE, type MentionCandidate as RoomMentionCandidate } from '../lib/mention-model'
import { createMentionState, readRecentMentions } from '../lib/mention-state'
import { foldForMatch, orderBySection, rankMentionCandidates } from '../lib/mention-ranking'
import { spaceWakeSequencer } from '../lib/wake-sequencer'
import { MentionPicker, type MentionPickerRow } from './MentionPicker'
import { AddressedAgentsBar } from './AddressedAgentsBar'
import { AgentAddDialog, type AgentAddTarget } from './AgentAddDialog'
import { DM_SPACE_FEATURE } from '../lib/space-kind'
import { canPostInSpace, postLength } from '../lib/space-posting'
import { SpaceConnectivityBanner } from './SpaceConnectivityBanner'
import { SpaceRunStrip } from './SpaceRunStrip'
import {
  SkillScope,
  SpaceComposerChipSlot,
  SpaceComposerToolbarSlot,
} from './composer/SpaceComposerSlots'
import { createTurnPrivacyTierResolver } from '@/features/dashboard/home/submit-privacy-tier'
import { readAiModelSelection, type AiModelSelection } from '@/shared/ai/model-selection'
import { OPENAI_CODEX_SUBSCRIPTION_PROVIDER } from '@/shared/api/chatgpt-subscription-client'
import { getSession } from '@/shared/session/session-store'
import { useHomeDraft } from '@/features/dashboard/home/home-draft'
import { spaceRoomFeature, spaceRoomFeatureWithDevOverride } from '../lib/space-room-features'
import { canAddRoomFiles, roomFilesDurableEnabled, roomFilesFlag, roomFilesFromChips, roomFilesLimits } from '../lib/room-files'
import { uploadSpaceFiles } from '@/shared/api/space-files-client'
import { applyFormat, markdownShortcut, type MarkdownFormatKind } from '../lib/markdown-edit'
import './space-send.css'

/**
 * The room's own composer — buzz's "the channel is the workspace" plus Grok's
 * teammate ergonomics, per `docs/space-defenition.md`. Deliberately NOT a
 * smaller copy of `/chat`: `/chat` is the singular, full-capability Verevon
 * agent surface; this is a distinct, room-scoped surface whose only shared
 * machinery is the underlying SSE transport (`streamChat`) every caller into
 * Model Plane ultimately uses.
 *
 * `@` offers this room's own roster — people and agents, one list — and
 * selecting an agent invokes it. The invocation carries only the agent's
 * Control `subject_id`; the gateway re-resolves membership, binding, and the
 * agent's own instructions server-side (`inject_mentioned_space_agent_persona`
 * in the gateway) before anything reaches the model. A mention never grants:
 * an agent not already bound to this room simply is not a candidate here —
 * proposing to bind one is Phase UI-3b, not this composer.
 *
 * # A turn can be stopped, and a stop is recorded as one (item 1b)
 *
 * Until this existed a member who sent a message could only wait. Stop does two
 * things, and the order matters: it aborts the browser's SSE read, and it asks
 * the gateway to cancel the invocation by the `requestId` the stream announced
 * in its `connected` frame. Only the second one records anything. Model Plane's
 * disconnect handling deliberately *detaches and finishes* a stream whose client
 * went away (so a closed tab can resume), which means an abort alone leaves the
 * run generating and its status `running` for every other member. The cancel
 * endpoint is what flips the run to `cancelled` in Session Core, which is what
 * the room's projection shows the next time it polls.
 *
 * If Stop is pressed before the `connected` frame arrived there is no
 * `requestId` to cancel with. That case is said plainly ("stopped before the
 * reply started") rather than claimed as a recorded stop, because it is not one.
 *
 * # The generated title is persisted for the room, not just for this browser
 * (item 4b)
 *
 * Model Plane summarises a thread's first exchange into a short title and
 * emits it as a `title` event — it does not store it. Chat keeps that title in
 * a browser-local snapshot, which is fine for one person's device and useless
 * for a room, where every member must see the same name. So when THIS composer
 * opened the thread, the title is written back through the room's own
 * presentation route (Session Core is owner-bound, and the sender owns the
 * thread they just started). A reply into someone else's thread never gets a
 * title event (it is not a first exchange) and would not be the owner anyway.
 * Best-effort by design: a turn must never fail or stall over a label.
 */

type MentionCandidate = {
  readonly id: string
  readonly label: string
  readonly kind: 'person' | 'agent'
  /** Only present for `kind: 'agent'` — the Control subject id to invoke. */
  readonly subjectId?: string
  /** Validated avatar colour for `kind: 'agent'`; absent renders neutral. */
  readonly color?: string
}

/**
 * One row of the composer's picker, whichever trigger opened it. `@` and `/`
 * share one list, one highlighted row and one set of keys (arrows, Enter, Tab,
 * Escape), so the two never behave differently — the mention picker used to
 * be mouse-only.
 */
type PickerItem =
  | { readonly kind: 'mention'; readonly candidate: MentionCandidate }
  | { readonly kind: 'mentionRow'; readonly row: MentionPickerRow }
  | { readonly kind: 'skill'; readonly skill: Skill }
  | { readonly kind: 'command'; readonly command: PlatformCommand }

export interface SpaceRoomComposerProps {
  readonly spaceRef: string
  readonly roster: () => readonly SpaceRosterMember[]
  readonly agents: () => readonly SpaceAgent[]
  /** Called once a turn completes, so the caller can refetch the thread list
   * — a new thread was minted server-side and belongs there now. */
  readonly onExchangeSettled?: () => void
  /** Hands the caller a focus function, so surfaces above the composer (the
   * room intro's "Start a conversation" card) can drop the cursor here. */
  readonly registerFocusHandle?: (focus: () => void) => void
  /** When set, the next message continues that existing room thread: the
   * gateway exchanges it for a fresh, content-bound append decision from
   * Control instead of minting a new thread. Cleared by the caller via
   * `onClearReplyTarget` — on success, or when the member dismisses it. */
  readonly replyTarget?: () => { threadId: string; title: string; awaitingApproval?: boolean } | undefined
  readonly onClearReplyTarget?: () => void
  /** The reply target is the surface itself (the thread pane's composer): no "cancel reply" control. */
  readonly replyTargetFixed?: boolean
  /**
   * The room's threads, for the "someone else's agent is working" line above
   * the textarea. Read through the live-work store's projection, which the page
   * publishes on its poll — so this is the server's view, not this browser's.
   */
  readonly threads?: () => readonly SpaceThread[]
  /**
   * "Kari skriver …" — other members writing in this room right now, from the
   * page's presence beat. A person, unlike the working line above it, which is
   * an agent.
   */
  readonly typingSentence?: () => string | undefined
  /** This member is typing. The page decides how often that reaches the server. */
  readonly onTyping?: () => void
  /**
   * A live artifact from this member's own turn, with the thread it landed in.
   * The page keeps it in memory until the next transcript read; nothing here
   * stores it. Absent while the room artifact surfaces are off.
   */
  readonly onArtifact?: (artifact: ChatArtifact, threadId: string) => void
  /**
   * Human posts (WS01-S6). `humanPostsEnabled` is the room's `features.human_posts`: off, the box is
   * byte-for-byte today's single "ask the agent" composer. On, Send posts the message as the
   * member's own words with no model run, and Ask Verevon is the explicit second mode. None of
   * these props authorizes anything; Control decides per send.
   */
  readonly humanPostsEnabled?: () => boolean
  /** The reading member's Control subject: Ask is offered in a thread only when they started it. */
  readonly viewerSubjectId?: () => string | undefined
  /** The member's role; a role below editor gets a disabled box with the reason stated. */
  readonly viewerRole?: () => string | undefined
  /** The reply target's starter, when the caller knows it; otherwise read from `threads`. */
  readonly threadOwnerSubjectId?: () => string | undefined
  /** `limits.post_max_chars` from the room context. */
  readonly postMaxChars?: () => number | undefined
  /**
   * WS12-S3: whether "Ask Verevon" may be offered at all. A direct message is people only, so the
   * page passes false there; absent, it follows the room's kind trait (`space_dm`). Presentation
   * only: the gateway and Control refuse a model run in a DM regardless.
   */
  readonly canAsk?: () => boolean
  /** SD-16: which mode a fresh box starts in while posts are on. Defaults to Send (post). */
  readonly defaultSendMode?: 'post' | 'ask'
  /** The room's send journal (the page shares it with the timeline for optimistic rows). */
  readonly journal?: SpaceSendJournal
  /**
   * `data.agent_work` from the thread listing (WS09-S2). `steer_owner` keeps the box open while the
   * member's own turn works and offers "Send when ready" on a busy thread. Absent or false, the box
   * is disabled during a turn exactly as before. A rollout answer only: the owner plane re-authorizes
   * every steer.
   */
  readonly agentWork?: () => SpaceAgentWork | undefined
  /**
   * Click-through from the activity strip (WS09-S6): open the working conversation. Absent, the
   * strip's lines are plain text.
   */
  readonly onOpenRun?: (threadId: string) => void
}

export function SpaceRoomComposer(props: SpaceRoomComposerProps) {
  const i18n = useI18n()
  const orgId = getSession().activeOrg?.id?.trim() ?? ''
  const [selectedModel, setSelectedModel] = createSignal<AiModelSelection>(readAiModelSelection(orgId))
  // One catalogue request for the picker and for a send that has to wait for
  // it, as in the main composer: a catalogue model's tier rides every turn.
  const privacyTiers = createTurnPrivacyTierResolver({ orgId })
  // Drafts (WS15-S2): one per room and per reply target, kept in this tab only and only while the
  // room's `features.composer_drafts` is on. Off, the scope is undefined, the hook reads and writes
  // nothing, and the box is today's. `zeroRetention` is false because a room has no Temporary chat;
  // the workspace's retention verdict still closes the hook's gate. Presentation only.
  const draftsOn = () => spaceRoomFeatureWithDevOverride('composer_drafts', import.meta.env.VITE_SPACE_COMPOSER_DRAFTS)
  const [text, setText, discardDraft] = useHomeDraft({
    zeroRetention: () => false,
    scope: () => (draftsOn() ? `space:${props.spaceRef}:${props.replyTarget?.()?.threadId ?? 'new'}` : undefined),
  })
  // Markdown authoring (WS15-S3): a small format bar, a Write/Preview toggle, a box that grows from two
  // to ten rows and an always-on counter, all behind `features.composer_markdown`. Off, the box is
  // today's byte for byte. The text stays plain markdown in the textarea; nothing is stored beyond
  // the draft above. Presentation only, never an authorization input.
  const markdownOn = () => spaceRoomFeatureWithDevOverride('composer_markdown', import.meta.env.VITE_SPACE_COMPOSER_MARKDOWN)
  const [previewing, setPreviewing] = createSignal(false)
  const [mentionQuery, setMentionQuery] = createSignal<string | undefined>(undefined)
  const [mentionedAgentRef, setMentionedAgentRef] = createSignal<string | undefined>(undefined)
  // Structured mentions (WS08-S6): who the words name, and the agents the address bar holds. The
  // legacy `mentionedAgentRef` above stays the path for Ask Verevon, skills and a room with the
  // flags off.
  const mention = createMentionState(() => props.spaceRef, () => props.replyTarget?.()?.threadId)
  createEffect(() => {
    mention.rebind(props.replyTarget?.()?.threadId)
  })
  const [mentionNotice, setMentionNotice] = createSignal<string | undefined>(undefined)
  const [addTarget, setAddTarget] = createSignal<AgentAddTarget | undefined>(undefined)
  const [installable, setInstallable] = createSignal<readonly InstallableSpaceAgent[] | undefined>(undefined)
  let installableLoading = false
  // `/` skills (later tier). The catalogue is fetched on the first `/`, never on
  // mount: a room that never picks a skill never pays for the list. `undefined`
  // means not loaded yet; a failed load is its own state so the picker can say
  // so instead of showing an empty list that looks like "you have none".
  const [skillQuery, setSkillQuery] = createSignal<string | undefined>(undefined)
  const [skills, setSkills] = createSignal<readonly Skill[] | undefined>(undefined)
  const [skillsFailed, setSkillsFailed] = createSignal(false)
  const [pickedSkills, setPickedSkills] = createSignal<readonly Skill[]>([])
  const [activeIndex, setActiveIndex] = createSignal(0)
  // Platform slash commands (WS13-S7): a local, ephemeral result line. Never a timeline entry.
  const [commandStatus, setCommandStatus] = createSignal<string | undefined>(undefined)
  let skillsLoading = false
  const pickerId = `room-picker-${Math.random().toString(36).slice(2, 8)}`
  // The member's own pick between Send and Ask; `effectiveMode` below is what the box does.
  const [pickedMode, setPickedMode] = createSignal<SpaceComposerModeId | undefined>(undefined)
  const [zdrRefused, setZdrRefused] = createSignal(false)
  // Per-turn attachments (WS14-S8). In memory only: chips hold object URLs that are revoked on
  // removal, on a clean send and on unmount; nothing is written to browser storage or to the room.
  // `features.turn_attachments` (gateway `space_turn_attachments_ui`) switches the whole surface;
  // off, the box has no attach button and ignores paste and drop exactly as before.
  const [files, setFiles] = createSignal<readonly RoomAttachment[]>([])
  const [attachNotice, setAttachNotice] = createSignal<string | undefined>(undefined)
  const [dragActive, setDragActive] = createSignal(false)
  const [preparing, setPreparing] = createSignal(false)
  // The gateway states it as `features.room_files.turn_attachments`; a dev build may force it.
  const attachFlagOn = () =>
    spaceRoomFeatureWithDevOverride('turn_attachments', import.meta.env.VITE_SPACE_TURN_ATTACHMENTS) ||
    roomFilesFlag('turn_attachments')
  // WS14-S9: the member's choice to also keep this turn's text files in the room's Files tab.
  // Per message, never remembered: it resets after every send. Offered only where the library is on.
  const [keepInRoom, setKeepInRoom] = createSignal(false)
  const keepOffered = () => roomFilesDurableEnabled() && canAddRoomFiles(props.viewerRole?.())
  let fileInputRef: HTMLInputElement | undefined
  let textareaRef: HTMLTextAreaElement | undefined
  const MARKDOWN_ROWS_MIN = 2
  const MARKDOWN_ROWS_MAX = 10
  // Grow with the text, two rows up to ten, then scroll. Measured from scrollHeight so wrapped lines
  // count; a layout-less environment measures zero and stays at two.
  createEffect(() => {
    text()
    previewing()
    const element = textareaRef
    if (!element) return
    if (!markdownOn()) {
      element.rows = MARKDOWN_ROWS_MIN
      return
    }
    element.rows = MARKDOWN_ROWS_MIN
    const lineHeight = Number.parseFloat(getComputedStyle(element).lineHeight)
    if (!Number.isFinite(lineHeight) || lineHeight <= 0 || element.scrollHeight <= 0) return
    const style = getComputedStyle(element)
    const padding = (Number.parseFloat(style.paddingTop) || 0) + (Number.parseFloat(style.paddingBottom) || 0)
    const needed = Math.ceil((element.scrollHeight - padding) / lineHeight)
    element.rows = Math.max(MARKDOWN_ROWS_MIN, Math.min(MARKDOWN_ROWS_MAX, needed))
  })
  // An emptied box (a sent message) leaves Preview.
  createEffect(() => {
    if (previewing() && (!markdownOn() || text().trim() === '')) setPreviewing(false)
  })
  // What to clear once the in-flight turn is accepted; set only while drafts keep the words in the box.
  let acceptSend: (() => void) | undefined
  // The send path (in-flight handles, exchange state, Stop) lives in the send
  // pipeline; the composer keeps the box, the pickers and the chosen chips.
  // Owner mid-run steer (WS09-S2): the strip, the messages the run never took and the one waiting
  // for the agent to be free. In memory only.
  const midRun = createSpaceMidRun({
    tr: i18n.tr,
    settled: () => !pipeline.submitting(),
  })
  // Messages the run never took, to go out as ordinary turns one at a time.
  let resendQueue: SpaceMidRunResend[] = []
  function flushResends(found: readonly SpaceMidRunResend[]): void {
    resendQueue = [...resendQueue, ...found]
    if (pipeline.submitting()) return
    const next = resendQueue.shift()
    if (!next) return
    midRun.startTurn(true)
    void pipeline.send({
      content: next.content,
      mode: 'ask',
      model: selectedModel(),
      locale: i18n.locale(),
      skills: [],
      replyThreadId: pipeline.liveThreadId(),
      ...(next.clientMessageId ? { sendId: next.clientMessageId } : {}),
    })
  }
  const pipeline = createSpaceSendPipeline({
    host: props,
    resolveTier: privacyTiers.resolve,
    onQueuedInput: (messages, ids) => midRun.markDelivered(messages, ids),
    // With drafts on, the box is emptied when the server has the turn, not when Send is pressed
    // (WS15-S2): a send that fails or is stopped before `connected` keeps its words.
    onStage: (stage) => {
      const accept = acceptSend
      if (stage === 'accepted') {
        acceptSend = undefined
        accept?.()
      } else if (stage === 'settled') {
        acceptSend = undefined
        // The server said it already had this send: accepted, though no `connected` came.
        if (pipeline.exchange()?.status === 'done') accept?.()
        // The run ended on its own: a steer it never took goes out again as an ordinary turn. A
        // stop or a failure sends nothing the member did not ask for.
        if (pipeline.exchange()?.status === 'done') flushResends(midRun.settle())
        else resendQueue = []
      }
    },
    // Known codes get the room's own words, and a typed Space denial (its
    // details name the reason) gets the denial copy and next step; an unknown
    // code keeps the server's message rather than a generic line that hides it.
    describeError: (event) => translateApiError({ code: event.code, details: event.details, feature: event.feature }, i18n.tr, {
      no: event.message,
      en: event.message,
    }),
  })
  const submitting = pipeline.submitting
  const exchange = pipeline.exchange

  // Human posts (WS01-S6): their own transport, journal and states beside the streamed turn.
  const journal = props.journal ?? createSpaceSendJournal()
  const postSender = createSpacePostSender({
    journal,
    spaceRef: () => props.spaceRef,
    describeError: (error) => translateApiError(error, i18n.tr, {
      no: 'Meldingen ble ikke sendt. Den er beholdt i boksen; prøv igjen.',
      en: 'The message was not sent. It is kept in the box; try again.',
    }),
    baselineUpdatedAt: (threadId) => props.threads?.().find((thread) => thread.thread_id === threadId)?.updated_at,
    onAccepted: (entry, receipt) => {
      // Cleared only now, on acceptance: until then the words are still here to retry or edit.
      if (text().trim() === entry.content) {
        setText('')
        discardDraft()
      }
      startWakes(entry.content, entry.mentions, receipt)
      props.onClearReplyTarget?.()
      props.onExchangeSettled?.()
    },
    onZdrRefused: () => setZdrRefused(true),
  })
  onCleanup(postSender.dispose)
  // WS08-S6: the post is stored; now wake each agent the server recorded as `expected`, one at a
  // time in the order named. Refused ones are shown with their reason and never run. A replayed
  // receipt carries no mention records, so nothing is woken for it.
  function startWakes(content: string, sent: SpaceMentionsInput | undefined, receipt: SpacePostReceipt): void {
    const records = receipt.mentions
    const names = new Map(roomCandidates().filter((c) => c.kind === 'agent').map((c) => [c.subjectId, c.name]))
    const barLabels = new Map(mention.agents().map((agent) => [agent.subjectId, agent.label]))
    const wakeOn = addressedWakeOn()
    mention.sent(sent)
    if (records) {
      const rejected = [...new Set(records.rejected.map((entry) => entry.code))]
      setMentionNotice(
        rejected.length > 0
          ? rejected.map((code) => translateApiError({ code }, i18n.tr, { no: 'En omtale ble ikke lagret.', en: 'A mention was not recorded.' })).join(' ')
          : undefined,
      )
    }
    if (!wakeOn || !records || !receipt.messageId) return
    const agents = records.recorded
      .filter((entry) => entry.kind === 'agent' && (entry.wake === 'expected' || entry.wake === 'refused'))
      .map((entry, index) => ({
        agentSubjectId: entry.subjectId,
        agentName: names.get(entry.subjectId) ?? barLabels.get(entry.subjectId) ?? i18n.tr('Agent', 'Agent'),
        ordinal: entry.ordinal ?? index,
        ...(entry.wake === 'refused'
          ? { refusedReason: REFUSAL_REASON_CODE[entry.reason ?? ''] ?? entry.reason ?? 'agent_respond_to_denied' }
          : {}),
      }))
    if (agents.length === 0) return
    const picked = selectedModel()
    const subscription = picked.provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER
    void (async () => {
      let minPrivacyTier: Awaited<ReturnType<typeof privacyTiers.resolve>>['tier'] | undefined
      if (!subscription) {
        try {
          minPrivacyTier = (await privacyTiers.resolve(picked.model, picked.provider)).tier
        } catch {
          minPrivacyTier = undefined
        }
      }
      spaceWakeSequencer.start({
        messageId: receipt.messageId,
        threadId: receipt.threadId,
        spaceRef: props.spaceRef,
        content,
        agents,
        ...(subscription ? {} : { model: picked.model }),
        ...(minPrivacyTier ? { minPrivacyTier } : {}),
        locale: i18n.locale(),
      })
    })()
  }
  // A wake that settles changes the room (a run ended, an answer landed): re-read it.
  onCleanup(spaceWakeSequencer.onSettled(() => props.onExchangeSettled?.()))
  const humanPostsActive = () => props.humanPostsEnabled?.() === true
  const roleMayPost = () => canPostInSpace(props.viewerRole?.())
  // Flag on and a role below editor: nothing is offered. Control would refuse every send, so the
  // box says so instead of accepting text it cannot send.
  const roleBlocked = () => humanPostsActive() && !roleMayPost()
  const postMax = () => {
    const limit = props.postMaxChars?.()
    return typeof limit === 'number' && Number.isFinite(limit) && limit > 0 ? limit : SPACE_POST_MAX_CHARS_DEFAULT
  }
  const replyThreadOwner = () => {
    const explicit = props.threadOwnerSubjectId?.()
    if (explicit !== undefined) return explicit
    const targetId = props.replyTarget?.()?.threadId
    return targetId ? props.threads?.().find((thread) => thread.thread_id === targetId)?.owner_subject_id : undefined
  }
  // A member-invoked run stays owner-gated: Ask is offered in a reply only to the thread's starter.
  const askAllowedHere = () => props.canAsk?.() ?? !spaceRoomFeature(DM_SPACE_FEATURE)
  const askAvailable = () => {
    if (!askAllowedHere()) return false
    if (!props.replyTarget?.()) return true
    const viewer = props.viewerSubjectId?.()?.trim()
    const owner = replyThreadOwner()?.trim()
    return Boolean(viewer) && viewer === owner
  }
  // An agent mention or a picked skill can only be a question to the agent.
  const mentionPickerOn = () => humanPostsActive() && spaceRoomFeature('mention_picker')
  // An agent mention is a post plus a wake only while the room says so and the org allows durable
  // writes; a zero-retention refusal turns it back into the legacy ephemeral turn.
  const addressedWakeOn = () => mentionPickerOn() && spaceRoomFeature('addressed_wake') && !zdrRefused()
  const askAgentRef = () => (addressedWakeOn() ? mention.agents()[0]?.subjectId : mentionedAgentRef())
  const impliesAsk = () =>
    askAllowedHere() && ((!addressedWakeOn() && Boolean(mentionedAgentRef())) || pickedSkills().length > 0)
  const offeredModes = createMemo<readonly SpaceComposerModeEntry[]>(() => {
    const registered = visibleSpaceComposerModes()
    const ask = askAllowedHere() ? registered.filter((entry) => entry.id === 'ask') : []
    if (!humanPostsActive()) return ask
    if (roleBlocked()) return []
    const post = registered.filter((entry) => entry.id === 'post')
    return [
      ...(!zdrRefused() && !impliesAsk() ? post : []),
      ...(askAvailable() ? ask : []),
    ]
  })
  const effectiveMode = (): SpaceComposerModeId => {
    const offered = offeredModes()
    const wanted = pickedMode() ?? (humanPostsActive() ? (props.defaultSendMode ?? 'post') : 'ask')
    return offered.some((entry) => entry.id === wanted) ? wanted : (offered[0]?.id ?? 'ask')
  }
  const nothingOffered = () => humanPostsActive() && offeredModes().length === 0
  const postBusy = () => postSender.busy()
  const postFailure = () => {
    const entry = postSender.current()
    return entry?.stage === 'failed' ? entry : undefined
  }
  const overLimit = () => effectiveMode() === 'post' && postLength(text().trim()) > postMax()

  // Per-turn attachments ride a turn for the agent only. A human post is a durable message of the
  // member's own words, so files never go with one; durable room files are the Files tab (WS14-S9).
  const attachAvailable = () => attachFlagOn() && effectiveMode() === 'ask' && !roleBlocked()
  // The model catalogue, for the vision check only. It is the same request the picker and a send
  // already share; read at mount and only when images ride as parts of the turn.
  const [catalogueModels, setCatalogueModels] = createSignal<readonly ModelInfo[] | undefined>(undefined)
  if (visionAsPartsEnabled()) {
    void privacyTiers.catalogue().then(setCatalogueModels, () => undefined)
  }
  const selectedCatalogModel = () => {
    const picked = selectedModel()
    return catalogueModels()?.find((model) => model.id === picked.model && model.provider === picked.provider)
  }
  // What this turn can carry as configured right now; read at pick time and again at send, because
  // the model, the route and the retention verdict can change after a chip is added.
  const attachmentPolicyContext = (): AttachmentPolicyContext => ({
    subscriptionBacked: selectedModel().provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER,
    planMode: false,
    // A room has no Temporary chat; a zero-retention workspace or a refused post closes images.
    zdr: zdrRefused() || !isLocalRetentionAllowed(),
    currentImageCount: files().filter(isImageAttachment).length,
    currentTotalBytes: files().reduce((total, file) => total + file.size, 0),
    imageParts: visionAsPartsEnabled()
      ? {
          modelReadsImages: modelSupportsVision(selectedCatalogModel(), getAttachmentPolicy(), {
            verevonMode: !selectedCatalogModel(),
          }),
          visionModelName: catalogueModels()?.find((model) => modelSupportsVision(model, undefined))?.name,
          maxImages: getAttachmentPolicy()?.maxImages ?? ATTACHMENT_MAX_IMAGES,
        }
      : undefined,
  })
  function addFiles(incoming: readonly File[]): void {
    if (!attachAvailable() || incoming.length === 0) return
    const verdict = vetRoomAttachments(incoming, attachmentPolicyContext(), files().length, i18n.tr)
    setAttachNotice(verdict.messages.length > 0 ? verdict.messages.join(' ') : undefined)
    if (verdict.accepted.length === 0) return
    setFiles((current) => [...current, ...verdict.accepted.map((file, index) => toRoomAttachment(file, index))])
  }
  function removeFile(id: string): void {
    setFiles((current) => {
      const gone = current.find((file) => file.id === id)
      if (gone) revokeRoomAttachment(gone)
      return current.filter((file) => file.id !== id)
    })
    setAttachNotice(undefined)
  }
  function clearFiles(): void {
    for (const file of files()) revokeRoomAttachment(file)
    setFiles([])
    setKeepInRoom(false)
  }
  onCleanup(clearFiles)
  function handlePaste(event: ClipboardEvent): void {
    if (!attachAvailable()) return
    const images = pastedImageFiles(event.clipboardData)
    if (images.length > 0) {
      event.preventDefault()
      addFiles(images)
      return
    }
    const pasted = event.clipboardData?.getData('text/plain') ?? ''
    if (pasteBecomesAttachment(pasted)) {
      event.preventDefault()
      addFiles([pastedTextFile(pasted, i18n.tr('Limt inn tekst', 'Pasted text'))])
    }
  }

  // Threads the server currently reports as working in this room, minus the
  // one this browser is streaming itself (that one has its own indicator in
  // the exchange block). What remains is other members' agent work.
  const othersWorking = () => {
    const own = ownStream()
    return liveThreadsIn(props.spaceRef).filter(
      (thread) => !(own && own.spaceRef === props.spaceRef && own.threadId === thread.thread_id),
    )
  }
  // Owner steer (WS09-S2). `steerAllowed` is the rollout answer from the thread listing; the queue
  // route still decides every message. Steer is only ever into THIS browser's own turn: the person
  // who started it owns it, and the registry on the other side binds it to them.
  const steerAllowed = () => props.agentWork?.()?.steer_owner === true
  const steerOpen = () =>
    steerAllowed() && submitting() && !preparing() && exchange()?.status === 'streaming' && effectiveMode() === 'ask'
  // The thread this ask would reply into already has a live run that is not this browser's: a
  // second reply would be refused (one run per thread), so the message waits here instead.
  const busyReplyThread = (): string | undefined => {
    if (!steerAllowed() || submitting() || effectiveMode() !== 'ask') return undefined
    const target = props.replyTarget?.()?.threadId
    if (!target) return undefined
    return othersWorking().some((thread) => thread.thread_id === target) ? target : undefined
  }
  const sendWhenReadyOffered = () => busyReplyThread() !== undefined && files().length === 0
  props.registerFocusHandle?.(() => textareaRef?.focus())
  // A message waiting for the agent to be free goes out once the room's projection no longer shows
  // its thread working. Only on an observed room: absence of data is not idleness.
  createEffect(() => {
    const waiting = midRun.deferred()
    if (!waiting || submitting() || !isSpaceObserved(props.spaceRef)) return
    if (liveThreadsIn(props.spaceRef).some((thread) => thread.thread_id === waiting.threadId)) return
    const ready = midRun.takeDeferred()
    if (!ready) return
    midRun.startTurn(false)
    void pipeline.send({
      content: ready.content,
      mode: 'ask',
      model: selectedModel(),
      mentionedAgentRef: ready.mentionedAgentRef,
      locale: i18n.locale(),
      skills: ready.skills ?? [],
      replyThreadId: ready.threadId,
    })
  })

  const candidates = createMemo<readonly MentionCandidate[]>(() => {
    const people: MentionCandidate[] = props.roster()
      .filter((member) => member.subject_type === 'user' && member.display_name.trim())
      .map((member) => ({ id: `user:${member.subject_id}`, label: member.display_name, kind: 'person' as const }))
    // Only agents actively bound right now are offered. A paused/failed one
    // would just be rejected server-side, and proposing to bind an unbound
    // one is a separate, governed flow (Phase UI-3b) this composer does not
    // attempt.
    const agents: MentionCandidate[] = props.agents()
      .filter((agent) => agent.identity_published && agent.status === 'active' && agent.name?.trim())
      .map((agent) => ({
        id: `agent:${agent.subject_id}`,
        label: agent.name as string,
        kind: 'agent' as const,
        subjectId: agent.subject_id,
        color: agentAvatarColor(agent),
      }))
    return [...agents, ...people]
  })

  const visibleCandidates = createMemo(() => {
    const query = mentionQuery()
    if (query === undefined) return []
    const normalized = query.trim().toLocaleLowerCase()
    const matches = candidates().filter((candidate) =>
      normalized === '' || candidate.label.toLocaleLowerCase().startsWith(normalized),
    )
    return matches.slice(0, 6)
  })

  // The structured picker's candidates (WS08-S6): the room's people and active agents, namesakes
  // qualified, without the reader themself.
  const roomCandidates = createMemo<readonly RoomMentionCandidate[]>(() => {
    const viewer = props.viewerSubjectId?.()?.trim()
    return qualifyCandidates([
      ...props.roster()
        .filter((member) => member.subject_type === 'user' && member.display_name.trim() && member.subject_id !== viewer)
        .map((member) => ({
          kind: 'person' as const,
          subjectId: member.subject_id,
          name: member.display_name.trim(),
          role: member.role,
        })),
      ...props.agents()
        .filter((agent) => agent.identity_published && agent.status === 'active' && agent.name?.trim())
        .map((agent) => ({
          kind: 'agent' as const,
          subjectId: agent.subject_id,
          name: (agent.name as string).trim(),
          role: 'agent',
          color: agentAvatarColor(agent),
        })),
    ])
  })
  const canBindAgents = () => props.viewerRole?.() === 'manager' || props.viewerRole?.() === 'owner'
  function ensureInstallableLoaded(): void {
    if (installable() !== undefined || installableLoading || !canBindAgents()) return
    installableLoading = true
    getInstallableSpaceAgents(props.spaceRef)
      .then((loaded) => setInstallable(loaded))
      .catch(() => setInstallable([]))
      .finally(() => {
        installableLoading = false
      })
  }
  // Most recent first: the last mentions this member sent, then the starters of the room's latest threads.
  const recentSubjects = () => [
    ...readRecentMentions(props.spaceRef),
    ...[...(props.threads?.() ?? [])]
      .sort((a, b) => (b.updated_at ?? '').localeCompare(a.updated_at ?? ''))
      .map((thread) => thread.owner_subject_id)
      .filter((id): id is string => typeof id === 'string' && id !== ''),
  ]
  const mentionRows = createMemo<readonly MentionPickerRow[]>(() => {
    const query = mentionQuery()
    if (query === undefined || !mentionPickerOn()) return []
    const ranked = orderBySection(rankMentionCandidates(roomCandidates(), query, recentSubjects()))
    const rows: MentionPickerRow[] = ranked.map((candidate) => ({ kind: 'candidate' as const, candidate }))
    const normalized = foldForMatch(query)
    if (canBindAgents()) {
      const inRoom = new Set(roomCandidates().filter((c) => c.kind === 'agent').map((c) => foldForMatch(c.name)))
      let added = 0
      for (const agent of (installable() ?? []).filter((item) => !item.already_bound && item.name?.trim())) {
        const name = (agent.name as string).trim()
        if (inRoom.has(foldForMatch(name)) || !foldForMatch(name).startsWith(normalized)) continue
        rows.push({ kind: 'add', agentRef: agent.agent_ref, name })
        added += 1
        if (added >= 3) break
      }
    } else if (ranked.length === 0 && query.trim().length >= 2) {
      rows.push({ kind: 'request', name: query.trim() })
    }
    return rows
  })
  const newMentionPickerOpen = () => mentionPickerOn() && mentionQuery() !== undefined && mentionRows().length > 0

  function ensureSkillsLoaded(): void {
    if (skills() !== undefined || skillsLoading) return
    skillsLoading = true
    listAvailableSkills()
      .then((loaded) => setSkills(loaded))
      .catch(() => {
        setSkills([])
        setSkillsFailed(true)
      })
      .finally(() => {
        skillsLoading = false
      })
  }

  const visibleSkills = createMemo<readonly Skill[]>(() => {
    const query = skillQuery()
    const catalogue = skills()
    if (query === undefined || !catalogue) return []
    const normalized = query.trim().toLocaleLowerCase()
    const picked = new Set(pickedSkills().map((skill) => skill.id))
    const matches = catalogue.filter(
      (skill) =>
        !picked.has(skill.id) &&
        (normalized === '' || skill.name.toLocaleLowerCase().includes(normalized)),
    )
    // Prefix matches first, so typing the start of a name lands on it.
    matches.sort((a, b) => {
      const aStarts = a.name.toLocaleLowerCase().startsWith(normalized) ? 0 : 1
      const bStarts = b.name.toLocaleLowerCase().startsWith(normalized) ? 0 : 1
      return aStarts - bStarts || a.name.localeCompare(b.name)
    })
    return matches.slice(0, 6)
  })

  const pickerItems = createMemo<readonly PickerItem[]>(() => {
    if (mentionQuery() !== undefined) {
      if (mentionPickerOn()) return mentionRows().map((row) => ({ kind: 'mentionRow' as const, row }))
      return visibleCandidates().map((candidate) => ({ kind: 'mention' as const, candidate }))
    }
    if (skillQuery() !== undefined) {
      // Platform commands list above skills; flag off, there are none and this is today's list.
      const commands = matchPlatformCommands(skillQuery() ?? '', slashContext()).map((command) => ({
        kind: 'command' as const,
        command,
      }))
      return [...commands, ...visibleSkills().map((skill) => ({ kind: 'skill' as const, skill }))]
    }
    return []
  })
  // The `/` box also opens on zero matches, to say why (loading, failed, none).
  const pickerOpen = () => pickerItems().length > 0 || skillQuery() !== undefined
  // The legacy list draws only when the structured picker does not.
  const legacyPickerOpen = () => pickerOpen() && !(mentionPickerOn() && mentionQuery() !== undefined)
  const skillEmptyText = () => {
    if (skills() === undefined) return i18n.tr('Henter ferdigheter …', 'Loading skills…')
    if (skillsFailed()) {
      return i18n.tr('Ferdighetene kunne ikke hentes akkurat nå.', 'Skills could not be loaded right now.')
    }
    if ((skills()?.length ?? 0) === 0) {
      return i18n.tr('Ingen ferdigheter er tilgjengelige for deg.', 'No skills are available to you.')
    }
    return i18n.tr('Ingen ferdigheter matcher.', 'No skills match.')
  }

  function detectTriggers(value: string, cursor: number): void {
    const before = value.slice(0, cursor)
    const mention = before.match(/(?:^|\s)@([^\s@]*)$/)
    const skill = before.match(/(?:^|\s)\/([^\s/]*)$/)
    const nextMention = mention ? mention[1] : undefined
    const nextSkill = mention ? undefined : skill ? skill[1] : undefined
    if (nextMention !== mentionQuery() || nextSkill !== skillQuery()) setActiveIndex(0)
    setMentionQuery(nextMention)
    setSkillQuery(nextSkill)
    if (nextSkill !== undefined) ensureSkillsLoaded()
    if (nextMention !== undefined && mentionPickerOn()) ensureInstallableLoaded()
  }

  function closePicker(): void {
    setMentionQuery(undefined)
    setSkillQuery(undefined)
  }

  function handleInput(event: InputEvent & { currentTarget: HTMLTextAreaElement }): void {
    const value = event.currentTarget.value
    // Only a message being composed counts as writing. Clearing the box, or
    // opening a picker with `@` or `/`, is not something to announce.
    if (value.trim()) props.onTyping?.()
    setText(value)
    setCommandStatus(undefined)
    setMentionNotice(undefined)
    // A pick leaves the list when its `@name` leaves the words (locked agents stay).
    if (mentionPickerOn()) mention.reconcile(value)
    const agentLabel = candidates().find((c) => c.subjectId === mentionedAgentRef())?.label
    if (agentLabel && !value.includes(`@${agentLabel}`)) setMentionedAgentRef(undefined)
    detectTriggers(value, event.currentTarget.selectionStart ?? value.length)
  }

  /** Replace the trailing trigger (`@query` or `/query`) before the caret. */
  function replaceTrigger(pattern: RegExp, replacement: string): void {
    const value = text()
    const cursor = textareaRef?.selectionStart ?? value.length
    const before = value.slice(0, cursor)
    const after = value.slice(cursor)
    const replaced = before.replace(pattern, (matched) => (matched.startsWith(' ') ? ' ' : '') + replacement)
    setText(replaced + after)
  }

  function selectCandidate(candidate: MentionCandidate): void {
    replaceTrigger(/(?:^|\s)@([^\s@]*)$/, `@${candidate.label} `)
    closePicker()
    if (candidate.kind === 'agent') setMentionedAgentRef(candidate.subjectId)
    textareaRef?.focus()
  }

  // A pick from the structured picker (WS08-S6). A person is recorded by id; an agent joins the
  // address bar when addressed wake is on, else it is the legacy single mention.
  function selectRoomCandidate(candidate: RoomMentionCandidate): void {
    replaceTrigger(/(?:^|\s)@([^\s@]*)$/, `@${candidate.label} `)
    closePicker()
    if (candidate.kind === 'agent' && !addressedWakeOn()) {
      setMentionedAgentRef(candidate.subjectId)
    } else if (!mention.pick(candidate)) {
      setMentionNotice(translateApiError({ code: 'invalid_mentions' }, i18n.tr))
    }
    textareaRef?.focus()
  }

  // An agent that is not in this room: leave the name in the words and ask what to do about it.
  function selectAddRow(row: Extract<MentionPickerRow, { kind: 'add' | 'request' }>): void {
    replaceTrigger(/(?:^|\s)@([^\s@]*)$/, `@${row.name} `)
    closePicker()
    setAddTarget(row.kind === 'add' ? { kind: 'add', agentRef: row.agentRef, name: row.name } : { kind: 'request', name: row.name })
  }

  // A picked skill is a chip, not text: the `/query` leaves the message, and the
  // skill rides to the server as an id the composer never has to re-parse out
  // of prose. Mirrors model-gateway's cap so the client never promises more
  // than the server will honour.
  function selectSkill(skill: Skill): void {
    replaceTrigger(/(?:^|\s)\/([^\s/]*)$/, '')
    closePicker()
    setPickedSkills((current) =>
      current.some((picked) => picked.id === skill.id) || current.length >= MAX_PICKED_SKILLS
        ? current
        : [...current, skill],
    )
    textareaRef?.focus()
  }

  function removeSkill(id: string): void {
    setPickedSkills((current) => current.filter((skill) => skill.id !== id))
  }

  // What a platform command may touch. Presentation and local navigation only; the owning plane
  // still decides anything a command asks of it (`/read` runs the shared mark-read action).
  function slashContext(): SlashCommandContext {
    return {
      spaceRef: props.spaceRef,
      openTab: openRoomTabByHash,
      openSearch: currentSearchOpener(),
      openShortcutSheet: openShortcutSheetViaHotkeyLayer,
      startNewThread: () => {
        if (!props.replyTargetFixed) props.onClearReplyTarget?.()
        openRoomTabByHash('chat')
        textareaRef?.focus()
      },
    }
  }

  // Runs a command locally: the text it came from leaves the box, nothing is sent, the model is
  // never called, and the outcome is one transient status line.
  async function runPlatformCommand(command: PlatformCommand, arg: string): Promise<void> {
    setText('')
    discardDraft()
    closePicker()
    setCommandStatus(undefined)
    let status: Awaited<ReturnType<PlatformCommand['run']>>
    try {
      status = await command.run(slashContext(), arg)
    } catch {
      status = {
        no: 'Kommandoen kunne ikke kjøres akkurat nå.',
        en: 'The command could not run right now.',
      }
    }
    setCommandStatus(status ? i18n.tr(status.no, status.en) : undefined)
    textareaRef?.focus()
  }

  function pickItem(item: PickerItem): void {
    if (item.kind === 'mentionRow') {
      if (item.row.kind === 'candidate') selectRoomCandidate(item.row.candidate)
      else selectAddRow(item.row)
    } else if (item.kind === 'mention') selectCandidate(item.candidate)
    else if (item.kind === 'command') {
      // A command picked from the list runs now; `/search` opens search with an empty query.
      void runPlatformCommand(item.command, '')
    } else selectSkill(item.skill)
  }

  // Apply one markdown format to the textarea's selection and put the caret where the transform says.
  function formatSelection(kind: MarkdownFormatKind): void {
    const element = textareaRef
    if (!element || !markdownOn() || previewing()) return
    const edit = applyFormat(element.value, element.selectionStart ?? element.value.length, element.selectionEnd ?? element.value.length, kind)
    element.value = edit.value
    setText(edit.value)
    element.focus()
    element.setSelectionRange(edit.selStart, edit.selEnd)
  }

  function handleKeyDown(event: KeyboardEvent & { currentTarget: HTMLTextAreaElement }): void {
    if (markdownOn() && (event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey) {
      const kind = markdownShortcut(event.key)
      if (kind) {
        event.preventDefault()
        formatSelection(kind)
        return
      }
    }
    const items = pickerItems()
    if (items.length > 0) {
      if (event.key === 'ArrowDown') {
        event.preventDefault()
        setActiveIndex((current) => Math.min(current + 1, items.length - 1))
        return
      }
      if (event.key === 'ArrowUp') {
        event.preventDefault()
        setActiveIndex((current) => Math.max(current - 1, 0))
        return
      }
      if (event.key === 'Enter' || event.key === 'Tab') {
        event.preventDefault()
        const item = items[activeIndex()]
        if (item) pickItem(item)
        return
      }
    }
    if (event.key === 'Escape' && (mentionQuery() !== undefined || skillQuery() !== undefined)) {
      event.preventDefault()
      closePicker()
      return
    }
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault()
      void submitMessage()
    }
  }

  // A thread paused on a human decision does not take new input. Sending into
  // it would queue a message behind a gate the same person is standing at, and
  // the reply would arrive after an answer they have not given yet.
  // A human post starts no run, so it is not queued behind the gate; only a turn for the agent is.
  const blockedByApproval = () => props.replyTarget?.()?.awaitingApproval === true && effectiveMode() !== 'post'
  const subscriptionIncompatible = () => selectedModel().provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER
    && (effectiveMode() === 'ask' || !addressedWakeOn())
    && (Boolean(askAgentRef()) || pickedSkills().length > 0)

  // `override` is the text to send when the box was just rewritten and the signal has not flushed
  // yet (the request-to-add path appends a line naming the managers).
  async function submitMessage(override?: string): Promise<void> {
    const content = (override ?? text()).trim()
    if (!content || nothingOffered()) return
    // A message that is exactly a platform command runs locally instead of being sent. Anything
    // else that starts with `/` is an ordinary message, as before.
    const slash = parseSlashCommandText(content, slashContext())
    if (slash) {
      await runPlatformCommand(slash.command, slash.arg)
      return
    }
    if (effectiveMode() === 'post') {
      if (submitting() || postBusy() || overLimit()) return
      if (files().length > 0) {
        setAttachNotice(i18n.tr(
          'Vedlegg følger bare med når du spør Verevon. Fjern dem eller bytt til Spør Verevon.',
          'Attachments only go with Ask Verevon. Remove them or switch to Ask Verevon.',
        ))
        return
      }
      closePicker()
      await postSender.send({
        content,
        replyThreadId: props.replyTarget?.()?.threadId,
        // Ids of the people and agents picked in the box; only the words still in the text count.
        mentions: mentionPickerOn() ? mention.payload(content) : undefined,
      })
      return
    }
    // The member's own turn is still working and the room allows steering it: hand the message to
    // the running agent instead of starting another turn.
    if (steerOpen()) {
      await steerMessage(content)
      return
    }
    if (submitting() || preparing() || postBusy() || blockedByApproval() || subscriptionIncompatible()) return
    // Another run is live in the thread being replied to: keep the message here until it is free.
    const busyThread = busyReplyThread()
    if (busyThread && files().length === 0) {
      midRun.defer({
        content,
        threadId: busyThread,
        mentionedAgentRef: askAgentRef(),
        skills: pickedSkills(),
      })
      setText('')
      discardDraft()
      setMentionedAgentRef(undefined)
      mention.clear()
      setPickedSkills([])
      closePicker()
      return
    }

    // Read every chip before anything is cleared or sent. A file the turn can no longer carry
    // (the route, the model or the retention verdict changed since it was added) or one that cannot
    // be read whole stops the send with its reason; the words and the chips stay.
    let sendContent = content
    let sendAttachments: Awaited<ReturnType<typeof prepareRoomTurn>>['attachments'] = []
    if (files().length > 0) {
      const verdict = vetRoomAttachments(files(), { ...attachmentPolicyContext(), currentImageCount: 0, currentTotalBytes: 0 }, 0, i18n.tr)
      if (verdict.messages.length > 0) {
        setAttachNotice(`${verdict.messages.join(' ')} ${i18n.tr('Fjern filen og send på nytt. Utkastet er beholdt.', 'Remove the file and send again. Your draft has been kept.')}`)
        return
      }
      setPreparing(true)
      try {
        const prepared = await prepareRoomTurn(content, files())
        sendContent = prepared.content
        sendAttachments = prepared.attachments
        setAttachNotice(undefined)
        if (keepInRoom() && keepOffered()) {
          // Read the chips back before they are cleared; the upload then runs beside the send.
          const keep = await roomFilesFromChips(files(), roomFilesLimits(undefined))
          if (keep.length > 0) {
            void uploadSpaceFiles(props.spaceRef, keep).then(
              () => undefined,
              (error: unknown) => setAttachNotice(translateApiError(error, i18n.tr, {
                no: 'Filene ble sendt i meldingen, men kunne ikke lagres i rommet.',
                en: 'The files were sent in the message but could not be kept in the room.',
              })),
            )
          }
        }
      } catch (error) {
        setAttachNotice(error instanceof Error ? error.message : i18n.tr('Kunne ikke lese vedleggene.', 'Could not read the attachments.'))
        return
      } finally {
        setPreparing(false)
      }
    }

    closePicker()
    // A post that failed earlier and is now being asked instead is not retried behind the new send.
    postSender.discard()
    const input = {
      content: sendContent,
      attachments: sendAttachments,
      mode: 'ask' as const,
      model: selectedModel(),
      mentionedAgentRef: askAgentRef(),
      locale: i18n.locale(),
      skills: pickedSkills(),
      replyThreadId: props.replyTarget?.()?.threadId,
    }
    // The pipeline raises `submitting` and the exchange block synchronously,
    // before its first await; the box is cleared right after, as before.
    if (draftsOn()) {
      acceptSend = () => {
        if (text().trim() !== content) return
        setText('')
        discardDraft()
        setMentionedAgentRef(undefined)
        mention.clear()
        setPickedSkills([])
        clearFiles()
      }
    }
    midRun.startTurn(false)
    const sent = pipeline.send(input)
    if (!draftsOn()) {
      setText('')
      setMentionedAgentRef(undefined)
      mention.clear()
      setPickedSkills([])
      clearFiles()
    }
    await sent
  }

  // Deliver a message to the running turn. The words leave the box once the gateway accepted them
  // (or will send them as a new turn); a refusal keeps them, and the strip says why.
  async function steerMessage(content: string): Promise<void> {
    if (files().length > 0) {
      setAttachNotice(i18n.tr(
        'Vedlegg kan ikke sendes mens agenten jobber. Fjern dem, eller vent til svaret er ferdig.',
        'Attachments cannot be sent while the agent works. Remove them, or wait until the reply is done.',
      ))
      return
    }
    closePicker()
    const outcome = await midRun.steer(content, {
      requestId: pipeline.liveRequestId(),
      threadId: pipeline.liveThreadId(),
      spaceRef: props.spaceRef,
    })
    if (outcome === 'refused') return
    if (text().trim() === content) {
      setText('')
      discardDraft()
    }
    // The answer came after the run had already ended: nobody is left to flush it at settle time.
    if (outcome === 'resend' && !pipeline.submitting()) flushResends(midRun.takeLateResends())
  }

  // The thread pane's composer is aimed at one thread for as long as the pane is open; the pane title
  // already names that thread, so the box talks about "the conversation", not "the room".
  const inThread = () => props.replyTargetFixed === true && props.replyTarget?.() !== undefined

  // A normal post that mentions the room's managers; nothing is bound or granted by asking.
  function requestToAdd(name: string): void {
    const managers: RoomMentionCandidate[] = roomCandidates().filter(
      (c) => c.kind === 'person' && (c.role === 'manager' || c.role === 'owner'),
    ).slice(0, 5)
    if (managers.length > 0) {
      for (const manager of managers) mention.pick(manager)
      const request = i18n.tr(`kan dere legge til ${name} i rommet?`, `could you add ${name} to this room?`)
      const next = `${text().trim()}

${managers.map((m) => `@${m.label}`).join(' ')} ${request}`
      setText(next)
      void submitMessage(next)
    } else {
      setMentionNotice(i18n.tr('Fant ingen ledere å spørre i dette rommet.', 'No managers to ask were found in this room.'))
    }
  }

  const slotContext: SpaceComposerSlotContext = {
    get spaceRef() { return props.spaceRef },
    orgId,
    mode: effectiveMode,
    setMode: setPickedMode,
    modes: offeredModes,
    inThread,
    disabled: () => submitting() || preparing() || blockedByApproval() || postBusy(),
    text,
    setText,
    focus: () => textareaRef?.focus(),
    model: { catalogue: privacyTiers.catalogue, onChange: setSelectedModel },
    skills: { picked: pickedSkills, remove: removeSkill },
  }

  return (
    <div class="verevon-space-room-composer">
      <Show when={exchange()}>
        {(current) => (
          <div class="verevon-space-room-exchange" role="log" aria-live="polite">
            <p class="verevon-space-room-exchange__user">
              <span class="verevon-space-room-exchange__speaker">{i18n.tr('Du', 'You')}</span>
              {splitRoomMessage(current().userContent).text}
            </p>
            <Show when={splitRoomMessage(current().userContent).files.length > 0}>
              <ul class="verevon-space-attachment-chips" aria-label={i18n.tr('Vedlegg', 'Attachments')}>
                <For each={splitRoomMessage(current().userContent).files}>
                  {(file) => <li class="verevon-space-attachment-chip">{file.name}</li>}
                </For>
              </ul>
            </Show>
            <div
              class={[
                'verevon-space-room-exchange__assistant',
                { 'verevon-chat-streaming': current().status === 'streaming' },
              ]}
            >
              <Show when={current().assistantContent}>
                {(content) => <ChatMarkdown content={content()} />}
              </Show>
            </div>
            {/* The turn answered without the organisation's knowledge because Control declined
                retrieval for this Space. The reason is a closed code; the words are the room's. */}
            <Show when={current().retrievalDeclined}>
              {(reason) => (
                <p class="verevon-space-room-exchange__stopped" role="status">
                  {i18n.tr(
                    spaceRetrievalDeclineCopy(reason()).no,
                    spaceRetrievalDeclineCopy(reason()).en,
                  )}
                </p>
              )}
            </Show>
            {/* The spend hard stop is a refusal BEFORE the run starts — Model
                Plane's pre-flight budget guard — so it is not "the reply
                stopped": nothing was replied. It says what stopped it and where
                the ceiling lives, so a member knows whom to ask. */}
            <Show
              when={current().status === 'error' && current().errorCode === 'budget_exceeded'}
              fallback={
                <Show when={current().status === 'error'}>
                  <p class="verevon-space-projection-error" role="alert">
                    <Show when={current().errorCode !== 'budget_unavailable'}>
                      {i18n.tr(
                        'Svaret stoppet før det var ferdig. ',
                        'The reply stopped before it finished. ',
                      )}
                    </Show>
                    {current().errorMessage}
                    {/* A dropped connection leaves the outcome unknown, so the send
                        can be repeated under its own key: the room gets it once. */}
                    <Show when={current().retryable && !submitting()}>
                      {' '}
                      <button type="button" onClick={() => { void pipeline.retry() }}>
                        {i18n.tr('Prøv igjen', 'Try again')}
                      </button>
                    </Show>
                  </p>
                </Show>
              }
            >
              <p class="verevon-space-projection-error verevon-space-room-exchange__budget" role="alert">
                {current().errorMessage}{' '}
                <a href="/settings/quotas" link>
                  {i18n.tr('Åpne Forbrukstak', 'Open Spend ceiling')}
                </a>
              </p>
            </Show>
            {/* A stop is neither an error nor a finished answer. Whether the
                stop was RECORDED is the load-bearing distinction: an aborted
                read with no server cancel leaves the run generating for
                everyone else, and the room must not be told otherwise. */}
            <Show when={current().status === 'stopped'}>
              <p class="verevon-space-room-exchange__stopped" role="status">
                {current().stopRecorded && current().stoppedBy === 'other'
                  ? i18n.tr(
                      'Kjøringen ble stoppet av en rombehandler. Rommet ser dette som stoppet.',
                      'This run was stopped by a room manager. The room records it as stopped.',
                    )
                  : current().stopRecorded
                  ? i18n.tr('Stoppet av deg. Rommet ser dette som stoppet.', 'Stopped by you. The room records it as stopped.')
                  : pipeline.liveRequestId()
                    ? i18n.tr('Stoppet her. Registrerer stoppet …', 'Stopped here. Recording the stop…')
                    : i18n.tr(
                        'Avbrutt før svaret startet. Kjøringen kan fortsatt fullføre på tjenersiden.',
                        'Cut off before the reply started. The run may still finish server-side.',
                      )}
              </p>
            </Show>
            {/* The server already had this send (a retry of one it took): accepted,
                not failed. The run shows in the room, not in this block. */}
            <Show when={current().acceptedEarlier}>
              <p class="verevon-space-room-exchange__stopped" role="status">
                {i18n.tr(
                  'Meldingen var allerede mottatt. Svaret vises i rommet.',
                  'This message was already received. The reply shows in the room.',
                )}
              </p>
            </Show>
            <Show when={current().status === 'streaming'}>
              <button
                type="button"
                class="verevon-space-room-exchange__stop"
                onClick={() => { void pipeline.stop() }}
              >
                {i18n.tr('Stopp', 'Stop')}
              </button>
            </Show>
          </div>
        )}
      </Show>

      <Show when={midRun.entries().length > 0}>
        <QueuedInputStrip entries={midRun.entries() as QueuedInput[]} />
      </Show>
      <Show when={steerOpen()}>
        <p class="verevon-space-room-composer__hint" role="status">
          {i18n.tr(
            'Agenten jobber. Det du skriver nå leveres til den ved neste verktøyrunde.',
            'The agent is working. What you write now reaches it at its next tool round.',
          )}
        </p>
      </Show>
      <Show when={midRun.deferred()}>
        {(waiting) => (
          <p class="verevon-space-room-composer__hint" role="status" aria-live="polite">
            {i18n.tr('Venter på at agenten blir ferdig: «', 'Waiting for the agent to finish: «')}
            {waiting().content.length > 80 ? `${waiting().content.slice(0, 80)}…` : waiting().content}
            {i18n.tr(
              '». Meldingen holdes her i nettleseren din, ikke på serveren, og går tapt hvis du lukker fanen. ',
              '». The message is kept in this browser, not on the server, and is lost if you close the tab. ',
            )}
            <button type="button" onClick={() => midRun.cancelDeferred()}>
              {i18n.tr('Avbryt', 'Cancel')}
            </button>
          </p>
        )}
      </Show>
      <Show when={sendWhenReadyOffered()}>
        <p class="verevon-space-room-composer__hint" role="status">
          {i18n.tr(
            'Agenten jobber allerede i denne samtalen. Meldingen din holdes her til agenten er ferdig.',
            'The agent is already working in this conversation. Your message is kept here until it is done.',
          )}
        </p>
      </Show>

      {/* Other members' agent work, from the server's projection. Shown above
          the textarea so a member about to type knows the room is busy —
          Slack's "typing" affordance, for agents. Never derived from this
          browser's own stream, which has its own block above. */}
      {/* People, above the agent line: in a room the person writing beside you
          is the more immediate fact, and the two are never merged — one is a
          colleague, the other is Verevon. */}
      <Show when={props.typingSentence?.()}>
        {(sentence) => (
          <p class="verevon-space-room-composer__typing" role="status" aria-live="polite">
            <span class="verevon-space-room-composer__typing-dots" aria-hidden="true">
              <span />
              <span />
              <span />
            </span>
            {sentence()}
          </p>
        )}
      </Show>
      <Show when={props.agentWork?.()?.live === true && othersWorking().length > 0}>
        <SpaceRunStrip spaceRef={props.spaceRef} threads={othersWorking} onOpenRun={props.onOpenRun} />
      </Show>
      <Show when={props.agentWork?.()?.live !== true && othersWorking().length > 0}>
        <p class="verevon-space-room-composer__working" role="status" aria-live="polite">
          <span class="verevon-space-room-composer__working-dot" aria-hidden="true" />
          {othersWorking().length === 1
            ? i18n.tr('Verevon jobber i ', 'Verevon is working in ')
              + `«${threadTitle(othersWorking()[0] as SpaceThread, i18n.tr)}»`
            : i18n.tr(
                `Verevon jobber i ${othersWorking().length} samtaler i rommet.`,
                `Verevon is working in ${othersWorking().length} conversations in this room.`,
              )}
        </p>
      </Show>

      <SpaceConnectivityBanner />

      <form
        onSubmit={(event) => { event.preventDefault(); void submitMessage() }}
        onDragOver={(event) => {
          if (!attachAvailable() || !event.dataTransfer?.types.includes('Files')) return
          event.preventDefault()
          setDragActive(true)
        }}
        onDragLeave={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDragActive(false)
        }}
        onDrop={(event) => {
          const dropped = Array.from(event.dataTransfer?.files ?? [])
          setDragActive(false)
          if (!attachAvailable() || dropped.length === 0) return
          event.preventDefault()
          addFiles(dropped)
        }}
        data-drop-active={dragActive() ? 'true' : undefined}
      >
        <Show when={inThread() ? undefined : props.replyTarget?.()}>
          {(target) => (
            <p class="verevon-space-room-composer__reply">
              {i18n.tr('Svarer i', 'Replying in')} «{target().title}»
              <Show when={!props.replyTargetFixed}>
                <button
                  type="button"
                  onClick={() => props.onClearReplyTarget?.()}
                  aria-label={i18n.tr('Avbryt svar', 'Cancel reply')}
                >
                  ✕
                </button>
              </Show>
            </p>
          )}
        </Show>
        <Show when={blockedByApproval()}>
          <p class="verevon-space-room-composer__blocked" role="status">
            {i18n.tr(
              'Godkjenn eller avslå for å fortsette denne samtalen.',
              'Approve or deny to continue this conversation.',
            )}
          </p>
        </Show>
        <Show when={roleBlocked()}>
          <p class="verevon-space-room-composer__blocked" role="status">
            {i18n.tr(
              'Du har lesetilgang i dette rommet og kan ikke skrive. Be en eier eller leder om redigeringstilgang.',
              'You have read access in this room and cannot post. Ask an owner or manager for edit access.',
            )}
          </p>
        </Show>
        <Show when={!roleBlocked() && nothingOffered()}>
          <p class="verevon-space-room-composer__blocked" role="status">
            {i18n.tr(
              'Du kan ikke starte agenten i en annens samtale. Fjern agenten eller ferdigheten for å skrive en melding.',
              'You cannot start the agent in someone else\'s conversation. Remove the agent or skill to write a message.',
            )}
          </p>
        </Show>
        <Show when={humanPostsActive() && zdrRefused()}>
          <p class="verevon-space-room-composer__blocked" role="status">
            {translateApiError({ code: 'zdr_durable_mutation_forbidden' }, i18n.tr)}
          </p>
        </Show>
        <SpaceComposerToolbarSlot ctx={slotContext} />
        <SpaceComposerChipSlot ctx={slotContext} />
        <Show when={addressedWakeOn()}>
          <AddressedAgentsBar
            agents={mention.agents}
            onToggleLock={mention.toggleLock}
            onRemove={mention.removeAgent}
            disabled={() => postBusy()}
          />
        </Show>
        <Show when={newMentionPickerOpen()}>
          <MentionPicker
            id={pickerId}
            rows={mentionRows}
            activeIndex={activeIndex}
            onHover={setActiveIndex}
            onPick={(row) => pickItem({ kind: 'mentionRow', row })}
          />
        </Show>
        <Show when={legacyPickerOpen()}>
          <ul
            id={pickerId}
            class="verevon-space-room-mentions"
            role="listbox"
            aria-label={
              mentionQuery() !== undefined
                ? i18n.tr('Nevn noen', 'Mention someone')
                : i18n.tr('Velg en ferdighet', 'Pick a skill')
            }
          >
            <For each={pickerItems()}>
              {(item, index) => (
                <li>
                  <button
                    type="button"
                    role="option"
                    id={`${pickerId}-${index()}`}
                    aria-selected={index() === activeIndex() ? 'true' : 'false'}
                    onMouseDown={(event) => event.preventDefault()}
                    onMouseEnter={() => setActiveIndex(index())}
                    onClick={() => pickItem(item)}
                  >
                    <Show
                      when={item.kind === 'command' ? item.command : undefined}
                      fallback={
                    <Show
                      when={item.kind === 'skill' ? item.skill : undefined}
                      fallback={
                        <>
                          <Show
                            when={item.kind === 'mention' && item.candidate.kind === 'agent' ? item.candidate : undefined}
                            fallback={<span class="verevon-space-room-mentions__kind" aria-hidden="true">·</span>}
                          >
                            {(agent) => <AgentAvatar kind="agent" size={18} name={agent().label} color={agent().color} />}
                          </Show>
                          {item.kind === 'mention' ? item.candidate.label : ''}
                        </>
                      }
                    >
                      {(skill) => (
                        <>
                          <WandSparkles class="verevon-space-room-mentions__icon" aria-hidden="true" />
                          <span class="verevon-space-room-mentions__label">{skill().name}</span>
                          <SkillScope scope={skill().scope} />
                        </>
                      )}
                    </Show>
                      }
                    >
                      {(command) => (
                        <>
                          <span class="verevon-space-room-mentions__label">/{command().names[0]}</span>
                          <span class="verevon-space-room-skill-scope" data-kind="command">
                            {i18n.tr('Kommando', 'Command')}
                          </span>
                          <span class="verevon-space-room-mentions__hint">
                            {i18n.tr(command().labelNo, command().labelEn)}
                          </span>
                        </>
                      )}
                    </Show>
                  </button>
                </li>
              )}
            </For>
            <Show when={skillQuery() !== undefined && pickerItems().length === 0}>
              <li class="verevon-space-room-mentions__empty" role="presentation">
                {skillEmptyText()}
              </li>
            </Show>
          </ul>
        </Show>
        <Show when={commandStatus()}>
          {(line) => (
            <p class="verevon-space-room-composer__hint" role="status" aria-live="polite">
              {line()}
            </p>
          )}
        </Show>
        <Show when={attachAvailable()}>
          <div class="verevon-space-attachments">
            <input
              ref={fileInputRef}
              type="file"
              accept={ATTACHMENT_ACCEPT}
              multiple
              class="sr-only"
              aria-label={i18n.tr('Legg til filer', 'Add files')}
              onChange={(event) => {
                if (event.currentTarget.files) addFiles(Array.from(event.currentTarget.files))
                event.currentTarget.value = ''
              }}
            />
            <button
              type="button"
              class="verevon-space-attachments__add"
              disabled={submitting() || preparing() || postBusy() || blockedByApproval()}
              aria-label={i18n.tr('Legg ved fil', 'Attach a file')}
              title={i18n.tr('Legg ved fil', 'Attach a file')}
              onClick={() => fileInputRef?.click()}
            >
              <Paperclip aria-hidden="true" />
            </button>
            <Show when={files().length > 0}>
              <ul class="verevon-space-attachment-chips" aria-label={i18n.tr('Vedlegg', 'Attachments')}>
                <For each={files()}>
                  {(file) => (
                    <li class="verevon-space-attachment-chip" data-kind={isImageAttachment(file) ? 'image' : 'document'}>
                      <span class="verevon-space-attachment-chip__name">{file.name}</span>
                      <span class="verevon-space-attachment-chip__size">{formatChipSize(file.size)}</span>
                      <button
                        type="button"
                        disabled={submitting() || preparing()}
                        aria-label={`${i18n.tr('Fjern', 'Remove')} ${file.name}`}
                        onClick={() => removeFile(file.id)}
                      >
                        ✕
                      </button>
                    </li>
                  )}
                </For>
              </ul>
              <Show when={keepOffered()}>
                <label class="verevon-space-attachments__keep">
                  <input
                    type="checkbox"
                    checked={keepInRoom()}
                    disabled={submitting() || preparing()}
                    onChange={(event) => setKeepInRoom(event.currentTarget.checked)}
                  />
                  <span>{i18n.tr('Behold tekstfiler i rommet (Filer)', 'Also keep text files in the room (Files)')}</span>
                </label>
              </Show>
            </Show>
          </div>
        </Show>
        <Show when={attachNotice()}>
          {(notice) => (
            <p class="verevon-space-room-composer__blocked" role="alert">{notice()}</p>
          )}
        </Show>
        <Show when={markdownOn() && !roleBlocked()}>
          <div class="verevon-space-room-composer__format" role="toolbar" aria-label={i18n.tr('Formatering', 'Formatting')}>
            <For
              each={[
                { kind: 'bold' as const, label: i18n.tr('Fet (Ctrl+B)', 'Bold (Ctrl+B)'), glyph: 'B' },
                { kind: 'italic' as const, label: i18n.tr('Kursiv (Ctrl+I)', 'Italic (Ctrl+I)'), glyph: 'I' },
                { kind: 'code' as const, label: i18n.tr('Kode', 'Code'), glyph: '</>' },
                { kind: 'link' as const, label: i18n.tr('Lenke (Ctrl+K)', 'Link (Ctrl+K)'), glyph: '🔗' },
                { kind: 'bullet' as const, label: i18n.tr('Punktliste', 'Bulleted list'), glyph: '•' },
                { kind: 'quote' as const, label: i18n.tr('Sitat', 'Quote'), glyph: '❝' },
              ]}
            >
              {(entry) => (
                <button
                  type="button"
                  class="verevon-space-room-composer__format-button"
                  data-format={entry.kind}
                  aria-label={entry.label}
                  title={entry.label}
                  disabled={previewing() || submitting() || preparing() || blockedByApproval() || postBusy()}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => formatSelection(entry.kind)}
                >
                  {entry.glyph}
                </button>
              )}
            </For>
            <span class="verevon-space-room-composer__format-spacer" />
            <button
              type="button"
              class="verevon-space-room-composer__format-toggle"
              aria-pressed={previewing() ? 'true' : 'false'}
              disabled={!previewing() && text().trim() === ''}
              onClick={() => setPreviewing((current) => !current)}
            >
              {previewing() ? i18n.tr('Skriv', 'Write') : i18n.tr('Forhåndsvis', 'Preview')}
            </button>
          </div>
        </Show>
        <Show when={markdownOn() && previewing()}>
          <div class="verevon-space-room-composer__preview" aria-label={i18n.tr('Forhåndsvisning', 'Preview')}>
            <ChatMarkdown content={text()} />
          </div>
        </Show>
        <textarea
          ref={textareaRef}
          hidden={markdownOn() && previewing()}
          value={text()}
          onInput={handleInput}
          onPaste={handlePaste}
          onKeyDown={handleKeyDown}
          aria-controls={pickerOpen() ? pickerId : undefined}
          aria-activedescendant={pickerItems().length > 0 ? `${pickerId}-${activeIndex()}` : undefined}
          placeholder={
            blockedByApproval()
              ? i18n.tr(
                  'Denne samtalen venter på en godkjenning.',
                  'This conversation is waiting on an approval.',
                )
              : humanPostsActive() && effectiveMode() === 'post'
                ? inThread()
                  ? i18n.tr(
                      'Svar i samtalen. Agenten svarer ikke. @ nevner noen, / velger en ferdighet.',
                      'Reply in the thread. The agent will not reply. @ mentions someone, / picks a skill.',
                    )
                  : i18n.tr(
                      'Skriv til rommet. Agenten svarer ikke. @ nevner noen, / velger en ferdighet.',
                      'Write to the room. The agent will not reply. @ mentions someone, / picks a skill.',
                    )
                : inThread()
                  ? i18n.tr(
                      'Svar i samtalen. @ nevner noen, / velger en ferdighet.',
                      'Reply in the thread. @ mentions someone, / picks a skill.',
                    )
                  : i18n.tr(
                      'Skriv i rommet. @ nevner noen, / velger en ferdighet.',
                      'Write in the room. @ mentions someone, / picks a skill.',
                    )
          }
          rows={2}
          disabled={(submitting() && !steerOpen()) || preparing() || blockedByApproval() || postBusy() || roleBlocked()}
        />
        {/* A human post that did not land (WS01-S6). The text is still in the box; Retry repeats
            the same send under its own key, so a post whose answer was lost lands once. */}
        <Show when={postFailure()}>
          {(entry) => (
            <p class="verevon-space-projection-error verevon-space-room-composer__post-failure" role="alert">
              {i18n.tr('Meldingen ble ikke sendt. ', 'The message was not sent. ')}
              {entry().failure?.message}
              <Show when={entry().failure?.retryable}>
                <button type="button" onClick={() => { void postSender.retry() }}>
                  {i18n.tr('Prøv igjen', 'Try again')}
                </button>
              </Show>
              <button type="button" onClick={() => postSender.discard()}>
                {i18n.tr('Forkast', 'Discard')}
              </button>
            </p>
          )}
        </Show>
        <Show when={effectiveMode() === 'post' && (markdownOn() || postLength(text().trim()) > postMax() * 0.8)}>
          <p
            class={overLimit() ? 'verevon-space-room-composer__blocked' : 'verevon-space-room-composer__hint'}
            role={overLimit() ? 'alert' : undefined}
          >
            {postLength(text().trim())} / {postMax()}
            {overLimit() ? i18n.tr(' tegn. Meldingen er for lang.', ' characters. The message is too long.') : ''}
          </p>
        </Show>
        <Show when={mentionedAgentRef()}>
          <p class="verevon-space-room-composer__hint">
            {i18n.tr('Vil invokere en agent når du sender.', 'Will invoke an agent when you send.')}
          </p>
        </Show>
        <Show when={addressedWakeOn() && effectiveMode() === 'post' && mention.agents().length > 0}>
          <p class="verevon-space-room-composer__hint">
            {i18n.tr(
              'Meldingen sendes til rommet først. Agentene svarer etterpå, én om gangen, i den rekkefølgen de er nevnt.',
              'The message is posted to the room first. The agents answer afterwards, one at a time, in the order they are named.',
            )}
          </p>
        </Show>
        <Show when={mentionNotice()}>
          {(notice) => <p class="verevon-space-room-composer__hint" role="status">{notice()}</p>}
        </Show>
        <Show when={pickedSkills().length > 0}>
          <p class="verevon-space-room-composer__hint">
            {i18n.tr(
              'Valgte ferdigheter legges til som veiledning når du sender.',
              'Chosen skills are added as guidance when you send.',
            )}
          </p>
        </Show>
        <Show when={subscriptionIncompatible()}>
          <p class="verevon-space-room-composer__blocked" role="status">
            {i18n.tr(
              'Abonnementsmodeller er tekstbaserte. Fjern agenten/ferdigheten, eller velg en Verevon-modell.',
              'Subscription models are text-only. Remove the agent/skill, or choose a Verevon model.',
            )}
          </p>
        </Show>
        <button
          type="submit"
          disabled={
            (submitting() && !steerOpen()) || preparing() || blockedByApproval() || subscriptionIncompatible() || !text().trim()
            || postBusy() || roleBlocked() || nothingOffered() || overLimit() || Boolean(midRun.deferred())
          }
        >
          {steerOpen()
            ? i18n.tr('Send til agenten', 'Send to the agent')
            : sendWhenReadyOffered()
              ? i18n.tr('Send når klar', 'Send when ready')
              : humanPostsActive() && effectiveMode() === 'ask' ? i18n.tr('Spør Verevon', 'Ask Verevon') : i18n.tr('Send', 'Send')}
        </button>
      </form>
      <AgentAddDialog
        spaceRef={props.spaceRef}
        target={addTarget}
        onClose={() => setAddTarget(undefined)}
        onBound={(bound) => {
          setAddTarget(undefined)
          setInstallable(undefined)
          if (addressedWakeOn()) {
            mention.pick({
              id: `agent:${bound.subjectId}`,
              kind: 'agent',
              subjectId: bound.subjectId,
              name: bound.name,
              label: bound.name,
              role: 'agent',
            })
          } else {
            setMentionedAgentRef(bound.subjectId)
          }
          void submitMessage()
        }}
        onSendWithout={() => { void submitMessage() }}
        onRequest={(name) => requestToAdd(name)}
      />
    </div>
  )
}

