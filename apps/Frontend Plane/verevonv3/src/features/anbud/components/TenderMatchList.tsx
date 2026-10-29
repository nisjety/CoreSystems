import { For, Show } from 'solid-js'
import type { TenderMatch, TenderMatches, TenderStrictness } from '@/shared/api/anbud-client'
import { useI18n } from '@/shared/i18n'
import { strictnessLabel, widerSteps } from '@/features/anbud/lib/strictness'

type Props = {
  data: TenderMatches | null
  busy: boolean
  error: string | null
  strictness: TenderStrictness
  selected: string | null
  onSelect: (noticeId: string) => void
}

/** The tenders a company can bid on, with what each one's gate and fit say. No percentages: a band only. */
export function TenderMatchList(props: Props) {
  const { tr } = useI18n()

  const verdictText = (match: TenderMatch) => {
    if (match.verdict === 'unknown') return tr('Ikke vurdert', 'Not assessed')
    if (match.verdict === 'pass') return tr('Kan by', 'Can bid')
    if (match.verdict === 'conditional') return tr('Kan by med forbehold', 'Can bid with conditions')
    return tr('Kan ikke by', 'Cannot bid')
  }

  const bandText = (match: TenderMatch) => {
    const labels: Record<TenderMatch['band'], string> = {
      strong: tr('Sterk match', 'Strong match'),
      good: tr('God match', 'Good match'),
      stretch: tr('Strekk', 'Stretch'),
      none: tr('Ingen match', 'No match'),
    }
    return labels[match.band]
  }

  const deadlineText = (match: TenderMatch) =>
    match.deadline
      ? tr(`${match.deadline.daysLeft} dager igjen`, `${match.deadline.daysLeft} days left`)
      : tr('Ingen frist oppgitt', 'No deadline stated')

  return (
    <section class="verevon-anbud__matches" aria-label={tr('Treff', 'Matches')}>
      <Show when={props.error}>
        {(text) => (
          <p class="verevon-anbud__state verevon-anbud__state--error" role="alert">
            {text()}
          </p>
        )}
      </Show>
      <Show when={props.busy && !props.data}>
        <p class="verevon-anbud__state" role="status">
          {tr('Søker…', 'Searching…')}
        </p>
      </Show>
      <Show when={!props.data && !props.busy && !props.error}>
        <p class="verevon-anbud__state">
          {tr('Legg inn en profil og trykk Søk for å se anbud.', 'Enter a profile and press Search to see tenders.')}
        </p>
      </Show>
      <Show when={props.data}>
        {(data) => (
          <>
            <p class="verevon-anbud__summary">
              {tr(
                `Leste ${data().funnel.scanned} kunngjøringer, ${data().funnel.opportunities} anbud og fant ${data().funnel.matched} treff.`,
                `Read ${data().funnel.scanned} notices, ${data().funnel.opportunities} tenders and found ${data().funnel.matched} matches.`,
              )}
            </p>
            <ul class="verevon-anbud__widen" aria-label={tr('Bredere søk', 'Wider searches')}>
              <For each={widerSteps(props.strictness)}>
                {(step) => {
                  const adds = data().gainedByWidening[step] ?? 0
                  const label = strictnessLabel(step, tr)
                  return (
                    <li class="verevon-anbud__chip">
                      <span>{tr(`+${adds} anbud ved ${label}`, `+${adds} tenders at ${label}`)}</span>
                    </li>
                  )
                }}
              </For>
            </ul>
            <Show
              when={data().results.length > 0}
              fallback={
                <p class="verevon-anbud__state">
                  {tr('Ingen anbud matcher profilen akkurat nå.', 'No tenders match the profile right now.')}
                </p>
              }
            >
              <ul class="verevon-anbud__list">
                <For each={data().results}>
                  {(match) => (
                    <li>
                      <button
                        type="button"
                        class="verevon-selectable-row verevon-anbud__row"
                        aria-pressed={props.selected === match.noticeId}
                        onClick={() => props.onSelect(match.noticeId)}
                      >
                        <strong>{match.title}</strong>
                        <small>{[match.buyer, deadlineText(match)].filter(Boolean).join(' · ')}</small>
                        <span class={`verevon-anbud__badge verevon-anbud__badge--${match.verdict}`}>
                          {verdictText(match)}
                        </span>
                        <small>{bandText(match)}</small>
                      </button>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </>
        )}
      </Show>
    </section>
  )
}
