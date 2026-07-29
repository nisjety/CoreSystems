import {
  For,
  Match,
  Show,
  Switch,
} from 'solid-js'
import {
  ArrowDown,
  Square,
} from 'lucide-solid'
import {
  DashboardComposer,
} from '@/features/dashboard/home/DashboardComposer'
import {
  submitFeedback,
} from '@/shared/api/chat-client'
import {
  DateDivider,
  EmptyChatState,
  MessageBlock,
} from './ChatMessages'
import {
  ArtifactsPanel,
  ChatHeader,
  ChatTabs,
  SourcesPanel,
  StepsPanel,
} from './ChatPanels'
import {
  shouldShowDateDivider,
} from './chat-media-markdown'
import { useChatController } from './use-chat-controller'

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
    input,
    setInput,
    setMessageListRef,
  } = useChatController()

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
      planMode={planMode()}
      showTurnReceipt={false}
      submitting={isStreaming()}
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
                        onFeedback={(rating) => {
                          if (turn.requestId) void submitFeedback(turn.requestId, rating).catch(() => undefined)
                        }}
                        onApprovalDecision={(approvalId, decision) =>
                          void handleApprovalDecision(turn.id, approvalId, decision)
                        }
                        onViewSteps={() => setActiveTab('steps')}
                      />
                    </>
                  )}
                </For>
                <Show when={state.error && state.status === 'error'}>
                  <div class="velion-chat-error" role="alert">{state.error}</div>
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
