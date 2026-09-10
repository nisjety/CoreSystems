import { createMemo, createSignal, For, Show } from 'solid-js'

import { getSpaceActivity, type SpaceActivity } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  buildSpaceRecord,
  type ApprovalLikeActivitySource,
  type AuthorityLikeActivitySource,
  type OperationLikeActivitySource,
  type RunLikeActivitySource,
  type SpaceActivityItem,
  type ThreadLikeActivitySource,
} from '../lib/activity-grammar'
import { SpaceActivityFeed } from './SpaceActivityFeed'

/**
 * What has happened in this room, on whose authority, and with what outcome.
 *
 * This tab rendered the conversation projection alone from the day the cockpit
 * shipped, with a footnote promising that "other owner-plane evidence joins
 * only when a correlated Space projection is published". That projection is
 * what S2.3's slice 5 asks for, and it exists now: an owner effect is bound to
 * the grant that authorized it, and that grant carries the Space, so
 * Conversation Core can answer which effects happened under this room's
 * authority without the operation ledger ever growing a Space column.
 *
 * # Five sources, one ordering
 *
 * Threads are the message spine. Runs add each run's own outcome plus the
 * token and step counts the run contract has always carried. Approvals add the
 * decisions a person owes or has made. Operations add the effects that actually
 * left the system. Authority adds who was permitted to cause them. All five go
 * through the same salience rule, so "what needs me" is the top of the list
 * regardless of which plane it came from.
 *
 * # Filtering is a lens, never a hiding place
 *
 * The count of what a filter excludes is always shown. A supervisor who
 * narrowed to one class and then read the list as complete would be misled by
 * their own control, and Activity is the surface where that matters most.
 */
export interface SpaceActivityPanelProps {
  readonly spaceRef: string
  /**
   * The room's threads, already fetched by the page.
   *
   * Passed in rather than re-read: the conversation spine is the one source
   * this tab shares with Chat, and fetching it twice would put two answers to
   * the same question on one screen.
   */
  readonly threads: () => readonly ThreadLikeActivitySource[]
  readonly threadsLoading: () => boolean
}

type ActivityLens = 'all' | 'attention' | 'work' | 'conversation' | 'authority'

const LENSES: readonly { readonly id: ActivityLens; readonly no: string; readonly en: string }[] = [
  { id: 'all', no: 'Alt', en: 'Everything' },
  { id: 'attention', no: 'Trenger oppmerksomhet', en: 'Needs attention' },
  { id: 'work', no: 'Arbeid og effekter', en: 'Work and effects' },
  { id: 'conversation', no: 'Samtaler', en: 'Conversations' },
  { id: 'authority', no: 'Fullmakter', en: 'Authority' },
]

function matchesLens(item: SpaceActivityItem, lens: ActivityLens): boolean {
  switch (lens) {
    case 'all':
      return true
    case 'attention':
      // Consequence, not class: an unknown outcome and a waiting approval both
      // belong here even though they come from different planes.
      return item.salience === 'critical' || item.salience === 'attention'
    case 'work':
      return item.renderClass === 'run' || item.renderClass === 'operation' || item.renderClass === 'approval'
    case 'conversation':
      return item.renderClass === 'conversation'
    case 'authority':
      return item.renderClass === 'authority'
    default:
      return true
  }
}

function sectionLabel(section: string, tr: (no: string, en: string) => string): string {
  if (section === 'runs') return tr('Kjøringer: ', 'Runs: ')
  if (section === 'approvals') return tr('Godkjenninger: ', 'Approvals: ')
  if (section === 'operations') return tr('Effekter: ', 'Effects: ')
  if (section === 'delivery') return tr('Levering: ', 'Delivery: ')
  if (section === 'watches') return tr('Overvåkinger: ', 'Watches: ')
  return `${section}: `
}

/**
 * The gap in the reader's language.
 *
 * Same contract as Work and Knowledge: a stable `code` is translated, an
 * unknown one falls back to the server's own sentence, because a gap stated in
 * the wrong language beats a gap not stated at all.
 */
function gapReason(
  gap: { readonly code?: string; readonly reason: string },
  tr: (no: string, en: string) => string,
): string {
  switch (gap.code) {
    case 'runs_read_not_authorized':
      return tr(
        'Du kan ikke se det andre medlemmer har kjørt i dette rommet.',
        'You cannot see what other members have run in this Space.',
      )
    case 'runs_upstream_unavailable':
      return tr('Kjøringene i rommet kunne ikke hentes.', 'This Space’s runs could not be loaded.')
    case 'runs_session_unavailable':
      return tr(
        'Denne økten kan ikke lese kjøringene i rommet nå.',
        'This session cannot read this Space’s runs right now.',
      )
    case 'approvals_upstream_unavailable':
      return tr(
        'Detaljene for godkjenninger kunne ikke hentes.',
        'Approval detail could not be loaded.',
      )
    case 'operations_upstream_unavailable':
      return tr(
        'Effektene som er utført i rommet kunne ikke hentes.',
        'The effects performed in this Space could not be loaded.',
      )
    case 'operations_endpoint_unavailable':
      return tr(
        'Denne installasjonen kan ennå ikke liste effektene i rommet.',
        'This deployment cannot yet list this Space’s effects.',
      )
    case 'delivery_ledger_not_built':
      return tr(
        'Leveringstilstand publiseres ikke ennå.',
        'Durable delivery state is not published yet.',
      )
    case 'watches_not_built':
      return tr('Overvåkinger publiseres ikke ennå.', 'Watches are not published yet.')
    default:
      return gap.reason
  }
}

