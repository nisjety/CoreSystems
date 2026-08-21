import { Bot, MessageCircle, Sparkles } from '@/shared/icons'
import { createMemo, For, Show } from 'solid-js'

import { ChatMarkdown } from '@/features/chat/components/ChatMessages'
import {
  FAILED_RUN_STATUSES,
  formatWhen,
  stripMarkdownPreview,
  threadStatus,
} from '@/features/spaces/lib/space-thread-presentation'
import { getChatThreadTranscript } from '@/shared/api/chat-client'
import type { SpaceAgent, SpaceRosterMember, SpaceThread } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'

/**
 * The room's shared record, buzz-style: every Space thread renders as a post
 * with its actual turns inline (the MS Teams channel model — a channel is a
 * list of expanded thread-posts), oldest at the top, the composer below.
 * Nothing here links a room conversation out to `/chat`; the room is the
 * workspace.
 *
 * Turn attribution is deliberately conservative. Transcript reads are
 * owner-bound at the gateway (`get_thread_transcript` only resolves threads in
 * the caller's own durable list), so every `user` turn a member can see is
 * their own — labeling those with the single human roster member's name is
 * exact today. When Control grows shared-thread transcript authority for
 * multi-member rooms, turns need real server-side author attribution before
 * this component may claim anyone else's words.
 */

type TranscriptTurn = {
  readonly role: 'user' | 'assistant'
  readonly content: string
  /** The persona this turn answered as, recorded at the time of the turn by
   * session-core. Absent on turns older than the attribution field and on
   * un-personified assistant turns. */
  readonly agentName?: string
}

const MAX_TURNS_PER_POST = 30

export function normalizeTranscriptTurns(turns: readonly unknown[] | undefined): TranscriptTurn[] {
  if (!turns) return []
  const normalized: TranscriptTurn[] = []
  for (const turn of turns) {
    if (!turn || typeof turn !== 'object') continue
    const record = turn as Record<string, unknown>
    const role = record.role
    const content = typeof record.content === 'string' ? record.content.trim() : ''
    if ((role !== 'user' && role !== 'assistant') || !content) continue
    const agentName = typeof record.agentName === 'string' && record.agentName.trim()
      ? record.agentName.trim()
      : undefined
    normalized.push({ role, content, ...(agentName ? { agentName } : {}) })
    if (normalized.length >= MAX_TURNS_PER_POST) break
  }
  return normalized
}

export interface SpaceRoomTimelineProps {
  readonly spaceName: string
  readonly threads: () => readonly SpaceThread[]
  readonly roster: () => readonly SpaceRosterMember[]
  readonly agents: () => readonly SpaceAgent[]
  /** Focuses the room composer — the intro block's primary action. */
  readonly onStartConversation?: () => void
  /** When provided, the intro block offers the in-room create-agent flow. */
  readonly onCreateAgent?: () => void
  /** When provided, every post offers "Reply" — the composer then continues
   * that thread through the append authority instead of starting a new one. */
  readonly onReply?: (thread: SpaceThread) => void
}

export function SpaceRoomTimeline(props: SpaceRoomTimelineProps) {
  const i18n = useI18n()

  const orderedThreads = createMemo(() =>
    [...props.threads()].sort((a, b) =>
      (a.updated_at ?? a.latest_run_updated_at ?? '').localeCompare(b.updated_at ?? b.latest_run_updated_at ?? ''),
    ),
  )

  const humanName = createMemo(() => {
    const humans = props.roster().filter((member) => member.subject_type === 'user')
    return humans.length === 1 ? humans[0]?.display_name?.trim() || i18n.tr('Du', 'You') : i18n.tr('Du', 'You')
  })

  const agentName = createMemo(() => {
    const active = props.agents().filter((agent) => agent.status === 'active' && agent.name?.trim())
    return active.length === 1 ? (active[0]?.name as string) : 'Verevon'
  })

  return (
    <Show
      when={orderedThreads().length > 0}
      fallback={
        <SpaceRoomIntro
          spaceName={props.spaceName}
          onStartConversation={props.onStartConversation}
          onCreateAgent={props.onCreateAgent}
        />
      }
    >
      <ol class="verevon-room-timeline" aria-label={i18n.tr('Samtaler i rommet', 'Room conversations')}>
        <For each={orderedThreads()}>
          {(thread) => (
            <SpaceRoomPost
              thread={thread}
              humanName={humanName()}
              agentName={agentName()}
              onReply={props.onReply}
            />
          )}
        </For>
      </ol>
    </Show>
  )
}

