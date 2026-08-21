import { useParams } from '@solidjs/router'
import { ArrowUpRight, Bot, Circle, Clock3, MessageCircle, Sparkles, Users } from '@/shared/icons'
import { createEffect, createSignal, For, Show } from 'solid-js'

import { createResource } from '@/shared/lib/create-resource-compat'
import {
  getPersonalSpaceDeletionReceipt,
  getSpaceContext,
  getSpaceThreads,
  listSpaces,
  requestPersonalSpaceDeletion,
  type SpaceDeletionReceipt,
  type SpaceSummary,
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
  const [threads, { refetch: refetchThreads }] = createResource(spaceRef, getSpaceThreads)
  const [spaces] = createResource(listSpaces)
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

  createEffect(
    () => undefined,
    () => {
      const interval = window.setInterval(() => {
        const refreshedContext = refetchContext()
        void Promise.resolve(refreshedContext).then((current) => {
          // A failed authority check leaves the page fail-closed. Only refresh
          // the derived thread projection after a new current context resolves.
          if (current) void refetchThreads()
        })
      }, SPACE_CONTEXT_RECHECK_MS)
      return () => window.clearInterval(interval)
    },
  )

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
            <SpaceRoomRail
              currentSpace={current().space}
              spaces={() => spaces() ?? []}
              threads={currentThreads}
            />

            <main class="verevon-space-canvas">
              <header class="verevon-space-header">
                <div class="verevon-space-header__title">
                  <p class="verevon-space-eyebrow">Shared workroom</p>
                  <h1 id="space-title">{current().space.name}</h1>
                  <div class="verevon-space-meta" aria-label="Current Space status">
                    <span>{formatLabel(current().space.kind)}</span>
                    <span aria-hidden="true">·</span>
                    <span>{formatLabel(current().space.lifecycle)}</span>
                    <span aria-hidden="true">·</span>
                    <span>Your role: {formatLabel(current().membership.role)}</span>
                  </div>
                </div>
                <a class="verevon-space-primary-action" href={spaceChatHref(current().space.space_ref)} link>
                  <MessageCircle size={16} aria-hidden="true" />
                  <span>Chat</span>
                  <ArrowUpRight size={15} aria-hidden="true" />
                </a>
              </header>

              <Show when={threads.error}>
                <p class="verevon-space-projection-error" role="alert">
                  Space conversation activity is temporarily unavailable. Your confirmed Space access remains unchanged.
                </p>
              </Show>

              <SpaceCockpit
                tabs={{
                  chat: (
                    <SpaceConversationPanel
                      spaceRef={current().space.space_ref}
                      threads={currentThreads}
                      loading={() => threads.loading}
                    />
                  ),
                  aktivitet: (
                    <SpaceActivityPanel
                      threads={currentThreads}
                      loading={() => threads.loading}
                    />
                  ),
                  medlemmer: (
                    <SpaceMembersPanel
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

function SpaceRoomRail(props: {
  readonly currentSpace: SpaceSummary
  readonly spaces: () => readonly SpaceSummary[]
  readonly threads: () => readonly SpaceThread[]
}) {
  const changeSpace = (event: Event) => {
    const select = event.currentTarget as HTMLSelectElement
    window.location.href = `/spaces/${encodeURIComponent(select.value)}`
  }
  const availableSpaces = () => {
    const resolved = props.spaces()
    return resolved.some((space) => space.space_ref === props.currentSpace.space_ref)
      ? resolved
      : [props.currentSpace, ...resolved]
  }

  return (
    <aside class="verevon-space-room-rail" aria-label="Space overview">
      <label class="verevon-space-switcher">
        <span>Switch Space</span>
        <select value={props.currentSpace.space_ref} onChange={changeSpace}>
          <For each={availableSpaces()}>
            {(space) => <option value={space.space_ref}>{space.name}</option>}
          </For>
        </select>
      </label>

      <div class="verevon-space-room-card">
        <span class="verevon-space-room-mark" aria-hidden="true">{spaceInitial(props.currentSpace.name)}</span>
        <p class="verevon-space-room-card__eyebrow">This Space</p>
        <p class="verevon-space-room-card__name">{props.currentSpace.name}</p>
        <p class="verevon-space-room-card__status">
          <Circle size={8} fill="currentColor" aria-hidden="true" />
          {formatLabel(props.currentSpace.lifecycle)}
        </p>
      </div>

      <div class="verevon-space-rail-section">
        <div class="verevon-space-rail-section__heading">
          <span>Conversations</span>
          <span>{props.threads().length}</span>
        </div>
        <Show
          when={props.threads().length > 0}
          fallback={<p class="verevon-space-rail-empty">Your room’s conversations will collect here.</p>}
        >
          <ul class="verevon-space-thread-rail-list">
            <For each={props.threads().slice(0, 8)}>
              {(thread) => (
                <li>
                  <a href={threadHref(thread.thread_id)} link aria-label={`Open ${threadTitle(thread)}`}>
                    <span class="verevon-space-thread-rail-list__title">{threadTitle(thread)}</span>
                    <span class="verevon-space-thread-rail-list__detail">{threadStatus(thread)}</span>
                  </a>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>
    </aside>
  )
}

function SpaceConversationPanel(props: {
  readonly spaceRef: string
  readonly threads: () => readonly SpaceThread[]
  readonly loading: () => boolean
}) {
  return (
    <section class="verevon-space-view" aria-labelledby="space-conversations-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">Conversation record</p>
          <h2 id="space-conversations-title">Samtaler</h2>
          <p>The people and agent work that belong to this Space.</p>
        </div>
        <a class="verevon-space-secondary-action" href={spaceChatHref(props.spaceRef)} link>
          <Sparkles size={15} aria-hidden="true" />
          Start a conversation
        </a>
      </div>

      <Show when={props.loading()}>
        <p class="verevon-space-inline-status" role="status">Loading Space conversations…</p>
      </Show>
      <Show
        when={props.threads().length > 0}
        fallback={
          <div class="verevon-space-empty-state">
            <MessageCircle size={18} aria-hidden="true" />
            <div>
              <strong>No conversations yet</strong>
              <p>Start the first conversation and it will become part of this room’s record.</p>
            </div>
          </div>
        }
      >
        <ul class="verevon-space-conversation-list">
          <For each={props.threads()}>
            {(thread) => <SpaceConversationRow thread={thread} />}
          </For>
        </ul>
      </Show>
    </section>
  )
}

function SpaceConversationRow(props: { readonly thread: SpaceThread }) {
  const status = () => props.thread.latest_run_status
  const isActive = () => ACTIVE_RUN_STATUSES.has(status() ?? '')

  return (
    <li class="verevon-space-conversation-row" data-active={isActive() || undefined}>
      <a href={threadHref(props.thread.thread_id)} link>
        <span class="verevon-space-conversation-row__icon" aria-hidden="true">
          <MessageCircle size={16} />
        </span>
        <span class="verevon-space-conversation-row__body">
          <strong>{threadTitle(props.thread)}</strong>
          <span>{props.thread.preview || 'Open conversation'}</span>
        </span>
        <span class="verevon-space-conversation-row__meta">
          <span class={{ 'verevon-space-status': true, 'verevon-space-status--active': isActive() }}>
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

function SpaceMembersPanel(props: {
  readonly role: string
  readonly kind: string
  readonly deletionError: () => string
  readonly deletionReceipt: (() => SpaceDeletionReceipt | undefined) & { readonly loading: boolean }
  readonly deletionSubmitting: () => boolean
  readonly onRequestDeletion: () => Promise<void>
}) {
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
          <p>A full roster will appear when Control Plane publishes a Space member projection.</p>
        </div>
      </div>

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
          <a class="verevon-space-pulse-card verevon-space-pulse-card--active" href={threadHref(run().thread_id)} link>
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

function formatWhen(value: string): string {
  try {
    return new Date(value).toLocaleDateString('nb-NO', { day: 'numeric', month: 'short' })
  } catch {
    return value
  }
}

function spaceInitial(name: string): string {
  return name.trim().charAt(0).toUpperCase() || 'S'
}
