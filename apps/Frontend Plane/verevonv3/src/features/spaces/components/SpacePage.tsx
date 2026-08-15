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
import { useI18n } from '@/shared/i18n'
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
                  <h1 id="space-title">{current().space.name}</h1>
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
  const i18n = useI18n()

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
        <a class="verevon-space-secondary-action" href={spaceChatHref(props.spaceRef)}>
          <Sparkles size={15} aria-hidden="true" />
          {i18n.tr('Start en samtale', 'Start a conversation')}
        </a>
      </div>

      <Show when={props.loading()}>
        <p class="verevon-space-inline-status" role="status">{i18n.tr('Laster samtaler i rommet …', 'Loading Space conversations…')}</p>
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
                  <h3 id="space-fresh-conversation-title">{i18n.tr('Utforsk boter i Agent Studio', 'Explore bots in Agent Studio')}</h3>
                  <p>{i18n.tr(
                    'Åpne Agent Studio for å utforske bot-malene som er tilgjengelige for organisasjonen din.',
                    'Open Agent Studio to explore the bot blueprints available to your organization.',
                  )}</p>
                  <A
                    class="verevon-space-fresh-conversation__action"
                    href="/agents?agent=chatbot&view=playground"
                    aria-label={i18n.tr('Åpne Agent Studio', 'Open Agent Studio')}
                  >
                    <Bot size={16} aria-hidden="true" />
                    {i18n.tr('Åpne Agent Studio', 'Open Agent Studio')}
                  </A>
                </div>
              }
            >
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

      <a
        class="verevon-space-composer-link"
        href={spaceChatHref(props.spaceRef)}
        aria-label={`${i18n.tr('Skriv til', 'Message')} ${props.spaceName}`}
      >
        <span class="verevon-space-composer-link__plus" aria-hidden="true"><Plus size={16} /></span>
        <span>{i18n.tr('Skriv til', 'Message')} {props.spaceName}</span>
        <ArrowUpRight size={16} aria-hidden="true" />
      </a>
    </section>
  )
}

