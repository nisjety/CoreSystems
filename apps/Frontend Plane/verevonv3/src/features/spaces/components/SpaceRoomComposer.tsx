import { createMemo, createSignal, For, Show } from 'solid-js'

import { ChatMarkdown } from '@/features/chat/components/ChatMessages'
import { cancelInvocation, streamChat } from '@/shared/api/chat-client'
import { listAvailableSkills, MAX_PICKED_SKILLS, type Skill } from '@/shared/api/skills-client'
import { WandSparkles, X } from '@/shared/icons'
import {
  updateSpaceThreadPresentation,
  type SpaceAgent,
  type SpaceRosterMember,
  type SpaceThread,
} from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { translateApiError } from '@/shared/i18n/errors'
import { beginOwnStream, endOwnStream, liveThreadsIn, ownStream } from '../lib/space-live-work'
import { threadTitle } from '../lib/space-thread-presentation'

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
}

/**
 * One row of the composer's picker, whichever trigger opened it. `@` and `/`
 * share one list, one highlighted row and one set of keys (arrows, Enter, Tab,
 * Escape), so the two never behave differently — the mention picker used to
 * be mouse-only.
 */
type PickerItem =
  | { readonly kind: 'mention'; readonly candidate: MentionCandidate }
  | { readonly kind: 'skill'; readonly skill: Skill }

type Exchange = {
  readonly userContent: string
  assistantContent: string
  /**
   * `stopped` is its own state, not an error and not done. A server-side stop
   * used to reach `onDone` and render a halted answer as a finished one; the
   * transport now emits `stopped` separately, and this composer keeps the
   * distinction all the way to the screen.
   */
  status: 'streaming' | 'done' | 'error' | 'stopped'
  errorMessage?: string
  /**
   * The gateway's error code, kept beside the message so a refusal the room
   * can act on — the spend hard stop — renders as what it is, with the way
   * out, rather than as a generic "the reply stopped".
   */
  errorCode?: string
  /** Whether the stop was recorded server-side, or only cut the local read. */
  stopRecorded?: boolean
}

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
}

