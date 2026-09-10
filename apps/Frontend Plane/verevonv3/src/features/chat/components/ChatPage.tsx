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
import { createResource } from '@/shared/lib/create-resource-compat'
import { createElementHeight } from '@/shared/ui/verevon/createElementHeight'
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
import { chatSurfaceClaimsFocus, isChatSurfaceAvailable, type ChatSurfaceAvailability } from '../lib/chat-surfaces'
import { isWorkStep } from './chat-normalizers'
import type { ChatTab } from './chat-types'
import { useI18n } from '@/shared/i18n'
import { pinnedMessagesFull } from '../lib/chat-pinned-messages'
import { selectChatThread } from '../lib/chat-thread-history'

export default function ChatPage() {
  const i18n = useI18n()
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
    pinnedMessages,
    togglePin,
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
  createEffect(
    () => {
      const streaming = isStreaming()
      const completedAnswer = streaming
        ? ''
        : [...state.turns]
            .reverse()
            .find((turn) => turn.role === 'assistant')
            ?.content?.trim() ?? ''
      return { completedAnswer, streaming }
    },
    ({ completedAnswer, streaming }) => {
      if (!streaming) {
        // The answer itself is in the transcript to navigate; this is the cue
        // that it is there, plus enough of it to know whether it is worth reading.
        setStreamAnnouncement(completedAnswer ? `Svar fullført. ${completedAnswer.slice(0, 180)}` : '')
        return
      }
      setStreamAnnouncement('Verevon svarer …')
      // Ten seconds: frequent enough that a long run does not read as dead, rare
      // enough not to be a metronome. The text alternates because a live region
      // drops a repeat of the string it is already showing.
      let tick = 0
      const heartbeat = setInterval(() => {
        tick += 1
        setStreamAnnouncement(tick % 2 === 1 ? 'Arbeider fortsatt …' : 'Fortsatt underveis …')
      }, 10_000)
      // Solid 2 runs a cleanup RETURNED from the effect fn; `onCleanup` inside
      // one is silently dropped.
      return () => clearInterval(heartbeat)
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
  // Evaluated once per dependency change, not once per prop read. Passed as an
  // inline `cond ? contextualPanel() : undefined` these became getters, and the
  // run panel reads `props.workContent` eight times (class, aria-label, title,
  // onClick, two <Show>s, the render slot). Every read instantiated a fresh
  // Work panel tree; only one reached the DOM, the other seven lived on as
  // orphans whose plan resources kept fetching -- seven identical /plans
  // requests per refresh, measured live. A memo hands every read the same tree.
  const liveRailWorkContent = createMemo(() => (workCanvasActive() ? contextualPanel() : undefined))
  const liveRailNavigation = createMemo(() => (workCanvasActive() ? workspaceNavigation() : undefined))

  /**
   * Arrow keys move between messages (audit item 26 / plan item 14).
   *
   * Scoped hard: it acts only on a bare ArrowUp/ArrowDown whose target is not
   * a control that owns arrows itself. The composer textarea, the model menu,
   * the workspace tab strip's roving tabindex and the canvas resize handle all
   * bind arrows, and stealing them here would break each one.
   */
  const handleTranscriptArrowKeys = (event: KeyboardEvent & { currentTarget: HTMLDivElement }) => {
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return
    if (event.defaultPrevented) return
    const target = event.target as HTMLElement | null
    if (!target) return
    if (target.closest('input, textarea, select, [contenteditable="true"], [role="tablist"], [role="listbox"], [role="menu"], [role="separator"]')) {
      return
    }
    const list = event.currentTarget
    const messages = Array.from(list.querySelectorAll<HTMLElement>('.verevon-chat-message'))
    if (messages.length === 0) return
    const current = target.closest<HTMLElement>('.verevon-chat-message')
    const index = current ? messages.indexOf(current) : -1
    // Entering the transcript from elsewhere lands on the newest message going
    // up, and the oldest going down, rather than jumping to an arbitrary end.
    const next = index < 0
      ? (event.key === 'ArrowUp' ? messages.length - 1 : 0)
      : Math.min(messages.length - 1, Math.max(0, index + (event.key === 'ArrowDown' ? 1 : -1)))
    if (next === index) return
    event.preventDefault()
    messages[next]?.focus()
  }

  const workspaceNavigation = () => (
    <ChatTabs
      active={activeTab()}
      artifactCount={artifactItems().length + conversationAttachments().length}
      includeChat={false}
      runAvailable={Boolean(liveRunId())}
      sourceCount={evidenceSources().length + (latestGrounding() ? 1 : 0)}
      stepCount={state.taskSteps.length}
      workStepCount={workStepCount()}
      toolCallCount={toolCallCount()}
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
            sourceCount={evidenceSources().length + (latestGrounding() ? 1 : 0)}
            stepCount={state.taskSteps.length}
            workStepCount={workStepCount()}
            toolCallCount={toolCallCount()}
            title={title()}
            traceAvailable={Boolean(liveRunId())}
            onChange={setActiveTab}
            onNewChat={startNewChat}
            onRegenerate={regenerateLatest}
          />
        </Show>

        <Switch>
          <Match when={!hasMessages()}>
            <EmptyChatState
              onSelectPrompt={setInput}
              onResumeThread={(threadId) => selectChatThread(threadId)}
              orgName={session.activeOrg?.name}
              userName={session.user?.name}
            >
              {composer()}
            </EmptyChatState>
          </Match>
          <Match when={hasMessages()}>
            <div
              id="verevon-chat-tabpanel-chat"
              role="tabpanel"
              aria-label="Chat"
              ref={setMessageListRef}
              class="verevon-chat-message-list"
              onScroll={handleScroll}
              onKeyDown={handleTranscriptArrowKeys}
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
                        onFeedback={(rating, note) => submitTurnFeedback(row.turn.id, rating, note)}
                        onApprovalDecision={(approvalId, decision) =>
                          void handleApprovalDecision(row.turn.id, approvalId, decision)
                        }
                        onApprovePlan={(rung, justification) =>
                          void approveTurnPlan(row.turn.id, rung, justification)
                        }
                        pinned={pinnedMessages().includes(row.turn.id)}
                        pinDisabled={pinnedMessagesFull(pinnedMessages())}
                        onTogglePin={() => togglePin(row.turn.id)}
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
                {/* Focus follows the error. `role="alert"` announces it, but a
                    keyboard user was left wherever they were -- the open half of
                    plan item 16. `tabindex=-1` keeps it out of the Tab order. */}
                <Show when={state.error && state.status === 'error'}>
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
        navigation={liveRailNavigation()}
        onCloseWork={() => setActiveTab('chat')}
        workContent={liveRailWorkContent()}
      />
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
