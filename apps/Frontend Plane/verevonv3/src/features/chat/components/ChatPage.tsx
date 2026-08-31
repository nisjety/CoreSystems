import {
  For,
  Match,
  Show,
  Switch,
  createMemo,
  createEffect,
  createSignal,
  untrack,
} from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  ArrowDown,
  EyeOff,
  Square,
} from '@/shared/icons'
import { isEffectfulChatTurn } from '@/shared/chat/effect-class'
import {
  DashboardComposer,
  type DashboardComposerAttachment,
} from '@/features/dashboard/home/DashboardComposer'
import { getThreadContext } from '@/shared/api/chat-client'
import { importUpload } from '@/shared/api/knowledge-client'
import { getSession } from '@/shared/session/session-store'
import {
  DateDivider,
  EmptyChatState,
  MessageBlock,
  QueuedInputStrip,
} from './ChatMessages'
import {
  ArtifactsPanel,
} from './ChatArtifactPanel'
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
import { ChatWorkspaceCanvas } from './ChatWorkspaceCanvas'
import { ChatAttachmentCanvas } from './ChatAttachmentCanvas'
import {
  shouldShowDateDivider,
} from './chat-media-markdown'
import { useChatController } from './use-chat-controller'
import { useChatShortcuts } from '@/features/chat/lib/use-chat-shortcuts'
import { isChatSurfaceAvailable, type ChatSurfaceAvailability } from '../lib/chat-surfaces'

