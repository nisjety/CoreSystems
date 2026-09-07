import { For, Show } from 'solid-js'

import { getSpaceWork, type SpaceWork } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  buildSpaceWork,
  type RunLikeActivitySource,
  type ScheduleLikeActivitySource,
} from '../lib/activity-grammar'
import { SpaceActivityFeed } from './SpaceActivityFeed'

/**
 * What this room has running and scheduled.
 *
 * This tab rendered "Model Plane has not published a Space projection for this
 * yet" from the day the cockpit shipped, and it was telling the truth: the run
 * listing was per-thread and owner-bound, and the schedule listing was org-wide
 * with no way to ask about one room. Both take a Space now.
 *
 * # It reuses the Activity grammar rather than inventing a second one
 *
 * A run that is waiting for a person says the same words here as in Activity,
 * ordered by the same consequence rule, because it is the same fact seen from a
 * different question. Work asks "what is in flight", Activity asks "what has
 * happened" — a shared vocabulary is what keeps those two answers from
 * disagreeing about one run.
 *
 * # A gap is rendered, never swallowed
 *
 * Runs and schedules fail independently, so the response can carry real rows
 * AND a named gap. Showing the rows while saying what is missing is the only
 * honest option for a tab whose purpose is "what needs me": a room with pending
 * work that looks idle is the failure this whole surface exists to avoid.
 */
export interface SpaceWorkPanelProps {
  readonly spaceRef: string
}

function sectionLabel(section: string, tr: (no: string, en: string) => string): string {
  if (section === 'runs') return tr('Kjøringer: ', 'Runs: ')
  if (section === 'schedules') return tr('Planlagt arbeid: ', 'Scheduled work: ')
  return `${section}: `
}

/**
 * The gap in the reader's language.
 *
 * The gateway sends a stable `code` alongside its English sentence. A known
 * code is translated; an unknown one falls back to the server's own words,
 * because a gap stated in the wrong language beats a gap not stated at all —
 * and inventing a translation for a reason this build does not know would be
 * guessing at what went wrong.
 */
function gapReason(
  gap: { readonly code?: string; readonly reason: string },
  tr: (no: string, en: string) => string,
): string {
  switch (gap.code) {
    case 'runs_read_not_authorized':
      return tr(
        'Du kan ikke se det andre medlemmer kjører i dette rommet.',
        'You cannot see what other members are running in this Space.',
      )
    case 'runs_upstream_unavailable':
      return tr(
        'Kjøringene i rommet kunne ikke hentes.',
        'This Space’s runs could not be loaded.',
      )
    case 'schedules_upstream_unavailable':
      return tr(
        'Det planlagte arbeidet i rommet kunne ikke hentes.',
        'This Space’s scheduled work could not be loaded.',
      )
    case 'schedules_session_unavailable':
      return tr(
        'Denne økten kan ikke lese planlagt arbeid nå.',
        'This session cannot read scheduled work right now.',
      )
    default:
      return gap.reason
  }
}

export function SpaceWorkPanel(props: SpaceWorkPanelProps) {
  const i18n = useI18n()
  const [work] = createResource(() => props.spaceRef, getSpaceWork)

  const resolved = (): SpaceWork | undefined => (work.error ? undefined : work())
  const items = () => {
    const current = resolved()
    if (!current) return []
    return buildSpaceWork(
      current.runs as readonly RunLikeActivitySource[],
      current.schedules as readonly ScheduleLikeActivitySource[],
    )
  }

  return (
    <section class="verevon-space-view" aria-labelledby="space-work-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Arbeid i rommet', 'Work in this Space')}</p>
          <h2 id="space-work-title">{i18n.tr('Arbeid', 'Work')}</h2>
          <p>{i18n.tr(
            'Kjøringer som pågår eller venter, og planlagte oppgaver som hører til dette rommet.',
            'Runs in flight or waiting, and the scheduled work that belongs to this Space.',
          )}</p>
        </div>
      </div>

      <Show when={work.loading}>
        <p class="verevon-space-inline-status" role="status">
          {i18n.tr('Henter arbeidet i rommet …', 'Loading this Space’s work…')}
        </p>
      </Show>

      {/* A failed read is not an idle room. */}
      <Show when={work.error}>
        <p class="verevon-space-projection-error" role="alert">
          {i18n.tr(
            'Vi fikk ikke hentet arbeidet i rommet. Det som kjører, kjører fortsatt.',
            'We could not load this Space’s work. Whatever is running is still running.',
          )}
        </p>
      </Show>

      {/* Named gaps, alongside whatever did resolve. */}
      <For each={resolved()?.unavailable ?? []}>
        {(gap) => (
          <p class="verevon-space-work__gap" role="status">
            {sectionLabel(gap.section, i18n.tr)}
            {gapReason(gap, i18n.tr)}
          </p>
        )}
      </For>

      <Show when={!work.loading && !work.error}>
        <SpaceActivityFeed
          threads={[]}
          items={items()}
          emptyLabel={i18n.tr(
            'Ingenting kjører eller er planlagt i dette rommet nå.',
            'Nothing is running or scheduled in this Space right now.',
          )}
        />
      </Show>
    </section>
  )
}
