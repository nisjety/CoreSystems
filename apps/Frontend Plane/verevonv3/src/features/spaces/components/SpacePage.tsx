import { A, useParams } from '@solidjs/router'
import { ArrowUpRight, Bot, Clock3, MessageCircle, Plus, Sparkles, Users } from 'lucide-solid'
import { createResource, createSignal, For, onCleanup, onMount, Show } from 'solid-js'

import {
  getPersonalSpaceDeletionReceipt,
  getSpaceContext,
  getSpaceRoster,
  getSpaceThreads,
  requestPersonalSpaceDeletion,
  type SpaceDeletionReceipt,
  type SpaceRosterMember,
  type SpaceThread,
} from '@/shared/api/spaces-client'
import { SpaceActivityFeed } from './SpaceActivityFeed'
import { SpaceCockpit } from './SpaceCockpit'

// Membership is authoritative only at the server. Revalidate while the Space
// is open so a removal/revocation cannot leave an old resolved value usable in
// the cockpit between navigations.
const SPACE_CONTEXT_RECHECK_MS = 30_000

const ACTIVE_RUN_STATUSES = new Set(['queued', 'running', 'awaiting_approval'])

/**
 * The Space is a workroom, not a second application shell. It composes the
 * existing server-authoritative context and thread projections into the
 * presentational cockpit without asking the BFF to invent a composite grant.
 */
export default function SpacePage() {
  const params = useParams<{ spaceId: string }>()
  const spaceRef = () => params.spaceId?.trim() ?? ''
  const [context, { refetch: refetchContext }] = createResource(spaceRef, getSpaceContext)
  const [threadsUnavailable, setThreadsUnavailable] = createSignal(false)
  let latestThreadRequest = 0
  const [threads, { refetch: refetchThreads }] = createResource(spaceRef, async (ref) => {
    const request = ++latestThreadRequest
    setThreadsUnavailable(false)
    try {
      const projection = await getSpaceThreads(ref)
      if (request === latestThreadRequest) setThreadsUnavailable(false)
      return projection
    } catch {
      if (request === latestThreadRequest) setThreadsUnavailable(true)
      return undefined
    }
  })
  // `undefined`, not '': Solid skips a fetch only for false/null/undefined, and
  // an empty string is none of those — so the receipt resource fired on mount
  // and requested `/spaces/deletion-requests/` with no id, producing a 404 on
  // every page load for a request nobody had made.
  const [deletionRequestId, setDeletionRequestId] = createSignal<string | undefined>(undefined)
  const [deletionReceipt] = createResource(deletionRequestId, getPersonalSpaceDeletionReceipt)
  const [deletionError, setDeletionError] = createSignal('')
  const [deletionSubmitting, setDeletionSubmitting] = createSignal(false)
  const currentThreads = () => threads()?.threads ?? []
  const activeRun = () => currentThreads().find((thread) => ACTIVE_RUN_STATUSES.has(thread.latest_run_status ?? ''))

  async function requestDeletion() {
    const selectedSpace = context()?.space
    if (!selectedSpace || selectedSpace.kind !== 'personal' || deletionSubmitting()) return
    if (!window.confirm('Request deletion of this Personal Space? Existing data will be deleted only after Control authorization and owner-plane receipts.')) return
    setDeletionError('')
    setDeletionSubmitting(true)
    try {
      const request = await requestPersonalSpaceDeletion(selectedSpace.space_ref, `space-delete:${crypto.randomUUID()}`)
      setDeletionRequestId(request.requestId)
    } catch {
      setDeletionError('The deletion request could not be recorded. No deletion has been confirmed.')
    } finally {
      setDeletionSubmitting(false)
    }
  }

  onMount(() => {
    const interval = window.setInterval(() => {
      const refreshedContext = refetchContext()
      void Promise.resolve(refreshedContext).then((current) => {
        // A failed authority check leaves the page fail-closed. Only refresh
        // the derived thread projection after a new current context resolves.
        if (current) void refetchThreads()
      })
    }, SPACE_CONTEXT_RECHECK_MS)
    onCleanup(() => window.clearInterval(interval))
  })

  return (
    <section class="space-page verevon-space" aria-labelledby="space-title">
      <Show when={context.loading}>
        <p class="verevon-space-loading" role="status" aria-live="polite">Loading Space…</p>
      </Show>
      <Show when={context.error}>
        <div class="verevon-space-unavailable" role="alert">
          <h1 id="space-title">Space unavailable</h1>
          <p>Your current membership could not be confirmed. No Space actions are available.</p>
        </div>
      </Show>
      <Show when={context.error ? undefined : context()}>
        {(current) => (
          <div class="verevon-space-workroom">
            <main class="verevon-space-canvas">
              <header class="verevon-space-header">
                <div class="verevon-space-header__title">
                  <p class="verevon-space-eyebrow">{spaceWorkroomLabel(current().space.kind)}</p>
                  <h1 id="space-title">{current().space.name}</h1>
                  <div class="verevon-space-meta" aria-label="Current Space status">
                    <span>{formatLabel(current().space.kind)}</span>
                    <span aria-hidden="true">·</span>
                    <span>{formatLabel(current().space.lifecycle)}</span>
                    <span aria-hidden="true">·</span>
                    <span>Your role: {formatLabel(current().membership.role)}</span>
                  </div>
                </div>
              </header>

              <Show when={threadsUnavailable()}>
                <p class="verevon-space-projection-error" role="alert">
                  Space conversation activity is temporarily unavailable. Your confirmed Space access remains unchanged.
                </p>
              </Show>

              <SpaceCockpit
                tabs={{
                  chat: (
                    <SpaceConversationPanel
                      spaceRef={current().space.space_ref}
                      spaceName={current().space.name}
                      threads={currentThreads}
                      loading={() => threads.loading}
                      unavailable={threadsUnavailable}
                    />
                  ),
                  aktivitet: (
                    <SpaceActivityPanel
                      threads={currentThreads}
                      loading={() => threads.loading}
                    />
                  ),
                  agent: <SpaceAgentPanel spaceRef={current().space.space_ref} />,
                  medlemmer: (
                    <SpaceMembersPanel
                      spaceRef={current().space.space_ref}
                      role={current().membership.role}
                      kind={current().space.kind}
                      deletionError={deletionError}
                      deletionReceipt={deletionReceipt}
                      deletionSubmitting={deletionSubmitting}
                      onRequestDeletion={requestDeletion}
                    />
                  ),
                }}
              />
            </main>

            <SpacePulse activeRun={activeRun} threads={currentThreads} />
          </div>
        )}
      </Show>
    </section>
  )
}

