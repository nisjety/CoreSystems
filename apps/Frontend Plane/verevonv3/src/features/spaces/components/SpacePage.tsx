import { useParams } from '@solidjs/router'
// `Blocks` stands in for lucide's `Puzzle` on the "add existing agent" action:
// the vendored Solid 2 icon barrel carries no Puzzle glyph, and lucide-solid
// itself is Solid 1-only, so it cannot be imported here.
import { Blocks, Bot, Clock3, Loader2, MessageCircle, Sparkles, Users } from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'

import { createResource, type ResourceAccessor } from '@/shared/lib/create-resource-compat'
import {
  getPersonalSpaceDeletionReceipt,
  getSpaceAgents,
  getSpaceContext,
  getSpaceInstructions,
  getSpaceRoster,
  getSpaceThreads,
  markSpaceRead,
  recordSpacePresence,
  requestPersonalSpaceDeletion,
  revokeSpaceAgent,
  setSpaceAgentState,
  updateSpaceInstructions,
  type SpaceAgent,
  type SpacePresence,
  type SpaceReadMarker,
  type SpaceDeletionReceipt,
  type SpaceRosterMember,
  type SpaceThread,
} from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { spaceDisplayName } from '../lib/space-name'
import {
  ACTIVE_RUN_STATUSES,
  AWAITING_APPROVAL_RUN_STATUS,
  formatLabel,
  threadStatus,
  threadTitle,
} from '../lib/space-thread-presentation'
import { SpaceBindAgentDialog } from './SpaceBindAgentDialog'
import { SpaceCockpit } from './SpaceCockpit'
import { SpaceConfirmButton } from './SpaceConfirmButton'
import { SpaceMemberControls, SpaceMemberRemoveButton } from './SpaceMemberControls'
import { SpaceCreateAgentDialog } from './SpaceCreateAgentDialog'
import { SpaceRoomComposer } from './SpaceRoomComposer'
import { SpaceRoomTimeline } from './SpaceRoomTimeline'
import { publishSpaceThreads, publishSpaceUnread, retractSpaceThreads } from '../lib/space-live-work'
import { unreadThreadIds } from '../lib/space-unread'
import { hereSentence, readPresence, typingSentence } from '../lib/space-presence'
import { SpaceActivityPanel } from './SpaceActivityPanel'
import { SpaceKnowledgePanel } from './SpaceKnowledgePanel'
import { SpaceWorkPanel } from './SpaceWorkPanel'

// Mirrors the gateway's own gate (`CREATE_AGENT_ROLES`): creation is a
// governed grant, so only roles that may grant get the entry points. The
// server enforces this regardless; hiding the door is presentation.
function canCreateAgent(role: string): boolean {
  return role === 'owner' || role === 'manager'
}

// ADR-0003's Space-instructions role floor: mirrors the gateway's own gate
// (`SPACE_INSTRUCTIONS_WRITE_ROLES`), which itself mirrors Control's
// `ValidateForSharedThread` floor — broader than `canCreateAgent` above
// because editing standing instructions is a lower bar than granting agent
// access.
function canEditSpaceInstructions(role: string): boolean {
  return role === 'editor' || role === 'manager' || role === 'owner'
}

// Deciding who may read a room's shared record is at least as consequential as
// granting an agent access to it, so it takes the same roles. Mirrors the
// gateway's own gate; the server enforces it regardless.
function canGrantSpaceMembership(role: string): boolean {
  return role === 'owner' || role === 'manager'
}

// Membership is authoritative only at the server. Revalidate while the Space
// is open so a removal/revocation cannot leave an old resolved value usable in
// the cockpit between navigations.
const SPACE_CONTEXT_RECHECK_MS = 30_000

// How often an open room re-reads its own conversation projection.
//
// A room where another member's message appears half a minute late does not
// read as a room, so this is deliberately much shorter than the authority
// recheck above — it is a cheap projection read, not an authorization. It is
// not a substitute for the durable delivery projection either: when that
// lands, this becomes the fallback rather than the mechanism.
const SPACE_TIMELINE_POLL_MS = 6_000

/**
 * The Space is a workroom, not a second application shell. It composes the
 * existing server-authoritative context and thread projections into the
 * presentational cockpit without asking the BFF to invent a composite grant.
 */
