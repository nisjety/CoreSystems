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
  requestPersonalSpaceDeletion,
  updateSpaceInstructions,
  type SpaceAgent,
  type SpaceDeletionReceipt,
  type SpaceRosterMember,
  type SpaceThread,
} from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { spaceDisplayName } from '../lib/space-name'
import {
  ACTIVE_RUN_STATUSES,
  formatLabel,
  threadStatus,
  threadTitle,
} from '../lib/space-thread-presentation'
import { SpaceActivityFeed } from './SpaceActivityFeed'
import { SpaceBindAgentDialog } from './SpaceBindAgentDialog'
import { SpaceCockpit } from './SpaceCockpit'
import { SpaceCreateAgentDialog } from './SpaceCreateAgentDialog'
import { SpaceRoomComposer } from './SpaceRoomComposer'
import { SpaceRoomTimeline } from './SpaceRoomTimeline'

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

// Membership is authoritative only at the server. Revalidate while the Space
// is open so a removal/revocation cannot leave an old resolved value usable in
// the cockpit between navigations.
const SPACE_CONTEXT_RECHECK_MS = 30_000

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
  const [roster] = createResource(spaceRef, getSpaceRoster)
  const [agents, { refetch: refetchAgents }] = createResource(spaceRef, getSpaceAgents)
  const [createAgentOpen, setCreateAgentOpen] = createSignal(false)
  const [bindAgentOpen, setBindAgentOpen] = createSignal(false)
  const [deletionError, setDeletionError] = createSignal('')
  const [deletionSubmitting, setDeletionSubmitting] = createSignal(false)
  const currentThreads = () => threads()?.threads ?? []
  const activeRun = () => currentThreads().find((thread) => ACTIVE_RUN_STATUSES.has(thread.latest_run_status ?? ''))

  async function requestDeletion() {
    const selectedSpace = context()?.space
    if (!selectedSpace || selectedSpace.kind !== 'personal' || deletionSubmitting()) return
    if (!window.confirm(i18n.tr(
      'Be om sletting av dette personlige rommet? Eksisterende data slettes først etter godkjenning fra Control og kvitteringer fra hver eierplan.',
      'Request deletion of this Personal Space? Existing data will be deleted only after Control authorization and owner-plane receipts.',
    ))) return
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
                      threads={currentThreads}
                      loading={() => threads.loading}
                      unavailable={threadsUnavailable}
                      roster={() => (roster.error ? [] : roster() ?? [])}
                      agents={() => (agents.error ? [] : agents() ?? [])}
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
                  aktivitet: (
                    <SpaceActivityPanel
                      threads={currentThreads}
                      loading={() => threads.loading}
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
  readonly threads: () => readonly SpaceThread[]
  readonly loading: () => boolean
  readonly unavailable: () => boolean
  readonly roster: () => readonly SpaceRosterMember[]
  readonly agents: () => readonly SpaceAgent[]
  readonly onExchangeSettled?: () => void
  readonly onCreateAgent?: () => void
}) {
  const i18n = useI18n()
  let focusComposer: (() => void) | undefined
  const [replyTarget, setReplyTarget] = createSignal<{ threadId: string; title: string } | undefined>(undefined)

  return (
    <section class="verevon-space-view verevon-space-view--conversations" aria-labelledby="space-conversations-title">
      <div class="verevon-space-view__heading">
        <div>
        <p class="verevon-space-eyebrow">{i18n.tr('Samtale i rommet', 'Space conversation')}</p>
          <h2 id="space-conversations-title">Samtaler</h2>
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
            spaceName={props.spaceName}
            threads={props.threads}
            roster={props.roster}
            agents={props.agents}
            onStartConversation={() => focusComposer?.()}
            onCreateAgent={props.onCreateAgent}
            onReply={(thread) => {
              setReplyTarget({ threadId: thread.thread_id, title: threadTitle(thread, i18n.tr) })
              focusComposer?.()
            }}
          />
        </Show>
      </Show>

      <SpaceRoomComposer
        spaceRef={props.spaceRef}
        roster={props.roster}
        agents={props.agents}
        onExchangeSettled={props.onExchangeSettled}
        registerFocusHandle={(focus) => { focusComposer = focus }}
        replyTarget={replyTarget}
        onClearReplyTarget={() => setReplyTarget(undefined)}
      />
    </section>
  )
}

function SpaceActivityPanel(props: {
  readonly threads: () => readonly SpaceThread[]
  readonly loading: () => boolean
}) {
  const i18n = useI18n()

  return (
    <section class="verevon-space-view" aria-labelledby="space-activity-title">
      <div class="verevon-space-view__heading">
        <div>
          {/* This eyebrow previously read "Room pulse" — copy-pasted from the
              SpacePulse aside, unrelated to this Activity view. Corrected while
              translating rather than carried forward. */}
          <p class="verevon-space-eyebrow">{i18n.tr('Aktivitet i rommet', 'Space activity')}</p>
          <h2 id="space-activity-title">Aktivitet</h2>
          <p>{i18n.tr(
            'Lesbar bevegelse, godkjenninger og utfall fra rommets samtaleprojeksjon.',
            'Readable movement, approvals, and outcomes from this Space’s conversation projection.',
          )}</p>
        </div>
      </div>
      <Show when={props.loading()}>
        <p class="verevon-space-inline-status" role="status">{i18n.tr('Laster aktivitet i rommet …', 'Loading Space activity…')}</p>
      </Show>
      <SpaceActivityFeed
        threads={props.threads()}
        emptyLabel={i18n.tr(
          'Ingen samtaleaktivitet er publisert til dette rommet ennå.',
          'No conversation activity has been published to this Space yet.',
        )}
      />
      <p class="verevon-space-view__footnote">
        {i18n.tr(
          'Kjørekvitteringer og godkjenninger vises fra den gjeldende trådprojeksjonen. Annen dokumentasjon fra eierplan kommer til når en korrelert romprojeksjon er publisert.',
          'Run receipts and approvals appear from the current thread projection. Other owner-plane evidence joins only when a correlated Space projection is published.',
        )}
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
  readonly onCreateAgent?: () => void
  readonly onBindAgent?: () => void
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
              {(agent) => <SpaceAgentCard agent={agent} />}
            </For>
          </ul>
        </Show>
      </Show>
    </section>
  )
}

/** One agent as a room participant. */
function SpaceAgentCard(props: { readonly agent: SpaceAgent }) {
  const i18n = useI18n()
  const agent = () => props.agent
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
          <h2 id="space-members-title">Medlemmer</h2>
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
          <button type="button" onClick={() => void props.onRequestDeletion()} disabled={props.deletionSubmitting()}>
            {props.deletionSubmitting() ? i18n.tr('Ber om sletting …', 'Requesting deletion…') : i18n.tr('Be om sletting', 'Request deletion')}
          </button>
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
        <span
          class="verevon-space-pulse__signal"
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
