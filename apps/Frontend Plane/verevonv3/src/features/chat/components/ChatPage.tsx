import {
  For,
  Match,
  Show,
  Switch,
  createMemo,
  createEffect,
  createSignal,
  onCleanup,
  untrack,
} from 'solid-js'
import { Portal } from '@solidjs/web'
import { createResource } from '@/shared/lib/create-resource-compat'
import { createElementHeight } from '@/shared/ui/verevon/createElementHeight'
import { VerevonTextButton } from '@/shared/ui/verevon/VerevonTextButton'
import {
  ArrowDown,
  CalendarClock,
  EyeOff,
  Square,
  X,
} from '@/shared/icons'
import { spaceDisplayName } from '@/features/spaces/lib/space-name'
import { isEffectfulChatTurn } from '@/shared/chat/effect-class'
import {
  DashboardComposer,
  type DashboardComposerAttachment,
  type DashboardComposerSubmitPayload,
} from '@/features/dashboard/home/DashboardComposer'
import { withChatWorkPeriod, type ChatWorkPeriod } from '@/shared/actions/chat-work-period'
import { getThreadContext } from '@/shared/api/chat-client'
import { importUpload } from '@/shared/api/knowledge-client'
import { getSession } from '@/shared/session/session-store'
import {
  DateDivider,
  EmptyChatState,
  MessageBlock,
  lifecycleNoticeSentence,
  rungLabel,
} from './ChatMessages'
import {
  ArtifactsPanel,
} from './ChatArtifactPanel'
import type { ArtifactSelectionRequest } from './ArtifactSelection'
import { ThreadWorkspacePanel } from './ThreadWorkspacePanel'
import { EffectivePolicyPanel } from './EffectivePolicyPanel'
import {
  ChatHeader,
  ChatTabs,
  SourcesPanel,
  ContextWindowPanel,
  StepsPanel,
  TracePanel,
} from './ChatPanels'
import {
  ChatLiveRunPanel,
} from './ChatLiveRunPanel'
import { ComposerTrays } from './ComposerTrays'
import { researchPassFacts } from './ArtifactProvenance'
import { ChatWideResearchPanel } from './ChatWideResearchPanel'
import { ChatWorkspaceCanvas } from './ChatWorkspaceCanvas'
import { ChatAttachmentCanvas } from './ChatAttachmentCanvas'
import { ScheduleDialog } from './ScheduleDialog'
import { ScheduledRunsPanel } from './ScheduledRunsPanel'
import { PersonalSetupCard } from './PersonalSetupCard'
import {
  partitionEvidenceSources,
  spokenSourceTally,
  shouldShowDateDivider,
} from './chat-media-markdown'
import { useChatController } from './use-chat-controller'
import { useChatShortcuts } from '@/features/chat/lib/use-chat-shortcuts'
import { dockedNodesFor, selectDockedTrays } from '../lib/composer-trays'
import { deriveStreamActivity, streamActivityLabel } from '@/features/chat/lib/stream-activity'
import { chatSurfaceClaimsFocus, isChatSurfaceAvailable, type ChatSurfaceAvailability } from '../lib/chat-surfaces'
import { isWorkStep } from './chat-normalizers'
import type { ChatTab } from './chat-types'
import { useI18n } from '@/shared/i18n'
import { speechPlayer } from '@/features/voice/speech-playback'
import { latestVoiceReply } from '@/features/voice/voice-reply'
import { pinnedMessagesFull } from '../lib/chat-pinned-messages'
import { selectChatThread } from '../lib/chat-thread-history'
import { audienceConfirmed, chatProjectsEnabled, readProjectAudience, readProjectContext, rememberAudienceConfirmed } from '../lib/chat-projects'
import { loadEffectiveAgentModeCap } from '@/features/customize/AgentModesSection'
import { audienceConfirmRequired, chatProjectRefusal, sandboxToolsRefusedNotice, type ChatProjectRefusal } from './use-chat-project'
import { planTranscriptWindow, resolveTurnHeights, virtualizeTranscript } from '../lib/transcript-window'
import { BrowserDockControls } from '@/features/browser/dock/BrowserDockControls'
import { InteractiveBrowserPane } from '@/features/browser/dock/InteractiveBrowserPane'
import { OpenBrowserButton } from '@/features/browser/dock/OpenBrowserButton'
import { useDockBrowserSession } from '@/features/browser/dock/dock-session'
import { createChatThreadByTab, planChatForTabSwitch } from '@/features/browser/dock/chat-per-tab'
import {
  browserDockPageClass,
  browserDockSizeCss,
  isChatBrowserDockEnabled,
  nextBrowserDockPosition,
  type BrowserDockVisiblePosition,
} from '@/features/browser/dock/browser-dock-state'
import { foldPlan, settledFoldEnabled } from '../lib/settled-fold'
import { createTranscriptDensity, transcriptDensityEnabled } from '../lib/transcript-density'
import { DensityMenu } from './DensityMenu'