function SpaceConversationRow(props: { readonly thread: SpaceThread }) {
  const i18n = useI18n()
  const status = () => props.thread.latest_run_status
  const isActive = () => ACTIVE_RUN_STATUSES.has(status() ?? '')

  return (
    <li class="verevon-space-conversation-row" data-active={isActive() || undefined}>
      <a href={threadHref(props.thread.thread_id)}>
        <span class="verevon-space-conversation-row__icon" aria-hidden="true">
          <MessageCircle size={16} />
        </span>
        <span class="verevon-space-conversation-row__body">
          <strong>{threadTitle(props.thread, i18n.tr)}</strong>
          <span>{props.thread.preview || i18n.tr('Åpne samtale', 'Open conversation')}</span>
        </span>
        <span class="verevon-space-conversation-row__meta">
          <span classList={{ 'verevon-space-status': true, 'verevon-space-status--active': isActive() }}>
            {threadStatus(props.thread, i18n.tr)}
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
function SpaceAgentPanel(props: { readonly spaceRef: string }) {
  const i18n = useI18n()
  const [roster] = createResource(() => props.spaceRef, getSpaceRoster)
  const agents = () => (roster() ?? []).filter((member) => member.subject_type === 'service')

  return (
    <section class="verevon-space-view" aria-labelledby="space-agents-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Agenter', 'Agents')}</p>
          <h2 id="space-agents-title">Agent</h2>
          <p>{i18n.tr(
            'Agenter som er gitt tilgang til dette rommet, med rollen de har her.',
            'Agents granted access to this room, with the role they hold here.',
          )}</p>
        </div>
      </div>

      <Show when={roster.loading}>
        <p class="verevon-space-inline-status" role="status">{i18n.tr('Henter agenter …', 'Loading agents…')}</p>
      </Show>

      <Show when={roster.error}>
        <p class="verevon-space-projection-error" role="alert">
          {i18n.tr(
            'Agentlisten kunne ikke hentes. Din egen tilgang er uendret.',
            'The agent list could not be loaded. Your own access is unchanged.',
          )}
        </p>
      </Show>

      <Show when={roster.error ? undefined : roster()}>
        <Show
          when={agents().length > 0}
          fallback={
            /* An empty list here is a real answer, not a missing projection:
               Control was asked and no agent holds a binding in this room. */
            <p>{i18n.tr('Ingen agenter er bundet til dette rommet ennå.', 'No agents are bound to this room yet.')}</p>
          }
        >
          <ul class="verevon-space-roster">
            <For each={agents()}>
              {(agent: SpaceRosterMember) => (
                <li class="verevon-space-roster__row">
                  <span class="verevon-space-roster__name">
                    {agent.display_name || agent.subject_id}
                  </span>
                  <span class="verevon-space-roster__meta">{i18n.tr('Agent', 'Agent')} · {spaceRoleLabel(agent.role, i18n.tr)}</span>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Show>

      <p class="verevon-space-inline-status">
        {i18n.tr(
          'Ferdigheter, koblinger og kjørestatus per agent kommer når bindingsmodellen publiserer dem; dette viser tilgangen Control faktisk har gitt.',
          'Skills, connectors and run status per agent arrive once the binding model publishes them; this shows the access Control has actually granted.',
        )}
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
  const i18n = useI18n()
  const [roster] = createResource(() => props.spaceRef, getSpaceRoster)

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
          <a class="verevon-space-pulse-card verevon-space-pulse-card--active" href={threadHref(run().thread_id)}>
            <span class="verevon-space-pulse-card__icon" aria-hidden="true"><Bot size={17} /></span>
            <span>
              <strong>{i18n.tr('Verevon jobber', 'Verevon is working')}</strong>
              <span>{threadTitle(run(), i18n.tr)}</span>
              <span class="verevon-space-pulse-card__detail">
                <Clock3 size={13} aria-hidden="true" />
                {threadStatus(run(), i18n.tr)}
              </span>
            </span>
          </a>
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

function threadHref(threadId: string): string {
  return `/chat?thread_id=${encodeURIComponent(threadId)}`
}

function spaceChatHref(spaceRef: string): string {
  return `/chat?space_ref=${encodeURIComponent(spaceRef)}`
}

function threadTitle(thread: SpaceThread, tr: (no: string, en: string) => string): string {
  return thread.title?.trim() || thread.preview?.trim() || tr('Samtale uten tittel', 'Untitled conversation')
}

function threadStatus(thread: SpaceThread, tr: (no: string, en: string) => string): string {
  const status = thread.latest_run_status
  if (!status) return tr('Samtale åpen', 'Conversation open')
  if (status === 'awaiting_approval') return tr('Venter på godkjenning', 'Needs approval')
  if (status === 'running') return tr('Arbeider', 'Working')
  if (status === 'queued') return tr('I kø', 'Queued')
  if (status === 'completed') return tr('Fullført', 'Completed')
  return formatLabel(status)
}

// Humanizes a raw server enum token this file has NOT given a translated
// dictionary — currently only the deletion receipt's `state`/`purgeStatus`/
// `ownerPlane.status`. Space `kind`/`lifecycle` and membership `role` used to
// fall through to this too; they now go through the dictionaries below
// instead, because those three are read on every page view (the header meta
// line, the membership card, every roster row) and a user asked for them
// translated. The receipt vocabulary stays here: it is seen rarely — only
// mid-deletion — and building that dictionary without a confirmed, complete
// value list would risk the same silent-gap problem this comment used to warn
// against for the other three.
function formatLabel(value: string): string {
  return value
    .trim()
    .replace(/[_-]+/g, ' ')
    .replace(/\b\w/g, (letter) => letter.toUpperCase())
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

function spaceWorkroomLabel(kind: string, tr: (no: string, en: string) => string): string {
  return kind === 'personal' ? tr('Personlig rom', 'Personal room') : tr('Delt arbeidsrom', 'Shared workroom')
}

function formatWhen(value: string): string {
  try {
    return new Date(value).toLocaleDateString('nb-NO', { day: 'numeric', month: 'short' })
  } catch {
    return value
  }
}