function SpaceConversationPanel(props: {
  readonly spaceRef: string
  readonly spaceName: string
  readonly threads: () => readonly SpaceThread[]
  readonly loading: () => boolean
  readonly unavailable: () => boolean
}) {
  return (
    <section class="verevon-space-view verevon-space-view--conversations" aria-labelledby="space-conversations-title">
      <div class="verevon-space-view__heading">
        <div>
        <p class="verevon-space-eyebrow">Space conversation</p>
          <h2 id="space-conversations-title">Samtaler</h2>
          <p>The shared record for people and agent work connected to this Space.</p>
        </div>
        <a class="verevon-space-secondary-action" href={spaceChatHref(props.spaceRef)}>
          <Sparkles size={15} aria-hidden="true" />
          Start a conversation
        </a>
      </div>

      <Show when={props.loading()}>
        <p class="verevon-space-inline-status" role="status">Loading Space conversations…</p>
      </Show>
      <Show when={!props.loading()}>
        <Show
          when={!props.unavailable() && props.threads().length > 0}
          fallback={
            <Show
              when={props.unavailable()}
              fallback={
                <div class="verevon-space-fresh-conversation" aria-labelledby="space-fresh-conversation-title">
                  <span class="verevon-space-fresh-conversation__mark" aria-hidden="true" />
                  <h3 id="space-fresh-conversation-title">Explore bots in Agent Studio</h3>
                  <p>Open Agent Studio to explore the bot blueprints available to your organization.</p>
                  <A
                    class="verevon-space-fresh-conversation__action"
                    href="/agents?agent=chatbot&view=playground"
                    aria-label="Open Agent Studio"
                  >
                    <Bot size={16} aria-hidden="true" />
                    Open Agent Studio
                  </A>
                </div>
              }
            >
              <div class="verevon-space-empty-state" role="status">
                <MessageCircle size={20} aria-hidden="true" />
                <div>
                  <h3>Conversation record unavailable</h3>
                  <p>Try again shortly. We could not confirm whether this Space has conversations.</p>
                </div>
              </div>
            </Show>
          }
        >
          <ul class="verevon-space-conversation-list">
            <For each={props.threads()}>
              {(thread) => <SpaceConversationRow thread={thread} />}
            </For>
          </ul>
        </Show>
      </Show>

      <a class="verevon-space-composer-link" href={spaceChatHref(props.spaceRef)} aria-label={`Message ${props.spaceName}`}>
        <span class="verevon-space-composer-link__plus" aria-hidden="true"><Plus size={16} /></span>
        <span>Message {props.spaceName}</span>
        <ArrowUpRight size={16} aria-hidden="true" />
      </a>
    </section>
  )
}