export function SpaceRoomComposer(props: SpaceRoomComposerProps) {
  const i18n = useI18n()
  const [text, setText] = createSignal('')
  const [mentionQuery, setMentionQuery] = createSignal<string | undefined>(undefined)
  const [mentionedAgentRef, setMentionedAgentRef] = createSignal<string | undefined>(undefined)
  // `/` skills (later tier). The catalogue is fetched on the first `/`, never on
  // mount: a room that never picks a skill never pays for the list. `undefined`
  // means not loaded yet; a failed load is its own state so the picker can say
  // so instead of showing an empty list that looks like "you have none".
  const [skillQuery, setSkillQuery] = createSignal<string | undefined>(undefined)
  const [skills, setSkills] = createSignal<readonly Skill[] | undefined>(undefined)
  const [skillsFailed, setSkillsFailed] = createSignal(false)
  const [pickedSkills, setPickedSkills] = createSignal<readonly Skill[]>([])
  const [activeIndex, setActiveIndex] = createSignal(0)
  let skillsLoading = false
  const pickerId = `room-picker-${Math.random().toString(36).slice(2, 8)}`
  const [submitting, setSubmitting] = createSignal(false)
  const [exchange, setExchange] = createSignal<Exchange | undefined>(undefined)
  let textareaRef: HTMLTextAreaElement | undefined
  // The in-flight turn's transport handles. `requestId` arrives on the
  // `connected` frame and is the only thing the cancel endpoint accepts.
  let abortController: AbortController | undefined
  let liveRequestId: string | undefined
  // The thread the in-flight turn is in, from the `connected` frame — the id a
  // new post gets is minted server-side, so it is only known once the stream
  // announces it.
  let liveThreadId: string | undefined

  // Threads the server currently reports as working in this room, minus the
  // one this browser is streaming itself (that one has its own indicator in
  // the exchange block). What remains is other members' agent work.
  const othersWorking = () => {
    const own = ownStream()
    return liveThreadsIn(props.spaceRef).filter(
      (thread) => !(own && own.spaceRef === props.spaceRef && own.threadId === thread.thread_id),
    )
  }
  props.registerFocusHandle?.(() => textareaRef?.focus())

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
      return visibleCandidates().map((candidate) => ({ kind: 'mention' as const, candidate }))
    }
    if (skillQuery() !== undefined) {
      return visibleSkills().map((skill) => ({ kind: 'skill' as const, skill }))
    }
    return []
  })
  // The `/` box also opens on zero matches, to say why (loading, failed, none).
  const pickerOpen = () => pickerItems().length > 0 || skillQuery() !== undefined
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

  function pickItem(item: PickerItem): void {
    if (item.kind === 'mention') selectCandidate(item.candidate)
    else selectSkill(item.skill)
  }

  function handleKeyDown(event: KeyboardEvent & { currentTarget: HTMLTextAreaElement }): void {
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
  const blockedByApproval = () => props.replyTarget?.()?.awaitingApproval === true

  async function submitMessage(): Promise<void> {
    const content = text().trim()
    if (!content || submitting() || blockedByApproval()) return

    setSubmitting(true)
    closePicker()
    const agentRef = mentionedAgentRef()
    const picked = pickedSkills()
    const replyThreadId = props.replyTarget?.()?.threadId
    setExchange({ userContent: content, assistantContent: '', status: 'streaming' })
    setText('')
    setMentionedAgentRef(undefined)
    setPickedSkills([])
    const controller = new AbortController()
    abortController = controller
    liveRequestId = undefined
    liveThreadId = replyThreadId
    const openedNewThread = !replyThreadId
    beginOwnStream({ spaceRef: props.spaceRef, threadId: replyThreadId })

    try {
      await streamChat(
        {
          content,
          spaceRef: props.spaceRef,
          mentionedAgentRef: agentRef,
          threadId: replyThreadId,
          // Picked skills ride as actions; chat-client puts their ids on the
          // wire as `skill_ids`, never as tool specs.
          actions: picked.length > 0
            ? picked.map((skill) => ({ id: skill.id, name: skill.name, kind: 'skill' as const }))
            : undefined,
        },
        {
          onConnected: (event) => {
            if (event.requestId?.trim()) liveRequestId = event.requestId.trim()
            if (event.threadId?.trim()) liveThreadId = event.threadId.trim()
          },
          onTitle: ({ title }) => {
            // Only for a thread this browser opened: the title event fires on a
            // first exchange, and only the owner may write presentation. Writing
            // it back is what makes the room's other members see the same name
            // on their next poll instead of the raw first message.
            const name = title?.trim()
            const threadId = liveThreadId
            if (!openedNewThread || !name || !threadId) return
            const settled = props.onExchangeSettled
            void updateSpaceThreadPresentation(props.spaceRef, threadId, { title: name })
              .then(() => settled?.())
              .catch(() => undefined)
          },
          onMessage: ({ content: delta }) => {
            setExchange((current) => current && { ...current, assistantContent: current.assistantContent + delta })
          },
          onDone: () => {
            // A stop that already landed must not be overwritten by a trailing
            // `done`: the server emits `stopped` and may still close the
            // stream normally afterwards.
            setExchange((current) =>
              current && current.status === 'streaming' ? { ...current, status: 'done' } : current,
            )
            props.onClearReplyTarget?.()
            props.onExchangeSettled?.()
          },
          onStopped: () => {
            setExchange((current) =>
              current && { ...current, status: 'stopped', stopRecorded: true },
            )
          },
          onError: (event) => {
            setExchange((current) => current && {
              ...current,
              status: 'error',
              errorCode: event.code,
              // Known codes get the room's own words; an unknown one keeps the
              // server's message rather than a generic line that hides it.
              errorMessage: translateApiError({ code: event.code }, i18n.tr, {
                no: event.message,
                en: event.message,
              }),
            })
          },
        },
        controller.signal,
      )
    } catch (error) {
      // An abort we caused is not a failure. Anything else still is.
      if (!(error instanceof DOMException && error.name === 'AbortError') && !controller.signal.aborted) {
        setExchange((current) => current && current.status === 'streaming'
          ? { ...current, status: 'error', errorMessage: error instanceof Error ? error.message : String(error) }
          : current)
      }
    } finally {
      if (abortController === controller) abortController = undefined
      endOwnStream()
      setSubmitting(false)
    }
  }

  /**
   * Stop the turn in flight.
   *
   * Aborting the read is instant and local. Cancelling by `requestId` is what
   * records the stop: it flips the run to `cancelled` in Session Core, which is
   * what every other member's projection shows on its next poll. Without a
   * `requestId` there is nothing to cancel with, and the exchange says so
   * rather than claiming a stop it cannot prove.
   */
  async function stopMessage(): Promise<void> {
    const controller = abortController
    const requestId = liveRequestId
    if (!controller) return
    controller.abort()
    setExchange((current) =>
      current && current.status === 'streaming'
        ? { ...current, status: 'stopped', stopRecorded: false }
        : current,
    )
    if (!requestId) return
    try {
      await cancelInvocation(requestId)
      setExchange((current) => current && current.status === 'stopped' ? { ...current, stopRecorded: true } : current)
    } catch {
      // Recorded state stays false: the run may well finish server-side, and
      // saying "stopped" without proof is the one thing this must not do.
    } finally {
      // Re-read the projection either way, so the post's status follows the
      // server rather than this browser's belief.
      props.onClearReplyTarget?.()
      props.onExchangeSettled?.()
    }
  }

  return (
    <div class="verevon-space-room-composer">
      <Show when={exchange()}>
        {(current) => (
          <div class="verevon-space-room-exchange" role="log" aria-live="polite">
            <p class="verevon-space-room-exchange__user">
              <span class="verevon-space-room-exchange__speaker">{i18n.tr('Du', 'You')}</span>
              {current().userContent}
            </p>
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
                {current().stopRecorded
                  ? i18n.tr('Stoppet av deg. Rommet ser dette som stoppet.', 'Stopped by you. The room records it as stopped.')
                  : liveRequestId
                    ? i18n.tr('Stoppet her. Registrerer stoppet …', 'Stopped here. Recording the stop…')
                    : i18n.tr(
                        'Avbrutt før svaret startet. Kjøringen kan fortsatt fullføre på tjenersiden.',
                        'Cut off before the reply started. The run may still finish server-side.',
                      )}
              </p>
            </Show>
            <Show when={current().status === 'streaming'}>
              <button
                type="button"
                class="verevon-space-room-exchange__stop"
                onClick={() => { void stopMessage() }}
              >
                {i18n.tr('Stopp', 'Stop')}
              </button>
            </Show>
          </div>
        )}
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
      <Show when={othersWorking().length > 0}>
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

      <form onSubmit={(event) => { event.preventDefault(); void submitMessage() }}>
        <Show when={props.replyTarget?.()}>
          {(target) => (
            <p class="verevon-space-room-composer__reply">
              {i18n.tr('Svarer i', 'Replying in')} «{target().title}»
              <button
                type="button"
                onClick={() => props.onClearReplyTarget?.()}
                aria-label={i18n.tr('Avbryt svar', 'Cancel reply')}
              >
                ✕
              </button>
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
        <Show when={pickedSkills().length > 0}>
          <ul class="verevon-space-room-skills" aria-label={i18n.tr('Valgte ferdigheter', 'Chosen skills')}>
            <For each={pickedSkills()}>
              {(skill) => (
                <li class="verevon-space-room-skill-chip">
                  <WandSparkles class="verevon-space-room-skill-chip__icon" aria-hidden="true" />
                  <span>{skill.name}</span>
                  <SkillScope scope={skill.scope} />
                  <button
                    type="button"
                    class="verevon-space-room-skill-chip__remove"
                    onClick={() => removeSkill(skill.id)}
                    aria-label={i18n.tr(`Fjern ferdigheten ${skill.name}`, `Remove the skill ${skill.name}`)}
                  >
                    <X aria-hidden="true" />
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
        <Show when={pickerOpen()}>
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
                      when={item.kind === 'skill' ? item.skill : undefined}
                      fallback={
                        <>
                          <span class="verevon-space-room-mentions__kind" aria-hidden="true">
                            {item.kind === 'mention' && item.candidate.kind === 'agent' ? '●' : '·'}
                          </span>
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
        <textarea
          ref={textareaRef}
          value={text()}
          onInput={handleInput}
          onKeyDown={handleKeyDown}
          aria-controls={pickerOpen() ? pickerId : undefined}
          aria-activedescendant={pickerItems().length > 0 ? `${pickerId}-${activeIndex()}` : undefined}
          placeholder={
            blockedByApproval()
              ? i18n.tr(
                  'Denne samtalen venter på en godkjenning.',
                  'This conversation is waiting on an approval.',
                )
              : i18n.tr(
                  'Skriv i rommet. @ nevner noen, / velger en ferdighet.',
                  'Write in the room. @ mentions someone, / picks a skill.',
                )
          }
          rows={2}
          disabled={submitting() || blockedByApproval()}
        />
        <Show when={mentionedAgentRef()}>
          <p class="verevon-space-room-composer__hint">
            {i18n.tr('Vil invokere en agent når du sender.', 'Will invoke an agent when you send.')}
          </p>
        </Show>
        <Show when={pickedSkills().length > 0}>
          <p class="verevon-space-room-composer__hint">
            {i18n.tr(
              'Valgte ferdigheter legges til som veiledning når du sender.',
              'Chosen skills are added as guidance when you send.',
            )}
          </p>
        </Show>
        <button type="submit" disabled={submitting() || blockedByApproval() || !text().trim()}>
          {i18n.tr('Send', 'Send')}
        </button>
      </form>
    </div>
  )
}

/**
 * The scope a skill actually has. Two values exist in the registry — `org`
 * and `user` — and both are stated; anything else is left unbadged rather than
 * guessed. There is deliberately no "this room": the registry has no Space
 * scope to promise.
 */
function SkillScope(props: { readonly scope: Skill['scope'] }) {
  const i18n = useI18n()
  const label = () =>
    props.scope === 'user'
      ? i18n.tr('Personlig', 'Personal')
      : props.scope === 'org'
        ? i18n.tr('Organisasjon', 'Organization')
        : undefined
  return (
    <Show when={label()}>
      {(text) => (
        <span class="verevon-space-room-skill-scope" data-scope={props.scope}>
          {text()}
        </span>
      )}
    </Show>
  )
}
