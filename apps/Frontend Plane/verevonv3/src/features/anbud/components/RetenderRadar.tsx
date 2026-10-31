import { For, Show } from 'solid-js'
import type { Retenders, TenderAward, TenderDial } from '@/shared/api/anbud-client'
import { useI18n } from '@/shared/i18n'
import { strictnessLabel } from '@/features/anbud/lib/strictness'

type Props = {
  data: Retenders | null
  busy: boolean
  error: string | null
}

/** Contracts whose base period ends soon: the ones a buyer is likely to tender again. */
export function RetenderRadar(props: Props) {
  const { tr } = useI18n()

  const reachText = (award: TenderAward) => (award.reach === 'near' ? tr('Nær', 'Near') : tr('Eksakt', 'Exact'))

  const endText = (award: TenderAward) =>
    award.contractEnd ? award.contractEnd : tr('Sluttdato ikke oppgitt', 'End date not stated')

  const dialText = (dial: TenderDial) => {
    switch (dial) {
      case 'cpv_group':
        return tr('CPV: samme gruppe (tre første siffer)', 'CPV: the same group (first three digits)')
      case 'cpv_division':
        return tr('CPV: samme divisjon (to første siffer)', 'CPV: the same division (first two digits)')
      case 'nuts2_region':
        return tr('region: samme NUTS 2-region', 'region: the same NUTS 2 region')
      case 'nuts1_region':
        return tr('region: samme NUTS 1-region', 'region: the same NUTS 1 region')
    }
  }

  return (
    <section class="verevon-panel verevon-anbud__radar" aria-label={tr('Re-konkurranser', 'Re-tenders')}>
      <header>
        <h2>{tr('Re-konkurranser', 'Re-tenders')}</h2>
        <p class="verevon-anbud__hint">
          {tr(
            'Kontrakter som snart må lyses ut på nytt, de neste 12 månedene.',
            'Contracts that will soon need to be tendered again, in the next 12 months.',
          )}
        </p>
      </header>
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
          {tr('Legg inn en profil og trykk Søk for å se radaren.', 'Enter a profile and press Search to see the radar.')}
        </p>
      </Show>
      <Show when={props.data}>
        {(data) => (
          <>
            <p class="verevon-anbud__summary">
              {tr(
                `Viser ${data().results.length} av ${data().matching} kontrakter`,
                `Showing ${data().results.length} of ${data().matching} contracts`,
              )}
            </p>
            <ul class="verevon-anbud__widen" aria-label={tr('Bredere søk', 'Wider searches')}>
              <For each={data().widening}>
                {(step) => {
                  const label = strictnessLabel(step.to, tr)
                  return (
                    <li class="verevon-anbud__chip">
                      <span>{tr(`+${step.adds} ved ${label}`, `+${step.adds} at ${label}`)}</span>
                      <small>{step.dials.map(dialText).join(' · ')}</small>
                    </li>
                  )
                }}
              </For>
            </ul>
            <Show when={data().winners === 'not_configured'}>
              <p class="verevon-anbud__hint">
                {tr('Vinnere vises ikke før registeret er koblet til.', 'Winners are not shown until the register is connected.')}
              </p>
            </Show>
            <Show
              when={data().results.length > 0}
              fallback={
                <p class="verevon-anbud__state">
                  {tr('Ingen kontrakter utløper i perioden.', 'No contracts end in the period.')}
                </p>
              }
            >
              <ul class="verevon-anbud__list">
                <For each={data().results}>
                  {(award) => (
                    <li class="verevon-selectable-row verevon-anbud__row">
                      <strong>{award.title}</strong>
                      <small>{[award.buyer, endText(award)].filter(Boolean).join(' · ')}</small>
                      <span class={`verevon-anbud__badge verevon-anbud__badge--${award.reach === 'near' ? 'near' : 'exact'}`}>
                        {reachText(award)}
                      </span>
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