export default function ChatPage() {
  const session = getSession()
  const {
    hasMessages,
    isStreaming,
    evidenceSources,
    latestGrounding,
    artifactItems,
    latestScreen,
    liveRunId,
    runPanelCollapsed,
    toggleRunPanel,
    title,
    handleScroll,
    scrollToBottom,
    handleApprovalDecision,
    refreshApprovalsForRun,
    handleComposerSubmit,
    handleStop,
    copyTurn,
    submitTurnFeedback,
    feedbackNotice,
    dismissFeedbackNotice,
    regenerateLatest,
    rerunAsNewTurn,
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
    traceReplayTruncated,
    launchMotion,
    showScrollDown,
    imageMode,
    setImageMode,
    planMode,
    setPlanMode,
    approveTurnPlan,
    planApprovalPending,
    planApprovalError,
    browseWeb,
    setBrowseWeb,
    temporaryChat,
    setTemporaryChat,
    isActiveThreadTemporary,
    temporaryChatLocked,
    isForeignOriginThread,
    input,
    setInput,
    setMessageListRef,
  } = useChatController()

  // The attachment chip that summoned the canvas stays selected when several
  // files share the same conversation. The canvas still lets the user switch
  // files locally after it opens.
  const [selectedAttachmentId, setSelectedAttachmentId] = createSignal<string | null>(null)

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

  useChatShortcuts({ startNewChat })

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

  const composer = () => (
    <DashboardComposer
      appearance="chat"
      browseWeb={browseWeb()}
      imageMode={imageMode()}
      message={input()}
      onBrowseWebChange={setBrowseWeb}
      onImageModeChange={setImageMode}
      onMessageChange={setInput}
      onPlanModeChange={setPlanMode}
      onStop={handleStop}
      onSubmit={handleComposerSubmit}
      onTemporaryChatChange={setTemporaryChat}
      planMode={planMode()}
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
    />
  )

  // `<For>` mappers are untracked in Solid 2. Derive the neighboring turns and
  // all per-row reactive values up front so each mapper receives plain row data
  // instead of reading the live store while it renders.
  const messageRows = createMemo(() => state.turns.map((turn, index, turns) => ({
    turn,
    createdAt: turn.createdAt,
    showDateDivider: shouldShowDateDivider(turns[index - 1], turn),
    copied: copiedTurnId() === turn.id,
    planApproval: {
      pending: planApprovalPending() === turn.id,
      error: planApprovalError()[turn.id],
    },
    version: index === turns.length - 1 ? finalExchangeVersion() : null,
    editLocked: turn.role === 'user' && isEffectfulChatTurn(turns[index + 1]?.effectClass),
  })))

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
      return { threadId }
    },
    async (key) => getThreadContext(key.threadId),
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

  // A tab restored from an older snapshot may no longer have evidence (for
  // example after a failed regeneration or a retention boundary). Fail closed
  // to Chat instead of opening a canvas that only says "nothing here".
  createEffect(
    () => ({
      tab: activeTab(),
      availability: {
        sourceCount: evidenceSources().length,
        hasGrounding: Boolean(latestGrounding()),
        artifactCount: artifactItems().length,
        attachmentCount: conversationAttachments().length,
        stepCount: state.taskSteps.length,
        hasRun: Boolean(liveRunId()),
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

  const contextualPanel = () => (
    <Switch>
      <Match when={activeTab() === 'sources'}>
        <SourcesPanel grounding={latestGrounding()} sources={evidenceSources()} />
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
            <ArtifactsPanel items={artifactItems()} />
          </Show>
        </Show>
      </Match>
      <Match when={activeTab() === 'steps'}>
        <StepsPanel
          runId={liveRunId()}
          steps={state.taskSteps}
          threadId={state.threadId}
          toolCalls={activeRunTurn()?.toolCalls}
          screen={latestScreen()}
          events={uiEvents()}
          onStopTask={handleStop}
        />
        <ContextWindowPanel
          context={threadContext()}
          loading={threadContext.loading}
          failed={threadContext.error != null}
        />
      </Match>
      <Match when={activeTab() === 'trace'}>
        <TracePanel runId={liveRunId()} events={uiEvents()} replayTruncated={traceReplayTruncated()} />
      </Match>
    </Switch>
  )

  const canvasTab = () => {
    const tab = activeTab()
    return tab === 'chat' ? 'sources' : tab
  }
  const workCanvasActive = () => Boolean(liveRunId()) && activeTab() === 'steps'
  const workspaceNavigation = () => (
    <ChatTabs
      active={activeTab()}
      artifactCount={artifactItems().length + conversationAttachments().length}
      includeChat={false}
      runAvailable={Boolean(liveRunId())}
      sourceCount={evidenceSources().length + (latestGrounding() ? 1 : 0)}
      stepCount={state.taskSteps.length}
      traceAvailable={Boolean(liveRunId())}
      onChange={setActiveTab}
    />
  )

  return (
    <div
      class={[
        `verevon-chat-page${launchMotion() ? ' verevon-chat-page-launch' : ''}`,
        {
          'verevon-chat-page--split': Boolean(liveRunId()) && !runPanelCollapsed(),
          'verevon-chat-page--railed': Boolean(liveRunId()) && runPanelCollapsed(),
          'verevon-chat-page--canvas': activeTab() !== 'chat',
        },
      ]}
    >
      <Show when={launchMotion()}>
        <div class="verevon-chat-launch-wash" aria-hidden="true" />
      </Show>

      <section class="verevon-chat-section" aria-label="Verevon chat workspace">
        <Show when={hasMessages()}>
          <ChatHeader
            active={activeTab()}
            artifactCount={artifactItems().length + conversationAttachments().length}
            branchCount={state.branchCount}
            messageCount={state.turns.length}
            runAvailable={Boolean(liveRunId())}
            sourceCount={evidenceSources().length + (latestGrounding() ? 1 : 0)}
            stepCount={state.taskSteps.length}
            title={title()}
            traceAvailable={Boolean(liveRunId())}
            onChange={setActiveTab}
            onNewChat={startNewChat}
            onRegenerate={regenerateLatest}
          />
        </Show>

        <Switch>
          <Match when={!hasMessages()}>
            <EmptyChatState onSelectPrompt={setInput}>{composer()}</EmptyChatState>
          </Match>
          <Match when={hasMessages()}>
            <div
              id="verevon-chat-tabpanel-chat"
              role="tabpanel"
              aria-label="Chat"
              ref={setMessageListRef}
              class="verevon-chat-message-list"
              onScroll={handleScroll}
            >
              <ThreadVisibilityBanners
                temporary={isActiveThreadTemporary}
                foreignOrigin={isForeignOriginThread}
              />
              <div class="verevon-chat-thread">
                <For each={messageRows()}>
                  {(row) => (
                    <>
                      <Show when={row.showDateDivider}>
                        <DateDivider value={row.createdAt} />
                      </Show>
                      <MessageBlock
                        copied={row.copied}
                        message={row.turn}
                        onBranch={() => branchAt(row.turn.id)}
                        onCopy={() => void copyTurn(row.turn)}
                        onEdit={(text) => void editAndResubmit(row.turn.id, text)}
                        onRegenerate={regenerateLatest}
                        onRerunAsNewTurn={() => void rerunAsNewTurn(row.turn.id)}
                        onFeedback={(rating) => submitTurnFeedback(row.turn.id, rating)}
                        onApprovalDecision={(approvalId, decision) =>
                          void handleApprovalDecision(row.turn.id, approvalId, decision)
                        }
                        onApprovePlan={(rung, justification) =>
                          void approveTurnPlan(row.turn.id, rung, justification)
                        }
                        planApproval={row.planApproval}
                        onSelectFollowUp={setInput}
                        onViewSteps={() => setActiveTab('steps')}
                        onViewAttachments={(attachmentId) => {
                          setSelectedAttachmentId(attachmentId)
                          setActiveTab('artifacts')
                        }}
                        // The version switcher only ever applies to the trailing
                        // assistant turn — chat-versions.ts guards versioning to
                        // the final exchange, so no other turn can have one.
                        version={row.version}
                        onSelectVersion={selectExchangeVersion}
                        editLocked={row.editLocked}
                      />
                    </>
                  )}
                </For>
                {/*
                  Below the in-progress answer, because that is where the newest
                  thing the user did belongs. The durable record of these lives
                  on the thread itself (written when the message was accepted),
                  so a reload shows them as ordinary messages — this strip is
                  only the live status of a delivery in flight.
                */}
                <QueuedInputStrip entries={state.queuedInputs} />
                <Show when={state.error && state.status === 'error'}>
                  <div class="verevon-chat-error" role="alert">{state.error}</div>
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
          <div class="verevon-chat-composer-dock">
            <Show when={showScrollDown()}>
              <button
                type="button"
                class="verevon-chat-scroll-down"
                aria-label="Scroll to bottom"
                onClick={() => scrollToBottom()}
              >
                <ArrowDown size={16} />
              </button>
            </Show>
            <div class="verevon-chat-composer-dock__inner">
              <Show when={isStreaming()}>
                <div class="verevon-chat-stop-wrap">
                  <button type="button" class="verevon-chat-stop-btn" onClick={handleStop}>
                    <Square size={12} />
                    Stopp svar
                  </button>
                </div>
              </Show>
              {composer()}
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
        hidden={activeTab() !== 'chat' && !workCanvasActive()}
        onToggleCollapsed={toggleRunPanel}
        orgId={session.activeOrg?.id}
        runId={liveRunId()}
        zdr={isActiveThreadTemporary()}
        onRefreshApprovals={(runId) => void refreshApprovalsForRun(runId)}
        navigation={workCanvasActive() ? workspaceNavigation() : undefined}
        onCloseWork={() => setActiveTab('chat')}
        workContent={workCanvasActive() ? contextualPanel() : undefined}
      />
    </div>
  )
}

function ThreadVisibilityBanners(props: {
  temporary: () => boolean
  foreignOrigin: () => boolean
}) {
  return (
    <>
      <Show when={props.temporary()}>
        <div class="verevon-chat-temporary-banner" role="status">
          <EyeOff size={13} />
          <span>Midlertidig samtale – lagres ikke i historikk eller minne.</span>
        </div>
      </Show>
      <Show when={props.foreignOrigin()}>
        <div class="verevon-chat-foreign-thread-banner" role="status">
          <EyeOff size={13} />
          <span>Denne samtalen eies av en annen arbeidsflate og vises skrivebeskyttet.</span>
        </div>
      </Show>
    </>
  )
}
