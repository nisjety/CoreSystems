import { createResource, createSignal, onCleanup, onMount, Show } from 'solid-js'
import { useParams } from '@solidjs/router'

import {
  getPersonalSpaceDeletionReceipt,
  getSpaceContext,
  getSpaceThreads,
  listSpaces,
  requestPersonalSpaceDeletion,
} from '@/shared/api/spaces-client'
import { SpaceActivityFeed } from './SpaceActivityFeed'

// Membership is authoritative only at the server. Revalidate while the Space
// is open so a removal/revocation cannot leave an old resolved value usable in
// the cockpit between navigations.
const SPACE_CONTEXT_RECHECK_MS = 30_000

/**
 * The first Space cockpit surface. It displays only the independently resolved
 * lifecycle and membership facts, and starts a new chat using a bare Space selection. The BFF
 * obtains the short-lived effect decision; authority never reaches this page.
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
  const activeRun = () => threads()?.threads.find((thread) => ['queued', 'running', 'awaiting_approval'].includes(thread.latest_run_status ?? ''))

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
    <section class="space-page" aria-labelledby="space-title">
      <Show when={context.loading}>
        <p role="status" aria-live="polite">Loading Space…</p>
      </Show>
      <Show when={context.error}>
        <div role="alert">
          <h1 id="space-title">Space unavailable</h1>
          <p>Your current membership could not be confirmed. No Space actions are available.</p>
        </div>
      </Show>
      <Show when={context.error ? undefined : context()}>
        {(current) => (
          <>
            <header>
              <p>Space</p>
              <h1 id="space-title">{current().space.name}</h1>
              <p>{current().space.kind} · {current().membership.role}</p>
            </header>
            <nav aria-label="Space views">
              <Show when={(spaces()?.length ?? 0) > 0}>
                <label>
                  Switch Space
                  <select value={current().space.space_ref} onChange={(event) => { window.location.href = `/spaces/${encodeURIComponent(event.currentTarget.value)}` }}>
                    {spaces()?.map((space) => <option value={space.space_ref}>{space.name}</option>)}
                  </select>
                </label>
              </Show>
              <a href={`/chat?space_ref=${encodeURIComponent(current().space.space_ref)}`}>Chat</a>
              <a href="#members">Members</a>
              <a href="#work">Work</a>
              <a href="#activity">Activity</a>
            </nav>
            <section id="members" aria-labelledby="members-title">
              <h2 id="members-title">Members</h2>
              <p>You are currently confirmed as <strong>{current().membership.role}</strong>.</p>
            </section>
            <Show when={current().space.kind === 'personal'}>
              <section aria-labelledby="space-deletion-title">
                <h2 id="space-deletion-title">Delete Personal Space</h2>
                <p>Deletion is authorized and completed separately. A request is not proof that every owner has erased its data.</p>
                <button type="button" onClick={() => void requestDeletion()} disabled={deletionSubmitting()}>
                  {deletionSubmitting() ? 'Requesting deletion…' : 'Request deletion'}
                </button>
                <Show when={deletionError()}>
                  <p role="alert">{deletionError()}</p>
                </Show>
                <Show when={deletionReceipt.loading}>
                  <p role="status">Loading deletion receipt…</p>
                </Show>
                <Show when={deletionReceipt()}>
                  {(receipt) => (
                    <div aria-live="polite">
                      <p>Authorization: {receipt().request.state.replace(/_/g, ' ')}</p>
                      <p>Purge status: {receipt().purgeStatus.replace(/_/g, ' ')}</p>
                      <ul>
                        {receipt().receipts.map((owner) => <li>{owner.ownerPlane}: {owner.status}{owner.detail ? ` — ${owner.detail}` : ''}</li>)}
                      </ul>
                    </div>
                  )}
                </Show>
              </section>
            </Show>
            <section id="activity" aria-labelledby="activity-title">
              <h2 id="activity-title">Activity</h2>
              <Show when={threads.loading}>
                <p role="status">Loading Space conversations…</p>
              </Show>
              <Show when={threads.error}>
                <p role="alert">Space conversation activity is temporarily unavailable.</p>
              </Show>
              <section id="work" aria-labelledby="work-title">
                <h2 id="work-title">Work</h2>
                <Show
                  when={activeRun()}
                  fallback={<p>No agent run is currently active in this Space.</p>}
                >
                  {(run) => (
                    <p role="status">
                      Agent work is {run().latest_run_status?.replace(/_/g, ' ') ?? 'active'}
                      {' · '}
                      {run().title || run().preview || 'Untitled conversation'}
                    </p>
                  )}
                </Show>
                <p>Delivery receipts, processes, watches, and scheduled work appear here only when their owner planes publish a correlated Space projection.</p>
              </section>
              <Show when={threads()}>
                {(activity) => (
                  <SpaceActivityFeed
                    threads={activity().threads}
                    emptyLabel="No conversations have been started in this Space."
                  />
                )}
              </Show>
              <p>Run receipts and approvals are shown from the current thread projection. Cross-owner delivery receipts will join only through the Application-owned Space projection.</p>
            </section>
          </>
        )}
      </Show>
    </section>
  )
}
