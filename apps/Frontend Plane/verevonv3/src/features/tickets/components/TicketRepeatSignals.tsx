import { createSignal, For, Show } from 'solid-js'
import type { TicketRepeatSignal } from '@/features/tickets/lib/ticket-repeat-signals'
import { searchKnowledge, type SearchHit } from '@/shared/api/knowledge-client'
import { useI18n } from '@/shared/i18n'

function readableIntent(intent: string): string {
  return intent.replace(/_/g, ' ')
}

type KnowledgeCoverage = {
  state: 'checking' | 'covered' | 'gap_candidate' | 'unverified'
  hits?: readonly SearchHit[]
}

function signalID(signal: TicketRepeatSignal): string {
  return `${signal.workType}\u0000${signal.category}\u0000${signal.intent}`
}

/**
 * A compact, evidence-bounded recurrence read. It exposes only exact
 * taxonomy matches from the current canonical queue and deliberately does not
 * make a semantic or causal claim about the supporting tickets.
 */
export function TicketRepeatSignals(props: { orgId: string; signals: readonly TicketRepeatSignal[] }) {
  const i18n = useI18n()
  const [coverageBySignal, setCoverageBySignal] = createSignal<Readonly<Record<string, KnowledgeCoverage>>>({})

  const checkKnowledgeCoverage = async (signal: TicketRepeatSignal) => {
    const id = signalID(signal)
    if (!props.orgId.trim() || coverageBySignal()[id]?.state === 'checking') return
    setCoverageBySignal((current) => ({ ...current, [id]: { state: 'checking' } }))
    try {
      const result = await searchKnowledge(props.orgId, {
        query: `Support guidance for ${signal.category} ${readableIntent(signal.intent)}`,
        limit: 3,
      })
      const hits = result.results.slice(0, 3)
      setCoverageBySignal((current) => ({
        ...current,
        [id]: hits.length > 0 ? { state: 'covered', hits } : { state: 'gap_candidate' },
      }))
    } catch {
      // A failed or unavailable retrieval has no evidentiary value. Keeping it
      // distinct from an empty successful result prevents a false gap claim.
      setCoverageBySignal((current) => ({ ...current, [id]: { state: 'unverified' } }))
    }
  }

  return (
    <Show when={props.signals.length > 0}>
      <section class="verevon-ticketing-repeat-signals" aria-label={i18n.tr('Gjentatte støttesignaler', 'Recurring support signals')}>
        <div class="verevon-ticketing-repeat-signals__header">
          <div>
            <strong>{i18n.tr('Gjentatte støttesignaler', 'Recurring support signals')}</strong>
            <p>{i18n.tr('Eksakte treff i kategori og hensikt blant aktive saker i denne køen.', 'Exact category and intent matches among active tickets in this queue.')}</p>
          </div>
        </div>
        <ul>
          <For each={props.signals}>
            {(signal) => (
              <li>
                <div>
                  <strong>{signal.category}</strong>
                  <span>{readableIntent(signal.intent)}</span>
                </div>
              <small>{i18n.tr(`${signal.count} aktive saker`, `${signal.count} active tickets`)}</small>
              <small>{signal.ticketKeys.join(', ')}</small>
              <button
                type="button"
                aria-label={i18n.tr(`Sjekk kunnskapsdekning for ${signal.category}`, `Check Knowledge coverage for ${signal.category}`)}
                disabled={coverageBySignal()[signalID(signal)]?.state === 'checking'}
                onClick={() => void checkKnowledgeCoverage(signal)}
              >
                {coverageBySignal()[signalID(signal)]?.state === 'checking'
                  ? i18n.tr('Sjekker kunnskap …', 'Checking Knowledge…')
                  : i18n.tr('Sjekk kunnskapsdekning', 'Check Knowledge coverage')}
              </button>
              <Show when={coverageBySignal()[signalID(signal)]}>
                {(coverage) => (
                  <p class="verevon-ticketing-repeat-signals__coverage">
                    <Show when={coverage().state === 'covered'}>
                      {i18n.tr(`${coverage().hits?.length ?? 0} autoriserte kunnskapskilder ble funnet.`, `${coverage().hits?.length ?? 0} authorized Knowledge sources were returned.`)}
                    </Show>
                    <Show when={coverage().state === 'gap_candidate'}>
                      {i18n.tr('Ingen autorisert kunnskapskilde ble funnet. Dette er en gapkandidat, ikke bevis på at kunnskap mangler.', 'No authorized Knowledge source was returned. This is a gap candidate, not proof that knowledge is missing.')}
                    </Show>
                    <Show when={coverage().state === 'unverified'}>
                      {i18n.tr('Kunne ikke bekrefte kunnskapsdekning. Dette behandles ikke som en gapkandidat.', 'Could not verify Knowledge coverage. This is not treated as a gap candidate.')}
                    </Show>
                  </p>
                )}
              </Show>
            </li>
            )}
          </For>
        </ul>
        <p class="verevon-ticketing-repeat-signals__disclosure">{i18n.tr('Tolker ikke semantisk likhet og oppretter ikke hendelse eller problem automatisk.', 'Does not infer semantic similarity or automatically create an Incident or Problem.')}</p>
      </section>
    </Show>
  )
}