function SpaceConversationRow(props: { readonly thread: SpaceThread }) {
  const status = () => props.thread.latest_run_status
  const isActive = () => ACTIVE_RUN_STATUSES.has(status() ?? '')

  return (
    <li class="verevon-space-conversation-row" data-active={isActive() || undefined}>
      <a href={threadHref(props.thread.thread_id)}>
        <span class="verevon-space-conversation-row__icon" aria-hidden="true">
          <MessageCircle size={16} />
        </span>
        <span class="verevon-space-conversation-row__body">
          <strong>{threadTitle(props.thread)}</strong>
          <span>{props.thread.preview || 'Open conversation'}</span>
        </span>
        <span class="verevon-space-conversation-row__meta">
          <span classList={{ 'verevon-space-status': true, 'verevon-space-status--active': isActive() }}>
            {threadStatus(props.thread)}
          </span>
          <Show when={props.thread.updated_at ?? props.thread.latest_run_updated_at}>
            {(at) => <time>{formatWhen(at())}</time>}
          </Show>
        </span>
      </a>
    </li>
  )
}

function SpaceActivityPanel(props: {
  readonly threads: () => readonly SpaceThread[]
  readonly loading: () => boolean
}) {
  return (
    <section class="verevon-space-view" aria-labelledby="space-activity-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">Room pulse</p>
          <h2 id="space-activity-title">Aktivitet</h2>
          <p>Readable movement, approvals, and outcomes from this Space’s conversation projection.</p>
        </div>
      </div>
      <Show when={props.loading()}>
        <p class="verevon-space-inline-status" role="status">Loading Space activity…</p>
      </Show>
      <SpaceActivityFeed
        threads={props.threads()}
        emptyLabel="No conversation activity has been published to this Space yet."
      />
      <p class="verevon-space-view__footnote">
        Run receipts and approvals appear from the current thread projection. Other owner-plane evidence joins only when a correlated Space projection is published.
      </p>
    </section>
  )
}

/**
 * Agents bound to this Space.
 *
 * Control owns bindings, and it already expresses one: a `service` subject in
 * `space_memberships` IS an agent bound to a room, granted the same revisioned
 * way a person is. So this reads the roster rather than waiting for a separate
 * binding projection — the authority exists, and inventing a second one would
 * mean two places deciding which agents are in a room.
 *
 * What it deliberately does NOT show is everything a binding will eventually
 * carry: skills, connectors, availability, latest run. Those need the dedicated
 * model, and a card implying them from a membership row would be the false
 * promise this tab was left honest to avoid.
 */
