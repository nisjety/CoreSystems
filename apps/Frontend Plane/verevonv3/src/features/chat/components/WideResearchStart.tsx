/**
 * The composer's "Bred forskning" entry (EP-2 S8). The person picks a count preset or pastes a list,
 * one question per line, and asks for an estimate first: model-gateway answers with the items after
 * the organisation's cap, how many run at once, a time range and, on a metered and priced route, a
 * cost range. Start hands the choice to the composer, which sends it as one Do turn
 * (`chat.wide_research_start`); the server clamps the count again and names any items past the cap in
 * the answer, so nothing on this card can widen a run.
 *
 * The estimate route answers 404 while `WIDE_RESEARCH_ENTRY_ENABLED` is off; the card then says wide
 * research is not available and offers no Start. A zero-retention session gets the typed refusal and
 * the same outcome.
 */

import { createSignal, For, Match, Show, Switch } from 'solid-js'
import { ApiError } from '@/shared/api/api-error'
import {
  estimateWideResearch,
  parseWideResearchItems,
  WIDE_RESEARCH_PRESETS,
  type WideResearchEstimate,
  type WideResearchStart as WideResearchChoice,
} from '@/shared/api/chat-client'
import { useI18n } from '@/shared/i18n'

type EstimateLoader = (input: { itemCount: number; model?: string }) => Promise<WideResearchEstimate>

type Estimate =
  | { state: 'idle' }
  | { state: 'loading' }
  | { state: 'ready'; estimate: WideResearchEstimate }
  | { state: 'unavailable' }
  | { state: 'failed' }

/** The choice the composer sends: the pasted items when there are any, else the preset count. */
export function wideResearchChoice(preset: number, pasted: string): WideResearchChoice {
  const items = parseWideResearchItems(pasted)
  return items.length > 0 ? { items } : { itemCap: preset }
}

/** How many items a choice asks for, for the estimate. */
export function requestedItemCount(choice: WideResearchChoice): number {
  return choice.items?.length ?? choice.itemCap ?? 0
}

function formatUsd(low: number, high: number): string {
  const format = (value: number) => (value < 0.01 ? '< $0.01' : `$${value.toFixed(2)}`)
  return low === high ? format(low) : `${format(low)} – ${format(high)}`
}