export function SpaceActivityPanel(props: SpaceActivityPanelProps) {
  const i18n = useI18n()
  const [activity] = createResource(() => props.spaceRef, getSpaceActivity)
  const [lens, setLens] = createSignal<ActivityLens>('all')

  const resolved = (): SpaceActivity | undefined => (activity.error ? undefined : activity())
  const allItems = createMemo(() => {
    const current = resolved()
    return buildSpaceRecord({
      threads: props.threads(),
      runs: (current?.runs ?? []) as readonly RunLikeActivitySource[],
      approvals: (current?.approvals ?? []) as readonly ApprovalLikeActivitySource[],
      operations: (current?.operations ?? []) as readonly OperationLikeActivitySource[],
      authority: (current?.authority ?? []) as readonly AuthorityLikeActivitySource[],
    })
  })
  const shown = createMemo(() => allItems().filter((item) => matchesLens(item, lens())))
  const hidden = () => allItems().length - shown().length

  return (
    <section class="verevon-space-view" aria-labelledby="space-activity-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Aktivitet i rommet', 'Space activity')}</p>
          <h2 id="space-activity-title">{i18n.tr('Aktivitet', 'Activity')}</h2>
          <p>{i18n.tr(
            'Hva som har skjedd her, på hvilken fullmakt, og med hvilket utfall.',
            'What has happened here, on whose authority, and with what outcome.',
          )}</p>
        </div>
      </div>

      {/* A radiogroup rather than buttons: these are one exclusive choice, and
          arrow-key traversal is what a keyboard reader expects of that. */}
      <div
        class="verevon-activity-lenses"
        role="radiogroup"
        aria-label={i18n.tr('Filtrer aktivitet', 'Filter activity')}
      >
        <For each={LENSES}>
          {(option) => (
            <button
              type="button"
              role="radio"
              aria-checked={lens() === option.id ? 'true' : 'false'}
              class={`verevon-activity-lens${lens() === option.id ? ' verevon-activity-lens--active' : ''}`}
              onClick={() => setLens(option.id)}
            >
              {i18n.tr(option.no, option.en)}
            </button>
          )}
        </For>
      </div>

      <Show when={activity.loading || props.threadsLoading()}>
        <p class="verevon-space-inline-status" role="status">
          {i18n.tr('Henter aktiviteten i rommet …', 'Loading this Space’s activity…')}
        </p>
      </Show>

      {/* A failed read is not a quiet room. */}
      <Show when={activity.error}>
        <p class="verevon-space-projection-error" role="alert">
          {i18n.tr(
            'Vi fikk ikke hentet hele aktiviteten i rommet. Det som vises under er bare samtaleprojeksjonen.',
            'We could not load this Space’s full activity. What follows is the conversation projection only.',
          )}
        </p>
      </Show>

      <For each={resolved()?.unavailable ?? []}>
        {(gap) => (
          <p class="verevon-space-work__gap" role="status">
            {sectionLabel(gap.section, i18n.tr)}
            {gapReason(gap, i18n.tr)}
          </p>
        )}
      </For>

      {/* Rendered while loading ONLY once there is something to show, so the
          thread spine appears immediately without the feed also claiming the
          room is empty. A read in flight is not an empty room, the same way a
          failed one is not — and the two states were briefly on screen
          together before this guard. */}
      <Show when={!activity.loading || shown().length > 0}>
      <SpaceActivityFeed
        threads={[]}
        items={shown()}
        emptyLabel={
          lens() === 'all'
            ? i18n.tr(
                'Ingen aktivitet er publisert til dette rommet ennå.',
                'No activity has been published to this Space yet.',
              )
            : i18n.tr(
                'Ingenting i rommet passer dette filteret.',
                'Nothing in this Space matches this filter.',
              )
        }
      />
      </Show>

      {/* What the lens is hiding, always. A narrowed list read as a complete
          one is the failure this whole surface exists to prevent. */}
      <Show when={hidden() > 0}>
        <p class="verevon-space-view__footnote" role="status">
          {i18n.tr(
            `${hidden()} flere hendelser er skjult av dette filteret.`,
            `${hidden()} more events are hidden by this filter.`,
          )}
        </p>
      </Show>
    </section>
  )
}
