import {
  For,
  Match,
  Show,
  Switch,
} from 'solid-js'
import {
  ArrowDown,
  EyeOff,
  Square,
} from 'lucide-solid'
import {
  DashboardComposer,
} from '@/features/dashboard/home/DashboardComposer'
import {
  DateDivider,
  EmptyChatState,
  MessageBlock,
} from './ChatMessages'
import {
  ArtifactsPanel,
} from './ChatArtifactPanel'
import {
  ChatHeader,
  ChatTabs,
  SourcesPanel,
  StepsPanel,
} from './ChatPanels'
import {
  shouldShowDateDivider,
} from './chat-media-markdown'
import { useChatController } from './use-chat-controller'
import { useChatShortcuts } from '@/features/chat/lib/use-chat-shortcuts'

export default function ChatPage() {
  const {
    hasMessages,
    isStreaming,
    evidenceSources,
    latestGrounding,
    artifactItems,
    artifacts,
    latestScreen,
    title,
    handleScroll,
    scrollToBottom,
    handleApprovalDecision,
    handleComposerSubmit,
    handleStop,
    copyTurn,
    submitTurnFeedback,
    feedbackNotice,
    dismissFeedbackNotice,
    regenerateLatest,
    editAndResubmit,
    branchAt,
    startNewChat,
    state,
    activeTab,
    setActiveTab,
    copiedTurnId,
    launchMotion,
    showScrollDown,
    imageMode,
    setImageMode,
    planMode,
    setPlanMode,
    browseWeb,
    setBrowseWeb,
    temporaryChat,
    setTemporaryChat,
    isActiveThreadTemporary,
    temporaryChatLocked,
    input,
    setInput,
    setMessageListRef,
  } = useChatController()

  useChatShortcuts({ startNewChat })

  const composer = () => (
    <DashboardComposer
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
      temporaryChat={temporaryChat()}
      temporaryChatLocked={temporaryChatLocked()}
    />
  )

  return (
    <div class={`velion-chat-page${launchMotion() ? ' velion-chat-page-launch' : ''}`}>
      <Show when={launchMotion()}>
        <div class="velion-chat-launch-wash" aria-hidden="true" />
      </Show>

      <section class="velion-chat-section" aria-label="Velion chat workspace">
        <Show when={hasMessages()}>
          <ChatHeader
            branchCount={state.branchCount}
            messageCount={state.turns.length}
            title={title()}
            onNewChat={startNewChat}
            onRegenerate={regenerateLatest}
          />
          <ChatTabs
            active={activeTab()}
            artifactCount={artifacts().length}
            sourceCount={evidenceSources().length}
            stepCount={state.taskSteps.length}
            onChange={setActiveTab}
          />
        </Show>

        <Switch>
          <Match when={!hasMessages()}>
            <EmptyChatState onSelectPrompt={setInput}>{composer()}</EmptyChatState>
          </Match>
          <Match when={activeTab() === 'chat'}>
            <div ref={setMessageListRef} class="velion-chat-message-list" onScroll={handleScroll}>
              <Show when={isActiveThreadTemporary()}>
                <div class="velion-chat-temporary-banner" role="status">
                  <EyeOff size={13} />
                  <span>Midlertidig samtale – lagres ikke i historikk eller minne.</span>
                </div>
              </Show>
              <div class="velion-chat-thread">
                <For each={state.turns}>
                  {(turn, index) => (
                    <>
                      <Show when={shouldShowDateDivider(state.turns[index() - 1], turn)}>
                        <DateDivider value={turn.createdAt} />
                      </Show>
                      <MessageBlock
                        copied={copiedTurnId() === turn.id}
                        message={turn}
                        onBranch={() => branchAt(turn.id)}
                        onCopy={() => void copyTurn(turn)}
                        onEdit={(text) => void editAndResubmit(turn.id, text)}
                        onRegenerate={regenerateLatest}
                        onFeedback={(rating) => submitTurnFeedback(turn.id, rating)}
                        onApprovalDecision={(approvalId, decision) =>
                          void handleApprovalDecision(turn.id, approvalId, decision)
                        }
                        onSelectFollowUp={setInput}
                        onViewSteps={() => setActiveTab('steps')}
                      />
                    </>
                  )}
                </For>
                <Show when={state.error && state.status === 'error'}>
                  <div class="velion-chat-error" role="alert">{state.error}</div>
                </Show>
                {/*
                  A rating that did not persist must say so. Deliberately its own
                  notice rather than `state.error`: the answer is fine, only the
                  rating failed. `role="status"` keeps it quiet (polite, not an
                  interruption) and clicking it dismisses.
                */}
                <Show when={feedbackNotice()}>
                  {(message) => (
                    <div
                      class="velion-chat-error"
                      role="status"
                      onClick={dismissFeedbackNotice}
                    >
                      {message()}
                    </div>
                  )}
                </Show>
              </div>
            </div>
          </Match>
          <Match when={activeTab() === 'sources'}>
            <SourcesPanel grounding={latestGrounding()} sources={evidenceSources()} />
          </Match>
          <Match when={activeTab() === 'artifacts'}>
            <ArtifactsPanel items={artifactItems()} />
          </Match>
          <Match when={activeTab() === 'steps'}>
            <StepsPanel steps={state.taskSteps} screen={latestScreen()} onStopTask={handleStop} />
          </Match>
        </Switch>

        <Show when={hasMessages() && activeTab() === 'chat'}>
          <div class="velion-chat-composer-dock">
            <Show when={showScrollDown()}>
              <button
                type="button"
                class="velion-chat-scroll-down"
                aria-label="Scroll to bottom"
                onClick={() => scrollToBottom()}
              >
                <ArrowDown size={16} />
              </button>
            </Show>
            <div class="velion-chat-composer-dock__inner">
              <Show when={isStreaming()}>
                <div class="velion-chat-stop-wrap">
                  <button type="button" class="velion-chat-stop-btn" onClick={handleStop}>
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
    </div>
  )
}