function SpaceRoomPost(props: {
  readonly thread: SpaceThread
  readonly humanName: string
  readonly agentName: string
  readonly onReply?: (thread: SpaceThread) => void
}) {
  const i18n = useI18n()
  const [transcript] = createResource(
    // The revision suffix keeps a settled reply visible: refetching the thread
    // list bumps updated_at, which re-keys this source and refetches the
    // transcript for exactly the post that changed. NUL-joined because thread
    // ids may themselves contain any printable character, including spaces.
    () => `${props.thread.thread_id}\u0000${props.thread.updated_at ?? props.thread.latest_run_updated_at ?? ''}`,
    async (key) => {
      const threadId = key.split('\u0000')[0] ?? ''
      const snapshot = await getChatThreadTranscript(threadId).catch(() => null)
      return normalizeTranscriptTurns(snapshot?.turns)
    },
  )
  const turns = () => (transcript.error ? [] : transcript() ?? [])
  const isFailed = () => FAILED_RUN_STATUSES.has(props.thread.latest_run_status ?? '')
  const when = () => props.thread.updated_at ?? props.thread.latest_run_updated_at

  return (
    <li class="verevon-room-post" data-failed={isFailed() || undefined}>
      <Show
        when={turns().length > 0}
        fallback={
          <div class="verevon-room-turn">
            <span class="verevon-room-turn__avatar" aria-hidden="true">
              <MessageCircle size={14} />
            </span>
            <div class="verevon-room-turn__content">
              <PostTurnHeader author={props.humanName} />
              <p class="verevon-room-turn__plain">
                {(props.thread.preview && stripMarkdownPreview(props.thread.preview))
                  || props.thread.title?.trim()
                  || (transcript.loading
                    ? i18n.tr('Henter samtalen …', 'Loading the conversation…')
                    : i18n.tr('Samtalen har ikke noe lesbart innhold ennå.', 'This conversation has no readable content yet.'))}
              </p>
            </div>
          </div>
        }
      >
        <For each={turns()}>
          {(turn) => (
            <div class="verevon-room-turn" data-role={turn.role}>
              <span
                class={[
                  'verevon-room-turn__avatar',
                  { 'verevon-room-turn__avatar--agent': turn.role === 'assistant' },
                ]}
                aria-hidden="true"
              >
                <Show when={turn.role === 'assistant'} fallback={initials(props.humanName)}>
                  <Sparkles size={14} />
                </Show>
              </span>
              <div class="verevon-room-turn__content">
                <PostTurnHeader
                  // The turn's own recorded persona wins; the room-level
                  // heuristic (single active agent, else 'Verevon') covers
                  // turns persisted before attribution existed.
                  author={turn.role === 'assistant' ? turn.agentName ?? props.agentName : props.humanName}
                  isAgent={turn.role === 'assistant'}
                />
                <Show
                  when={turn.role === 'assistant'}
                  fallback={<p class="verevon-room-turn__plain">{turn.content}</p>}
                >
                  <div class="verevon-room-turn__markdown">
                    <ChatMarkdown content={turn.content} />
                  </div>
                </Show>
              </div>
            </div>
          )}
        </For>
      </Show>

      <div class="verevon-room-post__meta">
        <span
          class={[
            {
              'verevon-space-status': true,
              'verevon-space-status--failed': isFailed(),
            },
          ]}
        >
          {threadStatus(props.thread, i18n.tr)}
        </span>
        <Show when={when()}>
          {(at) => <time>{formatWhen(at())}</time>}
        </Show>
        <Show when={props.onReply}>
          {(reply) => (
            <button type="button" class="verevon-room-post__reply" onClick={() => reply()(props.thread)}>
              {i18n.tr('Svar', 'Reply')}
            </button>
          )}
        </Show>
      </div>
    </li>
  )
}

function PostTurnHeader(props: { readonly author: string; readonly isAgent?: boolean }) {
  const i18n = useI18n()
  return (
    <div class="verevon-room-turn__header">
      <strong class="verevon-room-turn__author">{props.author}</strong>
      <Show when={props.isAgent}>
        <span class="verevon-room-turn__agent-chip">
          <Bot size={12} aria-hidden="true" />
          {i18n.tr('Agent', 'Agent')}
        </span>
      </Show>
    </div>
  )
}

function SpaceRoomIntro(props: {
  readonly spaceName: string
  readonly onStartConversation?: () => void
  readonly onCreateAgent?: () => void
}) {
  const i18n = useI18n()
  return (
    <div class="verevon-room-intro">
      <span class="verevon-room-intro__mark" aria-hidden="true">
        <MessageCircle size={26} />
      </span>
      <h3>{props.spaceName}</h3>
      <p>
        {i18n.tr('Dette er begynnelsen på det delte arkivet for', 'This is the beginning of the shared record for')}{' '}
        <strong>{props.spaceName}</strong>.
      </p>
      <div class="verevon-room-intro__actions">
        <Show when={props.onStartConversation}>
          {(start) => (
            <button type="button" class="verevon-room-intro__action" onClick={() => start()()}>
              <span class="verevon-room-intro__action-icon" aria-hidden="true">
                <MessageCircle size={16} />
              </span>
              <span>
                <strong>{i18n.tr('Start en samtale', 'Start a conversation')}</strong>
                <small>{i18n.tr('Skriv til rommet, eller nevn en agent med @.', 'Write to the room, or mention an agent with @.')}</small>
              </span>
            </button>
          )}
        </Show>
        <Show when={props.onCreateAgent}>
          {(create) => (
            <button type="button" class="verevon-room-intro__action" onClick={() => create()()}>
              <span class="verevon-room-intro__action-icon" aria-hidden="true">
                <Bot size={16} />
              </span>
              <span>
                <strong>{i18n.tr('Opprett en agent', 'Create an agent')}</strong>
                <small>{i18n.tr('Gi rommet en kollega som kan svare når den nevnes.', 'Give the room a teammate that answers when mentioned.')}</small>
              </span>
            </button>
          )}
        </Show>
      </div>
    </div>
  )
}

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean)
  const first = parts[0]?.[0] ?? ''
  const last = parts.length > 1 ? parts[parts.length - 1]?.[0] ?? '' : ''
  return (first + last).toLocaleUpperCase() || '?'
}