export function WideResearchStart(props: {
  model?: string
  onStart: (choice: WideResearchChoice) => void
  onCancel?: () => void
  /** Injectable for tests; the real route by default. */
  estimate?: EstimateLoader
}) {
  const i18n = useI18n()
  const [preset, setPreset] = createSignal<number>(WIDE_RESEARCH_PRESETS[0])
  const [pasted, setPasted] = createSignal('')
  const [estimate, setEstimate] = createSignal<Estimate>({ state: 'idle' })
  const choice = () => wideResearchChoice(preset(), pasted())

  // Any change to the choice makes an earlier estimate stale, so Start waits for a new one.
  const changeChoice = (apply: () => void) => {
    apply()
    setEstimate({ state: 'idle' })
  }

  const loadEstimate = async () => {
    const load = props.estimate ?? estimateWideResearch
    setEstimate({ state: 'loading' })
    try {
      const answer = await load({ itemCount: requestedItemCount(choice()), model: props.model })
      setEstimate({ state: 'ready', estimate: answer })
    } catch (error) {
      const refused = error instanceof ApiError && (error.status === 404 || error.feature === 'agentic')
      setEstimate({ state: refused ? 'unavailable' : 'failed' })
    }
  }

  const ready = () => {
    const current = estimate()
    return current.state === 'ready' ? current.estimate : null
  }

  return (
    <section class="verevon-wide-research" aria-label={i18n.tr('Bred forskning', 'Wide research')}>
      <h3 class="verevon-wide-research__title">{i18n.tr('Bred forskning', 'Wide research')}</h3>
      <div role="group" aria-label={i18n.tr('Antall spørsmål', 'Number of questions')}>
        <For each={WIDE_RESEARCH_PRESETS}>
          {(count) => (
            <button
              type="button"
              class="verevon-wide-research__preset"
              aria-pressed={preset() === count && parseWideResearchItems(pasted()).length === 0 ? 'true' : 'false'}
              onClick={() => changeChoice(() => setPreset(count))}
            >
              {count}
            </button>
          )}
        </For>
      </div>
      <label class="verevon-wide-research__list">
        <span>{i18n.tr('Eller lim inn en liste, ett spørsmål per linje', 'Or paste a list, one question per line')}</span>
        <textarea
          rows={5}
          value={pasted()}
          onInput={(event) => changeChoice(() => setPasted(event.currentTarget.value))}
        />
      </label>
      <button type="button" class="verevon-wide-research__estimate" onClick={() => void loadEstimate()}>
        {i18n.tr('Beregn omfang', 'Estimate')}
      </button>
      <Switch>
        <Match when={estimate().state === 'loading'}>
          <p role="status">{i18n.tr('Beregner…', 'Estimating…')}</p>
        </Match>
        <Match when={estimate().state === 'unavailable'}>
          <p role="status">{i18n.tr('Bred forskning er ikke tilgjengelig her.', 'Wide research is not available here.')}</p>
        </Match>
        <Match when={estimate().state === 'failed'}>
          <p role="alert">{i18n.tr('Kunne ikke beregne omfanget. Prøv igjen.', 'The estimate failed. Try again.')}</p>
        </Match>
        <Match when={ready()}>
          {(current) => (
            <div class="verevon-wide-research__card">
              <Show
                when={current().itemsAfterCap > 0}
                fallback={
                  <p role="status">
                    {i18n.tr(
                      'Organisasjonen din har slått av bred forskning.',
                      'Your organisation has wide research turned off.',
                    )}
                  </p>
                }
              >
                <p data-testid="wide-research-items">
                  {i18n.tr(
                    `${current().itemsAfterCap} spørsmål undersøkes, ${current().concurrency} om gangen.`,
                    `${current().itemsAfterCap} questions are researched, ${current().concurrency} at a time.`,
                  )}
                </p>
                <Show when={current().itemsRequested > current().itemsAfterCap}>
                  <p data-testid="wide-research-capped">
                    {i18n.tr(
                      `${current().itemsRequested - current().itemsAfterCap} spørsmål er over grensen på ${current().cap} og blir ikke undersøkt; svaret nevner dem.`,
                      `${current().itemsRequested - current().itemsAfterCap} questions are past the limit of ${current().cap} and are not researched; the answer names them.`,
                    )}
                  </p>
                </Show>
                <p data-testid="wide-research-minutes">
                  {i18n.tr(
                    `Omtrent ${current().minutesLow}–${current().minutesHigh} minutter. Et anslag, ikke et løfte.`,
                    `About ${current().minutesLow}–${current().minutesHigh} minutes. An estimate, not a promise.`,
                  )}
                </p>
                <p data-testid="wide-research-cost">
                  {current().usdLow !== null && current().usdHigh !== null
                    ? i18n.tr(
                        `Anslått kostnad: ${formatUsd(current().usdLow as number, current().usdHigh as number)}`,
                        `Estimated cost: ${formatUsd(current().usdLow as number, current().usdHigh as number)}`,
                      )
                    : current().costBasis === 'subscription'
                      ? i18n.tr('Ingen kostnad per token på abonnementet ditt.', 'No per-token cost on your subscription.')
                      : i18n.tr('Kostnaden kan ikke anslås for denne modellen.', 'The cost cannot be estimated for this model.')}
                </p>
              </Show>
            </div>
          )}
        </Match>
      </Switch>
      <div class="verevon-wide-research__actions">
        <Show when={props.onCancel}>
          {(cancel) => (
            <button type="button" onClick={() => cancel()()}>
              {i18n.tr('Avbryt', 'Cancel')}
            </button>
          )}
        </Show>
        <button
          type="button"
          class="verevon-wide-research__start"
          disabled={(ready()?.itemsAfterCap ?? 0) === 0}
          onClick={() => props.onStart(choice())}
        >
          {i18n.tr('Start', 'Start')}
        </button>
      </div>
    </section>
  )
}