function SpaceAgentPanel(props: { readonly spaceRef: string }) {
  const [roster] = createResource(() => props.spaceRef, getSpaceRoster)
  const agents = () => (roster() ?? []).filter((member) => member.subject_type === 'service')

  return (
    <section class="verevon-space-view" aria-labelledby="space-agents-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">Agents</p>
          <h2 id="space-agents-title">Agent</h2>
          <p>Agenter som er gitt tilgang til dette rommet, med rollen de har her.</p>
        </div>
      </div>

      <Show when={roster.loading}>
        <p class="verevon-space-inline-status" role="status">Henter agenter …</p>
      </Show>

      <Show when={roster.error}>
        <p class="verevon-space-projection-error" role="alert">
          Agentlisten kunne ikke hentes. Din egen tilgang er uendret.
        </p>
      </Show>

      <Show when={roster.error ? undefined : roster()}>
        <Show
          when={agents().length > 0}
          fallback={
            /* An empty list here is a real answer, not a missing projection:
               Control was asked and no agent holds a binding in this room. */
            <p>Ingen agenter er bundet til dette rommet ennå.</p>
          }
        >
          <ul class="verevon-space-roster">
            <For each={agents()}>
              {(agent: SpaceRosterMember) => (
                <li class="verevon-space-roster__row">
                  <span class="verevon-space-roster__name">
                    {agent.display_name || agent.subject_id}
                  </span>
                  <span class="verevon-space-roster__meta">Agent · {formatLabel(agent.role)}</span>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Show>

      <p class="verevon-space-inline-status">
        Ferdigheter, koblinger og kjørestatus per agent kommer når bindingsmodellen
        publiserer dem; dette viser tilgangen Control faktisk har gitt.
      </p>
    </section>
  )
}

function SpaceMembersPanel(props: {
  readonly spaceRef: string
  readonly role: string
  readonly kind: string
  readonly deletionError: () => string
  readonly deletionReceipt: (() => SpaceDeletionReceipt | undefined) & { readonly loading: boolean }
  readonly deletionSubmitting: () => boolean
  readonly onRequestDeletion: () => Promise<void>
}) {
  const [roster] = createResource(() => props.spaceRef, getSpaceRoster)

  return (
    <section class="verevon-space-view" aria-labelledby="space-members-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">Access</p>
          <h2 id="space-members-title">Medlemmer</h2>
          <p>Your membership is rechecked by the server while this Space stays open.</p>
        </div>
      </div>

      <div class="verevon-space-membership-card">
        <span class="verevon-space-membership-card__icon" aria-hidden="true"><Users size={17} /></span>
        <div>
          <strong>You are confirmed as {formatLabel(props.role)}</strong>
          <p>Your own access is rechecked by the server; the roster below is Control's.</p>
        </div>
      </div>

      <Show when={roster.loading}>
        <p class="verevon-space-inline-status" role="status">Henter medlemmer …</p>
      </Show>

      {/* A refused roster is NOT an empty room. Control answers 404 when the
          caller is not a member, and every Space has at least an owner, so
          "could not be shown" and "nobody is here" must read differently. */}
      <Show when={roster.error}>
        <p class="verevon-space-projection-error" role="alert">
          Medlemslisten kunne ikke hentes. Din egen tilgang er uendret.
        </p>
      </Show>

      <Show when={roster()}>
        {(members) => (
          <ul class="verevon-space-roster">
            <For each={members()}>
              {(member: SpaceRosterMember) => (
                <li class="verevon-space-roster__row">
                  {/* An empty display name means the user projection has not
                      arrived yet. Showing the opaque id is honest; inventing a
                      name from it would not be. */}
                  <span class="verevon-space-roster__name">
                    {member.display_name || member.subject_id}
                  </span>
                  <span class="verevon-space-roster__meta">
                    {member.subject_type === 'service' ? 'Agent' : 'Person'} · {formatLabel(member.role)}
                  </span>
                </li>
              )}
            </For>
          </ul>
        )}
      </Show>

      <Show when={props.kind === 'personal'}>
        <section class="verevon-space-danger-zone" aria-labelledby="space-deletion-title">
          <div>
            <p class="verevon-space-eyebrow">Personal Space settings</p>
            <h3 id="space-deletion-title">Delete Personal Space</h3>
            <p>Deletion is authorized and completed separately. A request is not proof that every owner has erased its data.</p>
          </div>
          <button type="button" onClick={() => void props.onRequestDeletion()} disabled={props.deletionSubmitting()}>
            {props.deletionSubmitting() ? 'Requesting deletion…' : 'Request deletion'}
          </button>
          <Show when={props.deletionError()}>
            <p role="alert">{props.deletionError()}</p>
          </Show>
          <Show when={props.deletionReceipt.loading}>
            <p role="status">Loading deletion receipt…</p>
          </Show>
          <Show when={props.deletionReceipt()}>
            {(receipt) => (
              <div class="verevon-space-deletion-receipt" aria-live="polite">
                <p>Authorization: {receipt().request.state.replace(/_/g, ' ')}</p>
                <p>Purge status: {receipt().purgeStatus.replace(/_/g, ' ')}</p>
                <ul>
                  <For each={receipt().receipts}>
                    {(owner: { ownerPlane: string; status: string; detail?: string }) => (
                      <li>{owner.ownerPlane}: {owner.status}{owner.detail ? ` — ${owner.detail}` : ''}</li>
                    )}
                  </For>
                </ul>
              </div>
            )}
          </Show>
        </section>
      </Show>
    </section>
  )
}

function SpacePulse(props: {
  readonly activeRun: () => SpaceThread | undefined
  readonly threads: () => readonly SpaceThread[]
}) {
  return (
    <aside class="verevon-space-pulse" aria-labelledby="space-pulse-title">
      <div class="verevon-space-pulse__heading">
        <div>
          <p class="verevon-space-eyebrow">At a glance</p>
          <h2 id="space-pulse-title">Room pulse</h2>
        </div>
        <span class="verevon-space-pulse__signal" aria-label={props.activeRun() ? 'Active work' : 'No active work'} />
      </div>

      <Show
        when={props.activeRun()}
        fallback={
          <div class="verevon-space-pulse-card">
            <span class="verevon-space-pulse-card__icon" aria-hidden="true"><Bot size={17} /></span>
            <div>
              <strong>No agent work is active</strong>
              <p>When work starts in a conversation, its state appears here.</p>
            </div>
          </div>
        }
      >
        {(run) => (
          <a class="verevon-space-pulse-card verevon-space-pulse-card--active" href={threadHref(run().thread_id)}>
            <span class="verevon-space-pulse-card__icon" aria-hidden="true"><Bot size={17} /></span>
            <span>
              <strong>Verevon is working</strong>
              <span>{threadTitle(run())}</span>
              <span class="verevon-space-pulse-card__detail">
                <Clock3 size={13} aria-hidden="true" />
                {threadStatus(run())}
              </span>
            </span>
          </a>
        )}
      </Show>

      <dl class="verevon-space-pulse-stats">
        <div>
          <dt>Conversations</dt>
          <dd>{props.threads().length}</dd>
        </div>
        <div>
          <dt>Attention</dt>
          <dd>{props.threads().filter((thread) => thread.latest_run_status === 'awaiting_approval').length}</dd>
        </div>
      </dl>

      <p class="verevon-space-pulse__note">
        This pulse only reflects the current Space conversation projection.
      </p>
    </aside>
  )
}

function threadHref(threadId: string): string {
  return `/chat?thread_id=${encodeURIComponent(threadId)}`
}

function spaceChatHref(spaceRef: string): string {
  return `/chat?space_ref=${encodeURIComponent(spaceRef)}`
}

function threadTitle(thread: SpaceThread): string {
  return thread.title?.trim() || thread.preview?.trim() || 'Untitled conversation'
}

function threadStatus(thread: SpaceThread): string {
  const status = thread.latest_run_status
  if (!status) return 'Conversation open'
  if (status === 'awaiting_approval') return 'Needs approval'
  if (status === 'running') return 'Working'
  if (status === 'queued') return 'Queued'
  if (status === 'completed') return 'Completed'
  return formatLabel(status)
}

function formatLabel(value: string): string {
  return value
    .trim()
    .replace(/[_-]+/g, ' ')
    .replace(/\b\w/g, (letter) => letter.toUpperCase())
}

function spaceWorkroomLabel(kind: string): string {
  return kind === 'personal' ? 'Personal room' : 'Shared workroom'
}

function formatWhen(value: string): string {
  try {
    return new Date(value).toLocaleDateString('nb-NO', { day: 'numeric', month: 'short' })
  } catch {
    return value
  }
}