export default function ChatPage() {
  const i18n = useI18n()
  const session = getSession()
  const {
    hasMessages,
    isStreaming,
    followUpPending,
    evidenceSources,
    latestGrounding,
    artifactItems,
    latestScreen,
    liveRunId,
    runPanelCollapsed,
    toggleRunPanel,
    browserDock,
    setBrowserDockPosition,
    pinnedMessages,
    togglePin,
    title,
    handleScroll,
    scrollToBottom,
    handleApprovalDecision,
    handleEditedApprovalDecision,
    refreshApprovalsForRun,
    handleComposerSubmit,
    handleStop,
    copyTurn,
    submitTurnFeedback,
    feedbackNotice,
    dismissFeedbackNotice,
    regenerateLatest,
    continueGeneration,
    rerunAsNewTurn,
    resolveScheduleSeedForTurn,
    editAndResubmit,
    branchAt,
    finalExchangeVersion,
    selectExchangeVersion,
    startNewChat,
    state,
    activeTab,
    setActiveTab,
    copiedTurnId,
    uiEvents,
    ledgerInspect,
    inspectLedgerCall,
    traceReplayTruncated,
    launchMotion,
    showScrollDown,
    imageMode,
    setImageMode,
    planMode,
    setPlanMode,
    approveTurnPlan,
    rejectTurnPlan,
    planApprovalPending,
    planApprovalError,
    approvedPlanView,
    selectExecute,
    browseWeb,
    sourceScope,
    setSourceScope,
    retrievalScope,
    spaceScoped,
    activeProjectRef,
    setProjectSpaceRef,
    setRetrievalScope,
    setBrowseWeb,
    temporaryChat,
    setTemporaryChat,
    isActiveThreadTemporary,
    temporaryChatLocked,
    isForeignOriginThread,
    input,
    setInput,
    setMessageListRef,
    hasEarlierTurns,
    loadingEarlierTurns,
    loadEarlier,
    pendingQuestion,
    questionPending,
    questionError,
    answerPendingQuestion,
    skipPendingQuestion,
    pendingResearchPlan,
    startResearchPlan,
    cancelResearchPlan,
    threadGoal,
    goalPending,
    goalError,
    goalEntryAvailable,
    saveGoal,
    setGoalPaused,
    clearGoal,
    continueGoal,
    goalAutoContinue,
  } = useChatController()

  // The attachment chip that summoned the canvas stays selected when several
  // files share the same conversation. The canvas still lets the user switch
  // files locally after it opens.
  const [selectedAttachmentId, setSelectedAttachmentId] = createSignal<string | null>(null)
  const [workPeriod, setWorkPeriod] = createSignal<ChatWorkPeriod>({ kind: 'none' })

  // WS18-S4. The project this page writes to, from the controller. With the switch
  // off it is always null and none of the project state below is ever read.
  const projectRef = () => (chatProjectsEnabled() ? activeProjectRef() : null)
  const [projectContext] = createResource(projectRef, (spaceRef) => readProjectContext(spaceRef))
  // WS25-S11. The org's effective mode cap; the composer lists only the modes at or below it.
  const [agentModeCap] = createResource(() => loadEffectiveAgentModeCap())
  // EP-14 S2, behind `VITE_CHAT_TRANSCRIPT_DENSITY`. The level the transcript shows and the rules it applies. With the
  // switch off the rules change nothing, so the transcript renders as it did.
  const transcriptDensity = createTranscriptDensity()
  const projectName = () => {
    if (projectContext.loading) return i18n.tr('laster ...', 'loading ...')
    const context = projectContext()
    return context ? spaceDisplayName(context.space, i18n.tr) : i18n.tr('ikke tilgjengelig', 'unavailable')
  }
  const sandboxNoticeVisible = () => Boolean(projectRef()) && sandboxToolsRefusedNotice({
    kind: projectContext()?.space.kind,
    doMode: planMode(),
  })
  const [projectNotice, setProjectNotice] = createSignal('')
  // The first-send ask for a shared room. The send waits on the answer, and a cancel
  // is a refused send: the composer clears its draft only after a send it saw succeed.
  const [audienceAsk, setAudienceAsk] = createSignal<{
    name: string
    members: number | null
    settle: (confirmed: boolean) => void
  } | null>(null)
  const projectRefusalMessage = (refusal: ChatProjectRefusal) => {
    if (refusal === 'temporary') {
      return i18n.tr('Midlertidig samtale kan ikke brukes i et prosjekt.', 'Temporary chat cannot be used in a project.')
    }
    if (refusal === 'role_not_allowed') {
      return i18n.tr('Du kan lese dette rommet, men ikke starte samtaler i det.', 'You can read this room but cannot start conversations in it.')
    }
    return i18n.tr('Prosjektet er ikke tilgjengelig for nye samtaler.', 'This project is not available for new conversations.')
  }
  // Re-reads the Space and the caller's role before every project send: the same
  // question Control answers on the turn itself, asked here so a refusal reads as a
  // refusal rather than a failed stream. Resolves true when the send may go ahead.
  const confirmProjectAudience = async (spaceRef: string, temporary: boolean): Promise<boolean> => {
    const context = await readProjectContext(spaceRef)
    const refusal = chatProjectRefusal({ temporary, role: context?.role, space: context?.space })
    if (!context || refusal) throw new Error(projectRefusalMessage(refusal ?? 'not_eligible'))
    if (!audienceConfirmRequired({ kind: context.space.kind, confirmed: audienceConfirmed(spaceRef) })) return true
    const members = await readProjectAudience(spaceRef)
    return new Promise<boolean>((resolve) => {
      setAudienceAsk({
        name: spaceDisplayName(context.space, i18n.tr),
        members,
        settle: (confirmed) => {
          setAudienceAsk(null)
          if (confirmed) rememberAudienceConfirmed(spaceRef)
          resolve(confirmed)
        },
      })
    })
  }
  const submitWithWorkPeriod = async (payload: DashboardComposerSubmitPayload) => {
    const submission = {
      ...payload,
      text: withChatWorkPeriod(payload.text, workPeriod(), i18n.locale() === 'no' ? 'no' : 'en'),
    }
    const spaceRef = projectRef()
    if (!spaceRef) return handleComposerSubmit(submission)
    setProjectNotice('')
    const confirmed = await confirmProjectAudience(spaceRef, temporaryChat())
    if (!confirmed) throw new Error(i18n.tr('Meldingen ble ikke sendt. Utkastet er beholdt.', 'The message was not sent. Your draft is kept.'))
    return handleComposerSubmit(submission)
  }
  // The Output panel's "Ask for a change" (EP-3 S6) sends one composer turn that names the selected range. The panel has hashed the quote, so the selected text itself is not sent.
  const askAboutArtifactSelection = (request: ArtifactSelectionRequest) => submitWithWorkPeriod({
    actions: [],
    attachments: [],
    text: request.instruction,
    tools: [],
    contextRefs: [{
      kind: 'artifact_selection',
      id: request.artifactId,
      label: i18n.tr(`Utvalg i ${request.title}`, `Selection in ${request.title}`),
      selection: {
        artifactId: request.artifactId,
        version: request.version,
        start: request.start,
        end: request.end,
        quoteSha256: request.quoteSha256,
      },
    }],
  })
  // Temporary chat writes no thread, so it cannot be turned on for a project.
  const setTemporaryFromComposer = (value: boolean) => {
    if (value && projectRef()) {
      setProjectNotice(projectRefusalMessage('temporary'))
      return
    }
    setTemporaryChat(value)
  }

  // "Planlegg dette" (chat-wave2 "schedule" spec §2.1) — the message-action
  // entry point. `resolveScheduleSeedForTurn` reuses `rerunAsNewTurn`'s own
  // turnId -> preceding-user-turn resolution rather than re-deriving it.
  const [scheduleDialogOpen, setScheduleDialogOpen] = createSignal(false)
  const [scheduleDialogSeed, setScheduleDialogSeed] = createSignal({ title: '', goal: '' })
  const [scheduledRunsPanelOpen, setScheduledRunsPanelOpen] = createSignal(false)
  const openScheduleDialogForTurn = (turnId: string): void => {
    const goal = resolveScheduleSeedForTurn(turnId)
    if (!goal) return
    setScheduleDialogSeed({ title: goal.slice(0, 40).trim(), goal })
    setScheduleDialogOpen(true)
  }

  // Work surfaces belong to the durable run they describe. Looking at the
  // last transcript turn is subtly wrong when a user opens an older run after
  // sending a follow-up: the panel would show the newest turn's tool calls
  // beside the older run's plan and proof. Keep the lookup keyed to the same
  // run id that drives the live panel and trace tab.
  const activeRunTurn = createMemo(() => {
    const runId = liveRunId()
    if (!runId) return undefined
    for (let index = state.turns.length - 1; index >= 0; index -= 1) {
      const turn = state.turns[index]
      if (turn?.role === 'assistant' && turn.runId === runId) return turn
    }
    return undefined
  })
  const panelTurnStatus = () => {
    const turn = activeRunTurn()
    if (turn) return turn.status
    if (!liveRunId()) return state.turns.filter((turn) => turn.role === 'assistant').at(-1)?.status
    return undefined
  }

  // Browser dock (WS9.G): where the live panel sits beside the transcript. Off unless the build turns
  // VITE_CHAT_BROWSER_DOCK on, and nothing docks without a live run to show.
  const dockLayout = createMemo<BrowserDockVisiblePosition | null>(() => {
    const position = browserDock().position
    if (!isChatBrowserDockEnabled() || !liveRunId() || position === 'hidden') return null
    return position
  })

  // The browser body of the dock (WS9.H): the run watch, or the interactive browser beside the chat. It
  // is kept for this page only; the layout itself stays in the dock record. Showing the browser needs no
  // live run, so this body is what opens the dock on a chat without one.
  const [dockBody, setDockBody] = createSignal<'run' | 'browser'>('run')
  const browserBodyOpen = () => (
    isChatBrowserDockEnabled() && dockBody() === 'browser' && browserDock().position !== 'hidden'
  )
  // The browser body shows only on the chat tab. A canvas tab owns the rail, as it does for the run watch.
  const browserBodyShown = () => browserBodyOpen() && activeTab() === 'chat'
  // Hiding the dock returns the rail to the run watch, so restoring it shows the run again.
  createEffect(
    () => browserDock().position === 'hidden',
    (hidden) => {
      if (hidden) setDockBody('run')
    },
  )
  const openBrowserBody = () => {
    if (browserDock().position === 'hidden') toggleRunPanel()
    setDockBody('browser')
  }
  const closeBrowserBody = () => {
    setDockBody('run')
    if (!liveRunId()) setBrowserDockPosition('hidden')
  }
  const toggleBrowserBody = () => {
    if (browserBodyOpen()) closeBrowserBody()
    else openBrowserBody()
  }
  // The layout class of the browser body. The run watch's own class still comes from dockLayout.
  const browserBodyDock = () => (browserBodyOpen() ? browserDockPageClass(browserDock().position) : null) ?? ''

  // One interactive browser session per chat page (WS9.H). Its zero retention follows the thread on
  // screen and the workspace's retention verdict.
  const dockSession = useDockBrowserSession({
    orgId: () => session.activeOrg?.id,
    threadTemporary: () => isActiveThreadTemporary(),
    tr: i18n.tr,
  })

  // Chat per browser tab (WS9.H). The rule comes from the dock record: `keep` unless the record says `new`.
  // A chat moves only when the browser's active tab changes, and never without a browser session.
  const chatThreadsByTab = createChatThreadByTab()
  createEffect(
    () => dockSession.controller?.preview()?.browserSession?.session.tabs?.find((tab) => tab.active)?.tabId ?? null,
    (tabKey, previousKey) => {
      // A temporary chat is never remembered for a tab: the banner promises it is kept in no memory.
      if (previousKey && state.threadId && !isActiveThreadTemporary()) chatThreadsByTab.remember(previousKey, state.threadId)
      const plan = planChatForTabSwitch({
        previousTab: previousKey ?? null,
        nextTab: tabKey,
        rule: browserDock().chatPerTab,
        threadFor: chatThreadsByTab.threadFor,
      })
      if (plan.kind === 'new-thread') startNewChat()
      else if (plan.kind === 'select-thread') selectChatThread(plan.threadId)
    },
  )

  // The composer, found the way the focus-composer shortcut finds it.
  const focusComposer = () => {
    document.querySelector<HTMLTextAreaElement>('.verevon-dashboard-textarea')?.focus()
  }
  // The omnibox's "Ask Verevon" row: the typed text becomes the message and the composer takes focus.
  const askVerevonFromBrowser = (text: string) => {
    setInput(text)
    focusComposer()
  }

  useChatShortcuts({
    startNewChat,
    // The hook acts on these only while the dock is on; here they also need a live run to show.
    toggleBrowserDock: () => {
      if (liveRunId()) toggleRunPanel()
    },
    cycleBrowserDock: () => {
      if (liveRunId()) setBrowserDockPosition(nextBrowserDockPosition(browserDock().position))
    },
  })

  // Knowledge import is an explicit, durable action — separate from sending a
  // chat turn. The composer keeps object URLs alive for the current turn; we
  // read those bytes only when the user clicks the upload affordance and let
  // imports-core enforce its own file policy and ZDR rejection.
  const importAttachmentsToKnowledge = async (attachments: DashboardComposerAttachment[]) => {
    const orgId = session.activeOrg?.id?.trim()
    if (!orgId) throw new Error('Ingen aktiv organisasjon er tilgjengelig for import.')
    if (temporaryChat()) throw new Error('Midlertidig chat kan ikke lagre filer i kunnskapsbasen.')
    const files = await Promise.all(attachments.map(async (attachment) => {
      const response = await fetch(attachment.url)
      if (!response.ok) throw new Error(`Kunne ikke lese ${attachment.name}.`)
      const blob = await response.blob()
      return new File([blob], attachment.name, {
        type: attachment.type || blob.type || 'application/octet-stream',
      })
    }))
    if (files.length === 0) throw new Error('Ingen filer er valgt.')
    await importUpload(orgId, files, { zdr: false })
  }

  // Voice mode waits on the latest assistant turn after a spoken message and
  // shows that reply. A spoken message the running turn has not taken (yet)
  // keeps it waiting for the turn that answers it; see `latestVoiceReply`.
  const voiceReply = createMemo(() => latestVoiceReply(state.turns, followUpPending()))

  // Governed speech cannot yet attest zero data retention, so "Les høyt" is off
  // in Temporary chat, whether it was switched on or the thread already is one.
  const readAloudBlocked = () => temporaryChat() || isActiveThreadTemporary()

  // Reading aloud belongs to the conversation on screen: stop it when the
  // thread changes, when Temporary chat starts, and when the page goes away.
  createEffect(
    () => ({ threadId: state.threadId, blocked: readAloudBlocked() }),
    (next, previous) => {
      if (next.blocked || (previous && previous.threadId !== next.threadId)) speechPlayer().stop()
    },
  )
  onCleanup(() => {
    // Deferred: stopping notifies other components' signals, which must not
    // be written while this page is being torn down.
    queueMicrotask(() => speechPlayer().stop())
  })

  const composer = () => (
    <>
      <Show when={projectRef() || projectNotice() || audienceAsk()}>
        <div class="verevon-chat-project-strip">
          <Show when={projectRef()}>
            <p class="verevon-chat-project-chip" role="status">
              <span>{i18n.tr('Prosjekt', 'Project')}: {projectName()}</span>
              <Show when={!state.threadId}>
                <button
                  type="button"
                  class="verevon-chat-project-chip__clear"
                  aria-label={i18n.tr('Fjern prosjektvalget', 'Remove the project choice')}
                  onClick={() => setProjectSpaceRef(null)}
                >
                  <X class="size-3" aria-hidden="true" />
                </button>
              </Show>
            </p>
          </Show>
          <Show when={sandboxNoticeVisible()}>
            <p class="verevon-chat-project-notice" role="note">
              {i18n.tr(
                'Sandkasse-verktøy er av i delte prosjektrom, så Utfør-kjøringer her kan ikke bruke dem.',
                'Sandbox tools are off in shared project rooms, so Do runs here cannot use them.',
              )}
            </p>
          </Show>
          <Show when={projectNotice()}>
            <p class="verevon-chat-project-notice" role="status">{projectNotice()}</p>
          </Show>
          <Show when={audienceAsk()}>
            {(ask) => (
              <div class="verevon-chat-project-ask" role="alertdialog" aria-live="assertive">
                <p>
                  {ask().members === null
                    ? i18n.tr(`Alle som kan lese ${ask().name}, ser denne meldingen.`, `Everyone who can read ${ask().name} will see this message.`)
                    : i18n.tr(`${ask().members} medlemmer i ${ask().name} ser denne meldingen.`, `${ask().members} members of ${ask().name} will see this message.`)}
                </p>
                <div>
                  <button type="button" onClick={() => ask().settle(true)}>{i18n.tr('Send', 'Send')}</button>
                  <button type="button" onClick={() => ask().settle(false)}>{i18n.tr('Avbryt', 'Cancel')}</button>
                </div>
              </div>
            )}
          </Show>
        </div>
      </Show>
      <DashboardComposer
        appearance="chat"
        browseWeb={browseWeb()}
        sourceScope={sourceScope()}
        onSourceScopeChange={setSourceScope}
        sourceScopeLocked={state.turns.length > 0}
        retrievalScope={retrievalScope()}
        spaceScoped={spaceScoped()}
        onRetrievalScopeChange={setRetrievalScope}
        workPeriod={workPeriod()}
        onWorkPeriodChange={setWorkPeriod}
        imageMode={imageMode()}
        message={input()}
        onBrowseWebChange={setBrowseWeb}
        onImageModeChange={setImageMode}
        onMessageChange={setInput}
        onPlanModeChange={setPlanMode}
        onStop={handleStop}
        onSubmit={submitWithWorkPeriod}
        onTemporaryChatChange={setTemporaryFromComposer}
        planMode={planMode()}
        agentModeCap={agentModeCap()}
        // WS14 S2a: the third pill, shown only while an approved plan is waiting to
        // run on this thread. The controller owns whether it is selected.
        approvedPlan={
          approvedPlanView()
            ? { rungLabel: rungLabel(approvedPlanView()!.rung, i18n.tr), selected: approvedPlanView()!.selected }
            : undefined
        }
        onApprovedPlanSelect={selectExecute}
        showTurnReceipt={false}
        submitting={isStreaming()}
        allowMidRunSubmit
        // ^ chat opts in: a submit during the stream is a MID-RUN message, which
        // sendContent persists to the thread and queues into the live run
        // (deliverMidRun). Without it the composer swallowed Enter for the whole
        // stream and the queued-input arc had no reachable client.
        temporaryChat={temporaryChat()}
        temporaryChatLocked={temporaryChatLocked()}
        disabled={isForeignOriginThread()}
        onKnowledgeImport={importAttachmentsToKnowledge}
        voiceReply={voiceReply()}
      />
    </>
  )
  // ONE composer for the page, created once and moved between the empty
  // state and the thread dock below. With one per layout, the first message
  // of a new chat unmounted the composer that sent it: an open Voice mode
  // dialog closed mid-turn (no reply wait, no "Svar mottatt", no reading
  // aloud, focus dropped to the page) and the composer's own state was lost.
  // The two places are mutually exclusive (`hasMessages()`), and moving a DOM
  // node keeps its owner, its effects and the dialog it opened.
  const composerView = composer()

  // The newest assistant turn keeps its action row visible at rest (`data-latest`
  // in ChatMessages); older turns reveal theirs on hover or focus.
  const latestAssistantTurnId = createMemo(() => {
    const turns = state.turns
    for (let index = turns.length - 1; index >= 0; index -= 1) {
      const turn = turns[index]
      if (turn?.role === 'assistant') return turn.id
    }
    return null
  })

  // What waits on the person, docked above the composer. The transcript keeps
  // only a pointer for a control the dock holds (`dockedNodesFor` per row).
  const dockedTrays = createMemo(() =>
    selectDockedTrays({
      turns: state.turns,
      queuedInputs: state.queuedInputs,
      planApproval: (turnId) => ({
        pending: planApprovalPending() === turnId,
        error: planApprovalError()[turnId],
      }),
      goal: threadGoal(),
    }),
  )

  // `<For>` mappers are untracked in Solid 2. Derive the neighboring turns and
  // all per-row reactive values up front so each mapper receives plain row data
  // instead of reading the live store while it renders.
  const settledFolds = createMemo(() => (settledFoldEnabled && transcriptDensity.rules().foldSettled ? foldPlan(state.turns, { activeTurnId: (isStreaming() || liveRunId()) ? state.turns.at(-1)?.id : undefined }) : undefined))
  const messageRows = createMemo(() => state.turns.map((turn, index, turns) => ({
    turn,
    createdAt: turn.createdAt,
    isLatest: turn.role === 'assistant' && turn.id === latestAssistantTurnId(),
    showDateDivider: shouldShowDateDivider(turns[index - 1], turn),
    copied: copiedTurnId() === turn.id,
    planApproval: {
      pending: planApprovalPending() === turn.id,
      error: planApprovalError()[turn.id],
    },
    docked: dockedNodesFor(dockedTrays(), turn.id),
    settledFold: settledFolds()?.get(turn.id),
    version: index === turns.length - 1 ? finalExchangeVersion() : null,
    editLocked: turn.role === 'user' && isEffectfulChatTurn(turns[index + 1]?.effectClass),
  })))

  // WS09 S10, behind `VITE_CHAT_VIRTUALIZE` (see transcript-window.ts). The window
  // mounts the turns on screen, the tail, the streaming turn and the focused turn,
  // and stands in for each run it leaves out with a spacer. Heights are kept by
  // turn id, so a turn that scrolls out keeps the height it was measured at. With
  // the switch off none of this runs.
  const measuredTurnHeights = new Map<string, number>()
  const [turnHeightsVersion, setTurnHeightsVersion] = createSignal(0)
  const [focusedTurnId, setFocusedTurnId] = createSignal<string | null>(null)
  const [viewport, setViewport] = createSignal({ top: 0, height: 0, rowGap: 0 })
  let listElement: HTMLDivElement | undefined
  let threadElement: HTMLDivElement | undefined
  // WS09 S9. Older turns land above the reader. The row that followed the control
  // is measured before the request and again once the turns are in, and the list
  // scrolls by however far that row moved, so a reply streamed at the bottom in
  // the meantime does not count as a shift.
  const handleLoadEarlier = async (): Promise<void> => {
    const control = threadElement?.querySelector<HTMLElement>('.verevon-chat-load-earlier')
    const anchor = control?.nextElementSibling ?? null
    const topBefore = anchor?.getBoundingClientRect().top ?? 0
    await loadEarlier()
    requestAnimationFrame(() => {
      if (!listElement || !anchor?.isConnected) return
      listElement.scrollTop += anchor.getBoundingClientRect().top - topBefore
    })
  }
  const measureViewport = () => {
    if (!listElement || !threadElement) return
    // The viewport's top edge, measured from the top of the transcript. The
    // scroll offset cancels out of the difference of the two rects.
    const next = {
      top: Math.max(0, listElement.getBoundingClientRect().top - threadElement.getBoundingClientRect().top),
      height: listElement.clientHeight,
      rowGap: Number.parseFloat(getComputedStyle(threadElement).rowGap) || 0,
    }
    const previous = viewport()
    if (previous.top !== next.top || previous.height !== next.height || previous.rowGap !== next.rowGap) {
      setViewport(next)
    }
  }
  const observesResize = virtualizeTranscript && typeof ResizeObserver !== 'undefined'
  // A turn that leaves the window reports a zero size once more; it is dropped from
  // the observer here, not measured as empty.
  const turnObserver = observesResize
    ? new ResizeObserver((entries, observer) => {
        for (const entry of entries) {
          const element = entry.target as HTMLElement
          if (!element.isConnected) {
            observer.unobserve(element)
            continue
          }
          if (element.dataset.turnId) measuredTurnHeights.set(element.dataset.turnId, element.offsetHeight)
        }
        setTurnHeightsVersion((version) => version + 1)
        measureViewport()
      })
    : undefined
  const viewportObserver = observesResize
    ? new ResizeObserver((entries, observer) => {
        for (const entry of entries) {
          if (!entry.target.isConnected) observer.unobserve(entry.target)
        }
        measureViewport()
      })
    : undefined
  onCleanup(() => {
    turnObserver?.disconnect()
    viewportObserver?.disconnect()
  })
  const transcriptPlan = createMemo(() => {
    if (!virtualizeTranscript) return undefined
    turnHeightsVersion()
    const rows = messageRows()
    const view = viewport()
    const streamingIndex = isStreaming() ? rows.findLastIndex((row) => row.turn.role === 'assistant') : -1
    const focusedIndex = focusedTurnId() === null ? -1 : rows.findIndex((row) => row.turn.id === focusedTurnId())
    return planTranscriptWindow({
      heights: resolveTurnHeights(rows.map((row) => row.turn.id), measuredTurnHeights),
      rowGap: view.rowGap,
      scrollTop: view.top,
      viewportHeight: view.height,
      streamingIndex: streamingIndex >= 0 ? streamingIndex : undefined,
      focusedIndex: focusedIndex >= 0 ? focusedIndex : undefined,
    })
  })
  const windowRows = createMemo(() => {
    const plan = transcriptPlan()
    if (!plan) return []
    const rows = messageRows()
    const mounted: Array<{ index: number; gapBefore: number; row: (typeof rows)[number] }> = []
    for (const item of plan.items) {
      const row = rows[item.index]
      if (row) mounted.push({ ...item, row })
    }
    return mounted
  })

  // Context inspector data. Keyed on (thread, tab) so it is fetched only while
  // the Steps tab is open and refetched when the thread changes — assembling a
  // context window is real backend work, and doing it on every turn in case
  // someone might look is exactly the kind of cost that never shows up as a bug.
  const [threadContext] = createResource(
    () => {
      if (activeTab() !== 'steps') return null
      const threadId = state.threadId
      // A temporary chat has no durable context to inspect, and asking would
      // be a server round-trip for a thread that is not meant to persist.
      if (!threadId || isActiveThreadTemporary()) return null
      // `turnCount` is part of the key so the window refetches as the
      // conversation grows. Keyed on (tab, thread) alone it was fetched once
      // and then silently went stale — an inspector showing last-turn's token
      // count is worse than one that admits it is loading, because nothing on
      // screen says the number is old. Still gated on the tab being open, so
      // this costs nothing until someone actually looks.
      return {
        threadId,
        turnCount: state.turns.length,
        // Scope to the live run when there is one; the endpoint supports it
        // and it is the assembly the user is actually watching.
        runId: liveRunId() ?? undefined,
      }
    },
    async (key) => getThreadContext(key.threadId, key.runId),
  )

  // Contextual surfaces are siblings of the transcript. Opening one must not
  // unmount the message list or reset its scroll/streaming state.
  const conversationAttachments = () => {
    const seen = new Set<string>()
    return state.turns
      .filter((turn) => turn.role === 'user')
      .flatMap((turn) => turn.attachments)
      .filter((attachment) => {
        if (seen.has(attachment.id)) return false
        seen.add(attachment.id)
        return true
      })
  }

  // One derivation for every surface host: the header dropdown, the canvas tab
  // strip, the stale-tab guard and the auto-open rule all have to agree on what
  // counts as work (audit item 27 -- the header offered "Arbeid 4" for four
  // lifecycle rows). `isWorkStep` in chat-normalizers.ts owns the rule.
  const workStepCount = createMemo(() => state.taskSteps.filter(isWorkStep).length)
  // What the Kilder badge counts. A deep-research turn lists every hit it found
  // but reads a handful, and a badge showing the full list told the reader two
  // dozen pages had been read (F-16). Availability is deliberately left on the
  // full count above — an unread lead is still worth offering — so only the
  // number narrows, and the panel itself states both.
  const readSourceCount = createMemo(() => partitionEvidenceSources(evidenceSources()).read.length)
  // What the Kilder badge shows, in one place. The grounding overview is a row
  // of its own in the panel and was added inline at all three call sites; the
  // spoken tally below has to be the same number as the visible one, and four
  // copies of the expression is how those drift apart.
  const sourceBadgeTotal = createMemo(() => evidenceSources().length + (latestGrounding() ? 1 : 0))
  const sourceBadgeRead = createMemo(() => readSourceCount() + (latestGrounding() ? 1 : 0))
  /** The `[n]` the newest research report read in its second pass (EP-2-S6), for the Sources tab's grouping. */
  const secondPassNumbers = createMemo(() => {
    for (const item of [...artifactItems()].reverse()) {
      const facts = researchPassFacts((item.artifact as { provenance?: unknown }).provenance)
      if (facts) return facts.secondPassNumbers
    }
    return [] as number[]
  })
  const toolCallCount = createMemo(
    () => state.turns.reduce((total, turn) => total + (turn.toolCalls?.length ?? 0), 0),
  )

  // A tab restored from an older snapshot may no longer have evidence (for
  // example after a failed regeneration or a retention boundary). Fail closed
  // to Chat instead of opening a canvas that only says "nothing here".
  createEffect(
    () => ({
      tab: activeTab(),
      availability: {
        sourceCount: evidenceSources().length,
        readSourceCount: readSourceCount(),
        hasGrounding: Boolean(latestGrounding()),
        artifactCount: artifactItems().length,
        attachmentCount: conversationAttachments().length,
        stepCount: state.taskSteps.length,
        hasRun: Boolean(liveRunId()),
        workStepCount: workStepCount(),
        toolCallCount: toolCallCount(),
      } satisfies ChatSurfaceAvailability,
    }),
    ({ tab, availability }) => {
      if (!isChatSurfaceAvailable(tab, availability)) {
        // The effect computes availability reactively; changing the selected
        // surface is an event-style correction whose persistence guards read
        // lifecycle signals and must stay outside the untracked callback.
        untrack(() => setActiveTab('chat'))
      }
    },
  )

  // VEREVON_CHAT_DESIGN.md §3.1 — "the right panel is never opened by the
  // product, only by the work." The effect above only ever failed CLOSED, so
  // the panel never opened itself and the workspace felt inert: evidence
  // arrived and nothing happened until the user went looking for it.
  //
  // Auto-focus precedence is Work > Output > Sources, and a surface may claim
  // focus at most once per thread — after that the user's own selection wins,
  // so a late-arriving source cannot yank them out of the panel they chose.
  // Trace is deliberately excluded: the doc has it become *available* on run
  // completion without stealing focus.
  //
  // Opening is gated on `claimsFocus`, not `available`: availability offers a
  // tab, focus-claiming interrupts the reader. The two used to be the same
  // predicate, and since every lifecycle event is also a task step, a bare
  // answer counted as "work" and Work opened on plain Ask turns (live audit,
  // 2026-09-02; plan item 18). Now: first tool call, plan step or durable run
  // opens Work; first citation opens Sources; first artifact opens Output.
  const autoOpenedSurfaces = new Set<ChatTab>()
  let autoOpenThreadId: string | null = null
  createEffect(
    () => ({
      threadId: state.threadId,
      availability: {
        sourceCount: evidenceSources().length,
        readSourceCount: readSourceCount(),
        hasGrounding: Boolean(latestGrounding()),
        artifactCount: artifactItems().length,
        attachmentCount: conversationAttachments().length,
        stepCount: state.taskSteps.length,
        hasRun: Boolean(liveRunId()),
        workStepCount: workStepCount(),
        toolCallCount: toolCallCount(),
      } satisfies ChatSurfaceAvailability,
    }),
    ({ threadId, availability }) => {
      if (threadId !== autoOpenThreadId) {
        autoOpenThreadId = threadId
        autoOpenedSurfaces.clear()
      }
      const claimant = (['steps', 'artifacts', 'sources'] as const).find(
        (surface) =>
          !autoOpenedSurfaces.has(surface) && chatSurfaceClaimsFocus(surface, availability),
      )
      if (!claimant) return
      autoOpenedSurfaces.add(claimant)
      untrack(() => {
        // Only ever pull focus away from the conversation itself. If the user
        // is already reading another surface, record the claim above and leave
        // them where they are.
        if (activeTab() === 'chat') setActiveTab(claimant)
      })
    },
  )

  // The composer dock's height is unbounded (the textarea autosizes up to a
  // 50vh cap in expanded mode), so anything anchored to the viewport bottom
  // -- like the global feedback widget -- needs the real measured height,
  // not a guessed offset, to avoid sitting underneath the send button.
  /**
   * What a screen reader hears while a turn streams.
   *
   * Deliberately the turn's LIFECYCLE, not its tokens. The transcript is not a
   * live region, so today streaming is announced as nothing at all and a
   * multi-minute deep-research turn is indistinguishable from a hung page. The
   * opposite extreme is just as unusable: piping a growing answer into a live
   * region re-reads the whole thing on every token. So: one announcement when
   * the turn starts, a sparse heartbeat so a long run does not go silent, and
   * the opening of the answer once it lands.
   */
  const [streamAnnouncement, setStreamAnnouncement] = createSignal('')
  const transcriptErrorTurn = createMemo(() => {
    if (state.status !== 'error' || !state.error) return undefined
    const latest = state.turns.findLast((turn) => turn.role === 'assistant')
    // A failed turn can also contain partial output. Keep a distinct transport
    // error visible; suppress only the notice already rendered in that turn.
    return latest?.status === 'error' && latest.content.trim() === state.error.trim()
      ? latest.id
      : undefined
  })
  createEffect(
    () => transcriptErrorTurn(),
    (turnId) => {
      if (!turnId) return
      const frame = requestAnimationFrame(() => {
        const messages = document.querySelectorAll<HTMLElement>(
          '#verevon-chat-tabpanel-chat .verevon-chat-message--assistant',
        )
        messages.item(messages.length - 1)?.focus()
      })
      return () => cancelAnimationFrame(frame)
    },
  )
  /**
   * What the live region says about sources — the spoken equivalent of the
   * badge's "3 av 24". Built from the same two memos the badge renders, so the
   * number a screen reader hears and the number on screen cannot disagree.
   */
  const spokenTally = () => spokenSourceTally(sourceBadgeRead(), sourceBadgeTotal())
  /**
   * What the live region says the run is DOING — the spoken twin of the line
   * beside the thinking indicator.
   *
   * Built from the same derivation and the same label function `ThinkingDots`
   * renders, so the two cannot end up describing one wait differently. Empty
   * when the stream has reported nothing yet, which is the honest state: the
   * heartbeat then says only that work continues, exactly as before, rather
   * than naming a phase nobody observed.
   */
  const spokenActivity = () => {
    const pending = [...state.turns]
      .reverse()
      .find((turn) => turn.role === 'assistant' && turn.status === 'waiting')
    const activity = deriveStreamActivity(pending?.toolCalls, pending?.lifecycle)
    return activity ? ` ${streamActivityLabel(activity, i18n)}.` : ''
  }
  createEffect(
    () => {
      const streaming = isStreaming()
      const completedTurn = streaming
        ? undefined
        : [...state.turns]
            .reverse()
            .find((turn) => turn.role === 'assistant')
      return { completedAnswer: completedTurn?.content?.trim() ?? '', status: completedTurn?.status, streaming }
    },
    ({ completedAnswer, status, streaming }) => {
      if (!streaming) {
        // The answer itself is in the transcript to navigate; this is the cue
        // that it is there, plus enough of it to know whether it is worth
        // reading — and, before the prose, what it was actually built on.
        setStreamAnnouncement(
          status === 'error' ? `Svaret mislyktes. ${completedAnswer.slice(0, 180)}`
            : status === 'stopped' ? `Svaret ble stoppet. ${completedAnswer.slice(0, 180)}`
            : status === 'waiting' ? 'Svaret er ikke fullført ennå.'
            : completedAnswer
            ? `Svar fullført.${spokenTally()} ${completedAnswer.slice(0, 180)}`
            : '',
        )
        return
      }
      setStreamAnnouncement('Verevon svarer …')
      // Ten seconds: frequent enough that a long run does not read as dead, rare
      // enough not to be a metronome. The text alternates because a live region
      // drops a repeat of the string it is already showing. The tally is read
      // from the heartbeat rather than the opening line because at the moment a
      // turn starts it still describes the PREVIOUS turn's sources.
      let tick = 0
      const heartbeat = setInterval(() => {
        tick += 1
        const still = tick % 2 === 1 ? 'Arbeider fortsatt …' : 'Fortsatt underveis …'
        setStreamAnnouncement(`${still}${spokenActivity()}${spokenTally()}`)
      }, 10_000)
      // Solid 2 runs a cleanup RETURNED from the effect fn; `onCleanup` inside
      // one is silently dropped.
      return () => clearInterval(heartbeat)
    },
  )
  /**
   * One polite sentence per lifecycle notice on the turn that is waiting: a retry, a compaction, a context change or
   * a reduced answer is said once, as it arrives. The heartbeat above keeps the same live region for the rest of the
   * wait, so it does not repeat a notice. A plain stream has no notice and says nothing here.
   */
  createEffect(
    () => state.turns.findLast((turn) => turn.role === 'assistant' && turn.status === 'waiting')?.lifecycle?.at(-1),
    (notice) => {
      if (notice) setStreamAnnouncement(lifecycleNoticeSentence(notice, i18n))
    },
  )

  /**
   * Escape closes the contextual panel and puts focus back on the tab strip
   * that opened it (plan item 14, section 6 question 3).
   *
   * Menus and popovers already handle Escape locally; the panel itself did not,
   * so a keyboard user who opened Work had no way back to the conversation
   * without tabbing through the whole panel. Registered on the page rather than
   * the panel because focus may legitimately be inside either one.
   *
   * `defaultPrevented` is respected so an inner popover that already consumed
   * the key closes only itself — one Escape, one dismissal.
   */
  createEffect(
    () => activeTab(),
    (tab) => {
      if (tab === 'chat') return
      const onKeyDown = (event: KeyboardEvent) => {
        if (event.key !== 'Escape' || event.defaultPrevented) return
        setActiveTab('chat')
        // Focus follows the dismissal, or it is left stranded on a node that no
        // longer exists. The tab strip is what opened the panel, so prefer it —
        // but it unmounts along with the panel on a thread that has no other
        // evidence, and focus then falls to <body>. The composer is the honest
        // fallback: dismissing the panel means going back to the conversation.
        requestAnimationFrame(() => {
          const strip = document.querySelector<HTMLButtonElement>('[role="tab"][aria-selected="true"]')
          if (strip?.isConnected) {
            strip.focus()
            return
          }
          document.querySelector<HTMLTextAreaElement>('.verevon-chat-page textarea')?.focus()
        })
      }
      document.addEventListener('keydown', onKeyDown)
      return () => document.removeEventListener('keydown', onKeyDown)
    },
  )

  /**
   * Focus returns to the composer when an answer finishes (plan item 14).
   *
   * Deliberately conservative: it only reclaims focus that is sitting on
   * nothing (`body`) or on the Stop button, which is removed the instant the
   * turn settles and would otherwise leave focus on a detached node. If the
   * reader has moved focus somewhere real — a source card, the panel, a message
   * action — that is their choice and stealing it back would be worse than
   * doing nothing.
   */
  // Tracked here rather than read from a second effect argument: nothing in
  // this codebase uses that form, and on this Solid 2 RC the previous value is
  // not delivered, so a `previous !== true` guard silently never fired.
  let wasStreaming = false
  createEffect(
    () => isStreaming(),
    (streaming) => {
      const finished = wasStreaming && !streaming
      wasStreaming = streaming
      if (!finished) return
      const active = document.activeElement
      const stranded = !active
        || active === document.body
        || !active.isConnected
        || active.classList?.contains('verevon-chat-stop-btn')
      if (!stranded) return
      document.querySelector<HTMLTextAreaElement>('.verevon-chat-page textarea')?.focus()
    },
  )

  const composerDockSize = createElementHeight<HTMLDivElement>()
  createEffect(
    () => ({ visible: hasMessages(), height: composerDockSize.height() }),
    ({ visible, height }) => {
      if (!visible || height == null) {
        document.documentElement.style.removeProperty('--verevon-composer-dock-height')
        return
      }
      document.documentElement.style.setProperty('--verevon-composer-dock-height', `${height}px`)
    },
  )
  onCleanup(() => {
    document.documentElement.style.removeProperty('--verevon-composer-dock-height')
  })

  const contextualPanel = () => (
    <Switch>
      <Match when={activeTab() === 'sources'}>
        {/* WS09 S9. The sources are those of the loaded turns; say so while older ones remain. */}
        <Show when={hasEarlierTurns()}>
          <p class="verevon-chat-sources-partial" role="note">
            {i18n.tr(
              'Eldre meldinger er ikke lastet, så kildene i dem vises ikke. Last inn eldre meldinger for å se dem.',
              'Older turns are not loaded, so their sources are not shown. Load earlier messages to see them.',
            )}
          </p>
        </Show>
        <SourcesPanel grounding={latestGrounding()} sources={evidenceSources()} secondPassNumbers={secondPassNumbers()} />
      </Match>
      <Match when={activeTab() === 'artifacts'}>
        <Show
          when={conversationAttachments().length > 0 || artifactItems().length > 0}
          fallback={<ArtifactsPanel items={[]} />}
        >
          <Show when={conversationAttachments().length > 0}>
            <ChatAttachmentCanvas attachments={conversationAttachments()} selectedId={selectedAttachmentId()} />
          </Show>
          <Show when={artifactItems().length > 0}>
            <ArtifactsPanel items={artifactItems()} history={{ threadId: state.threadId ?? '', temporary: temporaryChat() || isActiveThreadTemporary() }} onAskAboutSelection={askAboutArtifactSelection} />
          </Show>
        </Show>
      </Match>
      <Match when={activeTab() === 'steps'}>
        <StepsPanel
          runId={liveRunId()}
          turnStatus={panelTurnStatus()}
          steps={state.taskSteps}
          threadId={state.threadId}
          toolCalls={activeRunTurn()?.toolCalls}
          meter={activeRunTurn()?.meter}
          screen={latestScreen()}
          events={uiEvents()}
          onStopTask={handleStop}
          onInspectCall={inspectLedgerCall}
        />
        <ContextWindowPanel
          context={threadContext()}
          loading={threadContext.loading}
          failed={threadContext.error != null}
          scopeApplied={activeRunTurn()?.scopeApplied}
        />
      </Match>
      <Match when={activeTab() === 'trace'}>
        <TracePanel runId={liveRunId()} events={uiEvents()} replayTruncated={traceReplayTruncated()} inspectRequest={ledgerInspect()} />
      </Match>
    </Switch>
  )

  const canvasTab = () => {
    const tab = activeTab()
    return tab === 'chat' ? 'sources' : tab
  }
  const workCanvasActive = () => Boolean(liveRunId()) && activeTab() === 'steps'
  // Evaluated once per dependency change, not once per prop read. Passed as an
  // inline `cond ? contextualPanel() : undefined` these became getters, and the
  // run panel reads `props.workContent` eight times (class, aria-label, title,
  // onClick, two <Show>s, the render slot). Every read instantiated a fresh
  // Work panel tree; only one reached the DOM, the other seven lived on as
  // orphans whose plan resources kept fetching -- seven identical /plans
  // requests per refresh, measured live. A memo hands every read the same tree.
  const liveRailWorkContent = createMemo(() => (workCanvasActive() ? contextualPanel() : undefined))
  const liveRailNavigation = createMemo(() => (workCanvasActive() ? workspaceNavigation() : undefined))
  // Wide-research fan-out (chat-wave2-specs, 2026-09-28): the parallel item
  // cards, hosted inside the run rail above rather than beside it. Renders
  // nothing of its own until the watched run actually contains a
  // subagent.research_fan_out call (≥2 delegated items sharing one batchId),
  // so an ordinary single subagent.task delegation never shows it. A memo for
  // the same reason as `liveRailWorkContent`: the rail reads its prop slot, and
  // each read of an inline JSX prop would build a fresh tree with its own
  // event-stream subscription.
  const liveRailWideResearch = createMemo(() => <ChatWideResearchPanel runId={liveRunId()} threadId={state.threadId ?? null} />)
  // The layout picker for the live panel's header (WS9.G). A memo, as the panel's other slots are, so
  // the panel reads one element however often it asks. The position is read inside the component.
  const liveRailDockControls = createMemo(() => (
    isChatBrowserDockEnabled() && liveRunId()
      ? <BrowserDockControls position={browserDock().position} onPositionChange={setBrowserDockPosition} />
      : undefined
  ))
  // The browser header control (WS9.H), built once. Absent while the dock is off in this build.
  const browserHeaderButton = createMemo(() => (
    isChatBrowserDockEnabled()
      ? <OpenBrowserButton open={browserBodyOpen} onToggle={toggleBrowserBody} />
      : undefined
  ))

  /**
   * Arrow keys move between messages (audit item 26 / plan item 14).
   *
   * Scoped hard: it acts only on a bare ArrowUp/ArrowDown whose target is not
   * a control that owns arrows itself. The composer textarea, the model menu,
   * the workspace tab strip's roving tabindex and the canvas resize handle all
   * bind arrows, and stealing them here would break each one.
   */
  const transcriptArrowStep = (event: KeyboardEvent): number => {
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return 0
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return 0
    if (event.defaultPrevented) return 0
    const target = event.target as HTMLElement | null
    if (!target) return 0
    if (target.closest('input, textarea, select, [contenteditable="true"], [role="tablist"], [role="listbox"], [role="menu"], [role="separator"]')) {
      return 0
    }
    return event.key === 'ArrowDown' ? 1 : -1
  }

  const handleTranscriptArrowKeys = (event: KeyboardEvent & { currentTarget: HTMLDivElement }) => {
    const step = transcriptArrowStep(event)
    if (step === 0) return
    const list = event.currentTarget
    const messages = Array.from(list.querySelectorAll<HTMLElement>('.verevon-chat-message'))
    if (messages.length === 0) return
    const current = (event.target as HTMLElement).closest<HTMLElement>('.verevon-chat-message')
    const index = current ? messages.indexOf(current) : -1
    // Entering the transcript from elsewhere lands on the newest message going
    // up, and the oldest going down, rather than jumping to an arbitrary end.
    const next = index < 0
      ? (step > 0 ? 0 : messages.length - 1)
      : Math.min(messages.length - 1, Math.max(0, index + step))
    if (next === index) return
    event.preventDefault()
    messages[next]?.focus()
  }

  // Keeps turn `index` mounted and moves focus to its message once the window has
  // rendered it. A focused turn stays in the window, so focus is not dropped when
  // the turn would otherwise leave the screen.
  const focusTurn = (index: number) => {
    const turn = messageRows()[index]?.turn
    if (!turn) return
    setFocusedTurnId(turn.id)
    requestAnimationFrame(() => {
      threadElement?.querySelector<HTMLElement>(`[data-turn-index="${index}"] .verevon-chat-message`)?.focus()
    })
  }

  // The arrow walk while the window is on. The messages mounted now are only the
  // window, so the step is taken over turn indices, and `focusTurn` is what walks
  // across a stretch the window has not mounted.
  const handleVirtualArrowKeys = (event: KeyboardEvent) => {
    const step = transcriptArrowStep(event)
    if (step === 0) return
    const count = messageRows().length
    if (count === 0) return
    const current = (event.target as HTMLElement).closest<HTMLElement>('[data-turn-index]')
    const index = current ? Number(current.dataset.turnIndex) : -1
    const next = index < 0
      ? (step > 0 ? 0 : count - 1)
      : Math.min(count - 1, Math.max(0, index + step))
    if (next === index) return
    event.preventDefault()
    focusTurn(next)
  }

  const handleVirtualScroll = () => {
    handleScroll()
    measureViewport()
  }

  const workspaceNavigation = () => (
    <ChatTabs
      active={activeTab()}
      artifactCount={artifactItems().length + conversationAttachments().length}
      includeChat={false}
      runAvailable={Boolean(liveRunId())}
      sourceCount={sourceBadgeTotal()}
      readSourceCount={sourceBadgeRead()}
      stepCount={state.taskSteps.length}
      workStepCount={workStepCount()}
      toolCallCount={toolCallCount()}
      traceAvailable={Boolean(liveRunId())}
      onChange={setActiveTab}
    />
  )

  // One row of the transcript, shared by the full list and the window so both
  // mount the same message. The row is passed as an accessor, as `<For>` gives it.
  type MessageRow = ReturnType<typeof messageRows>[number]
  const renderMessageRow = (row: () => MessageRow) => (
    <>
      <Show when={row().showDateDivider}>
        <DateDivider value={row().createdAt} />
      </Show>
      <MessageBlock
        copied={row().copied}
        message={row().turn}
        onBranch={() => branchAt(row().turn.id)}
        onCopy={() => void copyTurn(row().turn)}
        onEdit={(text) => void editAndResubmit(row().turn.id, text)}
        onRegenerate={regenerateLatest}
        onContinue={() => continueGeneration(row().turn.id)}
        onRerunAsNewTurn={() => void rerunAsNewTurn(row().turn.id)}
        onScheduleThis={() => openScheduleDialogForTurn(row().turn.id)}
        onFeedback={(rating, note) => submitTurnFeedback(row().turn.id, rating, note)}
        onApprovalDecision={(approvalId, decision) =>
          void handleApprovalDecision(row().turn.id, approvalId, decision)
        }
        onApprovalEditedDecision={(approvalId, edit) => handleEditedApprovalDecision(row().turn.id, approvalId, edit)}
        onApprovePlan={(rung, justification, steps) =>
          void approveTurnPlan(row().turn.id, rung, justification, steps)
        }
        onRejectPlan={(_planId, reason) => void rejectTurnPlan(row().turn.id, reason)}
        pinned={pinnedMessages().includes(row().turn.id)}
        pinDisabled={pinnedMessagesFull(pinnedMessages())}
        onTogglePin={() => togglePin(row().turn.id)}
        planApproval={row().planApproval}
        docked={row().docked}
        settledFold={row().settledFold}
        onSelectFollowUp={setInput}
        onViewSteps={() => setActiveTab('steps')}
        onViewAttachments={(attachmentId) => {
          setSelectedAttachmentId(attachmentId)
          setActiveTab('artifacts')
        }}
        // The version switcher only ever applies to the trailing
        // assistant turn — chat-versions.ts guards versioning to
        // the final exchange, so no other turn can have one.
        version={row().version}
        isLatest={row().isLatest}
        onSelectVersion={selectExchangeVersion}
        editLocked={row().editLocked}
        threadId={state.threadId}
        temporaryChat={readAloudBlocked()}
        spaceScoped={spaceScoped()}
      />
    </>
  )

  return (
    <div
      class={[
        `verevon-chat-page${launchMotion() ? ' verevon-chat-page-launch' : ''}`,
        {
          'verevon-chat-page--split': Boolean(liveRunId()) && !runPanelCollapsed(),
          'verevon-chat-page--railed': Boolean(liveRunId()) && runPanelCollapsed(),
          'verevon-chat-page--canvas': activeTab() !== 'chat',
          'verevon-chat-page--dock-right': dockLayout() === 'right',
          'verevon-chat-page--dock-bottom': dockLayout() === 'bottom',
          'verevon-chat-page--dock-maximised': dockLayout() === 'maximised',
        },
        browserBodyDock(),
      ]}
      style={dockLayout() ? { '--browser-dock-size': browserDockSizeCss(browserDock()) } : undefined}
    >
      {/* Streaming is silent to a screen reader: tokens append into the
          transcript, which is not a live region, so nothing is announced and a
          long deep-research turn is indistinguishable from a hung page. This
          announces the turn's lifecycle instead of its tokens — re-reading a
          growing answer every few hundred milliseconds would be unusable. See
          `streamAnnouncement`. */}
      <div class="sr-only" role="status" aria-live="polite" aria-atomic="true">
        {streamAnnouncement()}
      </div>

      <section
        class="verevon-chat-section"
        data-transcript-process={transcriptDensity.rules().showProcess ? undefined : 'hidden'}
        aria-label={i18n.tr('Verevon chat-arbeidsflate', 'Verevon chat workspace')}
      >
        <Show when={hasMessages()}>
          <ChatHeader
            active={activeTab()}
            artifactCount={artifactItems().length + conversationAttachments().length}
            attachmentCount={conversationAttachments().length}
            branchCount={state.branchCount}
            messageCount={state.turns.length}
            runAvailable={Boolean(liveRunId())}
            sourceCount={sourceBadgeTotal()}
            readSourceCount={sourceBadgeRead()}
            stepCount={state.taskSteps.length}
            workStepCount={workStepCount()}
            toolCallCount={toolCallCount()}
            title={title()}
            traceAvailable={Boolean(liveRunId())}
            onChange={setActiveTab}
            browserSlot={browserHeaderButton()}
            onNewChat={startNewChat}
            onRegenerate={regenerateLatest}
          />
          <Show when={transcriptDensityEnabled}>
            <DensityMenu density={transcriptDensity.level()} onChoose={transcriptDensity.choose} />
          </Show>
        </Show>
        <Show when={hasMessages() && state.threadId && !isActiveThreadTemporary()}>
          <ThreadWorkspacePanel threadId={state.threadId ?? ''} />
          <EffectivePolicyPanel scope={{ kind: 'thread', threadId: state.threadId ?? '' }} />
        </Show>

        <Switch>
          <Match when={!hasMessages()}>
            <PersonalSetupCard threadId={state.threadId} />
            <EmptyChatState
              onSelectPrompt={setInput}
              onResumeThread={(threadId) => selectChatThread(threadId)}
              orgId={session.activeOrg?.id}
              orgName={session.activeOrg?.name}
              userId={session.user?.id}
              userName={session.user?.name}
            >
              {composerView}
            </EmptyChatState>
          </Match>
          <Match when={hasMessages()}>
            <div
              id="verevon-chat-tabpanel-chat"
              role="tabpanel"
              aria-label="Chat"
              ref={(element) => {
                listElement = element
                setMessageListRef(element)
                viewportObserver?.observe(element)
              }}
              class="verevon-chat-message-list"
              onScroll={virtualizeTranscript ? handleVirtualScroll : handleScroll}
              onKeyDown={virtualizeTranscript ? handleVirtualArrowKeys : handleTranscriptArrowKeys}
            >
              <ThreadVisibilityBanners
                temporary={isActiveThreadTemporary}
                foreignOrigin={isForeignOriginThread}
              />
              <div class="verevon-scheduled-runs-entry">
                <button
                  type="button"
                  class="verevon-scheduled-runs-entry__trigger"
                  aria-expanded={scheduledRunsPanelOpen() ? 'true' : 'false'}
                  onClick={() => setScheduledRunsPanelOpen((open) => !open)}
                >
                  <CalendarClock size={14} aria-hidden="true" /> {i18n.tr('Planlagte kjøringer', 'Scheduled runs')}
                </button>
                <Show when={scheduledRunsPanelOpen()}>
                  <div class="verevon-scheduled-runs-entry__panel" role="region" aria-label={i18n.tr('Planlagte kjøringer', 'Scheduled runs')}>
                    <ScheduledRunsPanel />
                  </div>
                </Show>
              </div>
              <div
                class="verevon-chat-thread"
                data-chat-containment={import.meta.env.VITE_CHAT_CONTENT_CONTAINMENT === 'true' ? 'on' : undefined}
                role={virtualizeTranscript ? 'feed' : undefined}
                ref={(element) => {
                  threadElement = element
                  viewportObserver?.observe(element)
                }}
              >
                {/* WS09 S9. The thread was opened on its newest page; older turns are one request away. */}
                <Show when={hasEarlierTurns()}>
                  <VerevonTextButton
                    class="verevon-chat-load-earlier"
                    disabled={loadingEarlierTurns()}
                    onClick={() => void handleLoadEarlier()}
                  >
                    {loadingEarlierTurns()
                      ? i18n.tr('Laster eldre meldinger …', 'Loading earlier messages …')
                      : i18n.tr('Last inn eldre meldinger', 'Load earlier messages')}
                  </VerevonTextButton>
                </Show>
                {/* Keyed by turn id: messageRows rebuilds its row objects on every copy, feedback or approval change; unkeyed, each change remounted the whole transcript. */}
                <Show
                  when={virtualizeTranscript}
                  fallback={(
                    <For each={messageRows()} keyed={(row) => row.turn.id}>
                      {renderMessageRow}
                    </For>
                  )}
                >
                  {/* Each mounted turn is an article with its place in the whole transcript, so a screen reader hears "3 of 240" with the window in place. */}
                  <For each={windowRows()} keyed={(item) => item.row.turn.id}>
                    {(item) => (
                      <>
                        <Show when={item().gapBefore > 0}>
                          <div aria-hidden="true" style={{ height: `${item().gapBefore}px`, 'flex-shrink': 0 }} />
                        </Show>
                        <div
                          role="article"
                          aria-setsize={messageRows().length}
                          aria-posinset={item().index + 1}
                          data-turn-id={item().row.turn.id}
                          data-turn-index={item().index}
                          ref={(element) => turnObserver?.observe(element)}
                          onFocusIn={() => setFocusedTurnId(item().row.turn.id)}
                        >
                          {renderMessageRow(() => item().row)}
                        </div>
                      </>
                    )}
                  </For>
                  <Show when={(transcriptPlan()?.gapAfter ?? 0) > 0}>
                    <div aria-hidden="true" style={{ height: `${transcriptPlan()?.gapAfter ?? 0}px`, 'flex-shrink': 0 }} />
                  </Show>
                </Show>
                {/* The transcript's retry notice and live region already cover
                    an identical error. Distinct errors still need this alert
                    and keyboard focus, especially when partial output remains. */}
                <Show when={state.error && state.status === 'error' && !transcriptErrorTurn()}>
                  <div
                    class="verevon-chat-error"
                    role="alert"
                    tabindex={-1}
                    ref={(element: HTMLDivElement) => { requestAnimationFrame(() => element.focus()) }}
                  >{state.error}</div>
                </Show>
                {/*
                  A rating that did not persist must say so. Deliberately its own
                  notice rather than `state.error`: the answer is fine, only the
                  rating failed. `role="status"` keeps it quiet (polite, not an
                  interruption) and clicking it dismisses.
                */}
                <Show when={feedbackNotice()}>
                  {(message) => (
                    <button
                      type="button"
                      class="verevon-chat-error"
                      role="status"
                      onClick={dismissFeedbackNotice}
                    >
                      {message()}
                    </button>
                  )}
                </Show>
              </div>
            </div>
          </Match>
        </Switch>

        {/* The conversation remains the command centre while a document,
            source set, or work trace is open. Keeping this single composer
            mounted also preserves drafts, attachments, and model selection. */}
        <Show when={hasMessages()}>
          <div class="verevon-chat-composer-dock" ref={composerDockSize.setElement}>
            <Show when={showScrollDown()}>
              <button
                type="button"
                class="verevon-chat-scroll-down"
                aria-label={i18n.tr('Rull til nyeste', 'Scroll to bottom')}
                onClick={() => scrollToBottom()}
              >
                <ArrowDown size={16} />
              </button>
            </Show>
            <div class="verevon-chat-composer-dock__inner">
              {/*
                Approvals, the plan awaiting a grant and messages sent into the
                live run: docked here, where the person is looking when a run
                needs them. The durable record of a mid-run message lives on
                the thread itself (written when it was accepted), so a reload
                shows it as an ordinary message; the queue tray is only the
                live status of a delivery in flight.
              */}
              <ComposerTrays
                trays={dockedTrays()}
                onApprovalDecision={(turnId, approvalId, decision) =>
                  void handleApprovalDecision(turnId, approvalId, decision)
                }
                onApprovalEditedDecision={(turnId, approvalId, edit) => handleEditedApprovalDecision(turnId, approvalId, edit)}
                onApprovePlan={(turnId, rung, justification, steps) =>
                  void approveTurnPlan(turnId, rung, justification, steps)
                }
                onRejectPlan={(turnId, reason) => void rejectTurnPlan(turnId, reason)}
                question={pendingQuestion() ?? undefined}
                questionPending={questionPending()}
                questionError={questionError()}
                onAnswerQuestion={(answer) => void answerPendingQuestion(answer)}
                onSkipQuestion={() => void skipPendingQuestion()}
                researchPlan={pendingResearchPlan()}
                onStartResearchPlan={(edit) => void startResearchPlan(edit)}
                onCancelResearchPlan={cancelResearchPlan}
                goalEntry={goalEntryAvailable()}
                goalPending={goalPending()}
                goalError={goalError()}
                goalBusy={isStreaming()}
                onSaveGoal={saveGoal}
                onContinueGoal={() => void continueGoal()}
                goalAutoContinue={goalAutoContinue}
                onPauseGoal={(paused) => void setGoalPaused(paused)}
                onClearGoal={() => void clearGoal()}
              />
              <Show when={isStreaming()}>
                <div class="verevon-chat-stop-wrap">
                  <button type="button" class="verevon-chat-stop-btn" onClick={handleStop}>
                    <Square size={12} />
                    Stopp svar
                  </button>
                </div>
              </Show>
              {composerView}
            </div>
          </div>
        </Show>
      </section>

      <Show when={hasMessages() && activeTab() !== 'chat' && !workCanvasActive()}>
        <ChatWorkspaceCanvas
          active={canvasTab()}
          navigation={workspaceNavigation()}
          onClose={() => setActiveTab('chat')}
        >
          {contextualPanel()}
        </ChatWorkspaceCanvas>
      </Show>

      {/*
        Watch-the-agent-work split view. Ordinary Ask turns also receive a
        durable run id for receipts/feedback, but `liveRunId` deliberately
        filters those out so the base chat never becomes a permanent IDE.
        Planned and deep-research turns alone own this adjacent work canvas.
      */}
      <ChatLiveRunPanel
        collapsed={workCanvasActive() ? false : runPanelCollapsed()}
        hidden={(activeTab() !== 'chat' && !workCanvasActive()) || browserBodyShown()}
        onToggleCollapsed={toggleRunPanel}
        orgId={session.activeOrg?.id}
        runId={liveRunId()}
        meter={activeRunTurn()?.meter}
        zdr={isActiveThreadTemporary()}
        onRefreshApprovals={(runId) => void refreshApprovalsForRun(runId)}
        navigation={liveRailNavigation()}
        onCloseWork={() => setActiveTab('chat')}
        workContent={liveRailWorkContent()}
        wideResearch={liveRailWideResearch()}
        dockControls={liveRailDockControls()}
      />
      {/* The interactive browser body (WS9.H) takes the rail while the browser is open beside the chat. */}
      <Show when={browserBodyOpen() ? dockSession.controller : null}>
        {(controller) => (
          <InteractiveBrowserPane
            controller={controller()}
            orgId={() => session.activeOrg?.id}
            zdr={dockSession.zdr}
            dockPosition={browserDock().position}
            onDockPositionChange={setBrowserDockPosition}
            sizeCss={browserDockSizeCss(browserDock())}
            hidden={activeTab() !== 'chat'}
            onAskVerevon={askVerevonFromBrowser}
            onAskAboutPage={focusComposer}
            onClose={closeBrowserBody}
          />
        )}
      </Show>
      <Show when={scheduleDialogOpen()}>
        <Portal>
          <ScheduleDialog
            onClose={() => setScheduleDialogOpen(false)}
            seedTitle={scheduleDialogSeed().title}
            seedGoal={scheduleDialogSeed().goal}
            threadOriginHint="message-action"
          />
        </Portal>
      </Show>
    </div>
  )
}

function ThreadVisibilityBanners(props: {
  temporary: () => boolean
  foreignOrigin: () => boolean
}) {
  const i18n = useI18n()
  return (
    <>
      <Show when={props.temporary()}>
        <div class="verevon-chat-temporary-banner" role="status">
          <EyeOff size={13} />
          <span>{i18n.tr('Midlertidig samtale – lagres ikke i historikk eller minne.', 'Temporary conversation - not stored in history or memory.')}</span>
        </div>
      </Show>
      <Show when={props.foreignOrigin()}>
        <div class="verevon-chat-foreign-thread-banner" role="status">
          <EyeOff size={13} />
          <span>{i18n.tr(
            'Denne samtalen eies av en annen arbeidsflate og vises skrivebeskyttet.',
            'This conversation belongs to another workspace and is shown read-only.',
          )}</span>
        </div>
      </Show>
    </>
  )
}
