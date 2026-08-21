import { createMemo, createSignal, For, Show } from 'solid-js'

import { ChatMarkdown } from '@/features/chat/components/ChatMessages'
import { streamChat } from '@/shared/api/chat-client'
import type { SpaceAgent, SpaceRosterMember } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'

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
 */

type MentionCandidate = {
  readonly id: string
  readonly label: string
  readonly kind: 'person' | 'agent'
  /** Only present for `kind: 'agent'` — the Control subject id to invoke. */
  readonly subjectId?: string
}

type Exchange = {
  readonly userContent: string
  assistantContent: string
  status: 'streaming' | 'done' | 'error'
  errorMessage?: string
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
  readonly replyTarget?: () => { threadId: string; title: string } | undefined
  readonly onClearReplyTarget?: () => void
}

export function SpaceRoomComposer(props: SpaceRoomComposerProps) {
  const i18n = useI18n()
  const [text, setText] = createSignal('')
  const [mentionQuery, setMentionQuery] = createSignal<string | undefined>(undefined)
  const [mentionedAgentRef, setMentionedAgentRef] = createSignal<string | undefined>(undefined)
  const [submitting, setSubmitting] = createSignal(false)
  const [exchange, setExchange] = createSignal<Exchange | undefined>(undefined)
  let textareaRef: HTMLTextAreaElement | undefined
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

  function detectMention(value: string, cursor: number): void {
    const before = value.slice(0, cursor)
    const match = before.match(/(?:^|\s)@([^\s@]*)$/)
    setMentionQuery(match ? match[1] : undefined)
  }

  function handleInput(event: InputEvent & { currentTarget: HTMLTextAreaElement }): void {
    const value = event.currentTarget.value
    setText(value)
    // A mentioned agent's ref only stays valid while its name is still in the
    // text — editing the name back out un-targets it rather than silently
    // invoking whoever was last picked.
    const agentLabel = candidates().find((c) => c.subjectId === mentionedAgentRef())?.label
    if (agentLabel && !value.includes(`@${agentLabel}`)) setMentionedAgentRef(undefined)
    detectMention(value, event.currentTarget.selectionStart ?? value.length)
  }

  function selectCandidate(candidate: MentionCandidate): void {
    const value = text()
    const cursor = textareaRef?.selectionStart ?? value.length
    const before = value.slice(0, cursor)
    const after = value.slice(cursor)
    const replaced = before.replace(/(?:^|\s)@([^\s@]*)$/, (matched) =>
      (matched.startsWith(' ') ? ' ' : '') + `@${candidate.label} `)
    setText(replaced + after)
    setMentionQuery(undefined)
    if (candidate.kind === 'agent') setMentionedAgentRef(candidate.subjectId)
    textareaRef?.focus()
  }

  async function submitMessage(): Promise<void> {
    const content = text().trim()
    if (!content || submitting()) return

    setSubmitting(true)
    setMentionQuery(undefined)
    const agentRef = mentionedAgentRef()
    const replyThreadId = props.replyTarget?.()?.threadId
    setExchange({ userContent: content, assistantContent: '', status: 'streaming' })
    setText('')
    setMentionedAgentRef(undefined)

    try {
      await streamChat(
        { content, spaceRef: props.spaceRef, mentionedAgentRef: agentRef, threadId: replyThreadId },
        {
          onMessage: ({ content: delta }) => {
            setExchange((current) => current && { ...current, assistantContent: current.assistantContent + delta })
          },
          onDone: () => {
            setExchange((current) => current && { ...current, status: 'done' })
            props.onClearReplyTarget?.()
            props.onExchangeSettled?.()
          },
          onError: (event) => {
            setExchange((current) => current && {
              ...current,
              status: 'error',
              errorMessage: event.message,
            })
          },
        },
      )
    } finally {
      setSubmitting(false)
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
            <Show when={current().status === 'error'}>
              <p class="verevon-space-projection-error" role="alert">
                {i18n.tr(
                  'Svaret stoppet før det var ferdig. ',
                  'The reply stopped before it finished. ',
                )}
                {current().errorMessage}
              </p>
            </Show>
          </div>
        )}
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
        <Show when={visibleCandidates().length > 0}>
          <ul class="verevon-space-room-mentions" role="listbox" aria-label={i18n.tr('Nevn noen', 'Mention someone')}>
            <For each={visibleCandidates()}>
              {(candidate) => (
                <li>
                  <button type="button" role="option" onClick={() => selectCandidate(candidate)}>
                    <span class="verevon-space-room-mentions__kind" aria-hidden="true">
                      {candidate.kind === 'agent' ? '●' : '·'}
                    </span>
                    {candidate.label}
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
        <textarea
          ref={textareaRef}
          value={text()}
          onInput={handleInput}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && !event.shiftKey && visibleCandidates().length === 0) {
              event.preventDefault()
              void submitMessage()
            }
          }}
          placeholder={i18n.tr(
            'Skriv i rommet. Skriv @ for å nevne noen.',
            'Write in the room. Type @ to mention someone.',
          )}
          rows={2}
          disabled={submitting()}
        />
        <Show when={mentionedAgentRef()}>
          <p class="verevon-space-room-composer__hint">
            {i18n.tr('Vil invokere en agent når du sender.', 'Will invoke an agent when you send.')}
          </p>
        </Show>
        <button type="submit" disabled={submitting() || !text().trim()}>
          {i18n.tr('Send', 'Send')}
        </button>
      </form>
    </div>
  )
}
