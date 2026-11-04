import { createMemo, For, Show } from 'solid-js'
import { parseResearchProvenance } from '@/shared/api/chat-client'
import { useI18n } from '@/shared/i18n'
import { formatMoment } from '@/shared/i18n/format'
import { isSafeLinkHref } from '../lib/safe-href'

/** Claim counts by status, in the stored ledger's tokens (EP-2-S6). */
export type ClaimCounts = { supported: number; single_source: number; conflict: number; unsupported: number }

/** What a gap-driven second pass added to a stamp (EP-2-S6). */
export type ResearchPassFacts = {
  /** Research passes that ran: 2 when a gap search ran, 1 when it was skipped. */
  passes: number
  /** Ledger claims per status; absent when no ledger could be produced. */
  claims?: ClaimCounts
  /** The `[n]` the second pass's reads earned, for grouping the Sources tab by pass. */
  secondPassNumbers: number[]
}

const CLAIM_STATUSES = ['supported', 'single_source', 'conflict', 'unsupported'] as const

/**
 * The second-pass facts on a provenance stamp (EP-2-S6), read from the wire shape on their own so a stamp
 * without them (the switch off, an older turn) yields nothing and renders as before. Counts must all be
 * non-negative integers or none is shown: a half-read tally would understate what was unsupported.
 */
export function researchPassFacts(value: unknown): ResearchPassFacts | undefined {
  if (!value || typeof value !== 'object') return undefined
  const record = value as Record<string, unknown>
  const passes = record.passes
  if (typeof passes !== 'number' || !Number.isInteger(passes) || passes < 1) return undefined
  const rawCounts = record.claims_by_status
  let claims: ClaimCounts | undefined
  if (rawCounts && typeof rawCounts === 'object') {
    const counts = rawCounts as Record<string, unknown>
    const valid = CLAIM_STATUSES.every((status) => {
      const count = counts[status]
      return typeof count === 'number' && Number.isInteger(count) && count >= 0
    })
    if (valid) {
      claims = {
        supported: counts.supported as number,
        single_source: counts.single_source as number,
        conflict: counts.conflict as number,
        unsupported: counts.unsupported as number,
      }
    }
  }
  const secondPassNumbers = (Array.isArray(record.sources) ? record.sources : []).flatMap((entry) => {
    const source = entry && typeof entry === 'object' ? (entry as Record<string, unknown>) : undefined
    return source?.pass === 2 && typeof source.number === 'number' ? [source.number] : []
  })
  return { passes, claims, secondPassNumbers }
}

/**
 * The provenance stamp under a research report's artifact card (EP-2-S4): what the report was built from, the
 * coverage it reached and the model that wrote it. Takes the stamp as it arrived, live or from a stored turn, and
 * renders nothing unless it is complete, so a report without one (the switch off, an older turn, a ZDR or waived
 * turn reloaded) looks exactly as before.
 */
export function ArtifactProvenance(props: { provenance: unknown }) {
  const i18n = useI18n()
  const stamp = createMemo(() => parseResearchProvenance(props.provenance))
  const passFacts = createMemo(() => researchPassFacts(props.provenance))
  const when = (iso: string) => formatMoment(iso, i18n.locale())
  return (
    <Show when={stamp()}>
      {(value) => (
        <footer class="verevon-chat-artifact-provenance" aria-label={i18n.tr('Hvordan rapporten ble laget', 'How this report was made')}>
          <p class="verevon-chat-artifact-provenance__summary">
            {i18n.tr(
              `Bygget fra ${value().coverage.read} av ${value().coverage.found} kilder · ${value().model} · ${when(value().generated_at)}`,
              `Built from ${value().coverage.read} of ${value().coverage.found} sources · ${value().model} · ${when(value().generated_at)}`,
            )}
          </p>
          <Show when={value().coverage.deadline_hit}>
            <p class="verevon-chat-artifact-provenance__warning" role="note">
              {i18n.tr('Tiden gikk ut før alle kildene var lest.', 'Time ran out before every source was read.')}
            </p>
          </Show>
          <Show when={passFacts()}>
            {(facts) => (
              <p class="verevon-chat-artifact-provenance__passes">
                {facts().passes > 1
                  ? i18n.tr('To runder: hull i kildene ble søkt opp på nytt.', 'Two passes: gaps in the sources were searched again.')
                  : i18n.tr('Én runde.', 'One pass.')}
              </p>
            )}
          </Show>
          <Show when={passFacts()?.claims}>
            {(claims) => (
              <p class="verevon-chat-artifact-provenance__claims">
                {i18n.tr(
                  `Påstander: ${claims().supported} støttet · ${claims().single_source} fra én kilde · ${claims().conflict} motstridende · ${claims().unsupported} ikke verifisert`,
                  `Claims: ${claims().supported} supported · ${claims().single_source} single-source · ${claims().conflict} conflicting · ${claims().unsupported} not verified`,
                )}
              </p>
            )}
          </Show>
          <details class="verevon-chat-artifact-provenance__details">
            <summary>{i18n.tr('Spørsmål, delspørsmål og kilder', 'Question, sub-queries and sources')}</summary>
            <p>{value().question}</p>
            <Show when={value().sub_queries.length > 0}>
              <ul class="verevon-chat-artifact-provenance__queries">
                <For each={value().sub_queries}>{(query) => <li>{query}</li>}</For>
              </ul>
            </Show>
            <ol class="verevon-chat-artifact-provenance__sources">
              <For each={value().sources}>
                {(source) => (
                  <li value={source.number}>
                    {/* A search-supplied address: an unsafe one keeps its title but is not a link. */}
                    <Show when={isSafeLinkHref(source.url)} fallback={<span>{source.title || source.url}</span>}>
                      <a href={source.url} target="_blank" rel="noopener noreferrer">{source.title || source.url}</a>
                    </Show>
                  </li>
                )}
              </For>
            </ol>
          </details>
        </footer>
      )}
    </Show>
  )
}