export default function SpacePage() {
  const i18n = useI18n()
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
  // Shared by SpaceAgentPanel and the room composer's mention list: every tab
  // stays mounted underneath SpaceCockpit (panels hide, they don't unmount),
  // so fetching this per-panel would mean two independent network calls for
  // the same room's agents on every page load.
  const [roster, { refetch: refetchRoster }] = createResource(spaceRef, getSpaceRoster)
  const [agents, { refetch: refetchAgents }] = createResource(spaceRef, getSpaceAgents)
  const [createAgentOpen, setCreateAgentOpen] = createSignal(false)
  const [bindAgentOpen, setBindAgentOpen] = createSignal(false)
  const [deletionError, setDeletionError] = createSignal('')
  const [deletionSubmitting, setDeletionSubmitting] = createSignal(false)
  const currentThreads = () => threads()?.threads ?? []
  const activeRun = () => currentThreads().find((thread) => ACTIVE_RUN_STATUSES.has(thread.latest_run_status ?? ''))

  // Where the reader had caught up when they ARRIVED in this room (item 4b).
  // Snapshotted from the first successful listing per room and then held: the
  // durable marker keeps advancing while the room stays open, but "new since
  // your last visit" must keep pointing at what was new on arrival, or every
  // badge would vanish on the first poll before anyone had read anything.
  const [arrival, setArrival] = createSignal<{ ref: string; marker: SpaceReadMarker | undefined } | undefined>(undefined)
  // A plain variable, not a second read of `arrival()`. The original guard
  // read the `arrival` signal from inside the effect's untracked half — a
  // read Solid's dev build flags (STRICT_READ_UNTRACKED) precisely because it
  // will not update, and which manifested here as a real bug: it could leave
  // two render passes live at once (caught by a presence test, but the
  // exposure predates presence — this is item 4b's original code).
  let arrivalRef: string | undefined
  createEffect(
    () => ({ ref: spaceRef(), projection: threads.error ? undefined : threads() }),
    ({ ref, projection }) => {
      if (!ref || !projection) return
      // `read_marker` absent means the marker could not be read (named as a
      // gap by the gateway) — hold nothing rather than badge against a guess.
      if (arrivalRef !== ref) {
        arrivalRef = ref
        setArrival({ ref, marker: projection.read_marker })
      }
    },
  )
  const unreadIds = createMemo(() => {
    const snapshot = arrival()
    if (!snapshot || snapshot.ref !== spaceRef()) return new Set<string>()
    return unreadThreadIds(currentThreads(), snapshot.marker, context()?.membership.subject_id)
  })

  // Publish the projection this page already polls, so the sidebar and the
  // composer see the same live state at the same moment without a second
  // fetch or a second timer (item 1b: "both need the same refresh path").
  // Retracted on leave: a projection nobody is refreshing must not keep
  // telling the sidebar a room is busy. The unread set rides along so the
  // sidebar badges the same rows the timeline does.
  createEffect(
    () => ({ ref: spaceRef(), projection: threads.error ? undefined : threads(), unread: unreadIds() }),
    ({ ref, projection, unread }) => {
      if (!ref || !projection) return undefined
      publishSpaceThreads(ref, projection.threads)
      publishSpaceUnread(ref, unread)
      return () => retractSpaceThreads(ref)
    },
  )

  // Having the room open IS reading it. Advance the durable marker on every
  // successful listing while the tab is visible — the listing only runs then —
  // fire-and-forget: a marker write failing must never cost the room anything,
  // and the gateway answers it as its own honest 503 if it does.
  createEffect(
    () => ({ ref: spaceRef(), loaded: !threads.error && threads() !== undefined }),
    ({ ref, loaded }) => {
      if (!ref || !loaded) return
      if (typeof document !== 'undefined' && document.visibilityState !== 'visible') return
      void markSpaceRead(ref).catch(() => undefined)
    },
  )

  // Who else is in the room. `undefined` means unknown — Application could not
  // be reached, or the beat has not landed yet — and is rendered as nothing at
  // all. Drawing an empty room on an unreadable answer would tell a member
  // they are alone, which is a different and worse claim than saying nothing.
  const [presence, setPresence] = createSignal<SpacePresence | undefined>(undefined)
  let lastBeatAt = 0
  // The room this browser currently has open, as a PLAIN variable rather than
  // a second call to `spaceRef()`. `beat`'s continuation runs after an
  // `await`, well outside any tracked scope — reading a signal there is
  // exactly what trips Solid's own untracked-read diagnostic, and it is not
  // just a lint complaint here: it can leave two competing render passes
  // live at once. Written only from the effect's tracked half below, which is
  // a legitimate tracking scope, so setting it there is a plain, ungraded read.
  let currentSpaceRef: string | undefined
  async function beat(ref: string, status: 'online' | 'typing' | 'offline'): Promise<void> {
    if (!ref) return
    lastBeatAt = Date.now()
    try {
      const next = await recordSpacePresence(ref, status)
      // The room may have changed while this request was in flight; a late
      // answer for the room the reader just left must not paint over the one
      // they are looking at now.
      if (currentSpaceRef === ref && status !== 'offline') setPresence(next)
    } catch {
      if (currentSpaceRef === ref) setPresence(undefined)
    }
  }
  // Rides the listing the room already polls, so presence costs one small
  // request per beat and no second timer. Only while the tab is visible: a
  // background tab is not somebody being in the room.
  createEffect(
    () => {
      const ref = spaceRef()
      currentSpaceRef = ref || undefined
      return { ref, loaded: !threads.error && threads() !== undefined }
    },
    ({ ref, loaded }) => {
      if (!ref || !loaded) return undefined
      if (typeof document !== 'undefined' && document.visibilityState !== 'visible') return undefined
      void beat(ref, 'online')
      // Leaving the room says so, so the others do not wait out the expiry.
      // Best-effort by construction: a closed tab never gets here, which is
      // exactly why the server expires a heartbeat that stops coming.
      return () => {
        setPresence(undefined)
        void recordSpacePresence(ref, 'offline').catch(() => undefined)
      }
    },
  )
  // Typing is worth its own beat: waiting for the next poll would put "is
  // writing" on screen up to six seconds after the person started. Throttled,
  // because a keystroke is not a network event.
  const TYPING_BEAT_MS = 3_000
  function noteTyping(): void {
    const ref = spaceRef()
    if (!ref) return
    if (Date.now() - lastBeatAt < TYPING_BEAT_MS) return
    void beat(ref, 'typing')
  }
  const presenceReading = createMemo(() =>
    readPresence(presence()?.present, roster.error ? [] : roster() ?? []),
  )

  async function requestDeletion() {
    const selectedSpace = context()?.space
    if (!selectedSpace || selectedSpace.kind !== 'personal' || deletionSubmitting()) return
    setDeletionError('')
    setDeletionSubmitting(true)
    try {
      const request = await requestPersonalSpaceDeletion(selectedSpace.space_ref, `space-delete:${crypto.randomUUID()}`)
      setDeletionRequestId(request.requestId)
    } catch {
      setDeletionError(i18n.tr(
        'Sletteforespørselen kunne ikke registreres. Ingen sletting er bekreftet.',
        'The deletion request could not be recorded. No deletion has been confirmed.',
      ))
    } finally {
      setDeletionSubmitting(false)
    }
  }

  // Keep the room live while it is actually being looked at.
  //
  // Gated on visibility for two reasons, and the second is the load-bearing
  // one: a background tab polling forever is waste, but a tab that resumes
  // after an hour showing an hour-old room is a lie. Refetching on
  // `visibilitychange` means the first thing a returning reader sees is
  // current, not stale.
  createEffect(
    () => undefined,
    () => {
      if (typeof document === 'undefined') return undefined
      let timer: number | undefined
      const stop = () => {
        if (timer !== undefined) {
          window.clearInterval(timer)
          timer = undefined
        }
      }
      const start = () => {
        if (timer !== undefined) return
        timer = window.setInterval(() => {
          // The authority recheck below owns membership. This only refreshes
          // the projection, and a failure is already rendered as "temporarily
          // unavailable" rather than as an empty room.
          void Promise.resolve(refetchThreads()).catch(() => undefined)
        }, SPACE_TIMELINE_POLL_MS)
      }
      const onVisibilityChange = () => {
        if (document.visibilityState === 'visible') {
          void Promise.resolve(refetchThreads()).catch(() => undefined)
          start()
        } else {
          stop()
        }
      }
      if (document.visibilityState === 'visible') start()
      document.addEventListener('visibilitychange', onVisibilityChange)
      return () => {
        stop()
        document.removeEventListener('visibilitychange', onVisibilityChange)
      }
    },
  )

  createEffect(
    () => undefined,
    () => {
      const interval = window.setInterval(() => {
        const refreshedContext = refetchContext()
        void Promise.resolve(refreshedContext)
          .then((current) => {
            // A failed authority check leaves the page fail-closed. Only refresh
            // the derived thread projection after a new current context resolves.
            if (current) void refetchThreads()
          })
          // Solid 1's `refetch()` settled to `undefined` on failure and reported
          // the error only through `context.error`; the compat shim rethrows so
          // an awaited refetch can observe it. Nothing awaits this one — the
          // fail-closed render below reads `context.error` — so swallow the
          // rejection here instead of letting the recheck escape as an unhandled
          // rejection every 30s (and fail a test run after teardown).
          .catch(() => undefined)
      }, SPACE_CONTEXT_RECHECK_MS)
      return () => window.clearInterval(interval)
    },
  )

  return (
    <section class="space-page verevon-space" aria-labelledby="space-title">
      <Show when={context.loading}>
        <p class="verevon-space-loading" role="status" aria-live="polite">{i18n.tr('Laster rom …', 'Loading Space…')}</p>
      </Show>
      <Show when={context.error}>
        <div class="verevon-space-unavailable" role="alert">
          <h1 id="space-title">{i18n.tr('Rom utilgjengelig', 'Space unavailable')}</h1>
          <p>{i18n.tr(
            'Medlemskapet ditt kunne ikke bekreftes. Ingen romhandlinger er tilgjengelige.',
            'Your current membership could not be confirmed. No Space actions are available.',
          )}</p>
        </div>
      </Show>
      <Show when={context.error ? undefined : context()}>
        {(current) => (
          <div class="verevon-space-workroom">
            <main class="verevon-space-canvas">
              <header class="verevon-space-header">
                <div class="verevon-space-header__title">
                  <p class="verevon-space-eyebrow">{spaceWorkroomLabel(current().space.kind, i18n.tr)}</p>
                  <h1 id="space-title">{spaceDisplayName(current().space, i18n.tr)}</h1>
                  <div class="verevon-space-meta" aria-label={i18n.tr('Status for rommet', 'Current Space status')}>
                    <span>{spaceKindLabel(current().space.kind, i18n.tr)}</span>
                    <span aria-hidden="true">·</span>
                    <span>{spaceLifecycleLabel(current().space.lifecycle, i18n.tr)}</span>
                    <span aria-hidden="true">·</span>
                    <span>{i18n.tr('Din rolle: ', 'Your role: ')}{spaceRoleLabel(current().membership.role, i18n.tr)}</span>
                    <Show when={hereSentence(presenceReading(), i18n.tr)}>
                      {(sentence) => (
                        <>
                          <span aria-hidden="true">·</span>
                          <span class="verevon-space-presence">
                            <span class="verevon-space-presence__dot" aria-hidden="true" />
                            {sentence()}
                          </span>
                        </>
                      )}
                    </Show>
                  </div>
                </div>
              </header>

              <Show when={threadsUnavailable()}>
                <p class="verevon-space-projection-error" role="alert">
                  {i18n.tr(
                    'Samtaleaktiviteten i rommet er midlertidig utilgjengelig. Din bekreftede tilgang til rommet er uendret.',
                    'Space conversation activity is temporarily unavailable. Your confirmed Space access remains unchanged.',
                  )}
                </p>
              </Show>

              <SpaceCreateAgentDialog
                spaceRef={current().space.space_ref}
                open={createAgentOpen}
                onClose={() => setCreateAgentOpen(false)}
                onCreated={() => {
                  void refetchAgents()
                }}
              />
              <SpaceBindAgentDialog
                spaceRef={current().space.space_ref}
                open={bindAgentOpen}
                onClose={() => setBindAgentOpen(false)}
                onBound={() => {
                  void refetchAgents()
                }}
              />
              <SpaceCockpit
                tabs={{
                  chat: (
                    <SpaceConversationPanel
                      spaceRef={current().space.space_ref}
                      spaceName={spaceDisplayName(current().space, i18n.tr)}
                      viewerSubjectId={() => current().membership.subject_id}
                      threads={currentThreads}
                      loading={() => threads.loading}
                      unavailable={threadsUnavailable}
                      roster={() => (roster.error ? [] : roster() ?? [])}
                      agents={() => (agents.error ? [] : agents() ?? [])}
                      unreadThreadIds={unreadIds}
                      typingSentence={() => typingSentence(presenceReading(), i18n.tr)}
                      onTyping={noteTyping}
                      onExchangeSettled={() => {
                        void refetchThreads()
                      }}
                      onCreateAgent={
                        canCreateAgent(current().membership.role)
                          ? () => setCreateAgentOpen(true)
                          : undefined
                      }
                    />
                  ),
                  arbeid: <SpaceWorkPanel spaceRef={current().space.space_ref} />,
                  kunnskap: <SpaceKnowledgePanel spaceRef={current().space.space_ref} />,
                  aktivitet: (
                    <SpaceActivityPanel
                      spaceRef={current().space.space_ref}
                      threads={currentThreads}
                      threadsLoading={() => threads.loading}
                    />
                  ),
                  agent: (
                    <>
                      <SpaceInstructionsSection
                        spaceRef={current().space.space_ref}
                        role={current().membership.role}
                      />
                      <SpaceAgentPanel
                        agents={agents}
                        spaceRef={current().space.space_ref}
                        canGovern={canCreateAgent(current().membership.role)}
                        onChanged={() => {
                          void refetchAgents()
                        }}
                        onCreateAgent={
                          canCreateAgent(current().membership.role)
                            ? () => setCreateAgentOpen(true)
                            : undefined
                        }
                        onBindAgent={
                          canCreateAgent(current().membership.role)
                            ? () => setBindAgentOpen(true)
                            : undefined
                        }
                      />
                    </>
                  ),
                  medlemmer: (
                    <SpaceMembersPanel
                      spaceRef={current().space.space_ref}
                      role={current().membership.role}
                      kind={current().space.kind}
                      orgId={current().membership.org_id}
                      isOrganizationRoom={current().space.is_organization_room}
                      onMembersChanged={() => {
                        void refetchRoster()
                      }}
                      roster={roster}
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
  /** The reading member's own Control subject, so their own turns can be
   * marked as theirs. Never used to name an unattributed turn. */
  readonly viewerSubjectId: () => string | undefined
  readonly threads: () => readonly SpaceThread[]
  readonly loading: () => boolean
  readonly unavailable: () => boolean
  readonly roster: () => readonly SpaceRosterMember[]
  readonly agents: () => readonly SpaceAgent[]
  readonly onExchangeSettled?: () => void
  readonly onCreateAgent?: () => void
  /** Posts new since the reader arrived (item 4b). */
  readonly unreadThreadIds?: () => ReadonlySet<string>
  /** "Kari skriver …", from the room's presence beat. */
  readonly typingSentence?: () => string | undefined
  /** The member typed just now, so the room can say so before the next poll. */
  readonly onTyping?: () => void
}) {
  const i18n = useI18n()
  let focusComposer: (() => void) | undefined
  const [replyTarget, setReplyTarget] = createSignal<
    { threadId: string; title: string; awaitingApproval?: boolean } | undefined
  >(undefined)
  // Read from the live projection rather than from what was true when Reply was
  // pressed: an approval can be raised (or settled) while the reply is being
  // typed, and the composer must follow the room, not the click.
  const replyTargetWithStatus = () => {
    const target = replyTarget()
    if (!target) return undefined
    const thread = props.threads().find((item) => item.thread_id === target.threadId)
    return {
      ...target,
      awaitingApproval: thread?.latest_run_status === AWAITING_APPROVAL_RUN_STATUS,
    }
  }

  return (
    <section class="verevon-space-view verevon-space-view--conversations" aria-labelledby="space-conversations-title">
      <div class="verevon-space-view__heading">
        <div>
        <p class="verevon-space-eyebrow">{i18n.tr('Samtale i rommet', 'Space conversation')}</p>
          <h2 id="space-conversations-title">{i18n.tr('Samtaler', 'Chat')}</h2>
          <p>{i18n.tr(
            'Det delte arkivet for mennesker og agentarbeid knyttet til dette rommet.',
            'The shared record for people and agent work connected to this Space.',
          )}</p>
        </div>
        <button type="button" class="verevon-space-secondary-action" onClick={() => focusComposer?.()}>
          <Sparkles size={15} aria-hidden="true" />
          {i18n.tr('Start en samtale', 'Start a conversation')}
        </button>
      </div>

      <Show when={props.loading()}>
        <p class="verevon-space-inline-status" role="status">{i18n.tr('Laster samtaler i rommet …', 'Loading Space conversations…')}</p>
      </Show>
      <Show when={!props.loading()}>
        <Show
          when={!props.unavailable()}
          fallback={
            <div class="verevon-space-empty-state" role="status">
              <MessageCircle size={20} aria-hidden="true" />
              <div>
                <h3>{i18n.tr('Samtalearkivet er utilgjengelig', 'Conversation record unavailable')}</h3>
                <p>{i18n.tr(
                  'Prøv igjen om litt. Vi kunne ikke bekrefte om dette rommet har samtaler.',
                  'Try again shortly. We could not confirm whether this Space has conversations.',
                )}</p>
              </div>
            </div>
          }
        >
          <SpaceRoomTimeline
            spaceRef={props.spaceRef}
            spaceName={props.spaceName}
            viewerSubjectId={props.viewerSubjectId}
            threads={props.threads}
            roster={props.roster}
            agents={props.agents}
            onStartConversation={() => focusComposer?.()}
            onCreateAgent={props.onCreateAgent}
            onReply={(thread) => {
              setReplyTarget({ threadId: thread.thread_id, title: threadTitle(thread, i18n.tr) })
              focusComposer?.()
            }}
            onApprovalSettled={props.onExchangeSettled}
            unreadThreadIds={props.unreadThreadIds}
            // A pin or retitle changes the server's order and names; re-read
            // rather than trust this browser's copy of either.
            onPresentationChanged={props.onExchangeSettled}
          />
        </Show>
      </Show>

      <SpaceRoomComposer
        spaceRef={props.spaceRef}
        roster={props.roster}
        agents={props.agents}
        threads={props.threads}
        onExchangeSettled={props.onExchangeSettled}
        registerFocusHandle={(focus) => { focusComposer = focus }}
        typingSentence={props.typingSentence}
        onTyping={props.onTyping}
        replyTarget={replyTargetWithStatus}
        onClearReplyTarget={() => setReplyTarget(undefined)}
      />
    </section>
  )
}


/**
 * Agents bound to this Space.
 *
 * Two planes fill the tab in and the split is load-bearing: Control decides who
 * may act in the room (the `service` subjects in `space_memberships`), and an
 * Application binding says what each one is called and how it may be invoked.
 * The join happens server-side in the gateway, so this renders one list rather
 * than reconciling two.
 *
 * What a card still does NOT show is what no binding carries yet: skills,
 * connectors, availability, latest run. Implying them would be the false
 * promise this tab was left honest to avoid.
 */
const MAX_SPACE_INSTRUCTIONS_LENGTH = 4000

/**
 * ADR-0003's Space layer — owner/manager/editor-authored instructions that
 * apply to every turn in this Space, composed with the platform and org
 * layers (`apps/AUTHORED_INSTRUCTIONS_ADR_2026-08-19.md`). Lives in the Agent
 * tab: it shapes "the active model... and which actions are permitted here",
 * the tab's own stated purpose (`SpaceCockpit`'s tab definitions).
 */
function SpaceInstructionsSection(props: { readonly spaceRef: string; readonly role: string }) {
  const i18n = useI18n()
  // This ref is read out of the Space context, which the 30s membership
  // recheck replaces with a fresh-but-equivalent object. `createResource`
  // compares the source value, so an unchanged ref does not refetch.
  const [saved, { refetch }] = createResource(
    () => props.spaceRef,
    (spaceRef) => getSpaceInstructions(spaceRef),
  )
  const [draft, setDraft] = createSignal('')
  const [dirty, setDirty] = createSignal(false)
  const [submitting, setSubmitting] = createSignal(false)
  const [formError, setFormError] = createSignal<string | null>(null)
  const canEdit = createMemo(() => canEditSpaceInstructions(props.role))
  const value = createMemo(() => (dirty() ? draft() : saved() ?? ''))

  const handleSubmit = async (event: Event) => {
    event.preventDefault()
    if (submitting()) return
    setSubmitting(true)
    setFormError(null)
    try {
      await updateSpaceInstructions(props.spaceRef, value())
      setDirty(false)
      await refetch()
    } catch (err) {
      setFormError(
        translateApiError(err, i18n.tr, {
          no: 'Kunne ikke lagre instruksene for rommet.',
          en: 'Could not save the Space instructions.',
        }),
      )
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <section class="verevon-space-view" aria-labelledby="space-instructions-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Instrukser', 'Instructions')}</p>
          <h2 id="space-instructions-title">{i18n.tr('Instrukser for rommet', 'Instructions for this Space')}</h2>
          <p>{i18n.tr(
            'Legges til i hver samtale i dette rommet, sammen med organisasjonens instrukser. Kan utvide, men ikke overstyre dem.',
            'Added to every conversation in this Space, alongside the organization instructions. May add to, but not override, them.',
          )}</p>
        </div>
      </div>

      <Show when={formError()}>
        {(message) => (
          <p class="verevon-space-projection-error" role="alert">{message()}</p>
        )}
      </Show>

      <Show when={saved.error}>
        <p class="verevon-space-projection-error" role="alert">
          {i18n.tr('Kunne ikke laste instruksene for rommet.', 'Could not load the Space instructions.')}
        </p>
      </Show>

      <Show
        when={canEdit()}
        fallback={
          <Show
            when={!saved.loading}
            fallback={<p class="verevon-space-inline-status" role="status">{i18n.tr('Henter instrukser …', 'Loading instructions…')}</p>}
          >
            <Show
              when={value().trim().length > 0}
              fallback={<p>{i18n.tr('Ingen instrukser er lagt til for dette rommet ennå.', 'No instructions have been added to this Space yet.')}</p>}
            >
              <p style={{ "white-space": "pre-wrap" }}>{value()}</p>
            </Show>
          </Show>
        }
      >
        <form onSubmit={handleSubmit}>
          <Show
            when={!saved.loading}
            fallback={<p class="verevon-space-inline-status" role="status">{i18n.tr('Henter instrukser …', 'Loading instructions…')}</p>}
          >
            <textarea
              aria-label={i18n.tr('Instrukser for rommet', 'Instructions for this Space')}
              value={value()}
              rows={6}
              maxlength={MAX_SPACE_INSTRUCTIONS_LENGTH}
              onInput={(event) => {
                setDraft(event.currentTarget.value)
                setDirty(true)
              }}
              class="verevon-settings-input verevon-settings-textarea"
            />
          </Show>
          <div class="verevon-space-view__actions">
            <button type="submit" class="verevon-space-secondary-action" disabled={submitting() || !dirty()}>
              <Show when={submitting()} fallback={<><Sparkles size={15} aria-hidden="true" /> {i18n.tr('Lagre instrukser', 'Save instructions')}</>}>
                <Loader2 size={15} aria-hidden="true" /> {i18n.tr('Lagrer…', 'Saving…')}
              </Show>
            </button>
          </div>
        </form>
      </Show>
    </section>
  )
}

/**
 * The Space's agents, rendered as room participants rather than as a catalog.
 *
 * The presentation follows the coworker model the scope plan names (Grok/Buzz):
 * an agent is someone who is *in the room*, with a name, a standing, and a
 * reachable set of channels — not a configuration row. What it must not borrow
 * from those products is their looseness about authority: every field here is
 * server-published, and the two states people conflate are kept visibly apart.
 *
 * * **No agents** is an answer — Control was asked and no agent is bound.
 * * **Could not load** is not an answer, and never renders as an empty room.
 * * **No published identity** is a third state: Control authorizes the agent,
 *   but Application has not described it. It appears, honestly unnamed.
 *
 * Status is carried by text and shape, never colour alone, per the plan's
 * accessibility rule.
 */
function SpaceAgentPanel(props: {
  readonly agents: ResourceAccessor<readonly SpaceAgent[]>
  readonly spaceRef: string
  readonly onCreateAgent?: () => void
  readonly onBindAgent?: () => void
  /** Present only for roles that may govern a binding here. Absent hides the
   * controls; the server refuses regardless, so this is presentation. */
  readonly canGovern: boolean
  readonly onChanged?: () => void
}) {
  const i18n = useI18n()
  const agents = props.agents
  const current = () => agents() ?? []

  return (
    <section class="verevon-space-view" aria-labelledby="space-agents-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Agenter', 'Agents')}</p>
          <h2 id="space-agents-title">{i18n.tr('Agenter i rommet', 'Agents in this room')}</h2>
          <p>{i18n.tr(
            'Agenter som deltar her, med rollen Control har gitt dem og kanalene de kan nås gjennom.',
            'Agents taking part here, with the role Control granted them and the channels they can be reached through.',
          )}</p>
        </div>
        <div class="verevon-space-view__actions">
          <Show when={props.onBindAgent}>
            {(bind) => (
              <button type="button" class="verevon-space-secondary-action" onClick={() => bind()()}>
                <Blocks size={15} aria-hidden="true" />
                {i18n.tr('Legg til eksisterende agent', 'Add existing agent')}
              </button>
            )}
          </Show>
          <Show when={props.onCreateAgent}>
            {(create) => (
              <button type="button" class="verevon-space-secondary-action" onClick={() => create()()}>
                <Bot size={15} aria-hidden="true" />
                {i18n.tr('Opprett en agent', 'Create an agent')}
              </button>
            )}
          </Show>
        </div>
      </div>

      <Show when={agents.loading}>
        <p class="verevon-space-inline-status" role="status">{i18n.tr('Henter agenter …', 'Loading agents…')}</p>
      </Show>

      <Show when={agents.error}>
        <p class="verevon-space-projection-error" role="alert">
          {i18n.tr(
            'Agentlisten kunne ikke hentes, så vi vet ikke hvem som deltar her nå. Din egen tilgang er uendret.',
            'The agent list could not be loaded, so we do not know who is taking part right now. Your own access is unchanged.',
          )}
        </p>
      </Show>

      <Show when={agents.error ? undefined : agents()}>
        <Show
          when={current().length > 0}
          fallback={
            /* A real answer: Control was asked, and no agent is bound here. */
            <p>{i18n.tr(
              'Ingen agenter er bundet til dette rommet ennå.',
              'No agents are bound to this room yet.',
            )}</p>
          }
        >
          <ul class="verevon-space-agents">
            <For each={current()}>
              {(agent) => (
                <SpaceAgentCard
                  agent={agent}
                  spaceRef={props.spaceRef}
                  canGovern={props.canGovern}
                  onChanged={props.onChanged}
                />
              )}
            </For>
          </ul>
        </Show>
      </Show>
    </section>
  )
}

/** One agent as a room participant. */
function SpaceAgentCard(props: {
  readonly agent: SpaceAgent
  readonly spaceRef: string
  readonly canGovern: boolean
  readonly onChanged?: () => void
}) {
  const i18n = useI18n()
  const agent = () => props.agent
  const [busy, setBusy] = createSignal(false)
  const [actionError, setActionError] = createSignal('')

  // Only a settled binding can be governed here. `pending` is waiting on
  // Control, `failed` records that provisioning did not work, and `revoked` is
  // terminal — offering buttons for any of them would be a control that the
  // server is going to refuse.
  const governable = () => agent().status === 'active' || agent().status === 'paused'
  const bindingRef = () => agent().binding_ref?.trim()

  async function run(change: () => Promise<unknown>, failure: { no: string; en: string }) {
    if (busy()) return
    setBusy(true)
    setActionError('')
    try {
      await change()
      props.onChanged?.()
    } catch (err) {
      setActionError(translateApiError(err, i18n.tr, failure))
    } finally {
      setBusy(false)
    }
  }

  const setState = (status: 'active' | 'paused') => {
    const ref = bindingRef()
    if (!ref) return
    void run(
      () => setSpaceAgentState(props.spaceRef, ref, status),
      status === 'paused'
        ? {
            no: 'Agenten kunne ikke settes på pause. Ingenting er endret.',
            en: 'The agent could not be paused. Nothing was changed.',
          }
        : {
            no: 'Agenten kunne ikke gjenopptas. Ingenting er endret.',
            en: 'The agent could not be resumed. Nothing was changed.',
          },
    )
  }

  // Revocation is the one control here that the other button cannot undo, so
  // it arms before it acts. Getting the agent back means adding it again.
  const revoke = () => {
    const ref = bindingRef()
    if (!ref) return
    void run(
      () => revokeSpaceAgent(props.spaceRef, ref),
      {
        no: 'Agenten kunne ikke fjernes. Den deltar fortsatt i rommet.',
        en: 'The agent could not be removed. It is still taking part in the room.',
      },
    )
  }
  const displayName = () =>
    agent().name?.trim() || i18n.tr('Agent uten publisert navn', 'Agent with no published name')
  // The initial is decoration over a name we already show; when there is no
  // name it must not invent a letter, so it falls back to a neutral mark.
  const initial = () => (agent().name?.trim()?.[0] ?? '·').toUpperCase()

  return (
    <li class="verevon-space-agent">
      <span class="verevon-space-agent__avatar" aria-hidden="true">{initial()}</span>
      <div class="verevon-space-agent__body">
        <p class="verevon-space-agent__name">
          {displayName()}
          <Show when={agent().title?.trim()}>
            {(title) => <span class="verevon-space-agent__title"> · {title()}</span>}
          </Show>
        </p>

        <p class="verevon-space-agent__meta">
          <span>{spaceRoleLabel(agent().role, i18n.tr)}</span>
          <Show when={agent().status}>
            {(status) => (
              <>
                <span aria-hidden="true"> · </span>
                <span>
                  <span aria-hidden="true">{agentStatusMark(status())} </span>
                  {agentStatusLabel(status(), i18n.tr)}
                </span>
              </>
            )}
          </Show>
        </p>

        <Show when={agent().description?.trim()}>
          {(description) => <p class="verevon-space-agent__description">{description()}</p>}
        </Show>

        {/* Binding policy chips — rendered only for fields that exist, since
            absence means legacy behavior rather than a policy someone chose. */}
        <Show when={agent().trigger_modes || agent().approval_mode || agent().allowed_tools}>
          <ul class="verevon-space-agent__policy" aria-label={i18n.tr('Bindingspolicy', 'Binding policy')}>
            <Show when={agent().trigger_modes?.includes('mention')}>
              <li>{i18n.tr('Kun @-nevning', 'Mention only')}</li>
            </Show>
            <Show when={agent().approval_mode}>
              {(mode) => <li>{approvalModeLabel(mode(), i18n.tr)}</li>}
            </Show>
            <Show when={agent().allowed_tools}>
              {(tools) => (
                <li>
                  {tools().length === 0
                    ? i18n.tr('Uten verktøy', 'No tools')
                    : `${i18n.tr('Verktøy', 'Tools')}: ${tools().length}`}
                </li>
              )}
            </Show>
          </ul>
        </Show>

        {/* Control granted the access, but no plane has described the agent.
            Saying so is more useful than a blank card, and far more honest than
            dressing the subject id up as a name. */}
        <Show when={!agent().identity_published}>
          <p class="verevon-space-agent__unpublished">
            {i18n.tr(
              'Control har gitt denne agenten tilgang, men ingen identitet er publisert for den ennå.',
              'Control has granted this agent access, but no identity has been published for it yet.',
            )}
          </p>
        </Show>

        {/* A definition can be draft or inactive while its binding is active.
            That mismatch is worth surfacing rather than flattening. */}
        <Show when={agent().definition_status === 'active' ? undefined : agent().definition_status}>
          {(definitionStatus) => (
            <p class="verevon-space-agent__unpublished">
              {i18n.tr(
                `Agentdefinisjonen er ${definitionStatus() === 'draft' ? 'et utkast' : 'inaktiv'}, selv om bindingen står aktiv her.`,
                `The agent definition is ${definitionStatus() === 'draft' ? 'a draft' : 'inactive'}, even though its binding is active here.`,
              )}
            </p>
          )}
        </Show>

        {/* "Pause / mute / remove HERE" from the product model's dividing rule:
            this governs one binding in one room, and never the definition or
            its other installations — that is the Agent page's job. */}
        <Show when={props.canGovern && governable() && bindingRef()}>
          <div class="verevon-space-agent__controls">
            <Show
              when={agent().status === 'active'}
              fallback={
                <button
                  type="button"
                  class="verevon-space-agent__control"
                  disabled={busy()}
                  onClick={() => setState('active')}
                >
                  {i18n.tr('Gjenoppta', 'Resume')}
                </button>
              }
            >
              <button
                type="button"
                class="verevon-space-agent__control"
                disabled={busy()}
                onClick={() => setState('paused')}
              >
                {i18n.tr('Sett på pause', 'Pause')}
              </button>
            </Show>
            <SpaceConfirmButton
              class="verevon-space-agent__control verevon-space-agent__control--remove"
              label={i18n.tr('Fjern fra rommet', 'Remove from room')}
              confirmLabel={i18n.tr(
                `Bekreft at ${agent().name?.trim() || 'agenten'} fjernes`,
                `Confirm removing ${agent().name?.trim() || 'the agent'}`,
              )}
              consequence={i18n.tr(
                'Agenten slutter å delta her. For å få den tilbake må den legges til på nytt.',
                'The agent stops taking part here. Getting it back means adding it again.',
              )}
              disabled={busy()}
              onConfirm={revoke}
            />
          </div>
        </Show>

        <Show when={actionError()}>
          {(message) => (
            <p class="verevon-space-agent__error" role="alert">{message()}</p>
          )}
        </Show>

        <Show
          when={agent().delivery_targets.length > 0}
          fallback={
            <p class="verevon-space-agent__channels-empty">
              {i18n.tr('Ingen kanaler publisert', 'No channels published')}
            </p>
          }
        >
          <ul class="verevon-space-agent__channels" aria-label={i18n.tr('Kanaler', 'Channels')}>
            <For each={agent().delivery_targets}>
              {(target) => (
                <li class="verevon-space-agent__channel">
                  <span aria-hidden="true">{deliveryStatusMark(target.status)} </span>
                  {channelLabel(target.channel, i18n.tr)}
                  <span class="verevon-space-agent__channel-target"> · {target.label}</span>
                  <span class="verevon-space-agent__channel-status">
                    {' '}({deliveryStatusLabel(target.status, i18n.tr)})
                  </span>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>
    </li>
  )
}

function SpaceMembersPanel(props: {
  readonly spaceRef: string
  readonly role: string
  readonly kind: string
  readonly orgId: string
  /** The organization's own channel derives its roster from org-core, so it
   * has no editable member list — see `SpaceMemberControls`.
   *
   * `undefined` means the server did not say, which is NOT the same as "no".
   * An older gateway omits the field entirely, and showing the editor on that
   * silence would offer a door the server then refuses to open. */
  readonly isOrganizationRoom: boolean | undefined
  readonly onMembersChanged?: () => void
  readonly roster: ResourceAccessor<readonly SpaceRosterMember[]>
  readonly deletionError: () => string
  readonly deletionReceipt: (() => SpaceDeletionReceipt | undefined) & { readonly loading: boolean }
  readonly deletionSubmitting: () => boolean
  readonly onRequestDeletion: () => Promise<void>
}) {
  const i18n = useI18n()
  const roster = props.roster

  return (
    <section class="verevon-space-view" aria-labelledby="space-members-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Tilgang', 'Access')}</p>
          <h2 id="space-members-title">{i18n.tr('Medlemmer', 'Members')}</h2>
          <p>{i18n.tr(
            'Medlemskapet ditt sjekkes på nytt av serveren mens dette rommet er åpent.',
            'Your membership is rechecked by the server while this Space stays open.',
          )}</p>
        </div>
      </div>

      <div class="verevon-space-membership-card">
        <span class="verevon-space-membership-card__icon" aria-hidden="true"><Users size={17} /></span>
        <div>
          <strong>{i18n.tr('Du er bekreftet som ', 'You are confirmed as ')}{spaceRoleLabel(props.role, i18n.tr)}</strong>
          <p>{i18n.tr(
            'Din egen tilgang sjekkes på nytt av serveren; listen under kommer fra Control.',
            "Your own access is rechecked by the server; the roster below is Control's.",
          )}</p>
        </div>
      </div>

      <Show when={roster.loading}>
        <p class="verevon-space-inline-status" role="status">{i18n.tr('Henter medlemmer …', 'Loading members…')}</p>
      </Show>

      {/* A refused roster is NOT an empty room. Control answers 404 when the
          caller is not a member, and every Space has at least an owner, so
          "could not be shown" and "nobody is here" must read differently. */}
      <Show when={roster.error}>
        <p class="verevon-space-projection-error" role="alert">
          {i18n.tr(
            'Medlemslisten kunne ikke hentes. Din egen tilgang er uendret.',
            'The member list could not be loaded. Your own access is unchanged.',
          )}
        </p>
      </Show>

      {/* Only a room somebody made has a member list to edit. The organization
          channel's people come from the organization, and a personal Space has
          exactly one member by construction. */}
      <Show
        when={
          canGrantSpaceMembership(props.role)
          && props.kind !== 'personal'
          && props.isOrganizationRoom === false
        }
      >
        <SpaceMemberControls
          spaceRef={props.spaceRef}
          orgId={props.orgId}
          roster={() => (props.roster.error ? [] : props.roster() ?? [])}
          onChanged={props.onMembersChanged}
        />
      </Show>

      <Show when={props.isOrganizationRoom === true}>
        <p class="verevon-space-member-controls__derived">
          {i18n.tr(
            'Alle i organisasjonen er med i dette rommet. Medlemskapet følger organisasjonen og redigeres ikke her.',
            'Everyone in the organization is in this room. Its membership follows the organization and is not edited here.',
          )}
        </p>
      </Show>

      <Show when={roster.error ? undefined : roster()}>
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
                    {member.subject_type === 'service' ? i18n.tr('Agent', 'Agent') : i18n.tr('Person', 'Person')} · {spaceRoleLabel(member.role, i18n.tr)}
                  </span>
                  {/* The registered owner stays: Control keeps them as owner
                      regardless, so removing the grant would only make this
                      list disagree with the roster it describes. Agents are
                      governed from the Agent tab, not here. */}
                  <Show
                    when={
                      canGrantSpaceMembership(props.role)
                      && props.kind !== 'personal'
                      && props.isOrganizationRoom === false
                      && member.subject_type === 'user'
                      && member.role !== 'owner'
                    }
                  >
                    <SpaceMemberRemoveButton
                      spaceRef={props.spaceRef}
                      member={member}
                      onChanged={props.onMembersChanged}
                    />
                  </Show>
                </li>
              )}
            </For>
          </ul>
        )}
      </Show>

      <Show when={props.kind === 'personal'}>
        <section class="verevon-space-danger-zone" aria-labelledby="space-deletion-title">
          <div>
            <p class="verevon-space-eyebrow">{i18n.tr('Innstillinger for personlig rom', 'Personal Space settings')}</p>
            <h3 id="space-deletion-title">{i18n.tr('Slett personlig rom', 'Delete Personal Space')}</h3>
            <p>{i18n.tr(
              'Sletting godkjennes og fullføres separat. En forespørsel er ikke bevis på at alle eiere har slettet sine data.',
              'Deletion is authorized and completed separately. A request is not proof that every owner has erased its data.',
            )}</p>
          </div>
          <Show
            when={!props.deletionSubmitting()}
            fallback={
              <button type="button" disabled>
                {i18n.tr('Ber om sletting …', 'Requesting deletion…')}
              </button>
            }
          >
            <SpaceConfirmButton
              label={i18n.tr('Be om sletting', 'Request deletion')}
              confirmLabel={i18n.tr('Bekreft sletteforespørsel', 'Confirm deletion request')}
              consequence={i18n.tr(
                'Sletting godkjennes av Control og bekreftes av hver eierplan før noe faktisk fjernes.',
                'Deletion is authorized by Control and confirmed by every owner plane before anything is actually removed.',
              )}
              onConfirm={() => void props.onRequestDeletion()}
            />
          </Show>
          <Show when={props.deletionError()}>
            <p role="alert">{props.deletionError()}</p>
          </Show>
          <Show when={props.deletionReceipt.loading}>
            <p role="status">{i18n.tr('Laster slettekvittering …', 'Loading deletion receipt…')}</p>
          </Show>
          <Show when={props.deletionReceipt()}>
            {(receipt) => (
              <div class="verevon-space-deletion-receipt" aria-live="polite">
                <p>{i18n.tr('Godkjenning: ', 'Authorization: ')}{receipt().request.state.replace(/_/g, ' ')}</p>
                <p>{i18n.tr('Slettestatus: ', 'Purge status: ')}{receipt().purgeStatus.replace(/_/g, ' ')}</p>
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
  const i18n = useI18n()

  return (
    <aside class="verevon-space-pulse" aria-labelledby="space-pulse-title">
      <div class="verevon-space-pulse__heading">
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Kort oppsummert', 'At a glance')}</p>
          <h2 id="space-pulse-title">{i18n.tr('Rompuls', 'Room pulse')}</h2>
        </div>
        {/* `data-active` carries the state; the aria-label is for people. The
            sheet used to key the lit style on `[aria-label="Active work"]` —
            the ENGLISH label — so in Norwegian, the default, the signal never
            lit no matter how busy the room was. Style must never read a
            translated string. */}
        <span
          class="verevon-space-pulse__signal"
          data-active={props.activeRun() ? 'true' : undefined}
          aria-label={props.activeRun() ? i18n.tr('Aktivt arbeid', 'Active work') : i18n.tr('Ingen aktivt arbeid', 'No active work')}
        />
      </div>

      <Show
        when={props.activeRun()}
        fallback={
          <div class="verevon-space-pulse-card">
            <span class="verevon-space-pulse-card__icon" aria-hidden="true"><Bot size={17} /></span>
            <div>
              <strong>{i18n.tr('Ingen agentarbeid er aktivt', 'No agent work is active')}</strong>
              <p>{i18n.tr('Når arbeid starter i en samtale, vises tilstanden her.', 'When work starts in a conversation, its state appears here.')}</p>
            </div>
          </div>
        }
      >
        {(run) => (
          <div class="verevon-space-pulse-card verevon-space-pulse-card--active">
            <span class="verevon-space-pulse-card__icon" aria-hidden="true"><Bot size={17} /></span>
            <span>
              <strong>{i18n.tr('Verevon jobber', 'Verevon is working')}</strong>
              <span>{threadTitle(run(), i18n.tr)}</span>
              <span class="verevon-space-pulse-card__detail">
                <Clock3 size={13} aria-hidden="true" />
                {threadStatus(run(), i18n.tr)}
              </span>
            </span>
          </div>
        )}
      </Show>

      <dl class="verevon-space-pulse-stats">
        <div>
          <dt>{i18n.tr('Samtaler', 'Conversations')}</dt>
          <dd>{props.threads().length}</dd>
        </div>
        <div>
          <dt>{i18n.tr('Oppmerksomhet', 'Attention')}</dt>
          <dd>{props.threads().filter((thread) => thread.latest_run_status === 'awaiting_approval').length}</dd>
        </div>
      </dl>

      <p class="verevon-space-pulse__note">
        {i18n.tr(
          'Denne pulsen gjenspeiler kun den gjeldende samtaleprojeksjonen for rommet.',
          'This pulse only reflects the current Space conversation projection.',
        )}
      </p>
    </aside>
  )
}

// Space.kind, per Control's authority.go `Kind` enum (ADR-0001). Falls back to
// `formatLabel` for anything not listed, so an unmapped value still renders
// something readable instead of nothing.
const SPACE_KIND_LABELS: Record<string, { no: string; en: string }> = {
  personal: { no: 'Personlig', en: 'Personal' },
  room: { no: 'Rom', en: 'Room' },
  project: { no: 'Prosjekt', en: 'Project' },
  case: { no: 'Sak', en: 'Case' },
}

// Space.lifecycle, per Control's authority.go `SpaceLifecycle` enum.
const SPACE_LIFECYCLE_LABELS: Record<string, { no: string; en: string }> = {
  pending_registration: { no: 'Venter på registrering', en: 'Pending registration' },
  active: { no: 'Aktiv', en: 'Active' },
  suspended: { no: 'Suspendert', en: 'Suspended' },
  deleting: { no: 'Slettes', en: 'Deleting' },
  deleted: { no: 'Slettet', en: 'Deleted' },
  failed_registration: { no: 'Registrering feilet', en: 'Registration failed' },
}

// Membership/roster role, per user-core's `space_memberships_role_chk`
// constraint (viewer/editor/manager/owner).
const SPACE_ROLE_LABELS: Record<string, { no: string; en: string }> = {
  viewer: { no: 'Leser', en: 'Viewer' },
  editor: { no: 'Redaktør', en: 'Editor' },
  manager: { no: 'Leder', en: 'Manager' },
  owner: { no: 'Eier', en: 'Owner' },
}

function translatedEnumLabel(
  value: string,
  dictionary: Record<string, { no: string; en: string }>,
  tr: (no: string, en: string) => string,
): string {
  const entry = dictionary[value.trim().toLowerCase()]
  return entry ? tr(entry.no, entry.en) : formatLabel(value)
}

function spaceKindLabel(kind: string, tr: (no: string, en: string) => string): string {
  return translatedEnumLabel(kind, SPACE_KIND_LABELS, tr)
}

function spaceLifecycleLabel(lifecycle: string, tr: (no: string, en: string) => string): string {
  return translatedEnumLabel(lifecycle, SPACE_LIFECYCLE_LABELS, tr)
}

function spaceRoleLabel(role: string, tr: (no: string, en: string) => string): string {
  return translatedEnumLabel(role, SPACE_ROLE_LABELS, tr)
}

// Space agent binding lifecycle, per the Convex `spaceAgentBindings.status`
// union. `revoked` is filtered out server-side and so never reaches here, but
// it stays mapped: a value arriving unmapped would render as a raw token.
const AGENT_STATUS_LABELS: Record<string, { no: string; en: string }> = {
  pending: { no: 'Venter', en: 'Pending' },
  active: { no: 'Aktiv', en: 'Active' },
  paused: { no: 'Satt på pause', en: 'Paused' },
  revoked: { no: 'Tilbakekalt', en: 'Revoked' },
  failed: { no: 'Feilet', en: 'Failed' },
}

// Status must never be carried by colour alone (scope plan §UI-1), so each
// state gets a shape that reads without colour and survives a screenshot in
// greyscale. Marked aria-hidden wherever used — the adjacent text is the
// accessible answer, and a screen reader announcing punctuation helps nobody.
const AGENT_STATUS_MARKS: Record<string, string> = {
  pending: '◔',
  active: '●',
  paused: '❙❙',
  revoked: '✕',
  failed: '▲',
}

const DELIVERY_STATUS_LABELS: Record<string, { no: string; en: string }> = {
  active: { no: 'aktiv', en: 'active' },
  pending: { no: 'venter', en: 'pending' },
  failed: { no: 'feilet', en: 'failed' },
}

const DELIVERY_STATUS_MARKS: Record<string, string> = {
  active: '●',
  pending: '◔',
  failed: '▲',
}

// Channel display names. These are product names, so only the surrounding
// wording is translated — "Microsoft Teams" is called that in both languages.
const CHANNEL_LABELS: Record<string, { no: string; en: string }> = {
  teams: { no: 'Microsoft Teams', en: 'Microsoft Teams' },
  messenger: { no: 'Messenger', en: 'Messenger' },
  embed: { no: 'Innebygd widget', en: 'Embedded widget' },
}

function approvalModeLabel(mode: string, tr: (no: string, en: string) => string): string {
  if (mode === 'auto') return tr('Autonom', 'Autonomous')
  if (mode === 'blocked') return tr('Blokkert', 'Blocked')
  return tr('Krever bekreftelse', 'Requires confirmation')
}

function agentStatusLabel(status: string, tr: (no: string, en: string) => string): string {
  return translatedEnumLabel(status, AGENT_STATUS_LABELS, tr)
}

function agentStatusMark(status: string): string {
  return AGENT_STATUS_MARKS[status] ?? '·'
}

function deliveryStatusLabel(status: string, tr: (no: string, en: string) => string): string {
  return translatedEnumLabel(status, DELIVERY_STATUS_LABELS, tr)
}

function deliveryStatusMark(status: string): string {
  return DELIVERY_STATUS_MARKS[status] ?? '·'
}

function channelLabel(channel: string, tr: (no: string, en: string) => string): string {
  return translatedEnumLabel(channel, CHANNEL_LABELS, tr)
}

function spaceWorkroomLabel(kind: string, tr: (no: string, en: string) => string): string {
  return kind === 'personal' ? tr('Personlig rom', 'Personal room') : tr('Delt arbeidsrom', 'Shared workroom')
}
